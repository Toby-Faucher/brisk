//! brisk-probe: drive Sunshine by hand.
//!   brisk-probe [--record DIR] [--config DIR] pair --host HOST [--pin NNNN]
//!   brisk-probe [--record DIR] [--config DIR] apps --host HOST
//!   brisk-probe [--record DIR] [--config DIR] launch --host HOST --app ID
//!   brisk-probe [--record DIR] [--config DIR] control --host HOST --app ID [--secs N] [--input-demo]
//!   brisk-probe [--record DIR] [--config DIR] stream --host HOST --app ID [--secs N] [--record-samples DIR] [--input-demo] [--tap]
//!   brisk-probe [--record DIR] [--config DIR] cancel --host HOST
//!     cancel: explicit cleanup of a session left running; prints serverinfo state after.
//!     launch refuses a busy host unless --take-over (which cancels what is running first).
//!     launch: 1080p60 H.264, RTSP handshake, prints server ports, then cancels (also on Ctrl-C).
//!     control: as launch, then the ENet control stream for N s (default 10) with an IDR request
//!       at N/2; prints counters, disconnects, then cancels. Same refuse/cancel rules as launch.
//!     stream: as control (default 60 s), receiving and assembling video and audio, IDR requests
//!       at N/3 and 2N/3; prints the summary block; exits non-zero on a failed M1 gate (after the
//!       cancel). --record-samples DIR writes the first video/audio datagrams for the replay tests.
//!     --tap (stream): announce `a=x-brisk.tap:1` and listen on $BRISK_TAP_SOCK or
//!       $XDG_RUNTIME_DIR/brisk/tap.sock; a brisk-patched Sunshine sends the video there (tap gate:
//!       contiguous idx, IDR answered, 0 RTP video datagrams), otherwise RTP is used as without
//!       --tap. With --record-samples DIR the first 60 tap messages go to DIR/tap/.
//!     --input-demo (control, stream): at N/4 (control) or N/6 (stream) send a harmless input
//!       sequence: mouse +5/-5 px, Shift down/up, gamepad 0 neutral attach/detach. No clicks.
//! Identity lives in `<config>/identity/` (default `$XDG_CONFIG_HOME/brisk` or `~/.config/brisk`).

use brisk_client::control::{ControlSession, Stats};
use brisk_client::identity::{load_identity, load_or_create_identity, ServerCert};
use brisk_client::recorder::Recorder;
use brisk_client::rtsp::StreamPorts;
use brisk_client::session::Session;
use brisk_client::stream::{media_setup, stream, stream_tap, Outcome};
use brisk_client::tap::{tap_path, TapListener};
use brisk_client::{Error, SystemRng};
use brisk_proto::pair::Rng;
use brisk_proto::sdp::{Codec, StreamConfig, SS_ENC_AUDIO};
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: brisk-probe [--record DIR] [--config DIR] (pair --host HOST [--pin NNNN] | apps --host HOST | launch --host HOST --app ID [--take-over] | control --host HOST --app ID [--secs N] [--input-demo] [--take-over] | stream --host HOST --app ID [--secs N] [--record-samples DIR] [--input-demo] [--tap] [--take-over] | cancel --host HOST)";

struct Args {
    cmd: String,
    host: String,
    pin: Option<String>,
    app: Option<u32>,
    take_over: bool,
    input_demo: bool,
    tap: bool,
    secs: u64,
    record: Option<PathBuf>,
    samples: Option<PathBuf>,
    config: PathBuf,
}

fn check_secs(stream: bool, secs: u64) -> Result<(), String> {
    if !(1..=3600).contains(&secs) {
        return Err("--secs must be 1..=3600".into());
    }
    if stream && secs < 10 {
        return Err("--secs must be >= 10 for stream: the 2 IDR requests at secs/3 and 2secs/3 each need 2 s to be answered".into());
    }
    Ok(())
}

fn parse_args() -> Result<Args, String> {
    let (mut cmd, mut host, mut pin, mut app, mut record, mut config) =
        (None, None, None, None, None, None);
    let mut take_over = false;
    let mut input_demo = false;
    let mut tap = false;
    let mut secs = None;
    let mut samples = None;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        let mut val = || it.next().ok_or(format!("{a} needs a value"));
        match a.as_str() {
            "--host" => host = Some(val()?),
            "--pin" => pin = Some(val()?),
            "--app" => app = Some(val()?.parse().map_err(|_| "--app must be a number")?),
            "--take-over" => take_over = true,
            "--input-demo" => input_demo = true,
            "--tap" => tap = true,
            "--secs" => secs = Some(val()?.parse().map_err(|_| "--secs must be a number")?),
            "--record-samples" => samples = Some(PathBuf::from(val()?)),
            "--record" => record = Some(PathBuf::from(val()?)),
            "--config" => config = Some(PathBuf::from(val()?)),
            "pair" | "apps" | "launch" | "control" | "stream" | "cancel" if cmd.is_none() => {
                cmd = Some(a)
            }
            _ => return Err(format!("unexpected argument {a:?}")),
        }
    }
    if matches!(cmd.as_deref(), Some("launch" | "control" | "stream")) && app.is_none() {
        return Err("launch/control/stream need --app ID (see `apps`)".into());
    }
    if samples.is_some() && cmd.as_deref() != Some("stream") {
        return Err("--record-samples is only for stream".into());
    }
    if input_demo && !matches!(cmd.as_deref(), Some("control" | "stream")) {
        return Err("--input-demo is only for control and stream".into());
    }
    if tap && cmd.as_deref() != Some("stream") {
        return Err("--tap is only for stream".into());
    }
    if tap && input_demo {
        return Err("--tap and --input-demo do not combine (yet)".into());
    }
    let secs = secs.unwrap_or(if cmd.as_deref() == Some("stream") {
        60
    } else {
        10
    });
    check_secs(cmd.as_deref() == Some("stream"), secs)?;
    if let Some(p) = &pin {
        if p.len() != 4 || !p.bytes().all(|b| b.is_ascii_digit()) {
            return Err("--pin must be 4 digits".into());
        }
    }
    let config = config
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(|d| PathBuf::from(d).join("brisk")))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config/brisk")))
        .ok_or("no --config, XDG_CONFIG_HOME or HOME")?;
    Ok(Args {
        cmd: cmd.ok_or("missing command")?,
        host: host.ok_or("missing --host")?,
        pin,
        app,
        take_over,
        input_demo,
        tap,
        secs,
        record,
        samples,
        config,
    })
}

/// Uniform 4-digit PIN from the OS CSPRNG.
fn random_pin() -> String {
    loop {
        let mut b = [0u8; 2];
        SystemRng.fill(&mut b);
        let v = u16::from_le_bytes(b);
        if v < 60000 {
            return format!("{:04}", v % 10000);
        }
    }
}

async fn run(a: Args) -> brisk_client::Result<()> {
    let dir = a.config.join("identity");
    let mut s = Session::new(&a.host);
    s.recorder = a.record.as_deref().map(Recorder::new);
    // Only `pair` has a PIN; `apps` must not scrub a made-up one out of recordings.
    let pin = (a.cmd == "pair").then(|| a.pin.clone().unwrap_or_else(random_pin));
    let r = match a.cmd.as_str() {
        "pair" => {
            let ident = load_or_create_identity(&dir)?;
            let pin = pin.as_deref().unwrap();
            println!("PIN {pin}: enter it in Sunshine's web UI (PIN tab). Waiting up to 5 min...");
            s.pair(&dir, &ident, pin).await.map(|_| {
                println!(
                    "paired; server cert pinned in {}",
                    dir.join("server.pem").display()
                )
            })
        }
        "launch" | "control" | "stream" => {
            async {
                let server = ServerCert::load(&dir)?;
                let ident = load_identity(&dir)?;
                let info = s.serverinfo(None).await?;
                let cfg = StreamConfig {
                    width: 1920,
                    height: 1080,
                    fps: 60,
                    bitrate_kbps: 20000,
                    codec: Codec::H264,
                    packet_size: 1392,
                    fec_percent: 20,
                    // stream asks for audio encryption, as Moonlight does by default, so the audio
                    // path (and the recorded samples) are the AES-CBC one.
                    enc_flags: if a.cmd == "stream" { SS_ENC_AUDIO } else { 0 },
                    tap: a.tap,
                };
                // Bound before the launch: Sunshine connects right after the first video ping.
                let listener = match a.tap {
                    true => {
                        let path = tap_path().ok_or(Error::Setup(
                            "--tap: no BRISK_TAP_SOCK or XDG_RUNTIME_DIR".into(),
                        ))?;
                        println!("tap\tlistening on {}", path.display());
                        Some(TapListener::bind(&path)?)
                    }
                    false => None,
                };
                let listener = listener.as_ref();
                let app = a.app.unwrap();
                let (host, secs, cmd, demo) = (a.host.clone(), a.secs, a.cmd.clone(), a.input_demo);
                let samples = a.samples.clone();
                let during = |p: StreamPorts| async move {
                    let stats = match cmd.as_str() {
                        "control" => Some(run_control(&host, &p, secs, demo).await?),
                        "stream" => {
                            let mut id = [0u8; 4];
                            SystemRng.fill(&mut id);
                            let secs = std::time::Duration::from_secs(secs);
                            let id = u32::from_le_bytes(id);
                            Some(match listener {
                                Some(l) => match stream_tap(&host, &p, id, secs, samples.as_deref(), l).await? {
                                    Outcome::Tap(s) => s.control,
                                    Outcome::Rtp(s) => s.control,
                                },
                                None => stream(&host, &p, id, secs, samples.as_deref(), demo).await?.control,
                            })
                        }
                        _ => None,
                    };
                    Ok((p, stats))
                };
                let stop = interrupted()?;
                let r = tokio::select! {
                    r = s.launch_session(&ident, &server, app, &info.app_version, &cfg, a.take_over, during) => Ok(r),
                    sig = stop => Err(sig),
                };
                let r = match r {
                    Ok(r) => r,
                    Err(sig) => {
                    eprintln!("{sig}: stopping");
                    if !s.launch_sent() {
                        return Err(Error::Session(if a.take_over {
                            "interrupted before launch; take-over cancel may have been processed".into()
                        } else {
                            "interrupted before launch; nothing cancelled".into()
                        }));
                    }
                    // The handlers stay installed, so a second signal would be swallowed while the
                    // cancel retries (45 s and more): let it exit at once instead.
                    let second = interrupted()?;
                    let confirmed = s.launch_confirmed();
                    let cancel = async {
                        if confirmed {
                            s.cancel_retry(&ident, &server).await
                        } else {
                            s.cancel_if_ours(&ident, &server, app).await
                        }
                    };
                    match unless_signalled(second, cancel).await {
                        Some(c) => c?,
                        None => {
                            eprintln!(
                                "second signal: exiting without waiting for cancel; run `brisk-probe cancel --host {}`",
                                a.host
                            );
                            std::process::exit(130);
                        }
                    }
                    return Err(Error::Session(format!("interrupted ({sig}); session cancelled")));
                    }
                };
                let (p, stats) = r?;
                if let Some(st) = stats {
                    print_stats(&st);
                }
                println!(
                    "audio\t{}\nvideo\t{}\ncontrol\t{}",
                    p.audio, p.video, p.control
                );
                println!("session\t{}\nencryption\t{:#x}", p.session_id, p.enc_flags);
                println!("cancelled");
                Ok(())
            }
            .await
        }
        "cancel" => {
            async {
                let server = ServerCert::load(&dir)?;
                let ident = load_identity(&dir)?;
                s.serverinfo(None).await?;
                let c = s.cancel_retry(&ident, &server).await;
                if c.is_ok() {
                    println!("cancelled");
                }
                match s.serverinfo(Some((&ident, &server))).await {
                    Ok(i) => println!("state\t{}\ncurrentgame\t{}", i.server_state, i.current_game),
                    Err(e) => eprintln!("serverinfo after cancel failed: {e}"),
                }
                c
            }
            .await
        }
        _ => {
            async {
                let server = ServerCert::load(&dir)?;
                let ident = load_identity(&dir)?;
                s.serverinfo(None).await?;
                for app in s.applist(&ident, &server).await? {
                    println!("{}\t{}", app.id, app.title);
                }
                Ok(())
            }
            .await
        }
    };
    if let Some(mut rec) = s.recorder.take() {
        if let Some(pin) = &pin {
            rec.set_pin(pin);
        }
        match (rec.finish(), r) {
            (Ok(f), r) => {
                eprintln!(
                    "recorded {} scrubbed files in {}",
                    f.len(),
                    a.record.unwrap().display()
                );
                return r;
            }
            (Err(f), Ok(())) => return Err(f.context("recorder finish failed")),
            (Err(f), Err(e)) => return Err(e.then(f.context("also recorder finish failed"))),
        }
    }
    r
}

/// ENet control stream for `secs`, IDR request at the midpoint, [`ControlSession::input_demo`]
/// at a quarter with `input_demo`, graceful disconnect.
async fn run_control(
    host: &str,
    p: &StreamPorts,
    secs: u64,
    input_demo: bool,
) -> brisk_client::Result<Stats> {
    let (addr, cd, av) = media_setup(host, p).await?;
    let mut id = [0u8; 4];
    SystemRng.fill(&mut id);
    println!(
        "control\tA/V pings to video {} audio {}; connecting {addr}",
        p.video, p.audio
    );
    let mut c =
        ControlSession::connect(addr, cd, p.key.rikey, u32::from_le_bytes(id), Some(av)).await?;
    println!("control\tconnected; IDR request + Start B sent");
    let start = std::time::Instant::now();
    let total = std::time::Duration::from_secs(secs);
    let r = async {
        if input_demo {
            c.run_until(start + total / 4).await?;
            if c.is_open() {
                let n = c.input_demo(&mut |_, _, _| false).await?;
                println!(
                    "input_demo\tsent {n} packets at {:.1}s (mouse +5/-5, Shift down/up, gamepad 0 neutral attach/detach)",
                    start.elapsed().as_secs_f32()
                );
            }
        }
        c.run_until(start + total / 2).await?;
        if c.is_open() {
            c.request_idr().await?;
            println!(
                "control\tIDR request at {:.1}s",
                start.elapsed().as_secs_f32()
            );
        }
        c.run_until(start + total).await
    }
    .await;
    if let Err(e) = r {
        print_stats(&c.stats);
        return Err(e.context(format!(
            "control phase failed after {:.1}s",
            start.elapsed().as_secs_f32()
        )));
    }
    c.disconnect().await
}

/// Resolves on Ctrl-C, SIGTERM or SIGHUP with the signal's name. The handlers are installed
/// before this returns, so a signal right after cannot kill the process before the cancel.
fn interrupted() -> std::io::Result<impl std::future::Future<Output = &'static str>> {
    use tokio::signal::unix::{signal, SignalKind};
    let mut term = signal(SignalKind::terminate())?;
    let mut hup = signal(SignalKind::hangup())?;
    let mut int = signal(SignalKind::interrupt())?;
    Ok(async move {
        tokio::select! {
            _ = int.recv() => "SIGINT",
            _ = term.recv() => "SIGTERM",
            _ = hup.recv() => "SIGHUP",
        }
    })
}

/// `f`'s output, or `None` if `stop` fires first.
async fn unless_signalled<T>(
    stop: impl std::future::Future<Output = &'static str>,
    f: impl std::future::Future<Output = T>,
) -> Option<T> {
    tokio::select! {
        v = f => Some(v),
        _ = stop => None,
    }
}

fn print_stats(st: &Stats) {
    println!("pings\tsent {} acked {}", st.pings_sent, st.pings_acked);
    println!(
        "idr_requests\t{} ({} on frame loss)",
        st.idr_sent, st.idr_on_loss
    );
    for (t, n) in &st.host_msgs {
        println!("host_msg\t{t:#06x}\t{n}");
    }
    println!("inputs_sent\t{}", st.inputs_sent);
    println!("bad_host_msgs\t{}", st.bad_msgs);
    match st.termination {
        Some(ec) => println!("termination\t{ec:#010x}"),
        None => println!("termination\tnone"),
    }
    println!("host_disconnected\t{}", st.host_disconnected);
    for (i, k) in ["video", "audio"].iter().enumerate() {
        println!(
            "{k}\tpings sent {} rx packets {} rx bytes {}",
            st.av_pings_sent[i], st.media_rx_packets[i], st.media_rx_bytes[i]
        );
    }
}

/// The library logs through `tracing`; this prints warnings and errors to stderr, nothing else.
struct StderrLog;

impl tracing::Subscriber for StderrLog {
    fn enabled(&self, m: &tracing::Metadata<'_>) -> bool {
        *m.level() <= tracing::Level::WARN
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1) // spans are not used
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, e: &tracing::Event<'_>) {
        struct Msg(String);
        impl tracing::field::Visit for Msg {
            fn record_debug(&mut self, f: &tracing::field::Field, v: &dyn std::fmt::Debug) {
                if f.name() == "message" {
                    self.0 = format!("{v:?}");
                }
            }
        }
        let mut m = Msg(String::new());
        e.record(&mut m);
        eprintln!("{}: {}", e.metadata().level(), m.0);
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let _ = tracing::subscriber::set_global_default(StderrLog);
    let a = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match run(a).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{check_secs, interrupted, unless_signalled};

    /// A signal during the (slow) cancel wins over it; without one the cancel's result comes back.
    #[tokio::test]
    async fn second_signal_cuts_the_cancel_short() {
        let stop = interrupted().unwrap();
        let pid = std::process::id().to_string();
        let hung_cancel = std::future::pending::<()>();
        let killer = async {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            std::process::Command::new("kill")
                .args(["-TERM", &pid])
                .status()
                .unwrap();
            std::future::pending::<()>().await
        };
        let got = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            tokio::select! {
                r = unless_signalled(stop, hung_cancel) => r,
                _ = killer => unreachable!(),
            }
        })
        .await;
        assert_eq!(got, Ok(None));
        let quiet = interrupted().unwrap();
        assert_eq!(unless_signalled(quiet, async { 7 }).await, Some(7));
    }

    /// SIGTERM and SIGHUP stop the run like Ctrl-C (and no longer kill the process outright).
    #[tokio::test]
    async fn sigterm_and_sighup_are_caught() {
        for (flag, name) in [("-TERM", "SIGTERM"), ("-HUP", "SIGHUP")] {
            let stop = interrupted().unwrap();
            let pid = std::process::id().to_string();
            assert!(std::process::Command::new("kill")
                .args([flag, &pid])
                .status()
                .unwrap()
                .success());
            let got = tokio::time::timeout(std::time::Duration::from_secs(5), stop).await;
            assert_eq!(got, Ok(name));
        }
    }

    #[test]
    fn stream_needs_at_least_10_secs() {
        assert!(check_secs(true, 9).unwrap_err().contains(">= 10"));
        assert!(check_secs(true, 10).is_ok());
        assert!(check_secs(false, 1).is_ok());
        assert!(check_secs(false, 3601).is_err());
    }
}
