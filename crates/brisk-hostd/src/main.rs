//! brisk-hostd: the host agent behind the Brisk web UI. Binds 127.0.0.1:8443 only; reach it with
//! `ssh -N -L 8443:127.0.0.1:8443 bawn`.
//!   brisk-hostd [DIST_DIR]      (default ./web/dist, or BRISK_DIST)

use brisk_hostd::config::Config;
use brisk_hostd::http::{router, AppState};
use std::sync::Arc;

#[tokio::main(flavor = "current_thread")]
async fn main() -> std::process::ExitCode {
    let dist = std::env::args_os()
        .nth(1)
        .or_else(|| std::env::var_os("BRISK_DIST"))
        .unwrap_or_else(|| "web/dist".into());
    let cfg = Config::from_env(dist.into());
    let l = match tokio::net::TcpListener::bind(cfg.listen).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("error: bind {}: {e}", cfg.listen);
            return std::process::ExitCode::FAILURE;
        }
    };
    println!("page\thttp://localhost:8443/?s={}", cfg.secret);
    let state = Arc::new(AppState::new(cfg));
    state.clip.start_watch();
    let app = router(state.clone());
    let stop = async move {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
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
