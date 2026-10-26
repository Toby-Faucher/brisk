//! Sans-io Moonlight/GameStream protocol: no sockets, clocks or threads here. Every module takes
//! bytes (or text) in and gives bytes or parsed values out; callers own the I/O. Every parser of
//! untrusted input is listed in `docs/fuzz-inventory.md` with its trust level and test coverage.
//!
//! - [`http`], [`pair`]: Sunshine's HTTP(S) XML replies and the PIN pairing handshake.
//! - [`rtsp`], [`sdp`]: session setup (RTSP framing and cipher, SDP build and attribute parsing).
//! - [`rtp`], [`fec`], [`audio`]: video/audio datagrams, Reed-Solomon recovery, frame assembly.
//! - [`enet`], [`control`], [`input`]: the control stream (ENet reliability, AES-GCM messages,
//!   input packets).
//! - [`tap`]: framing of the patched-Sunshine encoded-frame tap.
//! - [`scrub`]: blanking secrets out of recorded fixtures.

pub mod audio;
pub mod control;
pub mod enet;
pub mod fec;
pub mod http;
pub mod input;
pub mod pair;
pub mod rtp;
pub mod rtsp;
pub mod scrub;
pub mod sdp;
pub mod tap;

pub mod version {
    pub const SUNSHINE_APPVERSION: &str = "7.1.431.-1";
}
