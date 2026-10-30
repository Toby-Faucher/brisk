//! GameStream HTTP(S) XML layer, client side: query builders and response parsers.
//! Shapes mirror Sunshine `src/nvhttp.cpp` (`serverinfo`, `applist`, `launch`, `cancel`).

use quick_xml::Reader;
use quick_xml::events::Event;

/// Responses larger than this are rejected, never truncated.
pub const MAX_BODY: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
/// Why an HTTP reply was rejected.
pub enum HttpError {
    /// Body over [`MAX_BODY`].
    TooLarge,
    /// Malformed or truncated XML.
    BadXml,
    /// Missing or non-numeric required field.
    BadField(&'static str),
    /// HTTP framing we cannot read: no header end, bad status line or Content-Length, chunked,
    /// a body shorter than declared, non-UTF-8.
    Malformed(&'static str),
    /// Server answered with a non-200 `status_code`.
    Status {
        /// `status_code` from the reply.
        code: u32,
        /// `status_message` from the reply.
        message: String,
    },
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpError::TooLarge => write!(f, "http response too large"),
            HttpError::BadXml => write!(f, "http response is not valid xml"),
            HttpError::BadField(n) => write!(f, "http response missing or bad field {n}"),
            HttpError::Malformed(s) => f.write_str(s),
            HttpError::Status { code, message } => write!(f, "server status {code}: {message}"),
        }
    }
}
impl std::error::Error for HttpError {}

/// Parsed response: root status plus every text node as (element path, text).
/// Each `<App>` start also pushes a ("root/App", "") marker.
struct Doc {
    code: u32,
    message: String,
    texts: Vec<(String, String)>,
}

impl Doc {
    fn get(&self, path: &str) -> Option<&str> {
        self.texts
            .iter()
            .find(|(p, _)| p == path)
            .map(|(_, t)| t.as_str())
    }
    fn field(&self, name: &'static str) -> Result<&str, HttpError> {
        self.get(&format!("root/{name}"))
            .ok_or(HttpError::BadField(name))
    }
    fn num<T: std::str::FromStr>(&self, name: &'static str) -> Result<T, HttpError> {
        self.field(name)?
            .trim()
            .parse()
            .map_err(|_| HttpError::BadField(name))
    }
}

/// End of the header block (index of the blank line's `\r\n\r\n`), shared with RTSP framing.
pub(crate) fn header_end(raw: &[u8]) -> Option<usize> {
    raw.windows(4).position(|w| w == b"\r\n\r\n")
}

/// Whether a response must carry a `Content-Length` header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LengthHeader {
    /// A missing header is fine: the body runs to the end of the bytes.
    Optional,
    /// A missing header is an error (over TLS, where a close without close_notify is tolerated).
    Required,
}

/// Body of a raw HTTP/1.1 response; enforces Content-Length (see [`LengthHeader`]), rejects
/// chunked. A non-200 reply with an empty body is [`HttpError::Status`]; one with a body is
/// returned for the XML parser to report.
pub fn response_body(raw: &[u8], length: LengthHeader) -> Result<String, HttpError> {
    use HttpError::Malformed as bad;
    let end = header_end(raw).ok_or(bad("truncated http response (no header end)"))?;
    let head = std::str::from_utf8(&raw[..end]).map_err(|_| bad("non-utf8 http header"))?;
    let mut lines = head.split("\r\n");
    let status: u16 = lines
        .next()
        .and_then(|l| l.split(' ').nth(1))
        .and_then(|c| c.parse().ok())
        .ok_or(bad("bad http status line"))?;
    let mut body = &raw[end + 4..];
    let mut has_len = false;
    for (k, v) in lines.filter_map(|l| l.split_once(':')) {
        match k.trim().to_ascii_lowercase().as_str() {
            "transfer-encoding" => return Err(bad("chunked http response not supported")),
            "content-length" => {
                has_len = true;
                let n: usize = v.trim().parse().map_err(|_| bad("bad content-length"))?;
                if body.len() < n {
                    return Err(bad("truncated http response body"));
                }
                body = &body[..n];
            }
            _ => {}
        }
    }
    if length == LengthHeader::Required && !has_len {
        return Err(bad("https response without content-length"));
    }
    let body = String::from_utf8(body.to_vec()).map_err(|_| bad("non-utf8 http body"))?;
    if status != 200 && body.trim().is_empty() {
        return Err(HttpError::Status {
            code: status.into(),
            message: "empty body".into(),
        });
    }
    Ok(body) // non-200 with XML: the parser reports Sunshine's status_message
}

/// Text under a longer element path is not kept: every path a reply is read by is shorter
/// (`root/ServerCodecModeSupport`). Storing the path of each text node of a deeply nested or
/// long-named reply cost memory and time quadratic in its size (about 250 MB for 64 KiB).
const MAX_PATH: usize = 64;

fn pop(stack: &mut Vec<String>, path_len: &mut usize) {
    if let Some(n) = stack.pop() {
        *path_len -= n.len() + usize::from(!stack.is_empty());
    }
}

fn parse(xml: &str) -> Result<Doc, HttpError> {
    if xml.len() > MAX_BODY {
        return Err(HttpError::TooLarge);
    }
    let mut r = Reader::from_str(xml);
    let mut stack: Vec<String> = Vec::new();
    // Bytes of `stack.join("/")`, kept up to date so no event costs more than O(MAX_PATH).
    let mut path_len = 0usize;
    let mut texts = Vec::new();
    let mut buf = String::new(); // text of the current element; entities arrive as separate events
    let (mut code, mut message, mut seen_root) = (None, String::new(), false);
    loop {
        let ev = r.read_event().map_err(|_| HttpError::BadXml)?;
        match ev {
            Event::Text(ref t) => buf.push_str(&t.xml10_content()),
            Event::GeneralRef(ref g) => buf.push_str(
                &quick_xml::escape::unescape(&format!("&{};", &**g))
                    .map_err(|_| HttpError::BadXml)?,
            ),
            Event::Start(_) | Event::Empty(_) | Event::End(_) | Event::Eof => {
                if !stack.is_empty() && path_len <= MAX_PATH && !buf.trim().is_empty() {
                    texts.push((stack.join("/"), std::mem::take(&mut buf)));
                }
                buf.clear();
                match ev {
                    Event::Start(ref e) | Event::Empty(ref e) => {
                        let name = e.name().as_ref().to_string();
                        if stack.is_empty() {
                            if seen_root || name != "root" {
                                return Err(HttpError::BadXml);
                            }
                            seen_root = true;
                            for a in e.attributes() {
                                let a = a.map_err(|_| HttpError::BadXml)?;
                                let v = a
                                    .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                                    .map_err(|_| HttpError::BadXml)?
                                    .into_owned();
                                match a.key.as_ref() {
                                    "status_code" => code = v.trim().parse::<u32>().ok(),
                                    "status_message" => message = v,
                                    _ => {}
                                }
                            }
                        }
                        path_len += name.len() + usize::from(!stack.is_empty());
                        stack.push(name);
                        // stack[0] is always "root" (checked above).
                        if stack.len() == 2 && stack[1] == "App" {
                            texts.push(("root/App".to_string(), String::new()));
                        }
                        if matches!(ev, Event::Empty(_)) {
                            pop(&mut stack, &mut path_len);
                        }
                    }
                    Event::End(_) => pop(&mut stack, &mut path_len),
                    _ => break,
                }
            }
            _ => {}
        }
    }
    if !stack.is_empty() || !seen_root {
        return Err(HttpError::BadXml);
    }
    Ok(Doc {
        code: code.ok_or(HttpError::BadField("status_code"))?,
        message,
        texts,
    })
}

fn ok(d: Doc) -> Result<Doc, HttpError> {
    if d.code == 200 {
        Ok(d)
    } else {
        Err(HttpError::Status {
            code: d.code,
            message: d.message,
        })
    }
}

/// Ok for status 200, otherwise `HttpError::Status` carrying the server's message.
pub fn check_status(xml: &str) -> Result<(), HttpError> {
    ok(parse(xml)?).map(|_| ())
}

/// Text of `<root><name>` in a status-200 reply; pairing replies carry `paired`, `plaincert`,
/// `challengeresponse` and `pairingsecret` this way.
pub fn parse_field(xml: &str, name: &'static str) -> Result<String, HttpError> {
    Ok(ok(parse(xml)?)?.field(name)?.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// The fields of `/serverinfo` that Brisk uses.
pub struct ServerInfo {
    /// `appversion`.
    pub app_version: String,
    /// `GfeVersion`.
    pub gfe_version: String,
    /// `uniqueid` of the host.
    pub unique_id: String,
    /// `HttpsPort`.
    pub https_port: u16,
    /// `PairStatus`: Sunshine reports 1 only on HTTPS requests that carried `uniqueid`.
    pub paired: bool,
    /// `currentgame`: running app id, 0 when none.
    pub current_game: u32,
    /// `ServerCodecModeSupport` bits.
    pub codec_mode_support: u32,
    /// `state`, such as `SUNSHINE_SERVER_FREE` or `_BUSY`.
    pub server_state: String,
}

impl ServerInfo {
    /// `ServerCodecModeSupport` bits (moonlight-common-c `SCM_H264` 0x1, `SCM_HEVC` 0x100).
    pub fn supports(&self, c: crate::sdp::Codec) -> bool {
        let bit = match c {
            crate::sdp::Codec::H264 => 0x1,
            crate::sdp::Codec::Hevc => 0x100,
        };
        self.codec_mode_support & bit != 0
    }
}

/// Parses a `/serverinfo` reply.
pub fn parse_serverinfo(xml: &str) -> Result<ServerInfo, HttpError> {
    let d = ok(parse(xml)?)?;
    Ok(ServerInfo {
        app_version: d.field("appversion")?.to_string(),
        gfe_version: d.field("GfeVersion")?.to_string(),
        unique_id: d.field("uniqueid")?.to_string(),
        https_port: d.num("HttpsPort")?,
        paired: match d.num::<u8>("PairStatus")? {
            0 => false,
            1 => true,
            _ => return Err(HttpError::BadField("PairStatus")),
        },
        current_game: d.num("currentgame")?,
        codec_mode_support: d.num("ServerCodecModeSupport")?,
        server_state: d.field("state")?.to_string(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// One app from `/applist`.
pub struct App {
    /// App id to launch.
    pub id: u32,
    /// App title.
    pub title: String,
}

/// Parses an `/applist` reply.
pub fn parse_applist(xml: &str) -> Result<Vec<App>, HttpError> {
    let d = ok(parse(xml)?)?;
    let mut apps = Vec::new();
    let mut cur: Option<(Option<u32>, Option<String>)> = None;
    let mut finish = |c: Option<(Option<u32>, Option<String>)>| -> Result<(), HttpError> {
        if let Some((id, title)) = c {
            apps.push(App {
                id: id.ok_or(HttpError::BadField("ID"))?,
                title: title.ok_or(HttpError::BadField("AppTitle"))?,
            });
        }
        Ok(())
    };
    for (p, t) in &d.texts {
        match p.as_str() {
            "root/App" => finish(cur.replace((None, None)))?,
            "root/App/ID" => {
                cur.as_mut().ok_or(HttpError::BadXml)?.0 =
                    Some(t.trim().parse().map_err(|_| HttpError::BadField("ID"))?)
            }
            "root/App/AppTitle" => cur.as_mut().ok_or(HttpError::BadXml)?.1 = Some(t.clone()),
            _ => {}
        }
    }
    finish(cur)?;
    Ok(apps)
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// The fields of a `/launch` (or `/resume`) reply that Brisk uses.
pub struct LaunchReply {
    /// `rtsp://` (or `rtspenc://` when launched with corever >= 1) URL of the RTSP server.
    pub session_url: String,
    /// `gamesession`.
    pub game_session: u32,
}

/// Parses a `/launch` reply; the session URL must be `rtsp://` or `rtspenc://`.
pub fn parse_launch(xml: &str) -> Result<LaunchReply, HttpError> {
    let d = ok(parse(xml)?)?;
    let session_url = d.field("sessionUrl0")?.trim().to_string();
    if !(session_url.starts_with("rtsp://") || session_url.starts_with("rtspenc://")) {
        return Err(HttpError::BadField("sessionUrl0"));
    }
    Ok(LaunchReply {
        session_url,
        game_session: d.num("gamesession")?,
    })
}

/// Fixed launch options beyond mode/keys; `Default` is stereo, no HDR, guest audio muted on host,
/// one gamepad.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchOpts {
    /// `localAudioPlayMode`: play audio on the host too.
    pub host_audio: bool,
    /// `surroundAudioInfo`: (channel mask << 16) | channel count; Sunshine default 196610 = stereo.
    pub surround_info: u32,
    /// `gcmap`: attached-gamepad bitmask (moonlight sends its pads at launch). Sunshine only exports
    /// it to the app as `SUNSHINE_CLIENT_GCMAP` (nvhttp.cpp, process.cpp); a pad is created by its
    /// first multi-controller packet whatever this says (input.cpp `passthrough`).
    pub gcmap: u32,
    /// `sops`: let the host optimise game settings for the mode.
    pub sops: bool,
    /// `hdrMode`: ask for HDR.
    pub hdr: bool,
}

impl Default for LaunchOpts {
    fn default() -> Self {
        LaunchOpts {
            host_audio: false,
            surround_info: 196610,
            gcmap: 1, // pad 0: the one controller Brisk expects
            sops: false,
            hdr: false,
        }
    }
}

/// Query for `GET /launch` (HTTPS). `corever=1` selects encrypted RTSP (`rtspenc://`, Gen7Enc),
/// keyed by `rikey` with IV prefix `rikeyid`. Required by Sunshine: rikey, rikeyid, localAudioPlayMode, appid.
#[expect(clippy::too_many_arguments)]
pub fn launch_query(
    unique_id: &str,
    app_id: u32,
    w: u16,
    h: u16,
    fps: u16,
    rikey: [u8; 16],
    rikeyid: u32,
    opts: &LaunchOpts,
) -> Vec<(String, String)> {
    let b = |v: bool| (v as u8).to_string();
    [
        ("uniqueid", unique_id.to_string()),
        ("appid", app_id.to_string()),
        ("mode", format!("{w}x{h}x{fps}")),
        ("sops", b(opts.sops)),
        ("rikey", crate::pair::hex(&rikey)),
        ("rikeyid", rikeyid.to_string()),
        ("localAudioPlayMode", b(opts.host_audio)),
        ("surroundAudioInfo", opts.surround_info.to_string()),
        ("gcmap", opts.gcmap.to_string()),
        ("hdrMode", b(opts.hdr)),
        ("corever", "1".to_string()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

/// Query for `GET /cancel`.
pub fn cancel_query(unique_id: &str) -> Vec<(String, String)> {
    vec![("uniqueid".into(), unique_id.into())]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_body_checks_length_and_status() {
        let ok = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello trailing";
        assert_eq!(response_body(ok, LengthHeader::Optional).unwrap(), "hello");
        assert!(
            response_body(
                b"HTTP/1.1 200 OK\r\nContent-Length: 50\r\n\r\nshort",
                LengthHeader::Optional
            )
            .is_err()
        );
        assert!(
            response_body(
                b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello",
                LengthHeader::Optional
            )
            .is_err()
        );
        assert!(response_body(b"HTTP/1.1 404 Not Found\r\n\r\n", LengthHeader::Optional).is_err());
        assert!(response_body(b"HTTP/1.1 200 OK\r\nContent", LengthHeader::Optional).is_err());
        assert!(response_body(b"HTTP/1.1 200 OK\r\n\r\nhi", LengthHeader::Required).is_err());
        assert_eq!(
            response_body(
                b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nhi",
                LengthHeader::Required
            )
            .unwrap(),
            "hi"
        );
    }

    proptest::proptest! {
        /// Any response bytes (and any declared Content-Length): no panic, and the body is cut
        /// from the bytes that arrived, never sized by the header.
        #[test]
        fn response_body_takes_any_response(
            raw in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..400),
            n in proptest::prelude::any::<u64>(),
            tail in "[ -~]{0,80}",
        ) {
            let shaped = format!("HTTP/1.1 200 OK\r\nContent-Length: {n}\r\n\r\n{tail}");
            for r in [&raw[..], shaped.as_bytes()] {
                for length in [LengthHeader::Optional, LengthHeader::Required] {
                    if let Ok(b) = response_body(r, length) {
                        proptest::prop_assert!(b.len() <= r.len());
                    }
                }
            }
        }
    }
}
