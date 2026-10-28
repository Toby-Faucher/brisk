//! `brisk-hostd wrap <gameid> -- <command...>`: the Steam launch option shim. Steam is set once to
//! `brisk-hostd wrap steam:<appid> -- %command%`; this reads `launch.json` from the data directory (no
//! network, no auth), builds the wrapped command line and `exec`s it. Any problem (no file, bad file,
//! invalid options, a wrapper that is not installed) runs the original command unchanged: a broken
//! config must never stop a game from starting.

use crate::game::GameId;
use crate::launch::Command;
use crate::launchstore::{command_for, plain, read_file};
use std::path::Path;

/// Replaces the process; only returns the error when that failed.
pub fn real_exec(c: &Command) -> std::io::Error {
    use std::os::unix::process::CommandExt;
    std::process::Command::new(&c.program)
        .args(&c.args)
        .envs(c.env.iter().map(|(k, v)| (k, v)))
        .exec()
}

/// `args` are what follows `wrap`. Returns the exit code, which only matters when nothing could be
/// executed. `exec` is injected so tests never start anything.
pub fn run(
    args: &[String],
    data_dir: &Path,
    exec: &mut dyn FnMut(&Command) -> std::io::Error,
) -> u8 {
    let (Some(raw), rest) = (args.first(), args.get(1..).unwrap_or_default()) else {
        eprintln!("usage: brisk-hostd wrap <game id> -- <command...>");
        return 2;
    };
    let base = rest.strip_prefix(&["--".to_string()]).unwrap_or(rest);
    if base.is_empty() {
        eprintln!("usage: brisk-hostd wrap <game id> -- <command...>");
        return 2;
    }
    let (cmd, note) = match GameId::compat(raw, |_| None) {
        Some(id) => command_for(&read_file(&data_dir.join("launch.json")), id.as_str(), base),
        None => (plain(base), Some(format!("{raw:?} is not a game id"))),
    };
    let wrapped = cmd.program != base[0];
    eprintln!(
        "brisk-wrap: {raw}: {}{}",
        if wrapped {
            format!("starting through {}", cmd.program)
        } else {
            "starting unchanged".into()
        },
        note.map(|n| format!(" ({n})")).unwrap_or_default()
    );
    let e = exec(&cmd);
    if wrapped || !cmd.env.is_empty() {
        eprintln!("brisk-wrap: could not start it that way ({e}); starting unchanged");
        let e = exec(&plain(base));
        eprintln!("brisk-wrap: {e}");
    } else {
        eprintln!("brisk-wrap: {e}");
    }
    127
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }
    fn dir(json: Option<&str>) -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        if let Some(j) = json {
            std::fs::write(d.path().join("launch.json"), j).unwrap();
        }
        d
    }
    /// Runs the shim with an exec that records and fails (as exec does when it cannot start).
    fn go(d: &Path, args: &[&str]) -> (u8, Vec<Command>) {
        let mut got = vec![];
        let r = run(&s(args), d, &mut |c| {
            got.push(c.clone());
            std::io::Error::other("fake")
        });
        (r, got)
    }
    fn argv(c: &Command) -> Vec<&str> {
        std::iter::once(c.program.as_str())
            .chain(c.args.iter().map(String::as_str))
            .collect()
    }
    const ALL: &str = r#"{"allowCustomCommands":true,"defaults":{
        "wrappers":["mangohud","gamemode",{"gamescope":{"args":["-W","1920","-f"]}}],
        "env":{"DXVK_HUD":"fps"},"custom":["/opt/tool","a b"],"extra_args":["-x"]}}"#;

    #[test]
    fn steam_and_dolphin_commands_get_every_wrapper() {
        let d = dir(Some(ALL));
        for (id, base) in [
            (
                "steam:620",
                &["/steam/proton", "waitforexitandrun", "/g/game.exe"][..],
            ),
            (
                "emu:dolphin:GALE01",
                &["dolphin-emu", "-b", "-e", "/g/a b.iso"],
            ),
        ] {
            let mut args = vec![id, "--"];
            args.extend(base);
            let (_, got) = go(d.path(), &args);
            let mut want = vec![
                "gamescope",
                "-W",
                "1920",
                "-f",
                "--",
                "gamemoderun",
                "mangohud",
                "/opt/tool",
                "a b",
            ];
            want.extend(base);
            want.push("-x");
            assert_eq!(argv(&got[0]), want);
            assert_eq!(got[0].env, [("DXVK_HUD".to_string(), "fps".to_string())]);
            // exec failed (wrapper not installed): the original runs, with no env added
            assert_eq!(argv(&got[1]), base);
            assert!(got[1].env.is_empty());
            assert_eq!(got.len(), 2);
        }
    }

    #[test]
    fn bare_appid_and_missing_dashes_work() {
        let d = dir(Some(r#"{"games":{"steam:7":{"wrappers":["mangohud"]}}}"#));
        let (_, got) = go(d.path(), &["7", "game", "--arg"]);
        assert_eq!(argv(&got[0]), ["mangohud", "game", "--arg"]);
    }

    #[test]
    fn broken_config_runs_the_command_unchanged() {
        let nocustom = r#"{"allowCustomCommands":false,"defaults":{"wrappers":["mangohud"],"custom":["tool"]}}"#;
        let badenv = r#"{"allowCustomCommands":true,"defaults":{"env":{"LD_PRELOAD":"/x.so"}}}"#;
        for (name, j) in [
            ("missing", None),
            ("corrupt", Some("{oops")),
            ("custom off", Some(nocustom)),
            ("bad env", Some(badenv)),
            ("empty", Some("")),
        ] {
            let d = dir(j);
            let (code, got) = go(d.path(), &["steam:620", "--", "game", "-a"]);
            assert_eq!(argv(&got[0]), ["game", "-a"], "{name}");
            assert!(got[0].env.is_empty(), "{name}");
            assert_eq!((got.len(), code), (1, 127), "{name}");
        }
    }

    #[test]
    fn bad_input_is_never_a_shell_or_a_path() {
        let d = dir(Some(ALL));
        // a bad id: unchanged command
        let (_, got) = go(d.path(), &["steam:1;rm -rf", "--", "game"]);
        assert_eq!(argv(&got[0]), ["game"]);
        // game text stays one argv entry each
        let (_, got) = go(d.path(), &["nope", "--", "g", "$(id)", "a;b"]);
        assert_eq!(argv(&got[0]), ["g", "$(id)", "a;b"]);
        // nothing to run
        assert_eq!(go(d.path(), &["steam:1"]).0, 2);
        assert_eq!(go(d.path(), &["steam:1", "--"]).0, 2);
        assert_eq!(go(d.path(), &[]).0, 2);
    }

    #[test]
    fn injection_in_saved_names_and_values_is_refused_at_use() {
        for j in [
            r#"{"defaults":{"env":{"A=b":"1"}}}"#,
            r#"{"defaults":{"env":{"DXVK_HUD":"fps\nLD_PRELOAD=/x"}}}"#,
            r#"{"defaults":{"wrappers":[{"gamescope":{"args":["--exec","sh"]}}]}}"#,
            r#"{"allowCustomCommands":true,"defaults":{"custom":["../../bin/sh"]}}"#,
        ] {
            let d = dir(Some(j));
            let (_, got) = go(d.path(), &["steam:620", "--", "game"]);
            assert_eq!(got.len(), 1, "{j}");
            assert_eq!(argv(&got[0]), ["game"], "{j}");
        }
    }
}
