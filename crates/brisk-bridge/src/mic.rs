//! Mic passthrough: the browser's microphone as a PipeWire source on the game host.
//!
//! The page sends 10 ms PCM packets as WebTransport datagrams: `kind u8` ([`KIND_MIC`]),
//! `seq u16` LE, then [`MIC_FRAMES`] samples of s16 LE, 48 kHz mono ([`MIC_PACKET_LEN`] bytes).
//! [`parse`] checks a packet, [`Order`] drops late or repeated ones, and [`MicOut`] pipes the PCM
//! into a `pw-cat` child that PipeWire shows as the source `brisk-mic`.
//!
//! A broken mic path never touches the stream: the first failure is logged once, then packets
//! are dropped, and `push` never blocks.

use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::time::Instant;

/// Datagram kind of a mic packet.
pub const KIND_MIC: u8 = 0x30;
/// Samples per packet (10 ms at 48 kHz).
pub const MIC_FRAMES: usize = 480;
/// `kind` + `seq` + `MIC_FRAMES` s16 samples.
pub const MIC_PACKET_LEN: usize = 3 + MIC_FRAMES * 2;

/// Packets queued for the writer (30 ms); more than this and `push` drops. With the 4 KiB stdin
/// pipe (about 43 ms) the mic lags by at most about 73 ms, however long the session runs.
const QUEUE: usize = 3;
/// No mic packet for this long means the viewer toggled the mic: the next seq starts a new run.
const IDLE_RESET: Duration = Duration::from_millis(200);
/// A seq this far behind the last one is taken as a page restart, not a late packet.
const RESTART_GAP: u16 = 100;

/// `(seq, pcm)` of a well-formed mic packet, else `None`.
pub fn parse(b: &[u8]) -> Option<(u16, &[u8])> {
    if b.len() != MIC_PACKET_LEN || b[0] != KIND_MIC {
        return None;
    }
    Some((u16::from_le_bytes([b[1], b[2]]), &b[3..]))
}

/// Drops packets that arrive late or twice (datagrams can reorder).
#[derive(Debug, Default)]
pub struct Order {
    last: Option<u16>,
    at: Option<Instant>,
}

impl Order {
    /// True if the packet with `seq` should be played. Updates the last seq when it is.
    pub fn fresh(&mut self, seq: u16) -> bool {
        self.fresh_at(seq, Instant::now())
    }

    /// [`fresh`](Self::fresh) with the arrival time; after [`IDLE_RESET`] of silence any seq is fresh.
    fn fresh_at(&mut self, seq: u16, now: Instant) -> bool {
        if self.at.is_some_and(|t| now.duration_since(t) >= IDLE_RESET) {
            self.last = None;
        }
        self.at = Some(now);
        let ok = match self.last {
            None => true,
            Some(last) => {
                let d = seq.wrapping_sub(last) as i16;
                d > 0 || d < -(RESTART_GAP as i16)
            }
        };
        if ok {
            self.last = Some(seq);
        }
        ok
    }
}

/// The `pw-cat` command that plays raw PCM on stdin as the source `brisk-mic`.
pub fn pw_cat() -> Command {
    let mut c = Command::new("pw-cat");
    c.args([
        "--playback",
        "--raw",
        "--format",
        "s16",
        "--rate",
        "48000",
        "--channels",
        "1",
    ])
    .args(["--latency", "20ms", "-P"])
    .arg(r#"{ media.class=Audio/Source node.name=brisk-mic node.description="Brisk mic" }"#)
    .arg("-")
    .stdin(std::process::Stdio::piped())
    .stdout(std::process::Stdio::null())
    .stderr(std::process::Stdio::inherit())
    .kill_on_drop(true);
    c
}

/// Plays the viewer's mic datagrams until the connection ends (dropping `MicOut` stops `pw-cat`).
pub async fn serve(s: std::sync::Arc<crate::wt::Session>) {
    let (mut order, mut out) = (Order::default(), MicOut::new(pw_cat()));
    while let Ok(d) = s.read_datagram().await {
        if let Some((seq, pcm)) = parse(&d)
            && order.fresh(seq)
        {
            out.push(pcm);
        }
    }
}

enum State {
    /// Not started; holds the command to spawn on the first push.
    Idle(Command),
    /// The child is kept here so dropping `MicOut` kills it (kill_on_drop), even if a write is stuck.
    #[allow(dead_code, reason = "held only so drop kills the child")]
    Running(mpsc::Sender<Vec<u8>>, tokio::process::Child),
    /// Failed once (already logged); every push is a no-op.
    Off,
}

/// Feeds PCM to a child process through a small queue and a writer task.
pub struct MicOut {
    state: State,
}

impl MicOut {
    /// Spawns nothing yet; the child starts on the first [`push`](Self::push).
    pub fn new(cmd: Command) -> Self {
        Self {
            state: State::Idle(cmd),
        }
    }

    /// Queues `pcm` for the child. Never blocks; drops when the queue is full or the mic is off.
    pub fn push(&mut self, pcm: &[u8]) {
        if let State::Idle(cmd) = &mut self.state {
            self.state = match start(cmd) {
                Ok(r) => State::Running(r.0, r.1),
                Err(e) => {
                    tracing::warn!("mic: cannot start pw-cat: {e}");
                    State::Off
                }
            };
        }
        if let State::Running(tx, _) = &self.state {
            // Full: drop the packet. Closed: the writer failed and already logged.
            let _ = tx.try_send(pcm.to_vec());
        }
    }
}

fn start(cmd: &mut Command) -> std::io::Result<(mpsc::Sender<Vec<u8>>, tokio::process::Child)> {
    let mut child = cmd
        .stdin(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let Some(mut stdin) = child.stdin.take() else {
        return Err(std::io::Error::other("no stdin"));
    };
    // A small pipe bounds the lag: a full 64 KiB pipe would hold about 680 ms. Failure is harmless.
    let _ = rustix::pipe::fcntl_setpipe_size(&stdin, 4096);
    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(QUEUE);
    tokio::spawn(async move {
        while let Some(pcm) = rx.recv().await {
            if let Err(e) = stdin.write_all(&pcm).await {
                tracing::warn!("mic: write to pw-cat failed: {e}");
                break;
            }
        }
    });
    println!("mic\tstarted");
    Ok((tx, child))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::process::Command;

    #[test]
    fn parse_rejects_bad_packets() {
        let mut p = vec![KIND_MIC, 7, 1];
        p.resize(MIC_PACKET_LEN, 0);
        assert_eq!(parse(&p).map(|(s, b)| (s, b.len())), Some((0x0107, 960)));
        assert!(parse(&p[..962]).is_none());
        let mut q = p.clone();
        q.push(0);
        assert!(parse(&q).is_none());
        let mut k = p.clone();
        k[0] = 0x31;
        assert!(parse(&k).is_none());
        assert!(parse(&[]).is_none());
    }

    #[test]
    fn order_drops_old_and_wraps() {
        let mut o = Order::default();
        assert!(o.fresh(65534));
        assert!(o.fresh(65535));
        assert!(o.fresh(0));
        assert!(o.fresh(2));
        assert!(!o.fresh(1));
        assert!(!o.fresh(2));
    }

    #[test]
    fn order_takes_a_restart() {
        let mut o = Order::default();
        assert!(o.fresh(30000));
        assert!(!o.fresh(29950));
        assert!(o.fresh(0));
    }

    #[test]
    fn order_resets_after_idle() {
        let t0 = Instant::now();
        let mut o = Order::default();
        assert!(o.fresh_at(50, t0));
        assert!(!o.fresh_at(0, t0 + Duration::from_millis(10)));
        let mut o = Order::default();
        assert!(o.fresh_at(50, t0));
        assert!(o.fresh_at(0, t0 + Duration::from_millis(300)));
    }

    #[tokio::test]
    async fn writes_pcm_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("out");
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(r#"cat > "$F""#).env("F", &file);
        let mut mic = MicOut::new(cmd);
        mic.push(&[1, 2]);
        mic.push(&[3, 4]);
        // Dropping MicOut kills the child, so wait until the bytes landed first.
        let start = Instant::now();
        while std::fs::read(&file).map_or(true, |b| b.len() < 4) {
            assert!(
                start.elapsed() < Duration::from_secs(2),
                "pcm never written"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        drop(mic);
        assert_eq!(std::fs::read(&file).unwrap(), [1, 2, 3, 4]);
    }

    #[tokio::test]
    async fn missing_program_disables_quietly() {
        let mut mic = MicOut::new(Command::new("/nonexistent/pw-cat"));
        mic.push(&[0; 960]);
        mic.push(&[0; 960]);
    }

    #[tokio::test]
    async fn full_queue_drops() {
        let mut cmd = Command::new("sleep");
        cmd.arg("5");
        let mut mic = MicOut::new(cmd);
        let start = Instant::now();
        for _ in 0..1000 {
            mic.push(&[0; 960]);
        }
        assert!(start.elapsed() < Duration::from_millis(100));
    }
}
