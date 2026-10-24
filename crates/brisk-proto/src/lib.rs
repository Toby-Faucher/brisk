//! Sans-io Moonlight/GameStream protocol.

pub mod audio;
pub mod fec;
pub mod http;
pub mod pair;
pub mod rtp;
pub mod rtsp;
pub mod scrub;
pub mod sdp;

pub mod version {
    pub const SUNSHINE_APPVERSION: &str = "7.1.431.-1";
}

#[cfg(test)]
mod tests {
    #[test]
    fn sunshine_appversion() {
        assert_eq!(crate::version::SUNSHINE_APPVERSION, "7.1.431.-1");
    }
}
