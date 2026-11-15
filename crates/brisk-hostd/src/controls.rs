//! The Dolphin controller editor's file side (docs/plans/2026-10-28-emulator-controls-v2.md): reads and
//! rewrites one pad section of Dolphin's config for a Classic Controller on Wii Remote 1
//! (`WiimoteNew.ini`) or a GameCube controller on port 1 (`GCPadNew.ini`), fed by the Xbox-layout pad
//! the stream shows. A GameCube save also makes sure `Dolphin.ini` has `SIDevice0 = 6`. The same code
//! edits melonDS's `[Instance0.Joystick]` table in `melonDS.toml` for the DS (kind `ds`). Line based, so
//! everything it does not own survives byte for byte. Values come only from the tables below, picked
//! by an already validated pad name or a number in 0 to 50; user text never reaches a file. Written
//! whole to a 0600 temp file and renamed over the old one, with a one-time backup of each original.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::fsutil::write_atomic;

/// The known-good files; used when the machine has no file or no pad section yet.
const CLASSIC_INI: &str = include_str!("../../../deploy/dolphin/WiimoteNew.ini");
const GAMECUBE_INI: &str = include_str!("../../../deploy/dolphin/GCPadNew.ini");
const DS_TOML: &str = include_str!("../../../deploy/melonds/melonDS.toml");
const MAIN_INI: &str = "Dolphin.ini";
const CORE: &str = "[Core]";
const PORT_KEY: &str = "SIDevice0";
/// `SIDEVICE_GC_CONTROLLER` in Dolphin's `SIDevices` enum (verified from its source order).
const PORT_VALUE: &str = "6";

/// Which controller the editor works on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Classic,
    GameCube,
    Ds,
}

impl Kind {
    /// The controller type a URL names (`classic`, `gamecube`).
    pub fn parse(s: &str) -> Option<Kind> {
        match s {
            "classic" => Some(Kind::Classic),
            "gamecube" => Some(Kind::GameCube),
            "ds" => Some(Kind::Ds),
            _ => None,
        }
    }
    fn name(self) -> &'static str {
        match self {
            Kind::Classic => "classic",
            Kind::GameCube => "gamecube",
            Kind::Ds => "ds",
        }
    }
    /// The registry id of the emulator whose config this is.
    pub fn emulator(self) -> &'static str {
        match self {
            Kind::Classic | Kind::GameCube => "dolphin",
            Kind::Ds => "melonds",
        }
    }
    fn controller(self) -> &'static str {
        match self {
            Kind::Classic => "Wii Remote 1, Classic Controller",
            Kind::GameCube => "GameCube controller, port 1",
            Kind::Ds => "Nintendo DS",
        }
    }
    fn file(self) -> &'static str {
        match self {
            Kind::Classic => "WiimoteNew.ini",
            Kind::GameCube => "GCPadNew.ini",
            Kind::Ds => "melonDS.toml",
        }
    }
    fn section(self) -> &'static str {
        match self {
            Kind::Classic => "[Wiimote1]",
            Kind::GameCube => "[GCPad1]",
            Kind::Ds => "[Instance0.Joystick]",
        }
    }
    fn default_ini(self) -> &'static str {
        match self {
            Kind::Classic => CLASSIC_INI,
            Kind::GameCube => GAMECUBE_INI,
            Kind::Ds => DS_TOML,
        }
    }
    /// The button ids of this controller type, in editor order.
    pub fn ids(self) -> &'static [&'static str] {
        match self {
            Kind::Classic => &[
                "a", "b", "x", "y", "l", "r", "zl", "zr", "minus", "plus", "home", "ls", "rs", "dp",
            ],
            Kind::GameCube => &["a", "b", "x", "y", "z", "start", "l", "r", "ms", "cs", "dp"],
            Kind::Ds => &["a", "b", "x", "y", "l", "r", "start", "select", "dp"],
        }
    }
    /// The stick ids, in the order of the dead zone values (none on a DS).
    fn sticks(self) -> &'static [&'static str] {
        match self {
            Kind::Classic => &["ls", "rs"],
            Kind::GameCube => &["ms", "cs"],
            Kind::Ds => &[],
        }
    }
}

fn backup_name(file: &str) -> String {
    format!("{file}.before-brisk-edit")
}

const BUTTONS: [&str; 11] = [
    "A", "B", "X", "Y", "LB", "RB", "LT", "RT", "View", "Menu", "Guide",
];
const STICKS: [&str; 2] = ["Left stick", "Right stick"];
const DPAD: [&str; 1] = ["D-pad"];

/// Pad names a control takes (the allowlist, also in the web's `ACCEPTS`).
fn accepts(id: &str) -> &'static [&'static str] {
    match id {
        "ls" | "rs" | "ms" | "cs" => &STICKS,
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

/// Primary keys (the source of truth when reading) and the basic Wii Remote mirrors (write-only,
/// Classic only), in the order of `button_expr` / `group_exprs` values.
fn keys(kind: Kind, id: &str) -> (Vec<String>, Vec<String>) {
    match kind {
        Kind::Classic => classic_keys(id),
        Kind::GameCube => (gamecube_keys(id), vec![]),
        Kind::Ds => (ds_keys(id), vec![]),
    }
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|k| k.to_string()).collect()
}

fn group(g: &str) -> Vec<String> {
    D.map(|d| format!("{g}/{d}")).to_vec()
}

fn classic_keys(id: &str) -> (Vec<String>, Vec<String>) {
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
        "ls" => (group("Classic/Left Stick"), vec![]),
        "rs" => (group("Classic/Right Stick"), vec![]),
        _ => (group("Classic/D-Pad"), group("D-Pad")),
    }
}

// shortcut: format unverified against a file written by a live Dolphin; check when Toby can save a
// GameCube mapping in Dolphin's own window. Checked against Dolphin's source (GCPadEmu.h/.cpp,
// ControlGroup.cpp): group names `Main Stick` and `C-Stick`, `Dead Zone`, the `Group/Control` key
// scheme. Not seen in source: the control names `Buttons/Start`, `Triggers/L-Analog` and the
// `D-Pad` direction names. This is the only place with GameCube key text (plus `dead_zone`).
fn gamecube_keys(id: &str) -> Vec<String> {
    match id {
        "a" => s(&["Buttons/A"]),
        "b" => s(&["Buttons/B"]),
        "x" => s(&["Buttons/X"]),
        "y" => s(&["Buttons/Y"]),
        "z" => s(&["Buttons/Z"]),
        "start" => s(&["Buttons/Start"]),
        "l" => s(&["Triggers/L", "Triggers/L-Analog"]),
        "r" => s(&["Triggers/R", "Triggers/R-Analog"]),
        "ms" => group("Main Stick"),
        "cs" => group("C-Stick"),
        _ => group("D-Pad"),
    }
}

/// melonDS's names for the DS buttons in `[Instance0.Joystick]` (`EmuInstance::buttonNames`).
fn ds_keys(id: &str) -> Vec<String> {
    match id {
        "a" => s(&["A"]),
        "b" => s(&["B"]),
        "x" => s(&["X"]),
        "y" => s(&["Y"]),
        "l" => s(&["L"]),
        "r" => s(&["R"]),
        "start" => s(&["Start"]),
        "select" => s(&["Select"]),
        _ => s(&D),
    }
}

/// melonDS joystick values, checked in its 1.1 source (`EmuInstance::joystickButtonDown`): -1 is
/// none; low 16 bits a button number, or 0x100 | hat << 4 | direction (1 up, 2 right, 4 down,
/// 8 left); 0xFFFF there means no button. An axis adds 0x10000 | axis << 24 | mode << 20 (0 plus,
/// 1 minus, 2 trigger: pressed past half). Written in decimal, as melonDS's TOML writer does.
///
/// shortcut: the SDL numbers are worked out, not seen on the real pad; check them in melonDS's input
/// dialog with the streamed pad. melonDS reads raw SDL joystick numbers. SDL's Linux evdev backend
/// numbers buttons in key code order from BTN_JOYSTICK, and axes in ABS order without the hat axes
/// (ABS_HAT0X/Y are hat 0). libvirtualhid's Xbox Series pad has BTN_SOUTH to BTN_THUMBR with filler
/// codes in the gaps, so button n is code 0x130 + n: A 0, B 1, Y 3 (NORTH), X 4 (WEST, as Dolphin's
/// live mapping shows), LB 6, RB 7, View 10, Menu 11, Guide 12. Axes: LT 2, RT 5. If SDL's HIDAPI
/// driver gets the pad instead (hidraw access), the numbers differ.
fn ds_button(pad: &str) -> Option<&'static str> {
    Some(match pad {
        "A" => "0",
        "B" => "1",
        "X" => "4",
        "Y" => "3",
        "LB" => "6",
        "RB" => "7",
        "LT" => "35782655", // 0x0221FFFF: axis 2, trigger
        "RT" => "86114303", // 0x0521FFFF: axis 5, trigger
        "View" => "10",
        "Menu" => "11",
        "Guide" => "12",
        _ => return None,
    })
}

/// The plain expression for each primary and mirror key of `id` bound to `pad`.
fn exprs(kind: Kind, id: &str, pad: &str) -> Vec<&'static str> {
    if matches!(id, "ls" | "rs" | "ms" | "cs" | "dp") {
        if kind == Kind::Ds {
            // Hat 0 up, down, left, right; the DS has no sticks.
            return if pad == "D-pad" {
                vec!["257", "260", "264", "258"]
            } else {
                vec![]
            };
        }
        group_exprs(pad).map(|r| r.to_vec()).unwrap_or_default()
    } else {
        let e = if kind == Kind::Ds {
            ds_button(pad)
        } else {
            button_expr(pad)
        };
        vec![e.unwrap_or_default(); keys(kind, id).0.len()]
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
/// checked against a file Dolphin wrote (format unverified against a file written by a live Dolphin;
/// check when Toby can save a mapping with a dead zone in Dolphin's own window). The setting name
/// `Dead Zone` and the stick group names are confirmed in Dolphin's source; the `12.000000` text is not.
mod dead_zone {
    use super::Kind;
    pub fn key(kind: Kind, id: &str) -> String {
        match (kind, id) {
            (Kind::Classic, "ls") => "Classic/Left Stick/Dead Zone",
            (Kind::Classic, _) => "Classic/Right Stick/Dead Zone",
            (_, "ms") => "Main Stick/Dead Zone",
            // A DS has no sticks, so no dead zone key is ever asked for.
            _ => "C-Stick/Dead Zone",
        }
        .into()
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

/// (header index, end exclusive) of section `sec`: ends at the next line that starts a section.
fn section(sec: &str, lines: &[String]) -> Option<(usize, usize)> {
    let h = lines.iter().position(|l| l.trim() == sec)?;
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
    bindings: Vec<Option<&'static str>>,
    custom: Vec<bool>,
    dz: Vec<u8>,
    found: bool,
}

fn read_text(kind: Kind, text: &str) -> Read {
    let lines = split(text);
    let Some((h, e)) = section(kind.section(), &lines) else {
        return read_text(kind, kind.default_ini()).not_found();
    };
    let ids = kind.ids();
    // Last duplicate wins, as an ini loader would take it.
    let vals: BTreeMap<&str, &str> = lines[h + 1..e].iter().filter_map(|l| kv(l)).collect();
    let mut r = Read {
        bindings: vec![None; ids.len()],
        custom: vec![true; ids.len()],
        dz: vec![0; kind.sticks().len()],
        found: true,
    };
    let mut used: Vec<&str> = vec![];
    for (i, id) in ids.iter().enumerate() {
        let got: Option<Vec<String>> = keys(kind, id)
            .0
            .iter()
            .map(|k| vals.get(k.as_str()).and_then(|v| norm(v)))
            .collect();
        let pad = got.and_then(|g| {
            accepts(id).iter().copied().find(|p| {
                let want = exprs(kind, id, p);
                want.len() == g.len() && want.iter().zip(&g).all(|(w, x)| w == x)
            })
        });
        if let Some(p) = pad.filter(|p| !used.contains(p)) {
            used.push(p);
            r.bindings[i] = Some(p);
            r.custom[i] = false;
        }
    }
    for (n, id) in kind.sticks().iter().enumerate() {
        r.dz[n] = vals
            .get(dead_zone::key(kind, id).as_str())
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

/// Adds `block` (which starts with the header of `sec`) when `text` has no such section.
fn ensure_section(sec: &str, block: &str, text: &str) -> String {
    let lines = split(text);
    if section(sec, &lines).is_some() {
        return text.into();
    }
    let eol = eol_of(&lines);
    let mut out = text.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push_str(eol);
    }
    out.push_str(&block.replace('\n', eol));
    out
}

fn eol_of(lines: &[String]) -> &'static str {
    if lines.first().is_some_and(|l| l.ends_with("\r\n")) {
        "\r\n"
    } else {
        "\n"
    }
}

/// Applies the ops inside section `sec` (which must exist). Matching lines (duplicates too) keep
/// their key text; a missing key goes after the section's last non-blank line.
fn apply(sec: &str, text: &str, ops: &[Op]) -> String {
    let mut lines = split(text);
    let eol = eol_of(&lines);
    for op in ops {
        let Some((h, e)) = section(sec, &lines) else {
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

/// Set ops for one control bound to `pad` (primary keys and mirrors).
fn bind_ops(kind: Kind, id: &str, pad: &str) -> Vec<Op> {
    let (classic, mirror) = keys(kind, id);
    let ex = exprs(kind, id, pad);
    classic
        .into_iter()
        .chain(mirror)
        .enumerate()
        .map(|(i, k)| Op::Set(k, quote(ex[i % ex.len()])))
        .collect()
}

// ---- the API's data ----

/// The controller settings as the API shows them.
#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub emulator: &'static str,
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub controller: &'static str,
    pub source: &'static str,
    pub rev: String,
    pub running: bool,
    pub applies_on: &'static str,
    /// GameCube only: `Dolphin.ini` has port 1 set to a standard controller.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port_ready: Option<bool>,
    pub bindings: BTreeMap<&'static str, Option<&'static str>>,
    pub dead_zone: BTreeMap<&'static str, u8>,
    pub custom: BTreeMap<&'static str, bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Put {
    rev: Option<String>,
    bindings: BTreeMap<String, Option<String>>,
    dead_zone: BTreeMap<String, i64>,
}

/// Why a controller change failed.
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

type Checked = (Put, Vec<Option<&'static str>>, Vec<u8>);

/// All checks that need no file. Returns the validated bindings and dead zones.
fn validate(kind: Kind, b: &[u8]) -> Result<Checked, CtlError> {
    let p: Put = serde_json::from_slice(b).map_err(|_| CtlError::Bad("bad request"))?;
    if p.rev.as_ref().is_some_and(|r| r.chars().count() > 32) {
        return Err(CtlError::Bad("rev too long"));
    }
    let ids = kind.ids();
    if p.bindings.len() != ids.len() || ids.iter().any(|i| !p.bindings.contains_key(*i)) {
        return Err(CtlError::Bad("bindings must list every control"));
    }
    let mut pads = vec![];
    for id in ids {
        pads.push(match &p.bindings[*id] {
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
    let sticks = kind.sticks();
    let dz: Option<Vec<u8>> = sticks
        .iter()
        .map(|id| {
            p.dead_zone
                .get(*id)
                .and_then(|n| u8::try_from(*n).ok())
                .filter(|n| *n <= 50)
        })
        .collect();
    match dz {
        Some(dz) if p.dead_zone.len() == sticks.len() => Ok((p, pads, dz)),
        _ => Err(CtlError::Bad("dead zone must be 0 to 50")),
    }
}

/// Does `Dolphin.ini` text put port 1 on a standard controller?
fn port_ready(text: &str) -> bool {
    let lines = split(text);
    let Some((h, e)) = section(CORE, &lines) else {
        return false;
    };
    lines[h + 1..e]
        .iter()
        .filter_map(|l| kv(l))
        .rfind(|(k, _)| *k == PORT_KEY)
        .is_some_and(|(_, v)| v.trim() == PORT_VALUE)
}

/// The section block of the known-good file (the whole file for Dolphin's one-section files).
fn default_block(kind: Kind) -> String {
    let lines = split(kind.default_ini());
    let (h, e) = section(kind.section(), &lines).unwrap_or((0, 0));
    lines[h..e].concat()
}

/// melonDS drops a config it cannot parse and later saves its defaults over it. So a DS write goes
/// ahead only when the joystick table is one plain `[Instance0.Joystick]` block of `key = value`
/// lines with bare keys, or is absent and not set up in another form (dotted keys or an inline
/// table, which would make the added header a duplicate).
/// shortcut: not a TOML parser (a `[` line inside a multi-line string or array would confuse it);
/// melonDS writes neither near these tables. Use toml_edit if the file ever gets harder.
fn toml_safe(text: &str) -> bool {
    let sec = Kind::Ds.section();
    let lines = split(text);
    if text.contains('\0') || lines.iter().filter(|l| l.trim() == sec).count() > 1 {
        return false;
    }
    let mut cur = "";
    for l in &lines {
        let t = l.trim();
        if t.starts_with('[') {
            cur = t;
            continue;
        }
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let Some((k, _)) = t.split_once('=') else {
            if cur == sec {
                return false;
            }
            continue;
        };
        let k = k.trim();
        let bare = k.trim_matches(['"', '\'']);
        let bad = match cur {
            "" => bare == "Instance0" || bare.starts_with("Instance0."),
            "[Instance0]" => bare == "Joystick" || bare.starts_with("Joystick."),
            c => c == sec && bare != k,
        };
        if bad {
            return false;
        }
    }
    true
}

/// One machine's Dolphin and melonDS config directories. One mutex covers every read, edit and
/// write, for every controller type and `Dolphin.ini`.
pub struct Controls {
    dir: PathBuf,
    melonds: PathBuf,
    lock: Mutex<()>,
}

impl Controls {
    /// The editor for the config files in Dolphin's config dir `dir` and melonDS's `melonds`.
    pub fn new(dir: PathBuf, melonds: PathBuf) -> Controls {
        Controls {
            dir,
            melonds,
            lock: Mutex::new(()),
        }
    }

    /// The file to read and replace: a symlinked config resolves to its target. `melonDS.toml` is
    /// the only file in melonDS's dir; every other name is Dolphin's.
    fn path(&self, file: &str) -> PathBuf {
        let dir = if file == Kind::Ds.file() {
            &self.melonds
        } else {
            &self.dir
        };
        let p = dir.join(file);
        std::fs::canonicalize(&p).unwrap_or(p)
    }

    /// Writes the known-good melonDS config when there is none, so the first DS game starts with
    /// the pad mapped (melonDS's own first config leaves every control unset). Only logs errors.
    pub fn seed(&self, emulator: &str) {
        if emulator != Kind::Ds.emulator() {
            return;
        }
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        if let Ok((None, _)) = self.load(Kind::Ds.file(), "") {
            let _ = self.write_file(Kind::Ds.file(), None, DS_TOML);
        }
    }

    /// File bytes (None = no file) and text. A non-UTF-8 file is an error: nothing may be written
    /// over it. A missing file reads as `default`.
    fn load(&self, file: &str, default: &str) -> Result<(Option<Vec<u8>>, String), CtlError> {
        match std::fs::read(self.path(file)) {
            Ok(b) => {
                let t = String::from_utf8(b.clone()).map_err(|_| CtlError::Io)?;
                Ok((Some(b), t))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok((None, default.into())),
            Err(_) => Err(CtlError::Io),
        }
    }

    fn view(&self, kind: Kind, running: bool) -> Result<View, CtlError> {
        let (bytes, text) = self.load(kind.file(), kind.default_ini())?;
        let r = read_text(kind, &text);
        let ids = kind.ids();
        Ok(View {
            emulator: kind.emulator(),
            kind: kind.name(),
            controller: kind.controller(),
            source: if bytes.is_some() && r.found {
                "file"
            } else {
                "default"
            },
            rev: rev_of(bytes.as_deref()),
            running,
            applies_on: "next-launch",
            port_ready: (kind == Kind::GameCube).then(|| {
                self.load(MAIN_INI, "")
                    .is_ok_and(|(b, t)| b.is_some() && port_ready(&t))
            }),
            bindings: ids.iter().copied().zip(r.bindings).collect(),
            dead_zone: kind.sticks().iter().copied().zip(r.dz).collect(),
            custom: ids.iter().copied().zip(r.custom).collect(),
        })
    }

    /// The current mapping of `kind`.
    pub fn get(&self, kind: Kind, running: bool) -> Result<View, CtlError> {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        self.view(kind, running)
    }

    /// Validates and saves a new mapping; `Changed` when the file moved on since `rev`.
    pub fn put(&self, kind: Kind, body: &[u8], running: bool) -> Result<View, CtlError> {
        let (p, pads, dz) = validate(kind, body)?;
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let (bytes, text) = self.load(kind.file(), kind.default_ini())?;
        if p.rev.is_some_and(|r| r != rev_of(bytes.as_deref())) {
            return Err(CtlError::Changed(Box::new(self.view(kind, running)?)));
        }
        let cur = read_text(kind, &text);
        let ids = kind.ids();
        let mut ops = vec![];
        for (i, pad) in pads.iter().enumerate() {
            match pad {
                Some(pad) => ops.extend(bind_ops(kind, ids[i], pad)),
                None if !cur.custom[i] => {
                    return Err(CtlError::Bad("only a custom control can be left alone"));
                }
                None => {}
            }
        }
        for (n, id) in kind.sticks().iter().enumerate() {
            if dz[n] != cur.dz[n] {
                ops.push(Op::Set(dead_zone::key(kind, id), dead_zone::format(dz[n])));
            }
        }
        self.save(kind, bytes, &text, &ops)?;
        self.view(kind, running)
    }

    /// Xbox default for every control (custom ones included); the dead zone keys are removed.
    pub fn reset(&self, kind: Kind, running: bool) -> Result<View, CtlError> {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let (bytes, text) = self.load(kind.file(), kind.default_ini())?;
        let def = read_text(kind, kind.default_ini());
        let mut ops: Vec<Op> = kind
            .ids()
            .iter()
            .zip(def.bindings)
            .flat_map(|(id, p)| bind_ops(kind, id, p.unwrap_or_default()))
            .collect();
        ops.extend(
            kind.sticks()
                .iter()
                .map(|id| Op::Remove(dead_zone::key(kind, id))),
        );
        self.save(kind, bytes, &text, &ops)?;
        self.view(kind, running)
    }

    fn save(
        &self,
        kind: Kind,
        orig: Option<Vec<u8>>,
        text: &str,
        ops: &[Op],
    ) -> Result<(), CtlError> {
        if kind == Kind::Ds && !toml_safe(text) {
            return Err(CtlError::Bad(
                "melonDS.toml has its joystick table in a form brisk does not edit",
            ));
        }
        let new = apply(
            kind.section(),
            &ensure_section(kind.section(), &default_block(kind), text),
            ops,
        );
        self.write_file(kind.file(), orig, &new)?;
        if kind == Kind::GameCube {
            self.ensure_port()?;
        }
        Ok(())
    }

    /// Makes `Dolphin.ini` hold `SIDevice0 = 6`. Skipped when the file is absent (Dolphin builds it on
    /// its first run) or already right. Nothing else in it is touched.
    fn ensure_port(&self) -> Result<(), CtlError> {
        let (Some(orig), text) = self.load(MAIN_INI, "")? else {
            return Ok(());
        };
        if port_ready(&text) {
            return Ok(());
        }
        let new = apply(
            CORE,
            &ensure_section(CORE, &format!("{CORE}\n"), &text),
            &[Op::Set(PORT_KEY.into(), PORT_VALUE.into())],
        );
        self.write_file(MAIN_INI, Some(orig), &new)
    }

    /// Creates the directory (0700), backs the original up once, then writes atomically.
    fn write_file(&self, file: &str, orig: Option<Vec<u8>>, new: &str) -> Result<(), CtlError> {
        let path = self.path(file);
        let io = |e: std::io::Error| {
            tracing::warn!("{}: {e}", path.display());
            CtlError::Io
        };
        let dir = path.parent().unwrap_or(Path::new("."));
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .map_err(io)?;
        if let Some(o) = &orig {
            backup_once(dir, file, o).map_err(io)?;
        }
        write_atomic(&path, new.as_bytes()).map_err(io)
    }
}

/// Copies the original once; an existing backup holds the first original and is never touched.
fn backup_once(dir: &Path, file: &str, bytes: &[u8]) -> std::io::Result<()> {
    match crate::fsutil::write_new(&dir.join(backup_name(file)), bytes) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    const KINDS: [Kind; 3] = [Kind::Classic, Kind::GameCube, Kind::Ds];
    const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/controls");

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(format!("{FIX}/{name}")).unwrap()
    }

    fn pad_of(kind: Kind, text: &str, id: &str) -> Option<&'static str> {
        let i = kind.ids().iter().position(|i| *i == id).unwrap();
        read_text(kind, text).bindings[i]
    }

    fn set(kind: Kind, text: &str, id: &str, pad: &str) -> String {
        let sec = kind.section();
        apply(
            sec,
            &ensure_section(sec, &default_block(kind), text),
            &bind_ops(kind, id, pad),
        )
    }

    fn pairs(text: &str) -> BTreeMap<String, String> {
        text.lines()
            .filter_map(kv)
            .map(|(k, v)| (k.into(), v.trim().into()))
            .collect()
    }

    #[test]
    fn round_trip_every_control_and_name() {
        for kind in KINDS {
            for id in kind.ids() {
                for pad in accepts(id) {
                    let t = set(kind, &format!("{}\n", kind.section()), id, pad);
                    assert_eq!(pad_of(kind, &t, id), Some(*pad), "{kind:?} {id} {pad}");
                    let m = pairs(&t);
                    let (primary, mirror) = keys(kind, id);
                    let ex = exprs(kind, id, pad);
                    for (i, k) in primary.iter().chain(&mirror).enumerate() {
                        assert_eq!(m[k], quote(ex[i % ex.len()]), "{id} {pad} {k}");
                    }
                }
            }
        }
    }

    #[test]
    fn defaults_match_repo_files() {
        let want = |s: &str| -> Vec<String> {
            s.replace("Left stick", "Left_stick")
                .replace("Right stick", "Right_stick")
                .split(' ')
                .map(|s| s.replace('_', " "))
                .collect()
        };
        let expect = [
            (
                Kind::Classic,
                "B A Y X LT RT LB RB View Menu Guide Left stick Right stick D-pad",
                vec!["Source", "Device", "Extension"],
            ),
            (
                Kind::GameCube,
                "A X B Y RB Menu LT RT Left stick Right stick D-pad",
                vec!["Device"],
            ),
            (Kind::Ds, "B A Y X LB RB Menu View D-pad", vec![]),
        ];
        for (kind, names, skip) in expect {
            let r = read_text(kind, kind.default_ini());
            assert!(r.found && r.custom.iter().all(|c| !c));
            assert_eq!(r.dz, vec![0; kind.sticks().len()]);
            assert_eq!(
                r.bindings.iter().map(|b| b.unwrap()).collect::<Vec<_>>(),
                want(names)
            );
            // Applying the default to an empty section gives the repo file's keys (less the device lines).
            let mut ops = vec![];
            for (id, p) in kind.ids().iter().zip(&r.bindings) {
                ops.extend(bind_ops(kind, id, p.unwrap()));
            }
            let mut got = pairs(&apply(
                kind.section(),
                &format!("{}\n", kind.section()),
                &ops,
            ));
            let mut file = pairs(&default_block(kind));
            for k in skip {
                file.remove(k);
            }
            assert_eq!(got.len(), file.len(), "{kind:?}");
            got.retain(|k, v| file.get(k) != Some(v));
            assert!(got.is_empty(), "{kind:?} {got:?}");
        }
    }

    const MESSY: &str = "# top comment\n[Other]\nx = 1\n\n[Wiimote1]\nDevice = evdev/0/Pad\nIR/Auto Hide = False\n; note\nClassic/Buttons/A=EAST\nClassic/Buttons/A = `SOUTH`\nNunchuk/Stick/Up = W\n\n[Wiimote2]\nClassic/Buttons/A = X\n";
    const MESSY_GC: &str = "# top\n[Other]\nx = 1\n\n[GCPad1]\nDevice = evdev/0/Pad\nRumble/Motor = Y\n; note\nButtons/A=EAST\nButtons/A = `SOUTH`\nOptions/Always Connected = True\n\n[GCPad2]\nButtons/A = X\n";

    #[test]
    fn unknown_lines_survive_byte_for_byte() {
        let new = set(Kind::Classic, MESSY, "a", "Y");
        let old_lines: Vec<_> = MESSY
            .lines()
            .filter(|l| !l.contains("Classic/Buttons/A"))
            .collect();
        for l in &old_lines {
            assert!(new.lines().any(|n| n == *l), "lost {l:?}");
        }
        assert!(new.ends_with("[Wiimote2]\nClassic/Buttons/A = X\n"));
        assert!(new.contains("Device = evdev/0/Pad\nIR/Auto Hide = False\n; note\n"));
        assert_eq!(new.matches("Classic/Buttons/A = NORTH").count(), 2);
        assert!(!new.contains("Classic/Buttons/A=EAST"));
        assert!(new.contains("Nunchuk/Stick/Up = W\nButtons/A = NORTH\n\n[Wiimote2]"));

        let new = set(Kind::GameCube, MESSY_GC, "a", "Y");
        for l in MESSY_GC.lines().filter(|l| !l.starts_with("Buttons/A")) {
            assert!(new.lines().any(|n| n == l), "lost {l:?}");
        }
        assert_eq!(new.matches("Buttons/A = NORTH").count(), 2);
        assert!(new.ends_with("[GCPad2]\nButtons/A = X\n"));
        assert!(new.contains("Device = evdev/0/Pad\nRumble/Motor = Y\n; note\n"));
    }

    #[test]
    fn missing_section_is_added_and_crlf_kept() {
        for kind in KINDS {
            let sec = kind.section();
            let t = set(kind, "[Other]\r\nx = 1", "a", "A");
            assert!(t.starts_with(&format!("[Other]\r\nx = 1\r\n{sec}\r\n")));
            assert!(!t.replace("\r\n", "").contains('\n'));
            assert_eq!(pad_of(kind, &t, "a"), Some("A"));
            let t = set(kind, &format!("{sec}\nSource = 1"), "b", "B");
            assert!(t.starts_with(&format!("{sec}\nSource = 1\n")));
        }
    }

    #[test]
    fn custom_lines_read_as_custom() {
        let base =
            |k: &str, v: &str| apply("[Wiimote1]", CLASSIC_INI, &[Op::Set(k.into(), v.into())]);
        let a = read_text(Kind::Classic, &base("Classic/Buttons/A", "EAST | TL"));
        assert!(a.custom[0] && a.bindings[0].is_none());
        for odd in ["east", "`EAST", "EAST`x", "`EA`ST`", "", "EAST&TL"] {
            assert!(
                read_text(Kind::Classic, &base("Classic/Buttons/A", odd)).custom[0],
                "{odd:?}"
            );
        }
        let s = read_text(Kind::Classic, &base("Classic/Left Stick/Up", "`Axis 4-`"));
        assert!(s.custom[11] && !s.custom[12]);
        let d = apply(
            "[Wiimote1]",
            CLASSIC_INI,
            &bind_ops(Kind::Classic, "dp", "Left stick"),
        );
        assert!(read_text(Kind::Classic, &d).custom[13]);
        let t = base("Classic/Buttons/A", "  `EAST`  ")
            .replace("Classic/Buttons/B = SOUTH", "Classic/Buttons/B=SOUTH");
        assert_eq!(read_text(Kind::Classic, &t).custom[..2], [false, false]);
        let r = read_text(Kind::Classic, &base("Classic/Buttons/B", "EAST"));
        assert!(!r.custom[0] && r.custom[1]);

        // GameCube: odd values, a half-written trigger pair, a stick key on a d-pad.
        let g = |k: &str, v: &str| apply("[GCPad1]", GAMECUBE_INI, &[Op::Set(k.into(), v.into())]);
        assert!(read_text(Kind::GameCube, &g("Buttons/Z", "TR | TL")).custom[4]);
        assert!(read_text(Kind::GameCube, &g("Triggers/L-Analog", "`Full Axis 5+`")).custom[6]);
        assert!(read_text(Kind::GameCube, &g("Main Stick/Left", "`Axis 3-`")).custom[8]);
        let dup = read_text(Kind::GameCube, &g("Buttons/B", "SOUTH"));
        assert!(!dup.custom[0] && dup.custom[1]);
        let d = apply(
            "[GCPad1]",
            GAMECUBE_INI,
            &bind_ops(Kind::GameCube, "dp", "Right stick"),
        );
        assert!(read_text(Kind::GameCube, &d).custom[10]);
    }

    #[test]
    fn dead_zone_format() {
        for kind in [Kind::Classic, Kind::GameCube] {
            let (sec, def) = (kind.section(), kind.default_ini());
            let &[s0, s1] = kind.sticks() else {
                panic!("{kind:?} has two sticks")
            };
            assert_eq!(read_text(kind, def).dz, [0, 0]);
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
                let t = apply(sec, def, &[Op::Set(dead_zone::key(kind, s0), v.into())]);
                assert_eq!(read_text(kind, &t).dz, [n, 0], "{v}");
            }
            let key = dead_zone::key(kind, s1);
            let t = apply(sec, def, &[Op::Set(key.clone(), dead_zone::format(12))]);
            assert!(t.contains(&format!("{key} = 12.000000\n")));
            assert_eq!(apply(sec, &t, &[Op::Remove(key)]), def);
        }
    }

    #[test]
    fn validate_rejects_injection() {
        for (kind, name) in [
            (Kind::Classic, "v2-classic-put.json"),
            (Kind::GameCube, "v2-gamecube-put.json"),
        ] {
            let ok = fixture(name);
            assert!(validate(kind, ok.as_bytes()).is_ok(), "{name}");
            let v: serde_json::Value = serde_json::from_str(&ok).unwrap();
            let bad = |v: &serde_json::Value| validate(kind, v.to_string().as_bytes()).is_err();
            let with = |id: &str, val: serde_json::Value| {
                let mut v = v.clone();
                v["bindings"][id] = val;
                v
            };
            for b in [
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
                assert!(bad(&with("a", b.into())), "{b:?}");
            }
            let s0 = kind.sticks()[0];
            assert!(bad(&with(s0, "A".into())));
            assert!(bad(&with("dp", "Left stick".into())));
            assert!(bad(&with("a", v["bindings"]["b"].clone()))); // duplicate
            let mut w = v.clone();
            w["bindings"]["a\nb"] = "A".into();
            assert!(bad(&w));
            let mut w = v.clone();
            w["bindings"].as_object_mut().unwrap().remove("a");
            w["bindings"]["../x"] = "A".into();
            assert!(bad(&w));
            // The other type's ids are not accepted.
            let other = if kind == Kind::Classic { "z" } else { "zl" };
            let mut w = v.clone();
            w["bindings"].as_object_mut().unwrap().remove("a");
            w["bindings"][other] = "A".into();
            assert!(bad(&w));
            for dz in ["51", "-1", "1.5", "\"3\"", "null"] {
                let mut w = v.clone();
                w["deadZone"][s0] = serde_json::from_str(dz).unwrap();
                assert!(bad(&w), "{dz}");
            }
            let mut w = v.clone();
            w["deadZone"]["x"] = 1.into();
            assert!(bad(&w));
            let mut w = v.clone();
            w["deadZone"] = if kind == Kind::Classic {
                serde_json::json!({"ms": 0, "cs": 0})
            } else {
                serde_json::json!({"ls": 0, "rs": 0})
            };
            assert!(bad(&w));
            let mut w = v.clone();
            w["extra"] = 1.into();
            assert!(bad(&w));
            let mut w = v.clone();
            w["rev"] = "x".repeat(33).into();
            assert!(bad(&w));
            assert!(validate(kind, b"{").is_err());
        }
    }

    #[test]
    fn kind_parse_is_exact() {
        assert_eq!(Kind::parse("classic"), Some(Kind::Classic));
        assert_eq!(Kind::parse("gamecube"), Some(Kind::GameCube));
        for bad in [
            "../x",
            "classic%0a",
            "classic\n",
            "Classic",
            "GameCube",
            "",
            "wii",
        ] {
            assert_eq!(Kind::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn port_line() {
        let run = |text: Option<&str>| {
            let d = tempfile::tempdir().unwrap();
            if let Some(t) = text {
                std::fs::write(d.path().join(MAIN_INI), t).unwrap();
            }
            let c = Controls::new(d.path().into(), d.path().into());
            let v = c.reset(Kind::GameCube, false).unwrap();
            let after = std::fs::read_to_string(d.path().join(MAIN_INI)).ok();
            let bk = std::fs::read_to_string(d.path().join(backup_name(MAIN_INI))).ok();
            (v.port_ready, after, bk)
        };
        let (r, after, bk) = run(Some(
            "[Core]\nSIDevice0 = 0\nOther = 1\n[Interface]\nx = 2\n",
        ));
        assert_eq!(r, Some(true));
        assert_eq!(
            after.unwrap(),
            "[Core]\nSIDevice0 = 6\nOther = 1\n[Interface]\nx = 2\n"
        );
        assert_eq!(
            bk.unwrap(),
            "[Core]\nSIDevice0 = 0\nOther = 1\n[Interface]\nx = 2\n"
        );
        let (r, after, _) = run(Some("[Core]\nOther = 1\n\n[Interface]\nx = 2\n"));
        assert_eq!(r, Some(true));
        assert_eq!(
            after.unwrap(),
            "[Core]\nOther = 1\nSIDevice0 = 6\n\n[Interface]\nx = 2\n"
        );
        let (r, after, _) = run(Some("[Interface]\nx = 2\n"));
        assert_eq!(r, Some(true));
        assert_eq!(
            after.unwrap(),
            "[Interface]\nx = 2\n[Core]\nSIDevice0 = 6\n"
        );
        let (r, after, bk) = run(None);
        assert_eq!((r, after, bk), (Some(false), None, None));
        let same = "[Core]\nSIDevice0 = 6\n";
        let (r, after, bk) = run(Some(same));
        assert_eq!((r, after.as_deref(), bk), (Some(true), Some(same), None));
        // Classic never touches Dolphin.ini.
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join(MAIN_INI), "[Core]\nSIDevice0 = 0\n").unwrap();
        let c = Controls::new(d.path().into(), d.path().into());
        assert_eq!(c.reset(Kind::Classic, false).unwrap().port_ready, None);
        assert_eq!(
            std::fs::read_to_string(d.path().join(MAIN_INI)).unwrap(),
            "[Core]\nSIDevice0 = 0\n"
        );
    }

    #[test]
    fn atomic_write_failure_leaves_no_temp() {
        let d = tempfile::tempdir().unwrap();
        let target = d.path().join("WiimoteNew.ini");
        std::fs::create_dir(&target).unwrap(); // rename over a directory fails
        assert!(write_atomic(&target, b"x").is_err());
        let left: Vec<_> = std::fs::read_dir(d.path()).unwrap().collect();
        assert_eq!(left.len(), 1);
        let c = Controls::new(d.path().into(), d.path().into());
        assert!(c.reset(Kind::Classic, false).is_err());
    }

    #[test]
    fn writes_0600_and_backs_up_once() {
        for kind in KINDS {
            let d = tempfile::tempdir().unwrap();
            let dir = d.path().join("dolphin-emu");
            let c = Controls::new(dir.clone(), dir.clone());
            c.reset(kind, false).unwrap(); // no file: created, no backup
            let (file, bk) = (kind.file(), backup_name(kind.file()));
            assert!(!dir.join(&bk).exists());
            let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&dir.join(file)), 0o600);
            assert_eq!(mode(&dir), 0o700);
            let mine = format!("{}\nDevice = mine\n", kind.section());
            std::fs::write(dir.join(file), &mine).unwrap();
            c.reset(kind, false).unwrap();
            c.reset(kind, false).unwrap();
            assert_eq!(std::fs::read_to_string(dir.join(&bk)).unwrap(), mine);
            assert!(
                std::fs::read_to_string(dir.join(file))
                    .unwrap()
                    .contains("Device = mine")
            );
        }
    }

    /// The shape melonDS 1.1 writes (toml11: top-level keys, then `[a.b]` tables, decimal ints).
    const MELON: &str = "LastROMFolder = \"/games/nds\"\nRecentROM = [\"/games/nds/a.nds\"]\n\n[Instance0]\nJoystickID = 0\n\n[Instance0.Keyboard]\nA = 88\nB = 90\nHK_Lid = -1\n\n[Instance0.Joystick]\nA = 1\nB = 0\nSelect = 10\nStart = 11\nRight = 258\nLeft = 264\nUp = 257\nDown = 260\nR = 7\nL = 6\nX = 3\nY = 4\nHK_Lid = -1\nHK_Mic = 12\n\n[Instance0.Window0]\nWidth = 256\n";

    fn ds_dir(text: Option<&[u8]>) -> (tempfile::TempDir, Controls) {
        let d = tempfile::tempdir().unwrap();
        if let Some(t) = text {
            std::fs::write(d.path().join("melonDS.toml"), t).unwrap();
        }
        let c = Controls::new(d.path().join("nodolphin"), d.path().into());
        (d, c)
    }

    #[test]
    fn ds_edit_touches_only_its_lines() {
        let r = read_text(Kind::Ds, MELON);
        assert_eq!(
            r.bindings,
            ["B", "A", "Y", "X", "LB", "RB", "Menu", "View", "D-pad"].map(Some)
        );
        let t = set(Kind::Ds, MELON, "a", "LT");
        assert_eq!(
            t,
            MELON.replace(
                "[Instance0.Joystick]\nA = 1\n",
                "[Instance0.Joystick]\nA = 35782655\n"
            )
        );
        // The keyboard's own `A` and the hotkeys stay as they were.
        assert!(t.contains("[Instance0.Keyboard]\nA = 88\n") && t.contains("HK_Mic = 12\n"));
        assert_eq!(pad_of(Kind::Ds, &t, "a"), Some("LT"));
        // Unset (-1), a hat on the wrong direction, or a stick axis read as custom.
        for (k, v) in [("A", "-1"), ("Up", "258"), ("B", "0x0"), ("Up", "17104895")] {
            let t = apply(Kind::Ds.section(), MELON, &[Op::Set(k.into(), v.into())]);
            let r = read_text(Kind::Ds, &t);
            let i = if k == "A" {
                0
            } else if k == "B" {
                1
            } else {
                8
            };
            assert!(r.custom[i] && r.bindings[i].is_none(), "{k} = {v}");
        }
    }

    #[test]
    fn ds_missing_table_is_added_once() {
        let no_joy = "[Instance0]\nJoystickID = 0\n\n[Instance0.Keyboard]\nA = 88\n";
        let (d, c) = ds_dir(Some(no_joy.as_bytes()));
        let v = c.reset(Kind::Ds, false).unwrap();
        assert_eq!(v.emulator, "melonds");
        assert!(v.dead_zone.is_empty());
        let t = std::fs::read_to_string(d.path().join("melonDS.toml")).unwrap();
        assert!(t.starts_with(no_joy));
        assert_eq!(t.matches("[Instance0.Keyboard]").count(), 1);
        assert_eq!(t.matches("[Instance0.Joystick]").count(), 1);
        assert!(toml_safe(&t));
        assert_eq!(read_text(Kind::Ds, &t), read_text(Kind::Ds, DS_TOML));
    }

    #[test]
    fn ds_damaged_file_is_never_written() {
        let bad: [&[u8]; 6] = [
            b"[Instance0.Joystick]\nA = 1\n[Instance0.Joystick]\nB = 0\n",
            b"[Instance0]\nJoystick = { A = 1 }\n",
            b"[Instance0]\nJoystick.A = 1\n",
            b"Instance0.Joystick.A = 1\n",
            b"[Instance0.Joystick]\n\"A\" = 1\n",
            b"[Instance0.Joystick]\nA = [\n1,\n]\n",
        ];
        for b in bad
            .iter()
            .copied()
            .chain([&b"[Instance0.Joystick]\nA = \xff\n"[..]])
        {
            let (d, c) = ds_dir(Some(b));
            assert!(
                c.reset(Kind::Ds, false).is_err(),
                "{:?}",
                String::from_utf8_lossy(b)
            );
            let body = br#"{"bindings":{"a":"B","b":"A","x":"Y","y":"X","l":"LB","r":"RB","start":"Menu","select":"View","dp":"D-pad"},"deadZone":{}}"#;
            assert!(c.put(Kind::Ds, body, false).is_err());
            assert_eq!(std::fs::read(d.path().join("melonDS.toml")).unwrap(), b);
            assert!(!d.path().join(backup_name("melonDS.toml")).exists());
        }
        assert!(toml_safe(MELON) && toml_safe(DS_TOML) && toml_safe(""));
    }

    #[test]
    fn ds_put_validates_and_seed_writes_only_a_missing_file() {
        let (d, c) = ds_dir(None);
        let file = d.path().join("melonDS.toml");
        let body = |dz: &str| {
            format!(
                r#"{{"bindings":{{"a":"A","b":"B","x":"X","y":"Y","l":"LT","r":"RT","start":"Menu","select":"View","dp":"D-pad"}},"deadZone":{dz}}}"#
            )
        };
        assert!(
            c.put(Kind::Ds, body(r#"{"ls":0}"#).as_bytes(), false)
                .is_err()
        );
        let v = c.put(Kind::Ds, body("{}").as_bytes(), false).unwrap();
        assert_eq!((v.source, v.bindings["l"]), ("file", Some("LT")));
        // The first write kept the shipped keyboard keys.
        assert!(
            std::fs::read_to_string(&file)
                .unwrap()
                .contains("Start = 16777220\n")
        );
        std::fs::remove_file(&file).unwrap();
        c.seed("dolphin");
        assert!(!file.exists());
        c.seed("melonds");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), DS_TOML);
        std::fs::write(&file, MELON).unwrap();
        c.seed("melonds");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), MELON);
    }
}
