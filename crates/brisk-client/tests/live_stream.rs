//! M1 gate against a live Sunshine (never in CI): runs `brisk-probe stream` for 60 s and relies
//! on its exit code, which is non-zero when parse errors > 0, frames < 0.95 x 60 x secs, or an
//! IDR request goes unanswered for 2 s. The session is cancelled either way.
//!   BRISK_HOST=127.0.0.1 BRISK_APP=<id> cargo test -p brisk-client --test live_stream -- --ignored --nocapture

#[test]
#[ignore = "live: needs a paired Sunshine; set BRISK_HOST and BRISK_APP, run with --ignored"]
fn live_stream_60s_has_no_parse_errors() {
    let host = std::env::var("BRISK_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let app = std::env::var("BRISK_APP").expect("BRISK_APP=<app id from `brisk-probe apps`>");
    let st = std::process::Command::new(env!("CARGO_BIN_EXE_brisk-probe"))
        .args(["stream", "--host", &host, "--app", &app, "--secs", "60"])
        .status()
        .unwrap();
    assert!(st.success(), "brisk-probe stream failed the M1 gate: {st}");
}
