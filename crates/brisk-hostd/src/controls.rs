//! The Dolphin controller editor's file side (docs/plans/2026-10-27-emulator-controls.md): reads and
//! rewrites the `[Wiimote1]` section of `WiimoteNew.ini` for a Wii Remote 1 with a Classic Controller,
//! fed by the Xbox-layout pad the stream shows. Line based, so everything it does not own survives
//! byte for byte. Values come only from the tables below, picked by an already validated pad name or
//! a number in 0 to 50; user text never reaches the file. Written whole to a 0600 temp file and
//! renamed over the old one, with a one-time backup of the original.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The known-good file; used when the machine has no file or no `[Wiimote1]` section yet.
const DEFAULT_INI: &str = include_str!("../../../deploy/dolphin/WiimoteNew.ini");
const SECTION: &str = "[Wiimote1]";
const BACKUP: &str = "WiimoteNew.ini.before-brisk-edit";
const FILE: &str = "WiimoteNew.ini";

pub const IDS: [&str; 14] = [
    "a", "b", "x", "y", "l", "r", "zl", "zr", "minus", "plus", "home", "ls", "rs", "dp",
];
const BUTTONS: [&str; 11] = [
    "A", "B", "X", "Y", "LB", "RB", "LT", "RT", "View", "Menu", "Guide",
];
const STICKS: [&str; 2] = ["Left stick", "Right stick"];
const DPAD: [&str; 1] = ["D-pad"];

/// Pad names a control takes (the allowlist, also in the web's `ACCEPTS`).
fn accepts(id: &str) -> &'static [&'static str] {
    match id {
        "ls" | "rs" => &STICKS,
        "dp" => &DPAD,
        _ => &BUTTONS,
    }
}

/// Dolphin's name for a pad button (plain, without backticks).
fn button_expr(pad: &str) -> Option<&'static str> {
    Some(match pad {
        "A" => "SOUTH",
        "B" => "EAST",
        "X" => "WEST",
        "Y" => "NORTH",
        "LB" => "TL",
        "RB" => "TR",
        "LT" => "Full Axis 2+",
        "RT" => "Full Axis 5+",
        "View" => "SELECT",
        "Menu" => "START",
        "Guide" => "MODE",
        _ => return None,
    })
}

/// Up, Down, Left, Right for a stick or the d-pad hat.
fn group_exprs(pad: &str) -> Option<[&'static str; 4]> {
    Some(match pad {
        "Left stick" => ["Axis 1-", "Axis 1+", "Axis 0-", "Axis 0+"],
        "Right stick" => ["Axis 4-", "Axis 4+", "Axis 3-", "Axis 3+"],
        "D-pad" => ["Axis 7-", "Axis 7+", "Axis 6-", "Axis 6+"],
        _ => return None,
    })
}

const D: [&str; 4] = ["Up", "Down", "Left", "Right"];

/// Classic keys (the source of truth when reading) and the basic Wii Remote mirrors (write-only),
/// in the order of `button_expr` / `group_exprs` values.
fn keys(id: &str) -> (Vec<String>, Vec<String>) {
    let s = |v: &[&str]| v.iter().map(|k| k.to_string()).collect::<Vec<_>>();
    let group = |c: &str, m: &str| {
        (
            D.map(|d| format!("{c}/{d}")).to_vec(),
            if m.is_empty() {
                vec![]
            } else {
                D.map(|d| format!("{m}/{d}")).to_vec()
            },
        )
    };
    match id {
        "a" => (s(&["Classic/Buttons/A"]), s(&["Buttons/A"])),
        "b" => (s(&["Classic/Buttons/B"]), s(&["Buttons/B"])),
        "x" => (s(&["Classic/Buttons/X"]), s(&["Buttons/1"])),
        "y" => (s(&["Classic/Buttons/Y"]), s(&["Buttons/2"])),
        "zl" => (s(&["Classic/Buttons/ZL"]), vec![]),
        "zr" => (s(&["Classic/Buttons/ZR"]), vec![]),
        "l" => (
            s(&["Classic/Triggers/L", "Classic/Triggers/L-Analog"]),
            vec![],
        ),
        "r" => (
            s(&["Classic/Triggers/R", "Classic/Triggers/R-Analog"]),
            vec![],
        ),
        "minus" => (s(&["Classic/Buttons/-"]), s(&["Buttons/-"])),
        "plus" => (s(&["Classic/Buttons/+"]), s(&["Buttons/+"])),
        "home" => (s(&["Classic/Buttons/Home"]), s(&["Buttons/Home"])),
        "ls" => group("Classic/Left Stick", ""),
        "rs" => group("Classic/Right Stick", ""),
        _ => group("Classic/D-Pad", "D-Pad"),
    }
}

/// The plain expression for each Classic key of `id` bound to `pad`.
fn exprs(id: &str, pad: &str) -> Vec<&'static str> {
    if matches!(id, "ls" | "rs" | "dp") {
        group_exprs(pad).map(|r| r.to_vec()).unwrap_or_default()
    } else {
        let e = button_expr(pad).unwrap_or_default();
        vec![e; keys(id).0.len()]
    }
}

fn quote(e: &str) -> String {
    if e.contains(' ') {
        format!("`{e}`")
    } else {
        e.into()
    }
}

/// shortcut: the dead zone key names and number format are from memory of Dolphin's own files, not
/// checked against a file Dolphin wrote. Check when Toby can set a dead zone in Dolphin's Wii Remote 1
/// window (Classic Controller tab), then fix only this module and the doc's section 3.
mod dead_zone {
    pub fn key(id: &str) -> String {
        let stick = if id == "ls" {
            "Left Stick"
        } else {
            "Right Stick"
        };
        format!("Classic/{stick}/Dead Zone")
    }
    /// Whole percent 0 to 50 from `12`, `12.0` or `12.000000`; None for anything else.
    pub fn parse(v: &str) -> Option<u8> {
        let f: f64 = v.trim().parse().ok()?;
        (f.is_finite() && (0.0..=50.0).contains(&f)).then(|| f.round() as u8)
    }
    pub fn format(n: u8) -> String {
        format!("{n}.000000")
    }
}

// ---- reading and rewriting the ini text ----

/// Lines with their own endings (the last may have none).
fn split(text: &str) -> Vec<String> {
    text.split_inclusive('\n').map(String::from).collect()
}

fn body(l: &str) -> &str {
    l.trim_end_matches(['\r', '\n'])
}

/// (header index, end exclusive) of `[Wiimote1]`: ends at the next line that starts a section.
fn section(lines: &[String]) -> Option<(usize, usize)> {
    let h = lines.iter().position(|l| l.trim() == SECTION)?;
    let e = lines[h + 1..]
        .iter()
        .position(|l| l.trim().starts_with('['))
        .map_or(lines.len(), |p| h + 1 + p);
    Some((h, e))
}

fn kv(line: &str) -> Option<(&str, &str)> {
    let t = line.trim();
    if t.starts_with('#') || t.starts_with(';') {
        return None;
    }
    let (k, v) = t.split_once('=')?;
    Some((k.trim(), v))
}

/// Strips one pair of backticks, trims, collapses inner spaces. None when a backtick is left.
fn norm(v: &str) -> Option<String> {
    let v = v.trim();
    let v = match v.strip_prefix('`') {
        Some(r) => r.strip_suffix('`')?,
        None => v,
    };
    if v.contains('`') {
        return None;
    }
    Some(v.split_whitespace().collect::<Vec<_>>().join(" "))
}

#[derive(Debug, PartialEq)]
struct Read {
    bindings: [Option<&'static str>; 14],
    custom: [bool; 14],
    dz: [u8; 2],
    found: bool,
}

fn read_text(text: &str) -> Read {
    let lines = split(text);
    let Some((h, e)) = section(&lines) else {
        return read_text(DEFAULT_INI).not_found();
    };
    // Last duplicate wins, as an ini loader would take it.
    let vals: BTreeMap<&str, &str> = lines[h + 1..e].iter().filter_map(|l| kv(l)).collect();
    let mut r = Read {
        bindings: [None; 14],
        custom: [true; 14],
        dz: [0; 2],
        found: true,
    };
    let mut used: Vec<&str> = vec![];
    for (i, id) in IDS.iter().enumerate() {
        let got: Option<Vec<String>> = keys(id)
            .0
            .iter()
            .map(|k| vals.get(k.as_str()).and_then(|v| norm(v)))
            .collect();
        let pad = got.and_then(|g| {
            accepts(id).iter().copied().find(|p| {
                let want = exprs(id, p);
                want.len() == g.len() && want.iter().zip(&g).all(|(w, x)| w == x)
            })
        });
        if let Some(p) = pad.filter(|p| !used.contains(p)) {
            used.push(p);
            r.bindings[i] = Some(p);
            r.custom[i] = false;
        }
    }
    for (n, id) in ["ls", "rs"].iter().enumerate() {
        r.dz[n] = vals
            .get(dead_zone::key(id).as_str())
            .and_then(|v| dead_zone::parse(v))
            .unwrap_or(0);
    }
    r
}

impl Read {
    fn not_found(mut self) -> Read {
        self.found = false;
        self
    }
}

enum Op {
    Set(String, String),
    Remove(String),
}

/// Adds the embedded `[Wiimote1]` block when `text` has none.
fn ensure_section(text: &str) -> String {
    let lines = split(text);
    if section(&lines).is_some() {
        return text.into();
    }
    let eol = eol_of(&lines);
    let mut out = text.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push_str(eol);
    }
    out.push_str(&DEFAULT_INI.replace('\n', eol));
    out
}

fn eol_of(lines: &[String]) -> &'static str {
    if lines.first().is_some_and(|l| l.ends_with("\r\n")) {
        "\r\n"
    } else {
        "\n"
    }
}

/// Applies the ops inside `[Wiimote1]` (which must exist). Matching lines (duplicates too) keep
/// their key text; a missing key goes after the section's last non-blank line.
fn apply(text: &str, ops: &[Op]) -> String {
    let mut lines = split(text);
    let eol = eol_of(&lines);
    for op in ops {
        let Some((h, e)) = section(&lines) else {
            break;
        };
        let (key, val) = match op {
            Op::Set(k, v) => (k, Some(v)),
            Op::Remove(k) => (k, None),
        };
        let hits: Vec<usize> = (h + 1..e)
            .filter(|&i| kv(&lines[i]).is_some_and(|(k, _)| k == key))
            .collect();
        match val {
            None => {
                for i in hits.into_iter().rev() {
                    lines.remove(i);
                }
            }
            Some(v) if !hits.is_empty() => {
                for i in hits {
                    let end = &lines[i][body(&lines[i]).len()..];
                    lines[i] = format!("{key} = {v}{end}");
                }
            }
            Some(v) => {
                let at = (h + 1..e)
                    .rev()
                    .find(|&i| !lines[i].trim().is_empty())
                    .map_or(h + 1, |i| i + 1);
                if !lines[at - 1].ends_with('\n') {
                    lines[at - 1].push_str(eol);
                }
                lines.insert(at, format!("{key} = {v}{eol}"));
            }
        }
    }
    lines.concat()
}

/// Set ops for one control bound to `pad` (Classic keys and mirrors).
fn bind_ops(id: &str, pad: &str) -> Vec<Op> {
    let (classic, mirror) = keys(id);
    let ex = exprs(id, pad);
    classic
        .into_iter()
        .chain(mirror)
        .enumerate()
        .map(|(i, k)| Op::Set(k, quote(ex[i % ex.len()])))
        .collect()
}

// ---- the API's data ----

#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub emulator: &'static str,
    pub controller: &'static str,
    pub source: &'static str,
    pub rev: String,
    pub running: bool,
    pub applies_on: &'static str,
    pub bindings: BTreeMap<&'static str, Option<&'static str>>,
    pub dead_zone: BTreeMap<&'static str, u8>,
    pub custom: BTreeMap<&'static str, bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Dz {
    ls: i64,
    rs: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Put {
    rev: Option<String>,
    bindings: BTreeMap<String, Option<String>>,
    dead_zone: Dz,
}

#[derive(Debug)]
pub enum CtlError {
    Bad(&'static str),
    /// The file changed since `rev`; carries the current state.
    Changed(Box<View>),
    Io,
}

fn rev_of(bytes: Option<&[u8]>) -> String {
    use std::hash::Hasher;
    // shortcut: DefaultHasher is only compared inside one hostd run; use a real digest if rev must
    // outlive restarts.
    bytes.map_or("none".into(), |b| {
        let mut h = std::hash::DefaultHasher::new();
        h.write(b);
        format!("{:016x}", h.finish())
    })
}

/// All checks that need no file. Returns the validated bindings and dead zones.
#[allow(clippy::type_complexity)]
fn validate(b: &[u8]) -> Result<(Put, Vec<Option<&'static str>>, [u8; 2]), CtlError> {
    let p: Put = serde_json::from_slice(b).map_err(|_| CtlError::Bad("bad request"))?;
    if p.rev.as_ref().is_some_and(|r| r.chars().count() > 32) {
        return Err(CtlError::Bad("rev too long"));
    }
    if p.bindings.len() != IDS.len() || IDS.iter().any(|i| !p.bindings.contains_key(*i)) {
        return Err(CtlError::Bad("bindings must list the 14 controls"));
    }
    let mut pads = vec![];
    for id in IDS {
        pads.push(match &p.bindings[id] {
            None => None,
            Some(v) => Some(
                *accepts(id)
                    .iter()
                    .find(|a| **a == v)
                    .ok_or(CtlError::Bad("that button does not fit that control"))?,
            ),
        });
    }
    let named: Vec<_> = pads.iter().flatten().collect();
    if named
        .iter()
        .enumerate()
        .any(|(i, n)| named[..i].contains(n))
    {
        return Err(CtlError::Bad("a pad button can drive only one control"));
    }
    let dz = |n: i64| u8::try_from(n).ok().filter(|n| *n <= 50);
    let (Some(ls), Some(rs)) = (dz(p.dead_zone.ls), dz(p.dead_zone.rs)) else {
        return Err(CtlError::Bad("dead zone must be 0 to 50"));
    };
    Ok((p, pads, [ls, rs]))
}

/// One machine's `WiimoteNew.ini`. The mutex covers read, edit and write together.
pub struct Controls {
    dir: PathBuf,
    lock: Mutex<()>,
}

impl Controls {
    pub fn new(dir: PathBuf) -> Controls {
        Controls {
            dir,
            lock: Mutex::new(()),
        }
    }

    /// The file to read and replace: a symlinked config resolves to its target.
    fn path(&self) -> PathBuf {
        let p = self.dir.join(FILE);
        std::fs::canonicalize(&p).unwrap_or(p)
    }

    /// File bytes (None = no file). A non-UTF-8 file is an error: nothing may be written over it.
    fn load(&self) -> Result<(Option<Vec<u8>>, String), CtlError> {
        match std::fs::read(self.path()) {
            Ok(b) => {
                let t = String::from_utf8(b.clone()).map_err(|_| CtlError::Io)?;
                Ok((Some(b), t))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok((None, DEFAULT_INI.into())),
            Err(_) => Err(CtlError::Io),
        }
    }

    fn view(&self, running: bool) -> Result<View, CtlError> {
        let (bytes, text) = self.load()?;
        let r = read_text(&text);
        Ok(View {
            emulator: "dolphin",
            controller: "Wii Remote 1, Classic Controller",
            source: if bytes.is_some() && r.found {
                "file"
            } else {
                "default"
            },
            rev: rev_of(bytes.as_deref()),
            running,
            applies_on: "next-launch",
            bindings: IDS.iter().copied().zip(r.bindings).collect(),
            dead_zone: BTreeMap::from([("ls", r.dz[0]), ("rs", r.dz[1])]),
            custom: IDS.iter().copied().zip(r.custom).collect(),
        })
    }

    pub fn get(&self, running: bool) -> Result<View, CtlError> {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        self.view(running)
    }

    pub fn put(&self, body: &[u8], running: bool) -> Result<View, CtlError> {
        let (p, pads, dz) = validate(body)?;
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let (bytes, text) = self.load()?;
        if p.rev.is_some_and(|r| r != rev_of(bytes.as_deref())) {
            return Err(CtlError::Changed(Box::new(self.view(running)?)));
        }
        let cur = read_text(&text);
        let mut ops = vec![];
        for (i, pad) in pads.iter().enumerate() {
            match pad {
                Some(pad) => ops.extend(bind_ops(IDS[i], pad)),
                None if !cur.custom[i] => {
                    return Err(CtlError::Bad("only a custom control can be left alone"));
                }
                None => {}
            }
        }
        for (n, id) in ["ls", "rs"].iter().enumerate() {
            if dz[n] != cur.dz[n] {
                ops.push(Op::Set(dead_zone::key(id), dead_zone::format(dz[n])));
            }
        }
        self.save(bytes, &text, &ops)?;
        self.view(running)
    }

    /// Xbox default for all 14 controls (custom ones included); the dead zone keys are removed.
    pub fn reset(&self, running: bool) -> Result<View, CtlError> {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let (bytes, text) = self.load()?;
        let def = read_text(DEFAULT_INI);
        let mut ops: Vec<Op> = IDS
            .iter()
            .zip(def.bindings)
            .flat_map(|(id, p)| bind_ops(id, p.unwrap_or_default()))
            .collect();
        ops.extend(["ls", "rs"].map(|id| Op::Remove(dead_zone::key(id))));
        self.save(bytes, &text, &ops)?;
        self.view(running)
    }

    fn save(&self, orig: Option<Vec<u8>>, text: &str, ops: &[Op]) -> Result<(), CtlError> {
        let new = apply(&ensure_section(text), ops);
        let path = self.path();
        let io = |e: std::io::Error| {
            eprintln!("warning: {}: {e}", path.display());
            CtlError::Io
        };
        let dir = path.parent().unwrap_or(Path::new("."));
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .map_err(io)?;
        if let Some(o) = &orig {
            backup_once(dir, o).map_err(io)?;
        }
        write_atomic(&path, new.as_bytes()).map_err(io)
    }
}

/// Copies the original once; an existing backup holds the first original and is never touched.
fn backup_once(dir: &Path, bytes: &[u8]) -> std::io::Result<()> {
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(dir.join(BACKUP))
    {
        Ok(mut f) => f.write_all(bytes).and_then(|_| f.sync_all()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(e),
    }
}

/// Temp file in the same directory (0600, fsynced), then rename; a failure removes the temp file.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let tmp = dir.join(format!(".{FILE}.{}.tmp", std::process::id()));
    let _ = std::fs::remove_file(&tmp); // a leftover of a crashed run
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)?;
    let r = f
        .write_all(bytes)
        .and_then(|_| f.sync_all())
        .and_then(|_| std::fs::rename(&tmp, path));
    if r.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad_of(text: &str, id: &str) -> Option<&'static str> {
        read_text(text).bindings[IDS.iter().position(|i| *i == id).unwrap()]
    }

    fn set(text: &str, id: &str, pad: &str) -> String {
        apply(&ensure_section(text), &bind_ops(id, pad))
    }

    fn pairs(text: &str) -> BTreeMap<String, String> {
        text.lines()
            .filter_map(kv)
            .map(|(k, v)| (k.into(), v.trim().into()))
            .collect()
    }

    #[test]
    fn round_trip_every_control_and_name() {
        for id in IDS {
            for pad in accepts(id) {
                let t = set("[Wiimote1]\n", id, pad);
                assert_eq!(pad_of(&t, id), Some(*pad), "{id} {pad}");
                let m = pairs(&t);
                let (classic, mirror) = keys(id);
                let ex = exprs(id, pad);
                for (i, k) in classic.iter().chain(&mirror).enumerate() {
                    assert_eq!(m[k], quote(ex[i % ex.len()]), "{id} {pad} {k}");
                }
            }
        }
    }

    #[test]
    fn default_matches_repo_file() {
        let r = read_text(DEFAULT_INI);
        assert!(r.found && r.custom == [false; 14] && r.dz == [0, 0]);
        let want: Vec<_> = "B A Y X LT RT LB RB View Menu Guide Left stick Right stick D-pad"
            .replace("Left stick", "Left_stick")
            .replace("Right stick", "Right_stick")
            .split(' ')
            .map(|s| s.replace('_', " "))
            .collect();
        assert_eq!(r.bindings.map(|b| b.unwrap()).to_vec(), want);
        // Applying the default to an empty section gives the repo file's keys (less the device lines).
        let mut ops = vec![];
        for (id, p) in IDS.iter().zip(r.bindings) {
            ops.extend(bind_ops(id, p.unwrap()));
        }
        let mut got = pairs(&apply("[Wiimote1]\n", &ops));
        let mut file = pairs(DEFAULT_INI);
        for k in ["Source", "Device", "Extension"] {
            file.remove(k);
        }
        assert_eq!(got.len(), file.len());
        got.retain(|k, v| file.get(k) != Some(v));
        assert!(got.is_empty());
    }

    const MESSY: &str = "# top comment\n[Other]\nx = 1\n\n[Wiimote1]\nDevice = evdev/0/Pad\nIR/Auto Hide = False\n; note\nClassic/Buttons/A=EAST\nClassic/Buttons/A = `SOUTH`\nNunchuk/Stick/Up = W\n\n[Wiimote2]\nClassic/Buttons/A = X\n";

    #[test]
    fn unknown_lines_survive_byte_for_byte() {
        let new = set(MESSY, "a", "Y");
        let mask = |s: &str| {
            s.lines()
                .filter(|l| !l.contains("Buttons/A") && !l.contains("Buttons/1"))
                .collect::<Vec<_>>()
                .join("\n")
        };
        // Only lines mentioning the written keys differ; the old other-section A line is kept too.
        let old_lines: Vec<_> = MESSY
            .lines()
            .filter(|l| !l.contains("Classic/Buttons/A"))
            .collect();
        for l in &old_lines {
            assert!(new.lines().any(|n| n == *l), "lost {l:?}");
        }
        assert!(new.ends_with("[Wiimote2]\nClassic/Buttons/A = X\n"));
        assert!(mask(&new).contains("Device = evdev/0/Pad\nIR/Auto Hide = False\n; note\n"));
        // Both duplicate lines were rewritten.
        assert_eq!(new.matches("Classic/Buttons/A = NORTH").count(), 2);
        assert!(!new.contains("Classic/Buttons/A=EAST"));
        // Missing keys land inside the section, before its closing blank line.
        let w1 = new.find("[Wiimote1]").unwrap();
        let w2 = new.find("[Wiimote2]").unwrap();
        let mirror = new.find("Buttons/A = NORTH").unwrap();
        assert!(mirror > w1 && mirror < w2);
        assert!(new.contains("Nunchuk/Stick/Up = W\nButtons/A = NORTH\n\n[Wiimote2]"));
    }

    #[test]
    fn missing_section_is_added_and_crlf_kept() {
        let t = set("[Other]\r\nx = 1", "a", "A");
        assert!(t.starts_with("[Other]\r\nx = 1\r\n[Wiimote1]\r\n"));
        assert!(!t.replace("\r\n", "").contains('\n'));
        assert_eq!(pad_of(&t, "a"), Some("A"));
        // Appended at the end of a file with no trailing newline.
        let t = set("[Wiimote1]\nSource = 1", "b", "B");
        assert!(t.starts_with("[Wiimote1]\nSource = 1\n"));
    }

    #[test]
    fn custom_lines_read_as_custom() {
        let base = |k: &str, v: &str| apply(DEFAULT_INI, &[Op::Set(k.into(), v.into())]);
        let a = read_text(&base("Classic/Buttons/A", "EAST | TL"));
        assert!(a.custom[0] && a.bindings[0].is_none());
        for odd in [
            "east",
            "`EAST",
            "EAST`x",
            "`EA`ST`",
            "",
            "EAST&TL",
        ] {
            assert!(
                read_text(&base("Classic/Buttons/A", odd)).custom[0],
                "{odd:?}"
            );
        }
        let s = read_text(&base("Classic/Left Stick/Up", "`Axis 4-`"));
        assert!(s.custom[11] && !s.custom[12]);
        // A stick row on the d-pad is not a d-pad.
        let d = apply(DEFAULT_INI, &bind_ops("dp", "Left stick"));
        assert!(read_text(&d).custom[13]);
        // Tolerant spellings still read.
        let t = base("Classic/Buttons/A", "  `EAST`  ")
            .replace("Classic/Buttons/B = SOUTH", "Classic/Buttons/B=SOUTH");
        assert_eq!(read_text(&t).custom[..2], [false, false]);
        // A repeated name makes the later control custom.
        let dup = base("Classic/Buttons/B", "EAST");
        let r = read_text(&dup);
        assert!(!r.custom[0] && r.custom[1]);
    }

    #[test]
    fn dead_zone_format() {
        assert_eq!(read_text(DEFAULT_INI).dz, [0, 0]);
        for (v, n) in [
            ("12", 12),
            ("12.0", 12),
            ("12.000000", 12),
            (" 50.000000", 50),
            ("x", 0),
            ("51", 0),
            ("-1", 0),
            ("NaN", 0),
        ] {
            let t = apply(DEFAULT_INI, &[Op::Set(dead_zone::key("ls"), v.into())]);
            assert_eq!(read_text(&t).dz, [n, 0], "{v}");
        }
        let t = apply(
            DEFAULT_INI,
            &[Op::Set(dead_zone::key("rs"), dead_zone::format(12))],
        );
        assert!(t.contains("Classic/Right Stick/Dead Zone = 12.000000\n"));
        let t = apply(&t, &[Op::Remove(dead_zone::key("rs"))]);
        assert_eq!(t, DEFAULT_INI);
    }

    #[test]
    fn validate_rejects_injection() {
        let ok = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/controls/put.json"
        ))
        .unwrap();
        assert!(validate(ok.as_bytes()).is_ok());
        let v: serde_json::Value = serde_json::from_str(&ok).unwrap();
        let with = |id: &str, val: serde_json::Value| {
            let mut v = v.clone();
            v["bindings"][id] = val;
            v.to_string()
        };
        for bad in [
            "A\nDevice = x",
            "A\n[Wiimote2]",
            "A=B",
            "`A`",
            "EAST",
            "a",
            "ａ",
            "Ａ",
            &"A".repeat(100_000),
            "Left stick",
            "D-pad",
        ] {
            assert!(
                validate(with("a", bad.into()).as_bytes()).is_err(),
                "{bad:?}"
            );
        }
        assert!(validate(with("ls", "A".into()).as_bytes()).is_err());
        assert!(validate(with("dp", "Left stick".into()).as_bytes()).is_err());
        // Control ids are fixed too.
        let mut w = v.clone();
        w["bindings"]["a\nb"] = "A".into();
        assert!(validate(w.to_string().as_bytes()).is_err());
        let mut w = v.clone();
        w["bindings"].as_object_mut().unwrap().remove("a");
        w["bindings"]["../x"] = "A".into();
        assert!(validate(w.to_string().as_bytes()).is_err());
        // Duplicates, dead zone and shape.
        assert!(validate(with("a", "B".into()).as_bytes()).is_err());
        for dz in ["51", "-1", "1.5", "\"3\"", "null"] {
            let mut w = v.clone();
            w["deadZone"]["ls"] = serde_json::from_str(dz).unwrap();
            assert!(validate(w.to_string().as_bytes()).is_err(), "{dz}");
        }
        let mut w = v.clone();
        w["extra"] = 1.into();
        assert!(validate(w.to_string().as_bytes()).is_err());
        let mut w = v.clone();
        w["rev"] = "x".repeat(33).into();
        assert!(validate(w.to_string().as_bytes()).is_err());
        assert!(validate(b"{").is_err());
    }

    #[test]
    fn atomic_write_failure_leaves_no_temp() {
        let d = tempfile::tempdir().unwrap();
        let target = d.path().join(FILE);
        std::fs::create_dir(&target).unwrap(); // rename over a directory fails
        assert!(write_atomic(&target, b"x").is_err());
        let left: Vec<_> = std::fs::read_dir(d.path()).unwrap().collect();
        assert_eq!(left.len(), 1);
        let c = Controls::new(d.path().into());
        assert!(c.reset(false).is_err());
    }

    #[test]
    fn writes_0600_and_backs_up_once() {
        let d = tempfile::tempdir().unwrap();
        let c = Controls::new(d.path().join("dolphin-emu"));
        c.reset(false).unwrap(); // no file: created, no backup
        let dir = d.path().join("dolphin-emu");
        assert!(!dir.join(BACKUP).exists());
        assert_eq!(
            std::fs::metadata(dir.join(FILE))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        std::fs::write(dir.join(FILE), "[Wiimote1]\nDevice = mine\n").unwrap();
        c.reset(false).unwrap();
        c.reset(false).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join(BACKUP)).unwrap(),
            "[Wiimote1]\nDevice = mine\n"
        );
        assert!(
            std::fs::read_to_string(dir.join(FILE))
                .unwrap()
                .contains("Device = mine")
        );
    }
}
