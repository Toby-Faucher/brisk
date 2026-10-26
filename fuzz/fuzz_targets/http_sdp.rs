//! Sunshine HTTP XML replies, DESCRIBE SDP attributes and the launch session URL.
#![no_main]

use brisk_proto::http::{check_status, parse_applist, parse_field, parse_launch, parse_serverinfo};
use brisk_proto::pair::unhex;
use brisk_proto::sdp::sdp_attr;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|d: &[u8]| {
    let s = String::from_utf8_lossy(d);
    let _ = check_status(&s);
    let _ = parse_field(&s, "paired");
    let _ = parse_serverinfo(&s);
    if let Ok(apps) = parse_applist(&s) {
        assert!(apps.len() <= s.len());
    }
    let _ = parse_launch(&s);
    let _ = sdp_attr(&s, "x-ss-general.encryptionSupported");
    let _ = brisk_client::rtsp::parse_session_url(&s);
    if let Ok(b) = unhex(&s) {
        assert_eq!(b.len() * 2, s.len());
    }
});
