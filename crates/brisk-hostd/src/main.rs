//! brisk-hostd: the host agent behind the Brisk web UI. Binds 127.0.0.1:8443 only; reach it with
//! `ssh -N -L 8443:127.0.0.1:8443 bawn` or through `tailscale serve` (docs/ui-notes.md, Hosting).
//!   brisk-hostd [DIST_DIR]      (default ./web/dist, or BRISK_DIST)
//!   brisk-hostd wrap <game id> -- <command...>   Steam launch option shim (wrap.rs)
//! BRISK_STATIC=embed serves the build compiled in with `--features embed` (default: disk).
//!
//! Environment variables: see docs/env.md.

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
    brisk_hostd::log::StderrLog::init(std::env::var("BRISK_LOG").ok().as_deref());
    // `brisk-hostd add <file> [--move|--copy|--link]` registers a local file and exits.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "add") {
        let (code, msg) =
            brisk_hostd::ingest::add_cmd(&Config::from_env("web/dist".into()), &args[1..]);
        if code == 0 {
            println!("{msg}");
        } else {
            eprintln!("{msg}");
        }
        return std::process::ExitCode::from(code as u8);
    }
    // The Steam launch option shim (wrap.rs): no server, no secret.
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.first().is_some_and(|a| a == "wrap") {
        let data_dir = Config::from_env(PathBuf::new()).data_dir;
        let code = brisk_hostd::wrap::run(&argv[1..], &data_dir, &mut brisk_hostd::wrap::real_exec);
        return std::process::ExitCode::from(code);
    }
    let dist = std::env::args_os()
        .nth(1)
        .or_else(|| std::env::var_os("BRISK_DIST"))
        .unwrap_or_else(|| "web/dist".into());
    let mut cfg = Config::from_env(dist.into());
    let site = Site::choose(
        cfg.dist.clone(),
        std::env::var("BRISK_STATIC").ok().as_deref(),
    );
    tracing::info!("{}", site.describe());
    let conf = std::env::var_os("HOME")
        .map_or_else(|| PathBuf::from("."), PathBuf::from)
        .join(".config/brisk");
    let secret_file =
        std::env::var_os("BRISK_SECRET_FILE").map_or_else(|| conf.join("secret"), PathBuf::from);
    cfg.secret = match load_secret(std::env::var("BRISK_SECRET").ok(), &secret_file) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("secret {}: {e}", secret_file.display());
            return std::process::ExitCode::FAILURE;
        }
    };
    cfg.allowed_users
        .extend(load_users(&conf.join("allowed-users")));
    if !cfg.allowed_users.is_empty() {
        tracing::info!(
            "tailnet logins allowed without the secret: {}",
            cfg.allowed_users.join(", ")
        );
    }
    // Game ids changed in Stage 1: rekey the data files before anything reads them (idempotent).
    brisk_hostd::migrate::run(&cfg.data_dir);
    let l = match tokio::net::TcpListener::bind(cfg.listen).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("bind {}: {e}", cfg.listen);
            return std::process::ExitCode::FAILURE;
        }
    };
    println!("{}", page_line(&cfg.secret, cfg.service));
    let hub = match machines::load(&cfg.machines_file, cfg.machines_explicit) {
        Ok(h) => h.map(Arc::new),
        Err(e) => {
            tracing::error!("machines file {e}");
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
    tokio::spawn(brisk_hostd::playtime::watch(
        state.cfg.data_dir.clone(),
        state.cfg.proc_dir.clone(),
        state.cfg.playtime_poll,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
    ));
    state.files.clean_partials();
    {
        let (cfg, art) = (state.cfg.clone(), state.art.clone());
        tokio::task::spawn_blocking(move || art.sweep(&cfg));
    }
    state.uploads.clean_old();
    if state.cfg.scan_on_start {
        let cfg = state.cfg.clone();
        let art = state.art.clone();
        tokio::task::spawn_blocking(move || match brisk_hostd::ingest::scan_library(&cfg) {
            Ok(r) => {
                tracing::info!("scan: {} new games", r.merged.added.len());
                art.trigger_new(&cfg, &r.merged.added);
            }
            Err(e) => tracing::warn!("startup scan: {e}"),
        });
    }
    let app = router(state.clone()).into_make_service_with_connect_info::<SocketAddr>();
    // systemd (Type=notify): READY only once the router answers on the socket, then the watchdog.
    // Manual runs (no NOTIFY_SOCKET) skip all of it.
    let st = state.clone();
    if notify::enabled() {
        tokio::spawn(async move {
            if !notify::serving(st.cfg.listen).await {
                tracing::error!("{} bound but not answering; READY not sent", st.cfg.listen);
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
            tracing::error!("{e}");
            std::process::ExitCode::FAILURE
        }
    }
}
