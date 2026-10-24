//! ANNOUNCE SDP for Sunshine. Emits exactly the attributes Sunshine's `cmd_announce` (rtsp.cpp)
//! reads, with the values moonlight-common-c `SdpGenerator.c` sends for a generation-7 Sunshine host
//! (appversion `7.1.431.-1`): local stream, SDR, stereo, no reference-frame invalidation.

/// `x-ss-general.encryption*` bits (moonlight-common-c Limelight-internal.h, Sunshine stream.cpp).
pub const SS_ENC_CONTROL_V2: u32 = 0x01;
pub const SS_ENC_VIDEO: u32 = 0x02;
pub const SS_ENC_AUDIO: u32 = 0x04;

/// `x-nv-general.featureFlags`: base 0x07 | remote-input encryption 0x80; 0x20 = audio encryption.
const NVFF_BASE_RI: u32 = 0x87;
const NVFF_AUDIO_ENCRYPTION: u32 = 0x20;
/// `x-ml-general.featureFlags`: ML_FF_FEC_STATUS | ML_FF_SESSION_ID_V1 (ping payload / connect data).
const ML_FF: u32 = 0x03;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    H264,
    Hevc,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamConfig {
    pub width: u16,
    pub height: u16,
    pub fps: u16,
    /// What the user asked for; Sunshine derives the encoder bitrate from it (`configuredBitrateKbps`).
    pub bitrate_kbps: u32,
    pub codec: Codec,
    /// Video payload size, sent as is (moonlight subtracts the 32-byte header itself when video is encrypted).
    pub packet_size: u16,
    /// Not sent: Sunshine takes FEC percentage from its own config (`fec_percentage`), not the SDP.
    pub fec_percent: u8,
    /// Launch key: keys encrypted RTSP, control, and A/V encryption.
    pub rikey: [u8; 16],
    pub rikeyid: u32,
    /// `SS_ENC_*` bits sent verbatim as `x-ss-general.encryptionEnabled`; see [`negotiate_encryption`].
    pub enc_flags: u32,
}

/// moonlight-common-c's rule: control V2 whenever supported; video/audio when supported and
/// `wanted`, or whenever the host requests it (mandatory mode answers 403 otherwise).
pub fn negotiate_encryption(supported: u32, requested: u32, wanted: u32) -> u32 {
    let av = SS_ENC_VIDEO | SS_ENC_AUDIO;
    (supported & SS_ENC_CONTROL_V2) | (supported & wanted & av) | (requested & av)
}

/// Integer value of `a=<name>:<v>` in an SDP/DESCRIBE payload.
pub fn sdp_attr(payload: &str, name: &str) -> Option<u32> {
    payload.lines().find_map(|l| {
        let v = l
            .strip_prefix("a=")?
            .strip_prefix(name)?
            .strip_prefix(':')?;
        v.trim().parse().ok()
    })
}

pub fn build_sdp(cfg: &StreamConfig, app_version: &str) -> String {
    // rtspClientVersion by appversion major: 3 -> 10 ... 7+ -> 14 (RtspConnection.c).
    let major: u32 = app_version
        .split('.')
        .next()
        .and_then(|m| m.parse().ok())
        .unwrap_or(7);
    let client_version = 10 + major.clamp(3, 7) - 3;
    // SdpGenerator.c: 80% of the requested rate (20% reserved for FEC), capped at 100 Mbps.
    let adjusted = (cfg.bitrate_kbps as u64 * 80 / 100).min(100_000);
    let hevc = (cfg.codec == Codec::Hevc) as u8;
    let nvff = NVFF_BASE_RI
        | if cfg.enc_flags & SS_ENC_AUDIO != 0 {
            NVFF_AUDIO_ENCRYPTION
        } else {
            0
        };
    let attrs: [(&str, String); 25] = [
        ("x-ml-general.featureFlags", ML_FF.to_string()),
        ("x-ss-general.encryptionEnabled", cfg.enc_flags.to_string()),
        ("x-ss-video[0].chromaSamplingType", "0".into()), // 4:2:0
        ("x-ss-video[0].intraRefresh", "0".into()),
        ("x-nv-video[0].clientViewportWd", cfg.width.to_string()),
        ("x-nv-video[0].clientViewportHt", cfg.height.to_string()),
        ("x-nv-video[0].maxFPS", cfg.fps.to_string()),
        ("x-nv-video[0].packetSize", cfg.packet_size.to_string()),
        ("x-nv-vqos[0].bw.maximumBitrateKbps", adjusted.to_string()),
        (
            "x-ml-video.configuredBitrateKbps",
            cfg.bitrate_kbps.to_string(),
        ),
        ("x-nv-vqos[0].qosTrafficType", "5".into()), // local stream: DSCP tagging on
        ("x-nv-aqos.qosTrafficType", "4".into()),
        ("x-nv-general.featureFlags", nvff.to_string()),
        ("x-nv-general.useReliableUdp", "13".into()), // encrypted control protocol
        ("x-nv-vqos[0].fec.minRequiredFecPackets", "2".into()),
        ("x-nv-video[0].videoEncoderSlicesPerFrame", "1".into()),
        ("x-nv-vqos[0].bitStreamFormat", hevc.to_string()),
        ("x-nv-video[0].dynamicRangeMode", "0".into()), // SDR
        ("x-nv-video[0].maxNumReferenceFrames", "1".into()), // no RFI yet
        ("x-nv-video[0].clientRefreshRateX100", "0".into()), // Sunshine: use maxFPS
        ("x-nv-audio.surround.numChannels", "2".into()),
        ("x-nv-audio.surround.channelMask", "3".into()),
        ("x-nv-audio.surround.AudioQuality", "0".into()),
        ("x-nv-aqos.packetDuration", "5".into()),
        ("x-nv-video[0].encoderCscMode", "2".into()), // (Rec.709 << 1) | limited range
    ];
    // Sunshine ignores v=/o=/t=/m=; kept in SdpGenerator.c's shape, with placeholder address and port.
    let mut s = format!(
        "v=0\r\no=android 0 {client_version} IN IPv4 0.0.0.0\r\ns=NVIDIA Streaming Client\r\n"
    );
    for (k, v) in attrs {
        s.push_str(&format!("a={k}:{v} \r\n"));
    }
    s.push_str("t=0 0\r\nm=video 47998  \r\n");
    s
}
