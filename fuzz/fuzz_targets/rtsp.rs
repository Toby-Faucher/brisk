//! RTSP replies/requests from Sunshine (plain and `rtspenc://` frames).
#![no_main]
#[path = "common.rs"]
mod common;

use brisk_proto::rtsp::{RtspCipher, parse_request, parse_response};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|d: &[u8]| {
    if let Ok(r) = parse_response(d) {
        assert!(r.body.len() <= d.len());
        let _ = (r.cseq(), r.session_id(), r.server_port());
    }
    if let Ok(r) = parse_request(d) {
        assert!(r.body.len() <= d.len());
    }
    // An arbitrary frame against the client opener, then the same bytes sealed by the host so
    // the plaintext parser sees them through the decryption path too.
    let _ = RtspCipher::client(common::KEY).open(d);
    // Not for empty input: `open` refuses a frame with no ciphertext (no RTSP message is empty),
    // although `seal` makes one. Left as is (docs/fuzz-inventory.md).
    if let Some(f) = (!d.is_empty())
        .then(|| RtspCipher::host(common::KEY).seal(d).ok())
        .flatten()
    {
        let pt = RtspCipher::client(common::KEY)
            .open(&f)
            .expect("own frame opens");
        assert_eq!(pt, d);
    }
});
