use brisk_proto::tap::*;

/// One tap message laid out as the Sunshine patch's `brisk_tap_send` writes it (little-endian,
/// research/m2-tap-design.md section 2), with `extra` unknown header bytes before the payload.
#[expect(clippy::too_many_arguments)]
fn msg(
    flags: u8,
    connect_data: u32,
    codec: u32,
    idx: u64,
    capture: u64,
    tap: u64,
    dropped: u32,
    extra: usize,
    payload: &[u8],
) -> Vec<u8> {
    let mut m = b"BTAP".to_vec();
    m.push(1);
    m.push(flags);
    m.extend((44 + extra as u16).to_le_bytes());
    m.extend(connect_data.to_le_bytes());
    m.extend(codec.to_le_bytes());
    m.extend(idx.to_le_bytes());
    m.extend(capture.to_le_bytes());
    m.extend(tap.to_le_bytes());
    m.extend(dropped.to_le_bytes());
    m.extend(std::iter::repeat_n(0xEE, extra));
    m.extend(payload);
    m
}

const ANNEXB: &[u8] = &[0, 0, 0, 1, 0x67, 0x64, 0, 0x28, 0, 0, 1, 0x65, 0x88];

#[test]
fn parse_tap_roundtrip() {
    let m = msg(0b1001, 0x5348_4c5c, 0, 1234567, 111, 222, 3, 0, ANNEXB);
    let (h, p) = parse_tap(&m).unwrap();
    assert_eq!(
        h,
        TapHeader {
            flags: 0b1001,
            header_len: 44,
            connect_data: 0x5348_4c5c,
            codec: 0,
            frame_index: 1234567,
            capture_ns: 111,
            tap_ns: 222,
            dropped_total: 3,
        }
    );
    assert!(h.idr() && h.gap() && !h.after_rfi() && !h.no_capture_ts());
    assert_eq!(p, ANNEXB);
    let (h, _) = parse_tap(&msg(0b0110, 1, 0, 0, 0, 0, 0, 0, b"")).unwrap();
    assert!(!h.idr() && h.after_rfi() && h.no_capture_ts() && !h.gap());
}

#[test]
fn parse_tap_rejects_bad_magic() {
    let mut m = msg(0, 1, 0, 0, 0, 0, 0, 0, ANNEXB);
    m[3] = b'Q';
    assert_eq!(parse_tap(&m), Err(TapError::BadMagic));
    let mut m = msg(0, 1, 0, 0, 0, 0, 0, 0, ANNEXB);
    m[4] = 2;
    assert_eq!(parse_tap(&m), Err(TapError::BadVersion(2)));
}

#[test]
fn parse_tap_rejects_short_header_len() {
    let mut m = msg(0, 1, 0, 0, 0, 0, 0, 0, ANNEXB);
    m[6..8].copy_from_slice(&43u16.to_le_bytes());
    assert_eq!(parse_tap(&m), Err(TapError::BadHeaderLen(43)));
    // header_len past the end of the message
    let mut m = msg(0, 1, 0, 0, 0, 0, 0, 0, b"");
    m[6..8].copy_from_slice(&45u16.to_le_bytes());
    assert_eq!(parse_tap(&m), Err(TapError::BadHeaderLen(45)));
    // fewer than 44 bytes in all
    assert_eq!(parse_tap(&m[..43]), Err(TapError::Short(43)));
}

#[test]
fn parse_tap_rejects_oversize_payload() {
    let big = vec![0u8; MAX_PAYLOAD + 1];
    let m = msg(0, 1, 0, 0, 0, 0, 0, 0, &big);
    assert_eq!(parse_tap(&m), Err(TapError::TooLarge(MAX_PAYLOAD + 1)));
    let m = msg(0, 1, 0, 0, 0, 0, 0, 0, &big[..MAX_PAYLOAD]);
    assert_eq!(parse_tap(&m).unwrap().1.len(), MAX_PAYLOAD);
}

#[test]
fn parse_tap_skips_unknown_trailing_header_bytes() {
    let m = msg(1, 9, 0, 7, 1, 2, 0, 8, ANNEXB);
    let (h, p) = parse_tap(&m).unwrap();
    assert_eq!((h.header_len, h.connect_data, h.frame_index), (52, 9, 7));
    assert_eq!(p, ANNEXB);
}

/// Every recorded frame from the live tap (fixtures/tap, see its README) parses, carries one
/// session's connect data, H.264, and contiguous indices (counting the patch's own drops).
#[test]
fn recorded_tap_frames_parse() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/tap");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "bin"))
        .collect();
    files.sort();
    assert!(
        files.len() >= 30,
        "only {} fixtures in {dir:?}",
        files.len()
    );
    let mut prev: Option<TapHeader> = None;
    let mut idr = 0;
    for f in &files {
        let m = std::fs::read(f).unwrap();
        let (h, p) = parse_tap(&m).unwrap_or_else(|e| panic!("{f:?}: {e}"));
        assert_eq!(h.codec, 0, "{f:?}");
        assert!(
            p.starts_with(&[0, 0, 0, 1]) || p.starts_with(&[0, 0, 1]),
            "{f:?}"
        );
        assert!(h.capture_ns <= h.tap_ns || h.no_capture_ts(), "{f:?}");
        idr += h.idr() as u32;
        if let Some(q) = prev {
            assert_eq!(h.connect_data, q.connect_data, "{f:?}");
            let gap = h.frame_index - q.frame_index - 1;
            assert_eq!(gap, (h.dropped_total - q.dropped_total) as u64, "{f:?}");
        }
        prev = Some(h);
    }
    assert!(idr >= 1, "no IDR among the recorded frames");
}
