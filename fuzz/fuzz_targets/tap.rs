//! Tap messages from the patched Sunshine (unix SEQPACKET).
#![no_main]

use brisk_proto::tap::{HEADER_LEN, parse_tap};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|d: &[u8]| {
    if let Ok((h, p)) = parse_tap(d) {
        assert!(h.header_len as usize >= HEADER_LEN);
        assert_eq!(h.header_len as usize + p.len(), d.len());
        let _ = (h.idr(), h.after_rfi(), h.no_capture_ts(), h.gap());
    }
});
