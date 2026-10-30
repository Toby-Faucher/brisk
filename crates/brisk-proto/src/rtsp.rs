//! Sans-io RTSP for Sunshine's handshake: message encode/parse (RtspParser.c wire format) and the
//! encrypted framing used for `rtspenc://` sessions (rtsp.cpp `encrypted_rtsp_header_t`).
//!
//! Each request travels on its own TCP connection; Sunshine answers once and shuts the socket, so a
//! reply is everything read up to EOF.

use aes_gcm::aead::{AeadInPlace, KeyInit};
use aes_gcm::{Aes128Gcm, Nonce, Tag};
use std::fmt::Write;

/// Replies and frames larger than this are rejected, never truncated.
pub const MAX_MESSAGE: usize = 64 * 1024;
/// Sunshine reads each request into a 2048-byte buffer (rtsp.cpp `msg_buf`); a request (sealed
/// frame included) must stay below it or Sunshine answers 400.
pub const SUNSHINE_MAX_REQUEST: usize = 2048;
/// `X-GS-ClientVersion` moonlight-common-c sends to generation-7 hosts.
pub const CLIENT_VERSION: u32 = 14;

const ENCRYPTED_BIT: u32 = 0x8000_0000;
const HEADER: usize = 24; // typeAndLength(4) + sequenceNumber(4) + tag(16)

#[derive(Debug, Clone, PartialEq, Eq)]
/// Why an RTSP message was rejected.
pub enum RtspError {
    /// Message over the size limit.
    TooLarge,
    /// Message or frame ends early.
    Truncated,
    /// Start line, header or Content-Length does not parse.
    Malformed,
    /// Plain bytes where an encrypted frame was required.
    Unencrypted,
    /// AES-GCM tag did not verify: tampered, wrong key or wrong IV.
    BadTag,
    /// Host sequence number not above the last accepted one.
    Replay,
    /// 2^32 messages sent under one key; sending more would reuse an IV.
    SeqExhausted,
}

impl std::fmt::Display for RtspError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            RtspError::TooLarge => "rtsp message too large",
            RtspError::Truncated => "truncated rtsp message",
            RtspError::Malformed => "malformed rtsp message",
            RtspError::Unencrypted => "unencrypted rtsp message on an encrypted session",
            RtspError::BadTag => "rtsp message failed authentication",
            RtspError::Replay => "replayed rtsp sequence number",
            RtspError::SeqExhausted => "rtsp sequence number exhausted",
        };
        f.write_str(s)
    }
}
impl std::error::Error for RtspError {}

#[derive(Debug, PartialEq, Eq)]
/// A parsed RTSP request.
pub struct RtspRequest {
    /// Method, such as `DESCRIBE`.
    pub method: String,
    /// Request target.
    pub target: String,
    /// Headers in order.
    pub headers: Vec<(String, String)>,
    /// Body bytes.
    pub body: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
/// A parsed RTSP reply.
pub struct RtspReply {
    /// Status code.
    pub status: u16,
    /// Headers in order.
    pub headers: Vec<(String, String)>,
    /// Body bytes.
    pub body: Vec<u8>,
}

fn find<'a>(h: &'a [(String, String)], name: &str) -> Option<&'a str> {
    h.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

impl RtspRequest {
    /// Case-insensitive, first match.
    pub fn header(&self, name: &str) -> Option<&str> {
        find(&self.headers, name)
    }
}

impl RtspReply {
    /// Case-insensitive, first match.
    pub fn header(&self, name: &str) -> Option<&str> {
        find(&self.headers, name)
    }
    /// `CSeq` as a number.
    pub fn cseq(&self) -> Option<u32> {
        self.header("CSeq")?.parse().ok()
    }
    /// `Session` up to the first `;` (Sunshine sends `DEADBEEFCAFE;timeout = 90`).
    pub fn session_id(&self) -> Option<&str> {
        let s = self.header("Session")?.split(';').next()?.trim();
        (!s.is_empty()).then_some(s)
    }
    /// Port after `server_port=` in `Transport`, as moonlight-common-c parses it.
    pub fn server_port(&self) -> Option<u16> {
        let t = self.header("Transport")?;
        let p = &t[t.find("server_port=")? + "server_port=".len()..];
        let digits = p.bytes().take_while(u8::is_ascii_digit).count();
        p[..digits].parse().ok().filter(|&p| p != 0)
    }
}

/// `METHOD target RTSP/1.0`, then `CSeq` and `X-GS-ClientVersion` (as `initializeRtspRequest`),
/// then `headers` in order, a blank line and `body`. The caller supplies `Content-length` for a body
/// (Sunshine matches that exact spelling to wait for the payload).
pub fn request(
    method: &str,
    target: &str,
    seq: u32,
    headers: &[(&str, &str)],
    body: &[u8],
) -> Vec<u8> {
    let mut s = format!(
        "{method} {target} RTSP/1.0\r\nCSeq: {seq}\r\nX-GS-ClientVersion: {CLIENT_VERSION}\r\n"
    );
    for (k, v) in headers {
        let _ = write!(s, "{k}: {v}\r\n"); // writing to a String cannot fail
    }
    s.push_str("\r\n");
    let mut out = s.into_bytes();
    out.extend_from_slice(body);
    out
}

/// Start line tokens, headers, body.
type Parts = ([String; 3], Vec<(String, String)>, Vec<u8>);
fn parse(raw: &[u8]) -> Result<Parts, RtspError> {
    if raw.len() > MAX_MESSAGE {
        return Err(RtspError::TooLarge);
    }
    let end = crate::http::header_end(raw).ok_or(RtspError::Truncated)?;
    let head = std::str::from_utf8(&raw[..end]).map_err(|_| RtspError::Malformed)?;
    let mut lines = head.split("\r\n");
    let mut start = lines.next().unwrap_or("").splitn(3, ' ');
    let mut tok = || start.next().map(str::to_string).ok_or(RtspError::Malformed);
    let first = [tok()?, tok()?, tok()?];
    let mut headers = vec![];
    for l in lines {
        let (k, v) = l.split_once(':').ok_or(RtspError::Malformed)?;
        if k.trim().is_empty() {
            return Err(RtspError::Malformed);
        }
        headers.push((k.trim().to_string(), v.trim().to_string()));
    }
    let mut body = &raw[end + 4..];
    if let Some(n) = find(&headers, "Content-Length") {
        let n: usize = n.parse().map_err(|_| RtspError::Malformed)?;
        body = body.get(..n).ok_or(RtspError::Truncated)?;
    }
    Ok((first, headers, body.to_vec()))
}

/// Parses an RTSP/1.0 request.
pub fn parse_request(raw: &[u8]) -> Result<RtspRequest, RtspError> {
    let ([method, target, proto], headers, body) = parse(raw)?;
    if proto != "RTSP/1.0" {
        return Err(RtspError::Malformed);
    }
    Ok(RtspRequest {
        method,
        target,
        headers,
        body,
    })
}

/// Parses an RTSP/1.0 reply.
pub fn parse_response(raw: &[u8]) -> Result<RtspReply, RtspError> {
    let ([proto, code, _reason], headers, body) = parse(raw)?;
    let status = code.parse().map_err(|_| RtspError::Malformed)?;
    if proto != "RTSP/1.0" || !(100..1000).contains(&status) {
        return Err(RtspError::Malformed);
    }
    Ok(RtspReply {
        status,
        headers,
        body,
    })
}

/// AES-128-GCM keyed by the launch `rikey`. IV (NIST SP 800-38D 8.2.1, as rtsp.cpp): sequence number
/// little-endian in bytes 0..4, zeros, then `'C','R'` for client-sent or `'H','R'` for host-sent.
/// Each side counts its own messages from 1. Wire frame: BE `0x80000000 | len`, BE seq, tag, ciphertext.
pub struct RtspCipher {
    gcm: Aes128Gcm,
    tx_dir: u8,
    rx_dir: u8,
    tx: u32,
    rx: u32,
}

impl RtspCipher {
    /// The client side.
    pub fn client(key: [u8; 16]) -> Self {
        Self::new(key, b'C', b'H')
    }
    /// The host side, for fakes and tests.
    pub fn host(key: [u8; 16]) -> Self {
        Self::new(key, b'H', b'C')
    }
    fn new(key: [u8; 16], tx_dir: u8, rx_dir: u8) -> Self {
        RtspCipher {
            gcm: Aes128Gcm::new(&key.into()),
            tx_dir,
            rx_dir,
            tx: 0,
            rx: 0,
        }
    }

    fn iv(seq: u32, dir: u8) -> [u8; 12] {
        let mut iv = [0u8; 12];
        iv[..4].copy_from_slice(&seq.to_le_bytes());
        iv[10] = dir;
        iv[11] = b'R';
        iv
    }

    /// Seals `plain` as our next message, in wire framing.
    pub fn seal(&mut self, plain: &[u8]) -> Result<Vec<u8>, RtspError> {
        if plain.len() > MAX_MESSAGE {
            return Err(RtspError::TooLarge);
        }
        let seq = self.tx.checked_add(1).ok_or(RtspError::SeqExhausted)?;
        let mut ct = plain.to_vec();
        let tag = self
            .gcm
            .encrypt_in_place_detached(Nonce::from_slice(&Self::iv(seq, self.tx_dir)), b"", &mut ct)
            .map_err(|_| RtspError::TooLarge)?;
        self.tx = seq;
        let mut f = (ENCRYPTED_BIT | plain.len() as u32).to_be_bytes().to_vec();
        f.extend(seq.to_be_bytes());
        f.extend(tag);
        f.extend(ct);
        Ok(f)
    }

    /// One whole frame, exactly (a reply is one frame then EOF; trailing bytes are refused).
    pub fn open(&mut self, f: &[u8]) -> Result<Vec<u8>, RtspError> {
        if f.len() <= HEADER {
            return Err(RtspError::Truncated);
        }
        let tl = u32::from_be_bytes(
            f[..4]
                .try_into()
                .expect("length checked: header is present"),
        );
        if tl & ENCRYPTED_BIT == 0 {
            return Err(RtspError::Unencrypted);
        }
        let len = (tl & !ENCRYPTED_BIT) as usize;
        if len > MAX_MESSAGE {
            return Err(RtspError::TooLarge);
        }
        match (HEADER + len).cmp(&f.len()) {
            std::cmp::Ordering::Greater => return Err(RtspError::Truncated),
            std::cmp::Ordering::Less => return Err(RtspError::Malformed),
            _ => {}
        }
        let seq = u32::from_be_bytes(
            f[4..8]
                .try_into()
                .expect("length checked: header is present"),
        );
        if seq <= self.rx {
            return Err(RtspError::Replay);
        }
        let mut pt = f[HEADER..].to_vec();
        self.gcm
            .decrypt_in_place_detached(
                Nonce::from_slice(&Self::iv(seq, self.rx_dir)),
                b"",
                &mut pt,
                Tag::from_slice(&f[8..HEADER]),
            )
            .map_err(|_| RtspError::BadTag)?;
        self.rx = seq;
        Ok(pt)
    }
}
