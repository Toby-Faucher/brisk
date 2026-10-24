use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes128Gcm, Nonce};
use brisk_proto::rtsp::*;

const KEY: [u8; 16] = *b"0123456789abcdef";

#[test]
fn rtsp_request_roundtrip() {
    let body = b"v=0\r\na=x:1 \r\n";
    let len = body.len().to_string();
    let raw = request(
        "ANNOUNCE",
        "streamid=control/13/0",
        5,
        &[
            ("Session", "DEADBEEFCAFE"),
            ("Content-type", "application/sdp"),
            ("Content-length", &len),
        ],
        body,
    );
    // RtspParser.c serialization, CSeq/X-GS-ClientVersion first as initializeRtspRequest does.
    assert!(raw.starts_with(
        b"ANNOUNCE streamid=control/13/0 RTSP/1.0\r\nCSeq: 5\r\nX-GS-ClientVersion: 14\r\nSession: DEADBEEFCAFE\r\n"
    ));
    let r = parse_request(&raw).unwrap();
    assert_eq!(
        (r.method.as_str(), r.target.as_str()),
        ("ANNOUNCE", "streamid=control/13/0")
    );
    assert_eq!(r.header("cseq"), Some("5"));
    assert_eq!(r.header("content-type"), Some("application/sdp"));
    assert_eq!(r.body, body);
}

/// A Sunshine SETUP reply as rtsp.cpp `cmd_setup` + serializeRtspMessage produce it.
#[test]
fn rtsp_response_with_session_and_ports() {
    let raw = b"RTSP/1.0 200 OK\r\nCSeq: 3\r\nSession: DEADBEEFCAFE;timeout = 90\r\nTransport: server_port=48000\r\nX-SS-Ping-Payload: 0123456789ABCDEF\r\n\r\n";
    let r = parse_response(raw).unwrap();
    assert_eq!(r.status, 200);
    assert_eq!(r.cseq(), Some(3));
    assert_eq!(r.session_id(), Some("DEADBEEFCAFE"));
    assert_eq!(r.server_port(), Some(48000));
    assert_eq!(r.header("x-ss-ping-payload"), Some("0123456789ABCDEF"));
    assert!(r.body.is_empty());
    // DESCRIBE: Sunshine sends no Content-Length; the payload is everything after the header.
    let d = parse_response(b"RTSP/1.0 200 OK\r\nCSeq: 2\r\n\r\na=x-ss-general.featureFlags:3\n")
        .unwrap();
    assert_eq!(d.body, b"a=x-ss-general.featureFlags:3\n");
    assert_eq!(
        parse_response(b"RTSP/1.0 404 NOT FOUND\r\nCSeq: 9\r\n\r\n")
            .unwrap()
            .status,
        404
    );
    // Moonlight's port parse: "unicast;server_port=48000-48001;source=..." also works.
    let t = parse_response(
        b"RTSP/1.0 200 OK\r\nTransport: unicast;server_port=47998-47999;source=x\r\n\r\n",
    )
    .unwrap();
    assert_eq!(t.server_port(), Some(47998));
}

#[test]
fn truncated_rtsp_errors() {
    let full = b"RTSP/1.0 200 OK\r\nCSeq: 3\r\nContent-Length: 10\r\n\r\n0123456789";
    assert!(parse_response(full).is_ok());
    for n in [0, 5, 16, 30, full.len() - 1] {
        assert!(parse_response(&full[..n]).is_err(), "prefix {n} parsed");
    }
    assert_eq!(parse_response(b""), Err(RtspError::Truncated));
    assert_eq!(
        parse_response(b"HTTP/1.1 200 OK\r\n\r\n"),
        Err(RtspError::Malformed)
    );
    assert_eq!(
        parse_response(b"RTSP/1.0 abc OK\r\n\r\n"),
        Err(RtspError::Malformed)
    );
    assert_eq!(
        parse_response(b"RTSP/1.0 200 OK\r\nnocolon\r\n\r\n"),
        Err(RtspError::Malformed)
    );
    let mut big = b"RTSP/1.0 200 OK\r\n\r\n".to_vec();
    big.resize(MAX_MESSAGE + 1, b'a');
    assert_eq!(parse_response(&big), Err(RtspError::TooLarge));
}

fn reply(seq: u32) -> Vec<u8> {
    format!("RTSP/1.0 200 OK\r\nCSeq: {seq}\r\n\r\n").into_bytes()
}

#[test]
fn encrypted_roundtrip_both_directions() {
    let (mut c, mut h) = (RtspCipher::client(KEY), RtspCipher::host(KEY));
    let req = request("OPTIONS", "rtspenc://h:48010", 1, &[], b"");
    let f = c.seal(&req).unwrap();
    // encrypted_rtsp_header_t: BE(0x80000000 | len), BE(seq), 16-byte tag, ciphertext.
    assert_eq!(&f[..4], &(0x8000_0000u32 | req.len() as u32).to_be_bytes());
    assert_eq!(&f[4..8], &1u32.to_be_bytes());
    assert_eq!(f.len(), 24 + req.len());
    assert_eq!(h.open(&f).unwrap(), req);
    let f = h.seal(&reply(1)).unwrap();
    assert_eq!(c.open(&f).unwrap(), reply(1));
    // Second message per side uses sequence 2.
    assert_eq!(&c.seal(&req).unwrap()[4..8], &2u32.to_be_bytes());
}

/// IV layout from rtsp.cpp/RtspConnection.c, built independently here: seq little-endian in
/// bytes 0..4, zeros, then 'H','R' (host originated) or 'C','R' (client originated).
#[test]
fn host_frame_uses_sunshine_iv() {
    let pt = reply(7);
    let mut iv = [0u8; 12];
    iv[..4].copy_from_slice(&3u32.to_le_bytes());
    iv[10] = b'H';
    iv[11] = b'R';
    let ct = Aes128Gcm::new(&KEY.into())
        .encrypt(Nonce::from_slice(&iv), Payload { msg: &pt, aad: b"" })
        .unwrap(); // ciphertext || tag
    let (body, tag) = ct.split_at(pt.len());
    let mut f = (0x8000_0000u32 | pt.len() as u32).to_be_bytes().to_vec();
    f.extend(3u32.to_be_bytes());
    f.extend(tag);
    f.extend(body);
    assert_eq!(RtspCipher::client(KEY).open(&f).unwrap(), pt);
    // The same bytes under the client-originated IV must not verify.
    assert_eq!(RtspCipher::host(KEY).open(&f), Err(RtspError::BadTag));
}

#[test]
fn client_frame_uses_sunshine_iv() {
    // Frame: type|len(BE,4) seq(BE,4) tag(16) ciphertext; first sealed request has seq 1.
    let pt = b"OPTIONS * RTSP/1.0\r\nCSeq: 1\r\n\r\n";
    let f = RtspCipher::client(KEY).seal(pt).unwrap();
    assert_eq!(f[4..8], 1u32.to_be_bytes());
    let mut iv = [0u8; 12];
    iv[..4].copy_from_slice(&1u32.to_le_bytes());
    iv[10] = b'C';
    iv[11] = b'R';
    let mut ct = f[24..].to_vec();
    ct.extend(&f[8..24]); // aes-gcm crate wants ciphertext || tag
    let out = Aes128Gcm::new(&KEY.into())
        .decrypt(Nonce::from_slice(&iv), Payload { msg: &ct, aad: b"" })
        .unwrap();
    assert_eq!(out, pt);
}

#[test]
fn encrypted_rejects_tamper_wrong_key_truncation_oversize() {
    let f = RtspCipher::host(KEY).seal(&reply(1)).unwrap();
    for i in [0usize, 8, 23, 24, f.len() - 1] {
        if i < 4 {
            continue;
        }
        let mut t = f.clone();
        t[i] ^= 1;
        assert!(
            RtspCipher::client(KEY).open(&t).is_err(),
            "byte {i} flip accepted"
        );
    }
    let mut t = f.clone();
    t[4..8].copy_from_slice(&9u32.to_be_bytes()); // seq is bound through the IV
    assert_eq!(RtspCipher::client(KEY).open(&t), Err(RtspError::BadTag));
    assert_eq!(RtspCipher::client([1; 16]).open(&f), Err(RtspError::BadTag));
    for n in [0, 8, 24, f.len() - 1] {
        assert_eq!(
            RtspCipher::client(KEY).open(&f[..n]),
            Err(RtspError::Truncated),
            "len {n}"
        );
    }
    let mut extra = f.clone();
    extra.push(0);
    assert_eq!(
        RtspCipher::client(KEY).open(&extra),
        Err(RtspError::Malformed)
    );
    let mut plain = f.clone();
    plain[0] &= 0x7f;
    assert_eq!(
        RtspCipher::client(KEY).open(&plain),
        Err(RtspError::Unencrypted)
    );
    let mut big = vec![0u8; 24 + MAX_MESSAGE + 1];
    big[..4].copy_from_slice(&(0x8000_0000u32 | (MAX_MESSAGE as u32 + 1)).to_be_bytes());
    assert_eq!(RtspCipher::client(KEY).open(&big), Err(RtspError::TooLarge));
    assert_eq!(
        RtspCipher::client(KEY).seal(&vec![0; MAX_MESSAGE + 1]),
        Err(RtspError::TooLarge)
    );
}

/// Sunshine bumps `rtsp_iv_counter` once per response it sends, so host sequence numbers only
/// grow; a repeated or older one is a replay and is refused (moonlight-common-c does not check).
#[test]
fn encrypted_sequence_must_increase() {
    let mut h = RtspCipher::host(KEY);
    let f1 = h.seal(&reply(1)).unwrap();
    let f2 = h.seal(&reply(2)).unwrap();
    let f3 = h.seal(&reply(3)).unwrap();
    let mut c = RtspCipher::client(KEY);
    c.open(&f2).unwrap(); // gaps are fine: Sunshine also counts error replies we never read
    assert_eq!(c.open(&f2), Err(RtspError::Replay));
    assert_eq!(c.open(&f1), Err(RtspError::Replay));
    c.open(&f3).unwrap();
    // A failed open does not advance the window.
    let mut c = RtspCipher::client(KEY);
    let mut bad = f1.clone();
    bad[30] ^= 1;
    assert!(c.open(&bad).is_err());
    c.open(&f1).unwrap();
}

/// Parses every recorded Sunshine DESCRIBE reply in `fixtures/rtsp/` (decrypted, scrubbed).
#[test]
#[cfg_attr(
    not(rtsp_fixture),
    ignore = "SKIPPED: no fixtures/rtsp/*describe-resp.bin yet; recorded by live step 5"
)]
fn replays_recorded_rtsp_describe() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/rtsp");
    let files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.to_string_lossy().ends_with("describe-resp.bin"))
        .collect();
    assert!(
        !files.is_empty(),
        "no describe-resp.bin in {}",
        dir.display()
    );
    for f in files {
        let r = parse_response(&std::fs::read(&f).unwrap()).unwrap();
        assert_eq!(r.status, 200, "{}", f.display());
        let body = String::from_utf8_lossy(&r.body);
        for k in [
            "x-ss-general.featureFlags",
            "x-ss-general.encryptionSupported",
            "x-ss-general.encryptionRequested",
        ] {
            assert!(
                brisk_proto::sdp::sdp_attr(&body, k).is_some(),
                "{}: no {k}",
                f.display()
            );
        }
        assert!(
            body.contains("a=fmtp:97 surround-params="),
            "{}",
            f.display()
        );
    }
}
