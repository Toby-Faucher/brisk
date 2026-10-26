//! Property tests: every parser that reads attacker-influenced bytes (docs/fuzz-inventory.md)
//! takes arbitrary input without panicking, and its peak heap use stays proportional to the input
//! (a counting allocator below measures it), so a declared length can never make it allocate.
//! Inputs are small, so each case is quick; `PROPTEST_CASES=5000 cargo nextest run -p brisk-proto`
//! for a longer run.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use brisk_proto::http::{
    HttpError, check_status, parse_applist, parse_field, parse_launch, parse_serverinfo,
};
use proptest::prelude::*;

/// Counts live heap bytes per thread, so parallel tests in one process do not disturb each other.
struct Counting;

thread_local! {
    static LIVE: Cell<isize> = const { Cell::new(0) };
    static PEAK: Cell<isize> = const { Cell::new(0) };
}

fn track(delta: isize) {
    // try_with: the allocator also runs while thread-locals are being torn down.
    let _ = LIVE.try_with(|l| {
        let v = l.get() + delta;
        l.set(v);
        let _ = PEAK.try_with(|p| p.set(p.get().max(v)));
    });
}

// SAFETY: every call is forwarded unchanged to the system allocator; only a counter is added.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        // SAFETY: same contract as the caller's (non-zero size layout).
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            track(l.size() as isize);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        // SAFETY: `p` came from `alloc` above with the same layout.
        unsafe { System.dealloc(p, l) };
        track(-(l.size() as isize));
    }
}

#[global_allocator]
static A: Counting = Counting;

/// Peak extra heap bytes on this thread while `f` runs.
fn peak<R>(f: impl FnOnce() -> R) -> (R, usize) {
    let start = LIVE.with(Cell::get);
    PEAK.with(|p| p.set(start));
    let r = f();
    (r, (PEAK.with(Cell::get) - start).max(0) as usize)
}

/// What any parser here may use for an input of `n` bytes.
fn budget(n: usize) -> usize {
    16 * n + (64 << 10)
}

/// Found in this pass: the XML parser stored the joined element path of every text node, so a
/// deeply nested reply (or many text nodes under one long element name) cost memory and time
/// quadratic in its size: 257 MB and 20 s (debug) for a 64 KiB reply.
#[test]
fn deep_or_long_xml_paths_cost_no_quadratic_memory() {
    let mut deep = String::from(r#"<root status_code="200">"#);
    while deep.len() < 64_000 {
        deep.push_str("<a>x");
    }
    let mut long = format!(r#"<root status_code="200"><{}>"#, "n".repeat(30_000));
    while long.len() < 64_000 {
        long.push_str("x<b/>");
    }
    for x in [deep, long] {
        let (r, used) = peak(|| check_status(&x));
        assert_eq!(r, Err(HttpError::BadXml));
        assert!(used <= budget(x.len()), "used {used} bytes for {}", x.len());
    }
}

fn bytes(max: usize) -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), 0..max)
}

proptest! {
    #[test]
    fn http_parsers_take_any_text(s in "\\PC{0,600}") {
        let (_, used) = peak(|| {
            let _ = check_status(&s);
            let _ = parse_field(&s, "paired");
            let _ = parse_serverinfo(&s);
            let _ = parse_applist(&s);
            let _ = parse_launch(&s);
        });
        prop_assert!(used <= budget(s.len()));
    }

    #[test]
    fn http_parsers_take_any_xml_shape(
        parts in prop::collection::vec(
            prop::sample::select(vec![
                "<root status_code=\"200\">", "</root>", "<App>", "</App>", "<ID>1</ID>",
                "<AppTitle>&amp;x</AppTitle>", "<a>", "</a>", "<b/>", "&lt;", "&bogus;", "x", " ",
                "<HttpsPort>99999</HttpsPort>", "<PairStatus>2</PairStatus>", "<![CDATA[c]]>",
            ]),
            0..40,
        )
    ) {
        let s = parts.concat();
        let (_, used) = peak(|| {
            let _ = check_status(&s);
            let _ = parse_serverinfo(&s);
            let _ = parse_applist(&s);
            let _ = parse_launch(&s);
        });
        prop_assert!(used <= budget(s.len()));
    }

    #[test]
    fn rtsp_parsers_take_any_bytes(d in bytes(1024)) {
        let (_, used) = peak(|| {
            if let Ok(r) = brisk_proto::rtsp::parse_response(&d) {
                assert!(r.body.len() <= d.len());
                let _ = (r.cseq(), r.session_id(), r.server_port());
            }
            if let Ok(r) = brisk_proto::rtsp::parse_request(&d) {
                assert!(r.body.len() <= d.len());
            }
            let _ = brisk_proto::rtsp::RtspCipher::client(KEY).open(&d);
        });
        prop_assert!(used <= budget(d.len()));
    }

    /// A declared Content-Length never allocates: the body is cut from what arrived.
    #[test]
    fn rtsp_content_length_is_checked_against_the_bytes(
        n in prop_oneof![any::<u64>(), 0u64..80],
        body in bytes(64),
        transport in "[ -~]{0,40}",
    ) {
        let mut raw = format!(
            "RTSP/1.0 200 OK\r\nCSeq: 3\r\nTransport: {transport}\r\nContent-Length: {n}\r\n\r\n"
        )
        .into_bytes();
        raw.extend_from_slice(&body);
        let (r, used) = peak(|| brisk_proto::rtsp::parse_response(&raw));
        prop_assert!(used <= budget(raw.len()));
        match r {
            Ok(r) => {
                prop_assert_eq!(r.body.len() as u64, n);
                let _ = r.server_port();
            }
            Err(e) => prop_assert_eq!(e, brisk_proto::rtsp::RtspError::Truncated),
        }
    }

    /// An encrypted frame's declared length (any u32) is checked before anything is copied.
    #[test]
    fn rtsp_frame_length_prefix_is_capped(len in any::<u32>(), rest in bytes(128)) {
        let mut f = len.to_be_bytes().to_vec();
        f.extend_from_slice(&rest);
        let (r, used) = peak(|| brisk_proto::rtsp::RtspCipher::client(KEY).open(&f));
        prop_assert!(r.is_err());
        prop_assert!(used <= budget(f.len()));
    }

    #[test]
    fn video_parser_takes_any_bytes(d in prop_oneof![bytes(4200), video_packet()]) {
        let (_, used) = peak(|| {
            for enc in [0, brisk_proto::sdp::SS_ENC_VIDEO] {
                if let Ok(p) = brisk_proto::rtp::parse_video_packet(&d, enc) {
                    assert!(p.shard_idx < p.data_shards + p.parity_shards);
                    assert_eq!(p.payload, &d[brisk_proto::rtp::HEADER..]);
                }
            }
            let _ = brisk_proto::rtp::VideoCipher::new(&KEY).open(&d);
        });
        prop_assert!(used <= budget(d.len()));
    }

    /// Any packet sequence: the assembler never panics and yields frames in increasing order.
    #[test]
    fn frame_assembler_takes_any_packets(pkts in prop::collection::vec(video_packet(), 0..48)) {
        let mut a = brisk_proto::rtp::FrameAssembler::new();
        let mut last = 0u32;
        for d in &pkts {
            if let Ok(p) = brisk_proto::rtp::parse_video_packet(d, 0) {
                for f in a.push(p) {
                    prop_assert!(f.idx > last);
                    last = f.idx;
                }
            }
        }
        prop_assert!(a.stats().frames_ok <= pkts.len() as u64);
    }

    #[test]
    fn audio_parsers_take_any_bytes(d in bytes(1500), seq in any::<u16>()) {
        use brisk_proto::audio::*;
        let c = AudioCipher::new(&KEY, 7);
        let (_, used) = peak(|| {
            if let Ok(p) = parse_audio_payload(&d) {
                let _ = c.open(&p);
            }
            let _ = c.open(&AudioPayload { seq, ts: 0, payload: d.clone() });
        });
        prop_assert!(used <= budget(d.len()));
    }

    /// Any datagram sequence (RTP-ish headers, data and FEC payload types, random seqs).
    #[test]
    fn audio_fec_takes_any_datagrams(
        ds in prop::collection::vec(
            (prop::sample::select(vec![97u8, 127, 0]), any::<u16>(), bytes(48)),
            0..64,
        )
    ) {
        let mut fec = brisk_proto::audio::AudioFec::new(5);
        for (pt, seq, body) in ds {
            let mut d = vec![0x80, pt];
            d.extend_from_slice(&seq.to_be_bytes());
            d.extend_from_slice(&[0; 8]);
            d.extend_from_slice(&body);
            if let Ok(out) = fec.push(&d) {
                prop_assert!(out.len() <= 4);
            }
        }
    }

    #[test]
    fn tap_parser_takes_any_bytes(
        d in bytes(200),
        header_len in any::<u16>(),
        fill in bytes(120),
    ) {
        use brisk_proto::tap::*;
        let mut m = b"BTAP\x01\x00".to_vec();
        m.extend_from_slice(&header_len.to_le_bytes());
        m.extend_from_slice(&fill);
        for b in [&d, &m] {
            let (r, used) = peak(|| parse_tap(b));
            prop_assert!(used <= budget(b.len()));
            if let Ok((h, p)) = r {
                prop_assert!(h.header_len as usize >= HEADER_LEN);
                prop_assert_eq!(h.header_len as usize + p.len(), b.len());
            }
        }
    }

    #[test]
    fn control_decoder_takes_any_bytes(d in bytes(300), len in any::<u16>()) {
        let mut c = brisk_proto::control::Control::new(KEY);
        let mut e = vec![1, 0];
        e.extend_from_slice(&len.to_le_bytes());
        e.extend_from_slice(&d);
        for b in [&d, &e] {
            let (r, used) = peak(|| c.decode(b));
            prop_assert!(r.is_err(), "random bytes passed GCM");
            prop_assert!(used <= budget(b.len()));
        }
    }

    /// Any (type, payload) a host seals reaches ControlMsg's parser and comes back as a message.
    #[test]
    fn control_parser_takes_any_plaintext(t in any::<u16>(), p in bytes(251)) {
        use brisk_proto::control::*;
        let f = Control::host(KEY).encode(&ControlMsg::Unknown(t, p)).unwrap();
        let m = Control::new(KEY).decode(&f).unwrap();
        prop_assert_eq!(m.kind(), t);
    }

    /// ENet datagrams with any header and commands, into a connecting and a connected peer.
    #[test]
    fn enet_peer_takes_any_datagrams(
        ds in prop::collection::vec((bytes(80), 0u32..2_000), 0..24),
        cmds in prop::collection::vec((0u8..16, any::<u8>(), any::<u16>(), bytes(24)), 0..24),
    ) {
        use brisk_proto::enet::*;
        let (mut raw, _) = EnetPeer::connect(1, 9, 0);
        let (mut up, _) = EnetPeer::connect(1, 9, 0);
        up.handle_datagram(&verify(9), 0).unwrap();
        // Commands with a valid header, any number, channel, sequence and body.
        let shaped = cmds.into_iter().map(|(n, ch, seq, body)| {
            let mut d = vec![0x90, 0, 0, 1, n | 0x80, ch];
            d.extend_from_slice(&seq.to_be_bytes());
            d.extend_from_slice(&(body.len() as u16).to_be_bytes());
            d.extend_from_slice(&body);
            (d, 0)
        });
        let mut now = 0u32;
        for (d, dt) in ds.into_iter().chain(shaped) {
            now = now.wrapping_add(dt);
            for p in [&mut raw, &mut up] {
                let (_, used) = peak(|| p.handle_datagram(&d, now));
                prop_assert!(used <= budget(d.len()));
                let _ = p.poll_timeout(now);
                prop_assert!(p.in_flight() <= MAX_IN_FLIGHT);
            }
        }
    }

    #[test]
    fn sdp_attr_and_unhex_take_any_text(s in "\\PC{0,300}", name in "[a-z.\\[\\]0-9-]{0,24}") {
        let _ = brisk_proto::sdp::sdp_attr(&s, &name);
        if let Ok(b) = brisk_proto::pair::unhex(&s) {
            prop_assert_eq!(b.len() * 2, s.len());
        }
    }
}

/// A video packet header Sunshine could send, with every field random, then `payload`.
fn video_packet() -> impl Strategy<Value = Vec<u8>> {
    (
        any::<[u8; 32]>(),
        0u32..4,
        0u32..40,
        0u32..80,
        0u32..60,
        0u8..16,
        bytes(96),
    )
        .prop_map(|(mut h, frame, ds, idx, pct, blocks, payload)| {
            h[0] = 0x90;
            h[20..24].copy_from_slice(&frame.to_le_bytes());
            h[27] = blocks << 4;
            h[28..32].copy_from_slice(&(ds << 22 | idx << 12 | pct << 4).to_le_bytes());
            [&h[..], &payload].concat()
        })
}

const KEY: [u8; 16] = *b"0123456789abcdef";

/// VERIFY_CONNECT for a CONNECT with `connect_id` (peer id 0, session 1, sent time 1).
fn verify(connect_id: u32) -> Vec<u8> {
    let mut d = vec![0x90, 0x00, 0, 1, 0x83, 0xFF, 0, 1, 0, 7, 1, 2];
    for v in [900u32, 65536, 0x30, 0, 0, 5000, 2, 2] {
        d.extend_from_slice(&v.to_be_bytes());
    }
    d.extend_from_slice(&connect_id.to_ne_bytes());
    d
}
