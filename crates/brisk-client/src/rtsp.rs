//! RTSP handshake driver (RtspConnection.c order for a 7.1.431 Sunshine host): OPTIONS, DESCRIBE,
//! SETUP audio/video/control, ANNOUNCE (SDP), PLAY. One TCP connection per request, reply read to
//! EOF; sealed with [`RtspCipher`] when the launch URL is `rtspenc://`.

use crate::recorder::Recorder;
use brisk_proto::rtsp::{
    parse_response, request, RtspCipher, RtspReply, MAX_MESSAGE, SUNSHINE_MAX_REQUEST,
};
use brisk_proto::sdp::{build_sdp, negotiate_encryption, sdp_attr, StreamConfig};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// moonlight-common-c RTSP_RECEIVE_TIMEOUT_SEC.
const WAIT: Duration = Duration::from_secs(15);
const IF_MODIFIED: (&str, &str) = ("If-Modified-Since", "Thu, 01 Jan 1970 00:00:00 GMT");
const CONTROL: &str = "streamid=control/13/0";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamPorts {
    pub audio: u16,
    pub video: u16,
    pub control: u16,
    pub session_id: String,
    /// `X-SS-Ping-Payload` from the audio and video SETUP replies (Sunshine sends one value for both).
    pub ping_payload: Option<String>,
    /// `X-SS-Connect-Data` from the control SETUP reply.
    pub connect_data: Option<u32>,
    /// `SS_ENC_*` bits announced as `x-ss-general.encryptionEnabled`.
    pub enc_flags: u32,
}

struct Conn<'a> {
    host: String,
    port: u16,
    /// Address part of the session URL, sent as `Host` (Sunshine picks stereo audio quality from it).
    url_host: String,
    cseq: u32,
    cipher: Option<RtspCipher>,
    recorder: Option<&'a mut Recorder>,
}

/// Splits `rtsp[enc]://ADDR:PORT` into (encrypted, ADDR, PORT).
pub fn parse_session_url(url: &str) -> crate::Result<(bool, String, u16)> {
    let (enc, rest) = if let Some(r) = url.strip_prefix("rtspenc://") {
        (true, r)
    } else if let Some(r) = url.strip_prefix("rtsp://") {
        (false, r)
    } else {
        return Err(format!("unsupported rtsp session url {url:?}").into());
    };
    let rest = rest.trim_end_matches('/');
    let (addr, port) = rest
        .rsplit_once(':')
        .ok_or_else(|| format!("rtsp session url without port {url:?}"))?;
    let port = port
        .parse()
        .map_err(|_| format!("bad port in rtsp session url {url:?}"))?;
    Ok((enc, addr.to_string(), port))
}

/// Runs the handshake against `host` (the address already in use; only the port is taken from
/// `session_url`). `cfg.enc_flags` holds the `SS_ENC_VIDEO`/`SS_ENC_AUDIO` bits the client wants;
/// the announced set is negotiated from DESCRIBE as moonlight-common-c does.
pub async fn handshake(
    host: &str,
    session_url: &str,
    app_version: &str,
    cfg: &StreamConfig,
    recorder: Option<&mut Recorder>,
) -> crate::Result<StreamPorts> {
    let (enc, url_host, port) = parse_session_url(session_url)?;
    let mut c = Conn {
        host: host.to_string(),
        port,
        url_host,
        cseq: 1,
        cipher: enc.then(|| RtspCipher::client(cfg.rikey)),
        recorder,
    };

    c.call("options", "OPTIONS", session_url, &[], b"").await?;
    let d = c
        .call(
            "describe",
            "DESCRIBE",
            session_url,
            &[("Accept", "application/sdp"), IF_MODIFIED],
            b"",
        )
        .await?;
    let d = String::from_utf8_lossy(&d.body);
    let supported = sdp_attr(&d, "x-ss-general.encryptionSupported").unwrap_or(0);
    let requested = sdp_attr(&d, "x-ss-general.encryptionRequested").unwrap_or(0);

    let transport = ("Transport", "unicast;X-GS-ClientPort=50000-50001");
    let a = c
        .call(
            "setup-audio",
            "SETUP",
            "streamid=audio/0/0",
            &[transport, IF_MODIFIED],
            b"",
        )
        .await?;
    let session_id = a
        .session_id()
        .ok_or("rtsp SETUP audio: missing Session")?
        .to_string();
    let sess = ("Session", session_id.as_str());
    let v = c
        .call(
            "setup-video",
            "SETUP",
            "streamid=video/0/0",
            &[sess, transport, IF_MODIFIED],
            b"",
        )
        .await?;
    let k = c
        .call(
            "setup-control",
            "SETUP",
            CONTROL,
            &[sess, transport, IF_MODIFIED],
            b"",
        )
        .await?;
    let port = |r: &RtspReply, what: &str| {
        r.server_port()
            .ok_or_else(|| format!("rtsp SETUP {what}: no server_port in Transport"))
    };

    let mut announced = cfg.clone();
    announced.enc_flags = negotiate_encryption(supported, requested, cfg.enc_flags);
    let sdp = build_sdp(&announced, app_version);
    let len = sdp.len().to_string();
    c.call(
        "announce",
        "ANNOUNCE",
        CONTROL,
        &[
            sess,
            ("Content-type", "application/sdp"),
            ("Content-length", &len),
        ],
        sdp.as_bytes(),
    )
    .await?;
    c.call("play", "PLAY", "/", &[sess], b"").await?;

    Ok(StreamPorts {
        audio: port(&a, "audio")?,
        video: port(&v, "video")?,
        control: port(&k, "control")?,
        session_id,
        ping_payload: a.header("X-SS-Ping-Payload").map(str::to_string),
        connect_data: k.header("X-SS-Connect-Data").and_then(|s| s.parse().ok()),
        enc_flags: announced.enc_flags,
    })
}

impl Conn<'_> {
    /// One request/reply; any status but 200 is an error.
    async fn call(
        &mut self,
        label: &str,
        method: &str,
        target: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> crate::Result<RtspReply> {
        let mut h = vec![("Host", self.url_host.as_str())];
        h.extend_from_slice(headers);
        let plain = request(method, target, self.cseq, &h, body);
        self.cseq += 1;
        let wire = match &mut self.cipher {
            Some(c) => c.seal(&plain)?,
            None => plain.clone(),
        };
        if wire.len() >= SUNSHINE_MAX_REQUEST {
            return Err(format!(
                "rtsp {method}: request of {} bytes exceeds Sunshine's buffer",
                wire.len()
            )
            .into());
        }
        let raw = tokio::time::timeout(WAIT, async {
            let mut s = TcpStream::connect((self.host.as_str(), self.port)).await?;
            s.write_all(&wire).await?;
            s.flush().await?;
            let mut buf = Vec::new();
            // +24 for the encrypted header, +1 so an oversized reply is detected rather than cut.
            (&mut s)
                .take((MAX_MESSAGE + 25) as u64)
                .read_to_end(&mut buf)
                .await?;
            crate::Result::Ok(buf)
        })
        .await
        .map_err(|_| {
            format!(
                "rtsp {method}: no reply from {}:{} within {}s",
                self.host,
                self.port,
                WAIT.as_secs()
            )
        })?
        .map_err(|e| format!("rtsp {method} ({}:{}): {e}", self.host, self.port))?;
        let reply = match &mut self.cipher {
            Some(c) => c.open(&raw),
            None => Ok(raw),
        }
        .map_err(|e| format!("rtsp {method}: {e}"))?;
        if let Some(rec) = &mut self.recorder {
            let enc = self.cipher.is_some();
            rec.push(&format!("rtsp-{label}-req"), enc, &plain);
            rec.push(&format!("rtsp-{label}-resp"), enc, &reply);
        }
        let r = parse_response(&reply).map_err(|e| format!("rtsp {method}: {e}"))?;
        if r.status != 200 {
            return Err(format!("rtsp {method} {target}: status {}", r.status).into());
        }
        Ok(r)
    }
}
