//! brisk-probe: drive Sunshine by hand.
//!   brisk-probe [--record DIR] [--config DIR] pair --host HOST [--pin NNNN]
//!   brisk-probe [--record DIR] [--config DIR] apps --host HOST
//!   brisk-probe [--record DIR] [--config DIR] launch --host HOST --app ID
//!   brisk-probe [--record DIR] [--config DIR] cancel --host HOST
//!     cancel: explicit cleanup of a session left running; prints serverinfo state after.
//!     launch refuses a busy host unless --take-over (which cancels what is running first).
//!     launch: 1080p60 H.264, RTSP handshake, prints server ports, then cancels (also on Ctrl-C).
//! Identity lives in `<config>/identity/` (default `$XDG_CONFIG_HOME/brisk` or `~/.config/brisk`).

use brisk_client::identity::{load_identity, load_or_create_identity, ServerCert};
use brisk_client::recorder::Recorder;
use brisk_client::session::Session;
use brisk_client::SystemRng;
use brisk_proto::pair::Rng;
use brisk_proto::sdp::{Codec, StreamConfig};
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: brisk-probe [--record DIR] [--config DIR] (pair --host HOST [--pin NNNN] | apps --host HOST | launch --host HOST --app ID [--take-over] | cancel --host HOST)";

struct Args {
    cmd: String,
    host: String,
    pin: Option<String>,
    app: Option<u32>,
    take_over: bool,
    record: Option<PathBuf>,
    config: PathBuf,
}

fn parse_args() -> Result<Args, String> {
    let (mut cmd, mut host, mut pin, mut app, mut record, mut config) =
        (None, None, None, None, None, None);
    let mut take_over = false;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        let mut val = || it.next().ok_or(format!("{a} needs a value"));
        match a.as_str() {
            "--host" => host = Some(val()?),
            "--pin" => pin = Some(val()?),
            "--app" => app = Some(val()?.parse().map_err(|_| "--app must be a number")?),
            "--take-over" => take_over = true,
            "--record" => record = Some(PathBuf::from(val()?)),
            "--config" => config = Some(PathBuf::from(val()?)),
            "pair" | "apps" | "launch" | "cancel" if cmd.is_none() => cmd = Some(a),
            _ => return Err(format!("unexpected argument {a:?}")),
        }
    }
    if cmd.as_deref() == Some("launch") && app.is_none() {
        return Err("launch needs --app ID (see `apps`)".into());
    }
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
        record,
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
            s.pair(&dir, &ident, pin, Box::new(SystemRng))
                .await
                .map(|_| {
                    println!(
                        "paired; server cert pinned in {}",
                        dir.join("server.pem").display()
                    )
                })
        }
        "launch" => {
            async {
                let server = ServerCert::load(&dir)?;
                let ident = load_identity(&dir)?;
                let info = s.serverinfo(None).await?;
                let mut rikey = [0u8; 16];
                SystemRng.fill(&mut rikey);
                let mut id = [0u8; 4];
                SystemRng.fill(&mut id);
                let cfg = StreamConfig {
                    width: 1920,
                    height: 1080,
                    fps: 60,
                    bitrate_kbps: 20000,
                    codec: Codec::H264,
                    packet_size: 1392,
                    fec_percent: 20,
                    rikey,
                    // Sunshine parses rikeyid as a signed int; stay positive.
                    rikeyid: u32::from_le_bytes(id) & 0x7fff_ffff,
                    enc_flags: 0,
                };
                let app = a.app.unwrap();
                let r = tokio::select! {
                    r = s.launch_probe(&ident, &server, app, &info.app_version, &cfg, a.take_over) => Some(r),
                    _ = tokio::signal::ctrl_c() => None,
                };
                let Some(r) = r else {
                    if !s.launch_sent {
                        return Err(if a.take_over {
                            "interrupted before launch; take-over cancel may have been processed".into()
                        } else {
                            "interrupted before launch; nothing cancelled".into()
                        });
                    }
                    if s.launch_confirmed {
                        s.cancel_retry(&ident, &server).await?;
                    } else {
                        s.cancel_if_ours(&ident, &server, app).await?;
                    }
                    return Err("interrupted; session cancelled".into());
                };
                let p = r?;
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
            (Err(f), Ok(())) => return Err(format!("recorder finish failed: {f}").into()),
            (Err(f), Err(e)) => return Err(format!("{e}; also recorder finish failed: {f}").into()),
        }
    }
    r
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
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
