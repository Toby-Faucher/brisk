//! RetroArch specifics: which libretro core a system uses and where it lives, and the launch
//! argv. The registry row (`registry.rs`) holds the fixed core file name per system.
//!
//! Env (docs/env.md): `BRISK_RETROARCH_CORES_DIR` (default `/usr/lib/libretro`) and
//! `BRISK_RETROARCH_CORE_<SYSTEM>` (for example `..._CORE_NES`) to rename one core file; an
//! absolute value is used as is. Core file names are unverified except snes9x: the missing-file
//! error below is how a wrong one shows up.

use crate::registry::{Emulator, System};
use std::path::{Path, PathBuf};

pub const DEFAULT_CORES_DIR: &str = "/usr/lib/libretro";

/// The core file `sys` needs. `env` looks up a variable (tests pass a fake).
pub fn core_path(sys: &System, env: &dyn Fn(&str) -> Option<String>) -> PathBuf {
    let dir = env("BRISK_RETROARCH_CORES_DIR")
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| DEFAULT_CORES_DIR.into());
    let name = env(&format!(
        "BRISK_RETROARCH_CORE_{}",
        sys.id.to_ascii_uppercase()
    ))
    .filter(|n| !n.is_empty())
    .unwrap_or_else(|| sys.core.into());
    Path::new(&dir).join(name) // an absolute `name` replaces the dir
}

/// The full command line (program first) for `file` on `sys`: `retroarch -L <core> -f <file>`
/// for a system with a core, `cmd args file` otherwise. Err: the core file is missing, with
/// what to do about it. `exists` is `Path::is_file` outside tests.
pub fn argv(
    e: &Emulator,
    sys: Option<&str>,
    file: &str,
    env: &dyn Fn(&str) -> Option<String>,
    exists: &dyn Fn(&Path) -> bool,
) -> Result<Vec<String>, String> {
    let mut v = vec![e.cmd.to_string()];
    let core = e
        .systems
        .iter()
        .find(|s| Some(s.id) == sys)
        .filter(|s| !s.core.is_empty());
    if let Some(s) = core {
        let p = core_path(s, env);
        if !exists(&p) {
            return Err(format!(
                "{} core {} is not installed for {} (set BRISK_RETROARCH_CORES_DIR or BRISK_RETROARCH_CORE_{})",
                e.name,
                p.display(),
                s.id,
                s.id.to_ascii_uppercase()
            ));
        }
        v.push("-L".into());
        v.push(p.to_string_lossy().into_owned());
    } else if e.systems.iter().any(|s| !s.core.is_empty()) {
        return Err(format!("{} needs a known system to pick a core", e.name));
    }
    v.extend(e.args.iter().map(|a| a.to_string()));
    v.push(file.into());
    Ok(v)
}

pub fn process_env(k: &str) -> Option<String> {
    std::env::var(k).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry;

    fn no_env(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn every_system_gets_its_core_then_args_then_the_file() {
        let e = registry::get("retroarch").unwrap();
        let want = [
            ("nes", "mesen"),
            ("snes", "snes9x"),
            ("megadrive", "genesis_plus_gx"),
            ("gb", "gambatte"),
            ("gbc", "gambatte"),
            ("gba", "mgba"),
        ];
        for (sys, core) in want {
            let v = argv(e, Some(sys), "/g/a b.rom", &no_env, &|_| true).unwrap();
            assert_eq!(
                v,
                [
                    "retroarch",
                    "-L",
                    &format!("/usr/lib/libretro/{core}_libretro.so"),
                    "-f",
                    "/g/a b.rom"
                ]
            );
        }
    }

    #[test]
    fn dolphin_is_unchanged_and_retroarch_needs_a_system() {
        let d = registry::get("dolphin").unwrap();
        let v = argv(d, Some("wii"), "/g/a.iso", &no_env, &|_| false).unwrap();
        assert_eq!(v, ["dolphin-emu", "-b", "-e", "/g/a.iso"]);
        let r = registry::get("retroarch").unwrap();
        assert!(argv(r, None, "/g/a.nes", &no_env, &|_| true).is_err());
        assert!(argv(r, Some("wii"), "/g/a.nes", &no_env, &|_| true).is_err());
    }

    #[test]
    fn env_moves_the_dir_and_renames_one_core() {
        let e = registry::get("retroarch").unwrap();
        let env = |k: &str| match k {
            "BRISK_RETROARCH_CORES_DIR" => Some("/opt/cores".to_string()),
            "BRISK_RETROARCH_CORE_NES" => Some("nestopia_libretro.so".to_string()),
            "BRISK_RETROARCH_CORE_GBA" => Some("/x/vbam.so".to_string()),
            _ => None,
        };
        let a = |s| argv(e, Some(s), "f", &env, &|_| true).unwrap()[2].clone();
        assert_eq!(a("nes"), "/opt/cores/nestopia_libretro.so");
        assert_eq!(a("gba"), "/x/vbam.so");
        assert_eq!(a("snes"), "/opt/cores/snes9x_libretro.so");
    }

    #[test]
    fn a_missing_core_is_a_clear_error() {
        let e = registry::get("retroarch").unwrap();
        let err = argv(e, Some("snes"), "f", &no_env, &|_| false).unwrap_err();
        assert!(err.contains("snes9x_libretro.so") && err.contains("BRISK_RETROARCH_CORE_SNES"));
    }
}
