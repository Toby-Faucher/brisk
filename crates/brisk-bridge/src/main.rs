//! brisk-bridge: launches a Sunshine session with the brisk tap and forwards its H.264 or HEVC frames
//! to a browser over WebTransport (one uni stream per frame, see `session.rs`).
//!
//!   brisk-bridge --host HOST --app ID --listen IP:PORT --cert PEM --key PEM
//!                [--config DIR] [--secs N] [--page URL] [--take-over] [--codec h264|hevc]
//!
//! --listen must be a Tailscale or loopback IP (not 0.0.0.0). --cert/--key: an ECDSA P-256
//! certificate valid for at most 14 days (Chrome's rule for `serverCertificateHashes`; see
//! scripts/brisk-up.sh). The bridge prints the page URL with the session token and the
//! certificate hash. --secs stops after N seconds (default: until SIGINT/SIGTERM/SIGHUP or the
//! host ends the session). At exit it prints the latency report and the M2 gate.

use brisk_bridge::session::{AudioRx, Report, forward};
use brisk_bridge::{input, wt};
use brisk_client::control::ControlSession;
use brisk_client::identity::{ServerCert, load_identity};
use brisk_client::rtsp::StreamPorts;
use brisk_client::session::Session;
use brisk_client::stream::{StreamRx, media_setup, race};
use brisk_client::tap::{TapListener, tap_path};
use brisk_client::{Error, StderrLog, SystemRng};
use brisk_proto::pair::Rng;
use brisk_proto::sdp::{Codec, SS_ENC_AUDIO, StreamConfig};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use sha2::{Digest, Sha256};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

/// Audio datagrams waiting for the forward loop (64 = 160 ms of packets plus parity).
const AUDIO_QUEUE: usize = 64;

const USAGE: &str = "usage: brisk-bridge --host HOST --app ID --listen IP:PORT --cert PEM --key PEM [--config DIR] [--secs N] [--page URL] [--take-over] [--width N] [--height N] [--fps N] [--bitrate-kbps N] [--codec h264|hevc]";

struct Args {
    host: String,
    app: u32,
    listen: SocketAddr,
    cert: PathBuf,
    key: PathBuf,
    config: PathBuf,
    secs: Option<u64>,
    page: String,
    take_over: bool,
    width: u16,
    height: u16,
    fps: u16,
    bitrate_kbps: u32,
    codec: Codec,
}

fn parse_args() -> Result<Args, String> {
    parse_from(std::env::args().skip(1))
}

fn parse_from(args: impl Iterator<Item = String>) -> Result<Args, String> {
    let (mut host, mut app, mut listen, mut cert, mut key, mut config, mut secs) =
        (None, None, None, None, None, None, None);
    let mut page = "http://localhost:8443/".to_owned();
    let mut take_over = false;
    let (mut width, mut height, mut fps, mut bitrate_kbps) = (1920, 1080, 60, 20000);
    let mut codec = Codec::H264;
    let mut it = args;
    while let Some(a) = it.next() {
        let mut val = || it.next().ok_or(format!("{a} needs a value"));
        match a.as_str() {
            "--host" => host = Some(val()?),
            "--app" => app = Some(val()?.parse().map_err(|_| "--app must be a number")?),
            "--listen" => listen = Some(val()?.parse().map_err(|_| "--listen must be IP:PORT")?),
            "--cert" => cert = Some(PathBuf::from(val()?)),
            "--key" => key = Some(PathBuf::from(val()?)),
            "--config" => config = Some(PathBuf::from(val()?)),
            "--secs" => secs = Some(val()?.parse().map_err(|_| "--secs must be a number")?),
            "--page" => page = val()?,
            "--take-over" => take_over = true,
            "--width" => width = ranged(&a, val()?, 320, 3840)? as u16,
            "--height" => height = ranged(&a, val()?, 320, 3840)? as u16,
            "--fps" => fps = ranged(&a, val()?, 24, 144)? as u16,
            "--bitrate-kbps" => bitrate_kbps = ranged(&a, val()?, 1000, 150000)?,
            "--codec" => {
                codec = match val()?.as_str() {
                    "h264" => Codec::H264,
                    "hevc" => Codec::Hevc,
                    _ => return Err("--codec must be h264 or hevc".into()),
                }
            }
            _ => return Err(format!("unexpected argument {a:?}")),
        }
    }
    let config = config
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(|d| PathBuf::from(d).join("brisk")))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config/brisk")))
        .ok_or("no --config, XDG_CONFIG_HOME or HOME")?;
    Ok(Args {
        host: host.ok_or("missing --host")?,
        app: app.ok_or("missing --app")?,
        listen: listen.ok_or("missing --listen")?,
        cert: cert.ok_or("missing --cert")?,
        key: key.ok_or("missing --key")?,
        config,
        secs,
        page,
        take_over,
        width,
        height,
        fps,
        bitrate_kbps,
        codec,
    })
}

fn ranged(flag: &str, v: String, lo: u32, hi: u32) -> Result<u32, String> {
    v.parse()
        .ok()
        .filter(|n| (lo..=hi).contains(n))
        .ok_or(format!("{flag} must be a number in {lo}..={hi}"))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Resolves on SIGINT, SIGTERM or SIGHUP with the signal's name. Installed before the launch, so
/// a signal during it is handled (once the session runs) instead of killing the process before
/// the cancel.
fn interrupted() -> std::io::Result<impl std::future::Future<Output = &'static str>> {
    use tokio::signal::unix::{SignalKind, signal};
    let (mut t, mut h, mut i) = (
        signal(SignalKind::terminate())?,
        signal(SignalKind::hangup())?,
        signal(SignalKind::interrupt())?,
    );
    Ok(async move {
        tokio::select! {
            _ = i.recv() => "SIGINT",
            _ = t.recv() => "SIGTERM",
            _ = h.recv() => "SIGHUP",
        }
    })
}

async fn run(a: Args) -> brisk_client::Result<()> {
    let stop = interrupted()?;
    let setup = |e: String| Error::Setup(e);
    let certs: Vec<CertificateDer<'static>> = CertificateDer::pem_file_iter(&a.cert)
        .and_then(|i| i.collect())
        .map_err(|e| setup(format!("--cert {}: {e}", a.cert.display())))?;
    let key = PrivateKeyDer::from_pem_file(&a.key)
        .map_err(|e| setup(format!("--key {}: {e}", a.key.display())))?;
    let hash = hex(&Sha256::digest(
        certs
            .first()
            .ok_or(setup("--cert has no certificate".into()))?,
    ));
    let ep = wt::endpoint(a.listen, certs, key)?;
    let mut t = [0u8; 16];
    SystemRng.fill(&mut t);
    let token = hex(&t);
    let wt_url = format!("https://{}/{token}", a.listen);
    println!("listening\t{} (udp)", a.listen);
    println!("page\t{}?wt={wt_url}&hash={hash}", a.page);

    let dir = a.config.join("identity");
    let server = ServerCert::load(&dir)?;
    let ident = load_identity(&dir)?;
    let mut s = Session::new(&a.host);
    let info = s.serverinfo(None).await?;
    // H.264 is not checked, so its path stays as it was.
    if a.codec == Codec::Hevc && !info.supports(Codec::Hevc) {
        return Err(setup(format!(
            "Sunshine reports no HEVC support on this GPU (ServerCodecModeSupport {:#x})",
            info.codec_mode_support
        )));
    }
    let cfg = StreamConfig {
        width: a.width,
        height: a.height,
        fps: a.fps,
        bitrate_kbps: a.bitrate_kbps,
        codec: a.codec,
        packet_size: 1392,
        fec_percent: 20,
        enc_flags: SS_ENC_AUDIO,
        tap: true,
    };
    let path = tap_path().ok_or(setup("no BRISK_TAP_SOCK or XDG_RUNTIME_DIR".into()))?;
    // Bound before the launch: Sunshine connects right after the first video ping.
    let listener = TapListener::bind(&path)?;
    let mut report = Report::default();
    let start = Instant::now();
    let deadline = start
        + a.secs
            .map_or(Duration::from_secs(365 * 86400), Duration::from_secs);
    let (host, ep, token, rep) = (a.host.clone(), &ep, &token, &mut report);
    let during = |p: StreamPorts| async move {
        let mut id = [0u8; 4];
        SystemRng.fill(&mut id);
        let (addr, cd, av) = media_setup(&host, &p).await?;
        let mut c =
            ControlSession::connect(addr, cd, p.key.rikey, u32::from_le_bytes(id), Some(av))
                .await?;
        let r = async {
            let mut rx = StreamRx::new(p.enc_flags, p.key.rikey, p.key.rikeyid, None)?;
            let Some(mut tap) = race(&mut c, &listener, cd, cfg.codec, &mut rx).await? else {
                return Err(Error::Setup(
                    "Sunshine sent RTP, not the tap: is the brisk build running? (scripts/sunshine-brisk.sh status)".into(),
                ));
            };
            println!("tap\tconnected at {:.1}s", start.elapsed().as_secs_f32());
            let h = c.handle();
            let inp = std::sync::Arc::new(input::Shared::default());
            c.on_feedback({
                let inp = inp.clone();
                move |m| inp.rumble(m) // to the viewer's input stream (input::write_rumble)
            });
            // Audio stays on Sunshine's RTP path; RTP video must stay at 0 in tap mode.
            let (atx, mut arx_q) = tokio::sync::mpsc::channel(AUDIO_QUEUE);
            let mut queue_full = 0;
            let mut sink = |i: usize, d: &[u8], _: Instant| {
                if i == 1 && atx.try_send(bytes::Bytes::copy_from_slice(d)).is_err() {
                    queue_full += 1;
                }
                false
            };
            let run = c.run_until_with(deadline, &mut sink);
            let mut arx = AudioRx::new(p.enc_flags, p.key.rikey, p.key.rikeyid);
            let r = tokio::select! {
                r = run => r,
                r = forward(&mut tap, ep, token, &h, &mut arx_q, &mut arx, &inp, rep) => r,
                r = input::send_loop(&inp, |ev| h.send_input(ev)) => r,
                sig = stop => {
                    println!("{sig}: stopping");
                    Ok(())
                }
            };
            rep.audio.queue_full = queue_full;
            // No stuck keys on the host: release what the browser still holds (Sunshine also
            // resets its input state when the session ends, input.cpp `reset`).
            for ev in inp.with(input::InputQueue::shutdown) {
                if !c.is_open() || c.send_input(&ev).await.is_err() {
                    break;
                }
            }
            rep.input = inp.with(|q| q.stats);
            r
        }
        .await;
        let r = r.and_then(|()| match c.is_open() {
            true => Ok(()),
            false => Err(Error::Session(format!(
                "host ended the session: {:?}",
                c.stats.termination
            ))),
        });
        let d = c.disconnect().await;
        r?;
        d.map(|_| ())
    };
    let r = s
        .launch_session(
            &ident,
            &server,
            a.app,
            &info.app_version,
            &cfg,
            a.take_over,
            during,
        )
        .await;
    println!("{report}");
    if s.launch_confirmed() {
        println!("session\tended after {:.1}s", start.elapsed().as_secs_f32());
    }
    r
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
    use super::*;

    fn parse(extra: &[&str]) -> Result<Args, String> {
        let base = "--host h --app 1 --listen 127.0.0.1:4433 --cert c --key k --config /x";
        let v = base
            .split(' ')
            .chain(extra.iter().copied())
            .map(String::from);
        parse_from(v.collect::<Vec<_>>().into_iter())
    }

    #[test]
    fn parses_quality_flags() {
        let a = parse(&[
            "--width",
            "2560",
            "--height",
            "1440",
            "--fps",
            "120",
            "--bitrate-kbps",
            "40000",
        ])
        .unwrap();
        assert_eq!(
            (a.width, a.height, a.fps, a.bitrate_kbps),
            (2560, 1440, 120, 40000)
        );
    }

    #[test]
    fn rejects_out_of_range_fps() {
        assert!(parse(&["--fps", "500"]).is_err());
        assert!(parse(&["--bitrate-kbps", "999"]).is_err());
        assert!(parse(&["--width", "abc"]).is_err());
    }

    #[test]
    fn codec_flag_defaults_to_h264() {
        assert_eq!(parse(&[]).unwrap().codec, Codec::H264);
        assert_eq!(parse(&["--codec", "h264"]).unwrap().codec, Codec::H264);
        assert_eq!(parse(&["--codec", "hevc"]).unwrap().codec, Codec::Hevc);
        assert!(parse(&["--codec", "av1"]).is_err());
        assert!(parse(&["--codec"]).is_err());
    }

    #[test]
    fn defaults_are_1080p60_20mbps() {
        let a = parse(&[]).unwrap();
        assert_eq!(
            (a.width, a.height, a.fps, a.bitrate_kbps),
            (1920, 1080, 60, 20000)
        );
    }
}
