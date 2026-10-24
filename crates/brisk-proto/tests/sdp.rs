use brisk_proto::sdp::*;

fn cfg() -> StreamConfig {
    StreamConfig {
        width: 1920,
        height: 1080,
        fps: 60,
        bitrate_kbps: 20000,
        codec: Codec::H264,
        packet_size: 1392,
        fec_percent: 20,
        rikey: [7; 16],
        rikeyid: 1,
        enc_flags: SS_ENC_CONTROL_V2,
    }
}

fn attrs(sdp: &str) -> Vec<(String, String)> {
    sdp.split("\r\n")
        .filter_map(|l| l.strip_prefix("a="))
        .map(|l| {
            let (k, v) = l.split_once(':').unwrap();
            (k.to_string(), v.trim_end().to_string())
        })
        .collect()
}

/// Exactly the keys Sunshine's `cmd_announce` (rtsp.cpp) reads, with SdpGenerator.c's values for
/// appversion 7.1.431.-1 (Sunshine branch), 1080p60 H.264 SDR stereo, local stream, no RFI.
#[test]
fn sdp_contains_required_attributes() {
    let sdp = build_sdp(&cfg(), "7.1.431.-1");
    let expected = [
        ("x-ml-general.featureFlags", "3"),
        ("x-ss-general.encryptionEnabled", "1"),
        ("x-ss-video[0].chromaSamplingType", "0"),
        ("x-ss-video[0].intraRefresh", "0"),
        ("x-nv-video[0].clientViewportWd", "1920"),
        ("x-nv-video[0].clientViewportHt", "1080"),
        ("x-nv-video[0].maxFPS", "60"),
        ("x-nv-video[0].packetSize", "1392"),
        ("x-nv-vqos[0].bw.maximumBitrateKbps", "16000"),
        ("x-ml-video.configuredBitrateKbps", "20000"),
        ("x-nv-vqos[0].qosTrafficType", "5"),
        ("x-nv-aqos.qosTrafficType", "4"),
        ("x-nv-general.featureFlags", "135"),
        ("x-nv-general.useReliableUdp", "13"),
        ("x-nv-vqos[0].fec.minRequiredFecPackets", "2"),
        ("x-nv-video[0].videoEncoderSlicesPerFrame", "1"),
        ("x-nv-vqos[0].bitStreamFormat", "0"),
        ("x-nv-video[0].dynamicRangeMode", "0"),
        ("x-nv-video[0].maxNumReferenceFrames", "1"),
        ("x-nv-video[0].clientRefreshRateX100", "0"),
        ("x-nv-audio.surround.numChannels", "2"),
        ("x-nv-audio.surround.channelMask", "3"),
        ("x-nv-audio.surround.AudioQuality", "0"),
        ("x-nv-aqos.packetDuration", "5"),
        ("x-nv-video[0].encoderCscMode", "2"),
    ];
    let mut got = attrs(&sdp);
    let mut want: Vec<_> = expected
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    got.sort();
    want.sort();
    assert_eq!(got, want);
    // SdpGenerator.c framing: "a=k:v \r\n" (Sunshine strips the one trailing space), header and tail.
    assert!(
        sdp.starts_with("v=0\r\no=android 0 14 IN IPv4 0.0.0.0\r\ns=NVIDIA Streaming Client\r\n")
    );
    assert!(sdp.contains("a=x-nv-video[0].maxFPS:60 \r\n"));
    assert!(sdp.ends_with("t=0 0\r\nm=video 47998  \r\n"));
}

#[test]
fn sdp_hevc_audio_encryption_and_bitrate_cap() {
    let mut c = cfg();
    c.codec = Codec::Hevc;
    c.enc_flags = SS_ENC_CONTROL_V2 | SS_ENC_AUDIO;
    c.bitrate_kbps = 150_000;
    let a = attrs(&build_sdp(&c, "7.1.431.-1"));
    let get = |k: &str| a.iter().find(|(n, _)| n == k).unwrap().1.clone();
    assert_eq!(get("x-nv-vqos[0].bitStreamFormat"), "1");
    assert_eq!(get("x-ss-general.encryptionEnabled"), "5");
    assert_eq!(get("x-nv-general.featureFlags"), "167"); // 0x87 | 0x20 audio encryption
    assert_eq!(get("x-nv-vqos[0].bw.maximumBitrateKbps"), "100000");
    assert_eq!(get("x-ml-video.configuredBitrateKbps"), "150000");
}

#[test]
fn encryption_negotiation_mirrors_moonlight() {
    // Sunshine default LAN mode: supported CONTROL_V2|AUDIO, requested CONTROL_V2.
    assert_eq!(negotiate_encryption(5, 1, 0), SS_ENC_CONTROL_V2);
    assert_eq!(negotiate_encryption(5, 1, SS_ENC_AUDIO), 5);
    // Video wanted but not supported: dropped.
    assert_eq!(negotiate_encryption(5, 1, SS_ENC_VIDEO), 1);
    // Mandatory mode: host requests video+audio, we comply even without opting in.
    assert_eq!(negotiate_encryption(7, 7, 0), 7);
}

#[test]
fn sdp_attr_parses_describe_reply() {
    let d = "a=x-ss-general.featureFlags:3\r\na=x-ss-general.encryptionSupported:5\na=x-ss-general.encryptionRequested:1 \r\n";
    assert_eq!(sdp_attr(d, "x-ss-general.encryptionSupported"), Some(5));
    assert_eq!(sdp_attr(d, "x-ss-general.encryptionRequested"), Some(1));
    assert_eq!(sdp_attr(d, "x-ss-general.featureFlags"), Some(3));
    assert_eq!(sdp_attr(d, "x-ss-general.missing"), None);
}

/// Sunshine's 2048-byte request buffer must hold the whole sealed ANNOUNCE.
#[test]
fn sealed_announce_fits_sunshine_buffer() {
    use brisk_proto::rtsp::*;
    let mut c = cfg();
    c.width = 65535;
    c.height = 65535;
    c.bitrate_kbps = u32::MAX;
    let sdp = build_sdp(&c, "7.1.431.-1");
    let len = sdp.len().to_string();
    let req = request(
        "ANNOUNCE",
        "streamid=control/13/0",
        u32::MAX,
        &[
            ("Host", "[ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff]"),
            ("Session", "DEADBEEFCAFE"),
            ("Content-type", "application/sdp"),
            ("Content-length", &len),
        ],
        sdp.as_bytes(),
    );
    let f = RtspCipher::client([0; 16]).seal(&req).unwrap();
    assert!(f.len() < SUNSHINE_MAX_REQUEST, "{}", f.len());
}
