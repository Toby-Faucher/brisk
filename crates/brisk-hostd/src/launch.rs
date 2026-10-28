//! Launch options: a structured setting that becomes a safe command line.
//! Pure: no process is started here. Everything is argv, nothing is ever given to a shell.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const ENV_ALLOWED: &[&str] = &[
    "MANGOHUD",
    "DXVK_HUD",
    "DXVK_ASYNC",
    "PROTON_NO_ESYNC",
    "PROTON_NO_FSYNC",
    "PROTON_USE_WINED3D",
    "PROTON_ENABLE_NVAPI",
    "RADV_PERFTEST",
    "mesa_glthread",
    "__GL_THREADED_OPTIMIZATIONS",
    "SDL_VIDEODRIVER",
];
const MAX_ENV_VALUE: usize = 512;
const MAX_ARGS: usize = 32;
const MAX_ARG_LEN: usize = 256;
const MAX_CUSTOM_TOTAL: usize = 4096;
/// Gamescope flags that take a number, and the largest number allowed.
const GS_NUM: &[(&str, u32)] = &[
    ("-W", 16384),
    ("-H", 16384),
    ("-w", 16384),
    ("-h", 16384),
    ("-r", 1000),
];
const GS_BOOL: &[&str] = &["-f", "-b", "--fsr"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Wrapper {
    #[serde(rename = "mangohud")]
    MangoHud,
    #[serde(rename = "gamemode")]
    GameMode,
    #[serde(rename = "gamescope")]
    Gamescope {
        #[serde(default)]
        args: Vec<String>,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct LaunchOptions {
    pub wrappers: Vec<Wrapper>,
    pub env: BTreeMap<String, String>,
    /// Your own command, wrapped innermost (right before the game). Needs `allow_custom_commands`.
    pub custom: Option<Vec<String>>,
    /// Appended after the game's own command.
    pub extra_args: Vec<String>,
}

/// A per-game override: `None` inherits the default, `Some` replaces the whole field.
/// `custom: Some(vec![])` clears the inherited custom command.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct LaunchOverride {
    pub wrappers: Option<Vec<Wrapper>>,
    pub env: Option<BTreeMap<String, String>>,
    pub custom: Option<Vec<String>>,
    pub extra_args: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    DuplicateWrapper(&'static str),
    GamescopeArg(String),
    EnvName(String),
    EnvValue(String),
    ExtraArgs(String),
    CustomNotAllowed,
    Custom(String),
}

/// Proof that `validate` passed; only `validate` can make one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Validated(LaunchOptions);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

pub fn effective(defaults: &LaunchOptions, over: Option<&LaunchOverride>) -> LaunchOptions {
    let Some(o) = over else {
        return defaults.clone();
    };
    LaunchOptions {
        wrappers: o
            .wrappers
            .clone()
            .unwrap_or_else(|| defaults.wrappers.clone()),
        env: o.env.clone().unwrap_or_else(|| defaults.env.clone()),
        custom: match &o.custom {
            None => defaults.custom.clone(),
            Some(v) if v.is_empty() => None,
            Some(v) => Some(v.clone()),
        },
        extra_args: o
            .extra_args
            .clone()
            .unwrap_or_else(|| defaults.extra_args.clone()),
    }
}

fn check_args(args: &[String], what: &str) -> Option<String> {
    if args.len() > MAX_ARGS {
        return Some(format!("{what}: more than {MAX_ARGS} entries"));
    }
    args.iter()
        .any(|a| a.len() > MAX_ARG_LEN || a.contains('\0'))
        .then(|| format!("{what}: entry too long or contains NUL"))
}

fn gamescope_ok(args: &[String]) -> Result<(), String> {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if GS_BOOL.contains(&a.as_str()) {
            continue;
        }
        let Some(&(_, max)) = GS_NUM.iter().find(|(f, _)| f == a) else {
            return Err(format!("flag not allowed: {a}"));
        };
        let v = it
            .next()
            .and_then(|v| (v.len() <= 5).then(|| v.parse::<u32>().ok()).flatten());
        if !v.is_some_and(|n| (1..=max).contains(&n)) {
            return Err(format!("{a} needs a number from 1 to {max}"));
        }
    }
    Ok(())
}

fn custom_ok(c: &[String]) -> Result<(), String> {
    let Some(p) = c.first() else {
        return Err("empty command".into());
    };
    if c.len() > MAX_ARGS || c.iter().map(String::len).sum::<usize>() > MAX_CUSTOM_TOTAL {
        return Err("command too big".into());
    }
    if c.iter().any(|a| a.contains('\0') || a.len() > MAX_ARG_LEN) {
        return Err("entry too long or contains NUL".into());
    }
    if p.split('/').any(|s| s == "..") {
        return Err("program must not contain '..'".into());
    }
    if !p.starts_with('/') && p.contains('/') {
        return Err("program must be an absolute path or a bare name".into());
    }
    if p.is_empty() || p == "/" {
        return Err("program is empty".into());
    }
    Ok(())
}

pub fn validate(o: &LaunchOptions, allow_custom: bool) -> Result<Validated, Vec<Problem>> {
    let mut bad = Vec::new();
    let mut seen = [false; 3];
    for w in &o.wrappers {
        let (i, name) = match w {
            Wrapper::MangoHud => (0, "mangohud"),
            Wrapper::GameMode => (1, "gamemode"),
            Wrapper::Gamescope { .. } => (2, "gamescope"),
        };
        if std::mem::replace(&mut seen[i], true) {
            bad.push(Problem::DuplicateWrapper(name));
        }
        if let Wrapper::Gamescope { args } = w
            && let Some(e) = check_args(args, "gamescope").or_else(|| gamescope_ok(args).err())
        {
            bad.push(Problem::GamescopeArg(e));
        }
    }
    for (k, v) in &o.env {
        if !ENV_ALLOWED.contains(&k.as_str()) {
            bad.push(Problem::EnvName(k.clone()));
        } else if v.len() > MAX_ENV_VALUE || v.contains(['\0', '\n', '\r']) {
            bad.push(Problem::EnvValue(k.clone()));
        }
    }
    if let Some(e) = check_args(&o.extra_args, "extra_args") {
        bad.push(Problem::ExtraArgs(e));
    }
    match (&o.custom, allow_custom) {
        (Some(_), false) => bad.push(Problem::CustomNotAllowed),
        (Some(c), true) => {
            if let Err(e) = custom_ok(c) {
                bad.push(Problem::Custom(e));
            }
        }
        _ => {}
    }
    if bad.is_empty() {
        Ok(Validated(o.clone()))
    } else {
        Err(bad)
    }
}

/// `[gamescope args --] [gamemoderun] [mangohud] [custom...] base... extra...`, whatever order the wrappers were listed in.
pub fn build(v: &Validated, base_cmd: &[String]) -> Command {
    let o = &v.0;
    let mut argv: Vec<String> = Vec::new();
    if let Some(Wrapper::Gamescope { args }) = o
        .wrappers
        .iter()
        .find(|w| matches!(w, Wrapper::Gamescope { .. }))
    {
        argv.push("gamescope".into());
        argv.extend(args.iter().cloned());
        argv.push("--".into());
    }
    if o.wrappers.contains(&Wrapper::GameMode) {
        argv.push("gamemoderun".into());
    }
    if o.wrappers.contains(&Wrapper::MangoHud) {
        argv.push("mangohud".into());
    }
    argv.extend(o.custom.iter().flatten().cloned());
    argv.extend(base_cmd.iter().cloned());
    argv.extend(o.extra_args.iter().cloned());
    let mut it = argv.into_iter();
    Command {
        program: it.next().unwrap_or_default(), // empty only if base_cmd was empty
        args: it.collect(),
        env: o.env.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }
    fn gs(a: &[&str]) -> Wrapper {
        Wrapper::Gamescope { args: s(a) }
    }
    fn env(k: &str, v: &str) -> LaunchOptions {
        LaunchOptions {
            env: [(k.to_string(), v.to_string())].into(),
            ..Default::default()
        }
    }
    fn cust(c: &[&str]) -> LaunchOptions {
        LaunchOptions {
            custom: Some(s(c)),
            ..Default::default()
        }
    }
    fn full() -> LaunchOptions {
        LaunchOptions {
            wrappers: vec![
                Wrapper::MangoHud,
                gs(&["-W", "1920", "-H", "1080", "-r", "60", "-f", "--fsr"]),
                Wrapper::GameMode,
            ],
            env: [
                ("DXVK_HUD".into(), "fps".into()),
                ("MANGOHUD".into(), "1".into()),
            ]
            .into(),
            custom: Some(s(&["/usr/bin/mytool", "--x"])),
            extra_args: s(&["-windowed"]),
        }
    }

    #[test]
    fn env_table() {
        let long = "a".repeat(513);
        let ok = "a".repeat(512);
        let cases: Vec<(&str, &str, bool)> = vec![
            ("DXVK_HUD", "fps", true),
            ("mesa_glthread", "true", true),
            ("DXVK_HUD", "", true),
            ("DXVK_HUD", &ok, true),
            // MangoHud's config has an exec= key that runs a command, so it would bypass the custom switch.
            ("MANGOHUD_CONFIG", "fps", false),
            ("LD_PRELOAD", "/x.so", false),
            ("LD_LIBRARY_PATH", "/x", false),
            ("PATH", "/x", false),
            ("HOME", "/x", false),
            ("A=b;rm", "1", false),
            ("DXVK_HUD;rm", "1", false),
            ("dxvk_hud", "1", false),
            ("", "1", false),
            ("DXVK_HUD", "a\nLD_PRELOAD=x", false),
            ("DXVK_HUD", "a\rb", false),
            ("DXVK_HUD", "a\0b", false),
            ("DXVK_HUD", &long, false),
        ];
        for (k, v, ok) in cases {
            assert_eq!(validate(&env(k, v), false).is_ok(), ok, "{k:?}={v:?}");
        }
    }

    #[test]
    fn custom_table() {
        let huge = "x".repeat(300);
        let many = vec!["a"; 33];
        let cases: Vec<(&str, bool, Vec<&str>, bool)> = vec![
            ("abs", true, vec!["/usr/bin/foo", "-a", "b c;d"], true),
            ("bare", true, vec!["foo", "--bar"], true),
            ("flag off", false, vec!["foo"], false),
            ("relative path", true, vec!["./foo"], false),
            ("relative dir", true, vec!["bin/foo"], false),
            ("dotdot abs", true, vec!["/usr/../bin/foo"], false),
            ("dotdot bare", true, vec![".."], false),
            ("empty argv", true, vec![], false),
            ("empty program", true, vec![""], false),
            ("nul program", true, vec!["fo\0o"], false),
            ("nul arg", true, vec!["foo", "a\0"], false),
            ("huge arg", true, vec!["foo", &huge], false),
            ("too many", true, many, false),
        ];
        for (name, allow, c, ok) in cases {
            assert_eq!(validate(&cust(&c), allow).is_ok(), ok, "{name}");
        }
        assert_eq!(
            validate(&cust(&["foo"]), false),
            Err(vec![Problem::CustomNotAllowed])
        );
        let total = vec!["y".repeat(200); 25]; // each fine, 5000 bytes total
        assert!(
            validate(
                &LaunchOptions {
                    custom: Some(total),
                    ..Default::default()
                },
                true
            )
            .is_err()
        );
        assert!(validate(&LaunchOptions::default(), false).is_ok());
    }

    #[test]
    fn gamescope_and_wrapper_rules() {
        let ok = |a: &[&str]| {
            validate(
                &LaunchOptions {
                    wrappers: vec![gs(a)],
                    ..Default::default()
                },
                false,
            )
            .is_ok()
        };
        assert!(ok(&[]));
        assert!(ok(&[
            "-W", "1920", "-H", "1080", "-r", "144", "-f", "--fsr", "-b"
        ]));
        for bad in [
            &["--exec", "x"][..],
            &["-W"],
            &["-W", "abc"],
            &["-W", "0"],
            &["-r", "99999"],
            &["-W", "-1"],
            &["-f;rm"],
            &["-W", "1920;rm"],
        ] {
            assert!(!ok(bad), "{bad:?}");
        }
        let dup = LaunchOptions {
            wrappers: vec![Wrapper::MangoHud, Wrapper::MangoHud],
            ..Default::default()
        };
        assert_eq!(
            validate(&dup, false),
            Err(vec![Problem::DuplicateWrapper("mangohud")])
        );
        let ea = |a: Vec<String>| {
            validate(
                &LaunchOptions {
                    extra_args: a,
                    ..Default::default()
                },
                false,
            )
            .is_ok()
        };
        assert!(ea(s(&["-a", "b c"])));
        assert!(!ea(s(&["a\0"])));
        assert!(!ea(vec!["z".repeat(257)]));
        assert!(!ea(vec!["a".into(); 33]));
    }

    #[test]
    fn wrapper_order_is_fixed() {
        use Wrapper::*;
        let want = s(&["gamescope", "--", "gamemoderun", "mangohud", "game"]);
        let perms = [
            vec![MangoHud, GameMode, gs(&[])],
            vec![gs(&[]), GameMode, MangoHud],
            vec![GameMode, MangoHud, gs(&[])],
            vec![MangoHud, gs(&[]), GameMode],
        ];
        for w in perms {
            let o = LaunchOptions {
                wrappers: w,
                ..Default::default()
            };
            let c = build(&validate(&o, false).unwrap(), &s(&["game"]));
            let mut got = vec![c.program];
            got.extend(c.args);
            assert_eq!(got, want);
        }
        let c = build(
            &validate(&LaunchOptions::default(), false).unwrap(),
            &s(&["game", "-x"]),
        );
        assert_eq!((c.program.as_str(), c.args), ("game", s(&["-x"])));
    }

    #[test]
    fn final_argv_steam_and_dolphin() {
        let v = validate(&full(), true).unwrap();
        let want_env = vec![
            ("DXVK_HUD".to_string(), "fps".to_string()),
            ("MANGOHUD".to_string(), "1".to_string()),
        ];
        let steam = build(&v, &s(&["/home/u/.steam/ldr/run", "--", "game.exe"]));
        assert_eq!(steam.program, "gamescope");
        assert_eq!(
            steam.args,
            s(&[
                "-W",
                "1920",
                "-H",
                "1080",
                "-r",
                "60",
                "-f",
                "--fsr",
                "--",
                "gamemoderun",
                "mangohud",
                "/usr/bin/mytool",
                "--x",
                "/home/u/.steam/ldr/run",
                "--",
                "game.exe",
                "-windowed"
            ])
        );
        assert_eq!(steam.env, want_env);
        let dolphin = build(&v, &s(&["dolphin-emu", "-b", "-e", "/g/mkw.iso"]));
        assert_eq!(
            dolphin.args[8..],
            s(&[
                "--",
                "gamemoderun",
                "mangohud",
                "/usr/bin/mytool",
                "--x",
                "dolphin-emu",
                "-b",
                "-e",
                "/g/mkw.iso",
                "-windowed"
            ])[..]
        );
    }

    #[test]
    fn nothing_is_joined_into_a_shell_string() {
        // Hostile-looking but valid values stay single argv entries, untouched.
        let o = LaunchOptions {
            extra_args: s(&["a b", "$(id)", "x;y", "`z`", "--k=\"v\""]),
            custom: Some(s(&["tool", "&& rm -rf /"])),
            ..Default::default()
        };
        let c = build(&validate(&o, true).unwrap(), &s(&["g", "f with space.iso"]));
        assert_eq!(c.program, "tool");
        assert_eq!(
            c.args,
            s(&[
                "&& rm -rf /",
                "g",
                "f with space.iso",
                "a b",
                "$(id)",
                "x;y",
                "`z`",
                "--k=\"v\""
            ])
        );
        // No wrapper or shell program is ever introduced.
        let all = validate(&full(), true).unwrap();
        let c = build(&all, &s(&["g"]));
        for sh in ["sh", "bash", "zsh", "env", "-c"] {
            assert!(c.program != sh && !c.args.iter().any(|a| a == sh), "{sh}");
        }
        // The source has no shell invocation either.
        let src = include_str!("launch.rs");
        let (code, _) = src.split_once("#[cfg(test)]").unwrap();
        assert!(
            !code.contains("Command::new") && !code.contains("\"sh\"") && !code.contains("\"-c\"")
        );
    }

    #[test]
    fn merge_semantics() {
        let d = full();
        assert_eq!(effective(&d, None), d);
        assert_eq!(effective(&d, Some(&LaunchOverride::default())), d);
        let o = LaunchOverride {
            wrappers: Some(vec![]),
            env: Some(BTreeMap::new()),
            extra_args: Some(s(&["-q"])),
            custom: Some(vec![]),
        };
        let e = effective(&d, Some(&o));
        assert_eq!(
            e,
            LaunchOptions {
                wrappers: vec![],
                env: BTreeMap::new(),
                custom: None,
                extra_args: s(&["-q"])
            }
        );
        let o = LaunchOverride {
            env: Some([("MANGOHUD".to_string(), "0".to_string())].into()),
            ..Default::default()
        };
        let e = effective(&d, Some(&o));
        assert_eq!(e.env.len(), 1); // replaces the whole field, no key merge
        assert_eq!(e.wrappers, d.wrappers);
        assert_eq!(e.custom, d.custom);
    }

    #[test]
    fn serde_round_trip_and_unknown_fields() {
        let o = full();
        let j = serde_json::to_string(&o).unwrap();
        assert_eq!(serde_json::from_str::<LaunchOptions>(&j).unwrap(), o);
        assert!(j.contains("\"mangohud\"") && j.contains("\"gamescope\""));
        assert_eq!(
            serde_json::from_str::<LaunchOptions>("{}").unwrap(),
            LaunchOptions::default()
        );
        for bad in [
            r#"{"shell":"x"}"#,
            r#"{"wrappers":["env"]}"#,
            r#"{"wrappers":[{"gamescope":{"args":[],"exec":"x"}}]}"#,
            r#"{"custom":"rm -rf /"}"#,
        ] {
            assert!(serde_json::from_str::<LaunchOptions>(bad).is_err(), "{bad}");
        }
        assert!(serde_json::from_str::<LaunchOverride>(r#"{"nope":1}"#).is_err());
        let ov: LaunchOverride = serde_json::from_str(r#"{"env":{}}"#).unwrap();
        assert_eq!((ov.env, ov.wrappers), (Some(BTreeMap::new()), None));
    }
}
