//! Store page facts (`Info`) and the Store HTML-to-text helpers.
use super::{Entry, cached_with_misses};
use crate::config::Config;
use crate::steam::store_get;
use serde::{Deserialize, Serialize};

/// Store page facts for the detail panel. `available: false` when the Store has nothing (or is down).
#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    pub available: bool,
    /// Plain text: tags stripped, entities decoded.
    pub description: String,
    pub genres: Vec<String>,
    /// "full" or "partial".
    pub controller: Option<String>,
    pub singleplayer: bool,
    pub coop: bool,
    pub remote_play_together: bool,
    pub release: Option<String>,
    pub metacritic: Option<u32>,
    /// Thumbnail URLs, at most 6.
    pub screenshots: Vec<String>,
    /// The full-size image for each thumbnail, same order (the thumbnail again when the Store's
    /// full-size URL is not https). Empty in a cache entry written before this field existed.
    #[serde(default)]
    pub screenshots_full: Vec<String>,
}

pub(super) const WEEK_S: u64 = 7 * 86_400;

/// (controller, coop) from the info cache alone, no network. Reads the success entry, then the
/// outer one; an entry that is not `available` never counts. None when neither has data.
pub fn cached_controls(cfg: &Config, appid: u32) -> Option<(Option<String>, bool)> {
    ["info-{}-ok", "info-{}"].iter().find_map(|n| {
        let b = std::fs::read(cfg.data_dir.join(format!(
            "steam/{}.json",
            n.replace("{}", &appid.to_string())
        )))
        .ok()?;
        let e: Entry<Info> = serde_json::from_slice(&b).ok()?;
        let i = e.value;
        i.available
            .then_some((i.controller, i.coop || i.remote_play_together))
    })
}

pub async fn info(cfg: &Config, appid: u32, refresh: bool) -> Info {
    cached_with_misses(
        cfg,
        &format!("info-{appid}"),
        WEEK_S,
        refresh,
        fetch_info(cfg, appid),
    )
    .await
}

async fn fetch_info(cfg: &Config, appid: u32) -> Result<Info, String> {
    let v = store_get(cfg, &format!("api/appdetails?appids={appid}&l=english")).await?;
    let e = &v[appid.to_string()];
    if e["success"] != true {
        return Err("store has no details".into());
    }
    let d = &e["data"];
    let strs = |key: &str, field: &str| -> Vec<String> {
        d[key]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|x| x[field].as_str().map(String::from))
            .collect()
    };
    let cats: Vec<u64> = d["categories"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| c["id"].as_u64())
        .collect();
    let has = |ids: &[u64]| cats.iter().any(|c| ids.contains(c));
    let (screenshots, screenshots_full) = shots(&d["screenshots"]);
    Ok(Info {
        available: true,
        description: plain_text(d["short_description"].as_str().unwrap_or("")),
        genres: strs("genres", "description"),
        controller: d["controller_support"]
            .as_str()
            .filter(|c| matches!(*c, "full" | "partial"))
            .map(String::from),
        // Store category ids: 2 single-player; 9 co-op, 38 online co-op, 39 shared screen co-op,
        // 48 LAN co-op; 44 Remote Play Together.
        singleplayer: has(&[2]),
        coop: has(&[9, 38, 39, 48]),
        remote_play_together: has(&[44]),
        release: d["release_date"]["date"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(String::from),
        metacritic: d["metacritic"]["score"]
            .as_u64()
            .and_then(|n| n.try_into().ok()),
        screenshots,
        screenshots_full,
    })
}

/// (thumbnails, full-size) from the Store's `screenshots` array, at most 6 pairs. They become
/// `<img src>` in the UI: https only; a pair without an https thumbnail is dropped, a full-size URL
/// that is not https falls back to the thumbnail.
fn shots(list: &serde_json::Value) -> (Vec<String>, Vec<String>) {
    let https = |v: &serde_json::Value| {
        v.as_str()
            .filter(|u| u.starts_with("https://"))
            .map(String::from)
    };
    list.as_array()
        .into_iter()
        .flatten()
        .filter_map(|x| {
            let t = https(&x["path_thumbnail"])?;
            Some((https(&x["path_full"]).unwrap_or_else(|| t.clone()), t))
        })
        .take(6)
        .map(|(f, t)| (t, f))
        .unzip()
}

/// Store HTML to plain text: tags dropped (block ones become a space), then entities decoded, then
/// whitespace collapsed. Decoding last means `&lt;b&gt;` stays literal text; the UI never renders HTML.
pub(super) fn plain_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..i]);
        // only `<` + letter, `/` or `!` starts a tag; "a < b" stays text (`<` is ASCII, so i + 1 is a boundary)
        if !rest[i + 1..].starts_with(|c: char| c.is_ascii_alphabetic() || c == '/' || c == '!') {
            out.push('<');
            rest = &rest[i + 1..];
            continue;
        }
        let Some(j) = rest[i..].find('>') else {
            rest = "";
            break;
        };
        let name = rest[i + 1..i + j]
            .trim_start_matches('/')
            .to_ascii_lowercase();
        let name = name
            .split(|c: char| !c.is_ascii_alphanumeric())
            .next()
            .unwrap_or("");
        if matches!(
            name,
            "br" | "p" | "li" | "div" | "ul" | "ol" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
        ) {
            out.push(' ');
        }
        rest = &rest[i + j + 1..];
    }
    out.push_str(rest);
    decode_entities(&out)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        // bytes, not a str slice: byte 12 can fall inside a multi-byte char. `&` and `;` are ASCII, so
        // slicing at them below is always on a char boundary.
        let end = rest.bytes().take(12).position(|b| b == b';');
        let ch = end.and_then(|e| match &rest[1..e] {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            n => n
                .strip_prefix("#x")
                .or_else(|| n.strip_prefix("#X"))
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| n.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        });
        match (ch, end) {
            (Some(c), Some(e)) => {
                out.push(c);
                rest = &rest[e + 1..];
            }
            _ => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shots_pairs_thumbnails_with_full_size_https_only() {
        let (t, f) = shots(&serde_json::json!([
            {"path_thumbnail": "http://a/0.jpg", "path_full": "https://a/0f.jpg"},
            {"path_thumbnail": "https://a/1.jpg", "path_full": "https://a/1f.jpg"},
            {"path_thumbnail": "https://a/2.jpg", "path_full": "http://a/2f.jpg"},
            {"path_thumbnail": "https://a/3.jpg"},
        ]));
        assert_eq!(t, ["https://a/1.jpg", "https://a/2.jpg", "https://a/3.jpg"]);
        assert_eq!(
            f,
            ["https://a/1f.jpg", "https://a/2.jpg", "https://a/3.jpg"]
        );
        let many: Vec<_> = (0..9)
            .map(|i| serde_json::json!({"path_thumbnail": format!("https://a/{i}.jpg")}))
            .collect();
        assert_eq!(shots(&serde_json::Value::from(many)).0.len(), 6);
        assert_eq!(shots(&serde_json::Value::Null), (vec![], vec![]));
    }

    #[test]
    fn plain_text_strips_and_decodes() {
        assert_eq!(
            plain_text("a<br/>b &lt;i&gt; &#x27;c&#39; &bogus; & d<b"),
            "a b <i> 'c' &bogus; & d"
        );
        // `&` followed by multi-byte text: the entity scan must never cut inside a character
        for t in [
            "& ééééééé;",
            "&éééééééééééé;",
            "Q&—A™",
            "&™ and &—;",
            "tail &",
        ] {
            assert_eq!(plain_text(t), t);
        }
        // a `<` that does not start a tag stays text
        assert_eq!(plain_text("a < b, 3<4 <i>x</i>"), "a < b, 3<4 x");
    }
}
