//! brisk-hostd: the host agent behind the Brisk web UI. Binds 127.0.0.1:8443 only; reach it with
//! `ssh -N -L 8443:127.0.0.1:8443 bawn` or through `tailscale serve` (docs/ui-notes.md, Hosting).
//!   brisk-hostd [DIST_DIR]      (default ./web/dist, or BRISK_DIST)
//! BRISK_STATIC=embed serves the build compiled in with `--features embed` (default: disk).
//!
//! Environment (read in config.rs unless noted):
//!   BRISK_DIST             web build directory (main.rs; the DIST_DIR argument wins)
//!   BRISK_STATIC           `embed` or disk (main.rs, site.rs)
//!   BRISK_SECRET_FILE      the persistent secret (main.rs; default ~/.config/brisk/secret)
//!   BRISK_SECRET           secret override for tests (main.rs)
//!   BRISK_ALLOWED_USERS    tailnet logins let in without the secret, comma/newline separated
//!   BRISK_ALLOWED_HOSTS    extra Host headers allowed besides localhost:8443 and 127.0.0.1:8443
//!   BRISK_MACHINES_FILE    hub machine list (default ~/.config/brisk/machines.json; absent = single host)
//!   BRISK_APP              Sunshine app id to launch (default 870779643)
//!   BRISK_LISTEN           bridge listen IP:PORT (default: the tailscale IPv4, port 4433)
//!   BRISK_BRIDGE_BIN       brisk-bridge binary (default: `brisk-bridge` on PATH)
//!   BRISK_TLS_DIR          bridge certificate directory (default ~/.config/brisk/tls)
//!   BRISK_DATA_DIR         sessions, prefs and caches (default ~/.local/share/brisk)
//!   BRISK_DOWNLOADS_DIR    file drop target (default ~/Downloads/brisk)
//!   BRISK_STEAM_DIR        Steam install (default ~/.local/share/Steam)
//!   BRISK_STEAM_KEY_FILE   Steam API key file (default ~/.config/brisk/steam-api-key)
//!   BRISK_STEAM_API_BASE   Steam Web API base URL (default https://api.steampowered.com)
//!   BRISK_SPAWN_SCOPE      `1` starts spawned programs in a systemd scope (host.rs)

use brisk_hostd::config::{Config, load_secret, load_users, page_line};
use brisk_hostd::http::{AppState, router};
use brisk_hostd::machines;
use brisk_hostd::notify;
use brisk_hostd::site::Site;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

#[tokio::main(flavor = "current_thread")]
async fn main() -> std::process::ExitCode {
    let dist = std::env::args_os()
        .nth(1)
        .or_else(|| std::env::var_os("BRISK_DIST"))
        .unwrap_or_else(|| "web/dist".into());
    let mut cfg = Config::from_env(dist.into());
    let site = Site::choose(
        cfg.dist.clone(),
        std::env::var("BRISK_STATIC").ok().as_deref(),
    );
    eprintln!("{}", site.describe());
    let conf = std::env::var_os("HOME")
        .map_or_else(|| PathBuf::from("."), PathBuf::from)
        .join(".config/brisk");
    let secret_file =
        std::env::var_os("BRISK_SECRET_FILE").map_or_else(|| conf.join("secret"), PathBuf::from);
    cfg.secret = match load_secret(std::env::var("BRISK_SECRET").ok(), &secret_file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: secret {}: {e}", secret_file.display());
            return std::process::ExitCode::FAILURE;
        }
    };
    cfg.allowed_users
        .extend(load_users(&conf.join("allowed-users")));
    if !cfg.allowed_users.is_empty() {
        eprintln!(
            "tailnet logins allowed without the secret: {}",
            cfg.allowed_users.join(", ")
        );
    }
    let l = match tokio::net::TcpListener::bind(cfg.listen).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("error: bind {}: {e}", cfg.listen);
            return std::process::ExitCode::FAILURE;
        }
    };
    println!("{}", page_line(&cfg.secret, cfg.service));
    let hub = match machines::load(
        &cfg.machines_file,
        std::env::var_os("BRISK_MACHINES_FILE").is_some(),
    ) {
        Ok(h) => h.map(Arc::new),
        Err(e) => {
            eprintln!("error: machines file {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let mut state = AppState::new(cfg);
    state.site = site;
    state.hub = hub.clone();
    let state = Arc::new(state);
    if let Some(h) = &hub {
        machines::spawn_idle(h);
    }
    state.hosts.refresh().await;
    state.clip.start_watch();
    state.files.clean_partials();
    let app = router(state.clone()).into_make_service_with_connect_info::<SocketAddr>();
    // systemd (Type=notify): READY only once the router answers on the socket, then the watchdog.
    // Manual runs (no NOTIFY_SOCKET) skip all of it.
    let st = state.clone();
    if notify::enabled() {
        tokio::spawn(async move {
            if !notify::serving(st.cfg.listen).await {
                eprintln!(
                    "error: {} bound but not answering; READY not sent",
                    st.cfg.listen
                );
                return;
            }
            notify::ready(&notify::status_text(
                st.cfg.listen,
                &st.site,
                &st.stream.info(),
            ));
            if let Some(every) = notify::watchdog_every() {
                notify::watchdog(st, every).await;
            }
        });
    }
    let stop = async move {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
        notify::stopping();
        // Bridge first, so it cancels its Sunshine session before we go.
        state.stream.shutdown().await;
    };
    match axum::serve(l, app).with_graceful_shutdown(stop).await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}
