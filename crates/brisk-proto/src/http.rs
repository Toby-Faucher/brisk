//! GameStream HTTP(S) XML layer, client side: query builders and response parsers.
//! Shapes mirror Sunshine `src/nvhttp.cpp` (`serverinfo`, `applist`, `launch`, `cancel`).

use quick_xml::events::Event;
use quick_xml::Reader;

/// Responses larger than this are rejected, never truncated.
pub const MAX_BODY: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpError {
    TooLarge,
    /// Malformed or truncated XML.
    BadXml,
    /// Missing or non-numeric required field.
    BadField(&'static str),
    /// Server answered with a non-200 `status_code`.
    Status { code: u32, message: String },
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpError::TooLarge => write!(f, "http response too large"),
            HttpError::BadXml => write!(f, "http response is not valid xml"),
            HttpError::BadField(n) => write!(f, "http response missing or bad field {n}"),
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
        self.texts.iter().find(|(p, _)| p == path).map(|(_, t)| t.as_str())
    }
    fn field(&self, name: &'static str) -> Result<&str, HttpError> {
        self.get(&format!("root/{name}")).ok_or(HttpError::BadField(name))
    }
    fn num<T: std::str::FromStr>(&self, name: &'static str) -> Result<T, HttpError> {
        self.field(name)?.trim().parse().map_err(|_| HttpError::BadField(name))
    }
}

fn parse(xml: &str) -> Result<Doc, HttpError> {
    if xml.len() > MAX_BODY {
        return Err(HttpError::TooLarge);
    }
    let mut r = Reader::from_str(xml);
    let mut stack: Vec<String> = Vec::new();
    let mut texts = Vec::new();
    let mut buf = String::new(); // text of the current element; entities arrive as separate events
    let (mut code, mut message, mut seen_root) = (None, String::new(), false);
    loop {
        let ev = r.read_event().map_err(|_| HttpError::BadXml)?;
        match ev {
            Event::Text(ref t) => buf.push_str(&t.xml10_content()),
            Event::GeneralRef(ref g) => {
                buf.push_str(&quick_xml::escape::unescape(&format!("&{};", &**g)).map_err(|_| HttpError::BadXml)?)
            }
            Event::Start(_) | Event::Empty(_) | Event::End(_) | Event::Eof => {
                if !stack.is_empty() && !buf.trim().is_empty() {
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
                                let v = a.normalized_value(quick_xml::XmlVersion::Implicit1_0).map_err(|_| HttpError::BadXml)?.into_owned();
                                match a.key.as_ref() {
                                    "status_code" => code = v.trim().parse::<u32>().ok(),
                                    "status_message" => message = v,
                                    _ => {}
                                }
                            }
                        }
                        stack.push(name);
                        let path = stack.join("/");
                        if path == "root/App" {
                            texts.push((path, String::new()));
                        }
                        if matches!(ev, Event::Empty(_)) {
                            stack.pop();
                        }
                    }
                    Event::End(_) => {
                        stack.pop();
                    }
                    _ => break,
                }
            }
            _ => {}
        }
    }
    if !stack.is_empty() || !seen_root {
        return Err(HttpError::BadXml);
    }
    Ok(Doc { code: code.ok_or(HttpError::BadField("status_code"))?, message, texts })
}

fn ok(d: Doc) -> Result<Doc, HttpError> {
    if d.code == 200 {
        Ok(d)
    } else {
        Err(HttpError::Status { code: d.code, message: d.message })
    }
}

/// Ok for status 200, otherwise `HttpError::Status` carrying the server's message.
pub fn check_status(xml: &str) -> Result<(), HttpError> {
    ok(parse(xml)?).map(|_| ())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerInfo {
    pub app_version: String,
    pub gfe_version: String,
    pub unique_id: String,
    pub https_port: u16,
    /// `PairStatus`: Sunshine reports 1 only on HTTPS requests that carried `uniqueid`.
    pub paired: bool,
    pub current_game: u32,
    pub codec_mode_support: u32,
    pub server_state: String,
}

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
pub struct App {
    pub id: u32,
    pub title: String,
}

pub fn parse_applist(xml: &str) -> Result<Vec<App>, HttpError> {
    let d = ok(parse(xml)?)?;
    let mut apps = Vec::new();
    let mut cur: Option<(Option<u32>, Option<String>)> = None;
    let mut finish = |c: Option<(Option<u32>, Option<String>)>| -> Result<(), HttpError> {
        if let Some((id, title)) = c {
            apps.push(App { id: id.ok_or(HttpError::BadField("ID"))?, title: title.ok_or(HttpError::BadField("AppTitle"))? });
        }
        Ok(())
    };
    for (p, t) in &d.texts {
        match p.as_str() {
            "root/App" => finish(cur.replace((None, None)))?,
            "root/App/ID" => cur.as_mut().ok_or(HttpError::BadXml)?.0 = Some(t.trim().parse().map_err(|_| HttpError::BadField("ID"))?),
            "root/App/AppTitle" => cur.as_mut().ok_or(HttpError::BadXml)?.1 = Some(t.clone()),
            _ => {}
        }
    }
    finish(cur)?;
    Ok(apps)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchReply {
    /// `rtsp://` (or `rtspenc://` when launched with corever >= 1) URL of the RTSP server.
    pub session_url: String,
    pub game_session: u32,
}

pub fn parse_launch(xml: &str) -> Result<LaunchReply, HttpError> {
    let d = ok(parse(xml)?)?;
    let session_url = d.field("sessionUrl0")?.trim().to_string();
    if !(session_url.starts_with("rtsp://") || session_url.starts_with("rtspenc://")) {
        return Err(HttpError::BadField("sessionUrl0"));
    }
    Ok(LaunchReply { session_url, game_session: d.num("gamesession")? })
}

/// Fixed launch options beyond mode/keys; `Default` is stereo, no HDR, guest audio muted on host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchOpts {
    /// `localAudioPlayMode`: play audio on the host too.
    pub host_audio: bool,
    /// `surroundAudioInfo`: (channel mask << 16) | channel count; Sunshine default 196610 = stereo.
    pub surround_info: u32,
    /// `gcmap`: gamepad bitmask.
    pub gcmap: u32,
    /// `sops`: let the host optimise game settings for the mode.
    pub sops: bool,
    pub hdr: bool,
}

impl Default for LaunchOpts {
    fn default() -> Self {
        LaunchOpts { host_audio: false, surround_info: 196610, gcmap: 0, sops: false, hdr: false }
    }
}

fn hex_upper(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02X}")).collect()
}

/// Query for `GET /launch` (HTTPS). `corever=1` selects encrypted RTSP (`rtspenc://`, Gen7Enc),
/// keyed by `rikey` with IV prefix `rikeyid`. Required by Sunshine: rikey, rikeyid, localAudioPlayMode, appid.
#[allow(clippy::too_many_arguments)]
pub fn launch_query(unique_id: &str, app_id: u32, w: u16, h: u16, fps: u16, rikey: [u8; 16], rikeyid: u32, opts: &LaunchOpts) -> Vec<(String, String)> {
    let b = |v: bool| (v as u8).to_string();
    [
        ("uniqueid", unique_id.to_string()),
        ("appid", app_id.to_string()),
        ("mode", format!("{w}x{h}x{fps}")),
        ("sops", b(opts.sops)),
        ("rikey", hex_upper(&rikey)),
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
