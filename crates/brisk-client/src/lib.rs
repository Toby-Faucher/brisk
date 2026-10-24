//! Brisk client orchestration: identity, HTTP(S) session, RTSP handshake, control stream,
//! A/V stream receive, fixture recorder.

pub mod control;
pub mod identity;
pub mod recorder;
pub mod rtsp;
pub mod session;
pub mod stream;

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Result<T> = std::result::Result<T, Error>;

/// CSPRNG (OS `getrandom`) for pairing salt, challenge and secret.
pub struct SystemRng;
impl brisk_proto::pair::Rng for SystemRng {
    fn fill(&mut self, buf: &mut [u8]) {
        rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, buf)
    }
}
