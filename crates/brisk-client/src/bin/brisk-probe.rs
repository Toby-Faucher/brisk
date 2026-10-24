//! brisk-probe: drive Sunshine by hand.
//!   brisk-probe [--record DIR] [--config DIR] pair --host HOST [--pin NNNN]
//!   brisk-probe [--record DIR] [--config DIR] apps --host HOST
//! Identity lives in `<config>/identity/` (default `$XDG_CONFIG_HOME/brisk` or `~/.config/brisk`).

use brisk_client::identity::{load_or_create_identity, ServerCert};
use brisk_client::recorder::Recorder;
use brisk_client::session::Session;
use brisk_client::SystemRng;
use brisk_proto::pair::Rng;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: brisk-probe [--record DIR] [--config DIR] (pair --host HOST [--pin NNNN] | apps --host HOST)";

struct Args {
    cmd: String,
    host: String,
    pin: Option<String>,
    record: Option<PathBuf>,
    config: PathBuf,
}

fn parse_args() -> Result<Args, String> {
    let (mut cmd, mut host, mut pin, mut record, mut config) = (None, None, None, None, None);
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        let mut val = || it.next().ok_or(format!("{a} needs a value"));
        match a.as_str() {
            "--host" => host = Some(val()?),
            "--pin" => pin = Some(val()?),
            "--record" => record = Some(PathBuf::from(val()?)),
            "--config" => config = Some(PathBuf::from(val()?)),
            "pair" | "apps" if cmd.is_none() => cmd = Some(a),
            _ => return Err(format!("unexpected argument {a:?}")),
        }
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
    let pin = a.pin.clone().unwrap_or_else(random_pin);
    let r = match a.cmd.as_str() {
        "pair" => {
            let ident = load_or_create_identity(&dir)?;
            println!("PIN {pin}: enter it in Sunshine's web UI (PIN tab). Waiting up to 5 min...");
            s.pair(&dir, &ident, &pin, Box::new(SystemRng))
                .await
                .map(|_| {
                    println!(
                        "paired; server cert pinned in {}",
                        dir.join("server.pem").display()
                    )
                })
        }
        _ => {
            async {
                let server = ServerCert::load(&dir)?;
                let ident = load_or_create_identity(&dir)?;
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
        rec.set_pin(&pin);
        let n = rec.finish()?.len();
        eprintln!(
            "recorded {n} scrubbed files in {}",
            a.record.unwrap().display()
        );
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
