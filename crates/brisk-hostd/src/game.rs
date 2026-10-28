//! The one game model for every source: ids, the `Game` the API returns, and `Caps`.
//!
//! Ids are strings: `steam:<appid>` and `emu:<emulator>:<stable id>`. The stable id is chosen by the
//! emulator library record (a slug of the name today, a disc-header id later), so the scheme only
//! fixes the shape. Old numeric ids still read in requests (see [`GameId::compat`]) for one release.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Emulator games used ids from here up in the first emulator version (a hash of the name).
pub const LEGACY_EMU_BASE: u32 = 0xE000_0000;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Steam,
    Emu,
}

/// A validated game id. Only the two shapes above exist; the type is why no handler needs to
/// look at an id range.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GameId(String);

fn id_part(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'~' | b'-'))
}

impl GameId {
    pub fn steam(appid: u32) -> GameId {
        GameId(format!("steam:{appid}"))
    }

    /// `emu:<emulator>:<id>`; None when a part is empty or has characters outside `A-Za-z0-9._~-`.
    pub fn emu(emulator: &str, id: &str) -> Option<GameId> {
        let e_ok = !emulator.is_empty()
            && emulator.len() <= 32
            && emulator
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
        (e_ok && id_part(id)).then(|| GameId(format!("emu:{emulator}:{id}")))
    }

    /// The new form only. `steam:0620` is not canonical and is refused.
    pub fn parse(s: &str) -> Option<GameId> {
        if let Some(n) = s.strip_prefix("steam:") {
            let n: u32 = n.parse().ok()?;
            return Some(GameId::steam(n)).filter(|g| g.0 == s);
        }
        let rest = s.strip_prefix("emu:")?;
        let (emulator, id) = rest.split_once(':')?;
        GameId::emu(emulator, id)
    }

    /// What a request may carry: the new form, or a bare number. A bare number is a Steam appid,
    /// unless it is in the old emulator range, where `legacy` maps it to the new id (None when the
    /// machine does not know it).
    pub fn compat(raw: &str, legacy: impl Fn(u32) -> Option<GameId>) -> Option<GameId> {
        if raw.bytes().all(|b| b.is_ascii_digit()) && !raw.is_empty() {
            let n: u32 = raw.parse().ok()?;
            return if n >= LEGACY_EMU_BASE {
                legacy(n)
            } else {
                Some(GameId::steam(n))
            };
        }
        GameId::parse(raw)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn source(&self) -> Source {
        if self.0.starts_with("steam:") {
            Source::Steam
        } else {
            Source::Emu
        }
    }

    pub fn steam_appid(&self) -> Option<u32> {
        self.0.strip_prefix("steam:")?.parse().ok()
    }

    /// `(emulator, stable id)` of an emulator game.
    pub fn emu_parts(&self) -> Option<(&str, &str)> {
        self.0.strip_prefix("emu:")?.split_once(':')
    }
}

impl fmt::Display for GameId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for GameId {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for GameId {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        GameId::parse(&s).ok_or_else(|| serde::de::Error::custom("bad game id"))
    }
}

/// What the source can do with this game. The web app shows a control only when its flag is set.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Caps {
    pub install: bool,
    pub controls: bool,
    pub achievements: bool,
    pub launch_options: bool,
    pub saves: bool,
    pub screenshots: bool,
    /// Art and details come from the cache the `art` module fills (emulator games).
    pub art: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Game {
    /// `steam:620`, `emu:dolphin:mario-kart-wii`.
    pub id: GameId,
    pub source: Source,
    /// Old numeric id, kept for one release so older web builds work: the Steam appid, or for an
    /// emulator game its old 0xE0000000+ hash.
    pub appid: u32,
    /// The same number as decimal text (what notes and prefs were keyed by before the new ids).
    pub legacy_id: Option<String>,
    pub caps: Caps,
    /// "wii", "gamecube"; None for Steam and while a record has no system yet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    pub name: String,
    pub installed: bool,
    pub size_bytes: Option<u64>,
    pub last_played: Option<u64>,
    pub playtime_min: u64,
    /// Minutes played in the last two weeks (Steam leaves the field out when 0).
    pub playtime2wk_min: u64,
    pub cover: String,
    /// Wide library art (404s for some games: the UI falls back) and the title logo.
    pub hero: String,
    pub logo: String,
    /// The source is running it now.
    pub running: bool,
    /// Achievement counts from the cache (filled by the background warm); None until known.
    pub ach_unlocked: Option<u32>,
    pub ach_total: Option<u32>,
    /// Installed, and a patch note newer than the last play (from the news cache).
    pub updated: bool,
    /// Steam's own update state of an installed game: "needed" or "running" (from StateFlags).
    pub update: Option<String>,
    /// "full" or "partial", from the info cache (None until warmed or when the Store has none).
    pub controller: Option<String>,
    /// Co-op or Remote Play Together, from the info cache.
    pub coop: bool,
    /// Emulator games only (e.g. "dolphin"): the card's system badge.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emulator: Option<String>,
    /// Description fields from the art cache (emulator games, only what a source gave).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Details>,
    /// Who the cached art and details came from, for the credit line (`SteamGridDB`, `GameTDB`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub credits: Vec<String>,
    /// Art lookup found several possible games and waits for a pick.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub needs_match: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Details {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<u16>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub publisher: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub developer: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub languages: Vec<String>,
}

pub const STEAM_CAPS: Caps = Caps {
    install: true,
    controls: false,
    achievements: true,
    launch_options: true,
    saves: false,
    screenshots: true,
    art: false,
};

impl Game {
    /// A Steam game with nothing known yet beyond its id and name.
    pub fn steam(appid: u32, name: &str) -> Game {
        Game {
            id: GameId::steam(appid),
            source: Source::Steam,
            appid,
            legacy_id: Some(appid.to_string()),
            caps: STEAM_CAPS,
            system: None,
            name: name.into(),
            installed: false,
            size_bytes: None,
            last_played: None,
            playtime_min: 0,
            playtime2wk_min: 0,
            cover: String::new(),
            hero: String::new(),
            logo: String::new(),
            running: false,
            ach_unlocked: None,
            ach_total: None,
            updated: false,
            update: None,
            controller: None,
            coop: false,
            emulator: None,
            details: None,
            credits: vec![],
            needs_match: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_parse_in_the_two_shapes_only() {
        for ok in [
            "steam:620",
            "steam:0",
            "emu:dolphin:RMCE01",
            "emu:dolphin:a.b_c~2-d1",
        ] {
            assert_eq!(GameId::parse(ok).unwrap().as_str(), ok);
        }
        for bad in [
            "",
            "620",
            "steam:",
            "steam:x",
            "steam:0620",
            "steam:+5",
            "steam:-1",
            "steam:4294967296",
            "steam:620:x",
            "emu:",
            "emu:dolphin",
            "emu:dolphin:",
            "emu::x",
            "emu:Dolphin:x",
            "emu:dolphin:a:b",
            "emu:dolphin:a b",
            "emu:dolphin:a/b",
            "emu:dolphin:a%3Ab",
            "gog:5",
            "STEAM:5",
        ] {
            assert!(GameId::parse(bad).is_none(), "{bad:?}");
        }
        assert!(GameId::parse(&format!("emu:dolphin:{}", "a".repeat(65))).is_none());
        assert!(GameId::parse(&format!("emu:dolphin:{}", "a".repeat(64))).is_some());
    }

    #[test]
    fn parts_and_source() {
        let s = GameId::parse("steam:620").unwrap();
        assert_eq!(
            (s.source(), s.steam_appid(), s.emu_parts()),
            (Source::Steam, Some(620), None)
        );
        let e = GameId::parse("emu:dolphin:mkw").unwrap();
        assert_eq!(
            (e.source(), e.steam_appid(), e.emu_parts()),
            (Source::Emu, None, Some(("dolphin", "mkw")))
        );
    }

    #[test]
    fn compat_reads_bare_numbers_as_steam_or_known_legacy_emulator_ids() {
        let known = |n: u32| (n == 0xE123_4567).then(|| GameId::parse("emu:dolphin:mkw").unwrap());
        let c = |s: &str| GameId::compat(s, known).map(|g| g.to_string());
        assert_eq!(c("620").as_deref(), Some("steam:620"));
        assert_eq!(c("steam:620").as_deref(), Some("steam:620"));
        assert_eq!(c("emu:dolphin:mkw").as_deref(), Some("emu:dolphin:mkw"));
        assert_eq!(
            c(&0xE123_4567u32.to_string()).as_deref(),
            Some("emu:dolphin:mkw")
        );
        // old emulator range but not known here, too big, signs, spaces, junk
        for bad in [
            "3758096385",
            "4294967296",
            "-1",
            "+1",
            " 1",
            "1 ",
            "1.5",
            "",
            "x",
            "620;ls",
        ] {
            assert_eq!(c(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn serde_roundtrip_and_refusal() {
        let g = GameId::steam(5);
        assert_eq!(serde_json::to_string(&g).unwrap(), r#""steam:5""#);
        assert_eq!(serde_json::from_str::<GameId>(r#""steam:5""#).unwrap(), g);
        assert!(serde_json::from_str::<GameId>("5").is_err());
        assert!(serde_json::from_str::<GameId>(r#""5""#).is_err());
    }

    #[test]
    fn steam_game_shape_keeps_the_old_fields_and_adds_the_new() {
        let v = serde_json::to_value(Game::steam(620, "Portal 2")).unwrap();
        assert_eq!(v["appid"], 620);
        assert_eq!(v["id"], "steam:620");
        assert_eq!(v["source"], "steam");
        assert_eq!(v["legacyId"], "620");
        assert_eq!(v["caps"]["install"], true);
        assert_eq!(v["caps"]["controls"], false);
        assert!(v.get("system").is_none() && v.get("emulator").is_none());
    }
}
