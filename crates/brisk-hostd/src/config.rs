use rand_core::{OsRng, RngCore};
use std::ffi::OsString;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

/// Shared by every route. Deliberately has no field for the Steam API key: it is read from
/// `steam_key_file` per request and never stored.
#[derive(Clone, Debug)]
pub struct Config {
    pub listen: SocketAddr,
    pub dist: PathBuf,
    pub data_dir: PathBuf,
    pub steam_dir: PathBuf,
    pub steam_key_file: PathBuf,
    pub steam_api_base: String,
    pub steam_store_base: String,
    pub bridge_bin: PathBuf,
    /// `BRISK_LISTEN`; None = the tailscale IPv4 :4433, looked up at each stream start.
    pub bridge_listen: Option<String>,
    /// Holds bridge.pem / bridge.key.pem, made and renewed by hostd at each stream start.
    pub tls_dir: PathBuf,
    /// Host headers allowed besides localhost:8443 and 127.0.0.1:8443 (`BRISK_ALLOWED_HOSTS`).
    pub allowed_hosts: Vec<String>,
    /// Tailnet logins let in through `tailscale serve`'s identity header; empty = off.
    pub allowed_users: Vec<String>,
    pub app_id: u32,
    pub secret: String,
    /// Running under systemd (`INVOCATION_ID`): stdout is the journal, so no secrets or tokens in it.
    pub service: bool,
    /// File drop target, created on the first upload (`BRISK_DOWNLOADS_DIR`).
    pub downloads_dir: PathBuf,
    /// Where running games are looked up; tests point it at a fake.
    pub proc_dir: PathBuf,
    /// Games folder with one sub-folder per system (`BRISK_GAMES_DIR`, default `~/Games`).
    pub games_dir: PathBuf,
    /// Scan the games folder once at startup (`BRISK_SCAN_ON_START=1`).
    pub scan_on_start: bool,
    /// Hub machine list (`BRISK_MACHINES_FILE`); read by main.rs, absent = single host.
    pub machines_file: PathBuf,
    /// `BRISK_MACHINES_FILE` was set, so a missing file is an error rather than single host.
    pub machines_explicit: bool,
    /// Dolphin's config dir, holding `WiimoteNew.ini` (`BRISK_DOLPHIN_DIR`).
    pub dolphin_dir: PathBuf,
    /// Dolphin's user dir, holding `StateSaves/`, `GC/`, `Wii/` (`BRISK_DOLPHIN_USER_DIR`).
    pub dolphin_user_dir: PathBuf,
    /// How often the playtime watcher looks at running games (`BRISK_PLAYTIME_POLL_S`).
    pub playtime_poll: std::time::Duration,
    /// RetroAchievements: line 1 user name, line 2 Web API key (`BRISK_RA_KEY_FILE`). Missing = off.
    pub ra_key_file: PathBuf,
    /// `BRISK_RA_BASE`.
    pub ra_base: String,
    /// Least gap between two RetroAchievements requests; tests set it to zero.
    pub ra_min_interval: std::time::Duration,
}

impl Config {
    /// Defaults plus the `BRISK_*` overrides, with a fresh random secret: main.rs swaps in the
    /// persistent one (`load_secret`), so tests never touch ~/.config/brisk.
    pub fn from_env(dist: PathBuf) -> Config {
        Config::from_vars(dist, |n| std::env::var_os(n))
    }

    /// `from_env` with the variables read through `var`, so tests need not touch the environment.
    pub fn from_vars(dist: PathBuf, var: impl Fn(&str) -> Option<OsString>) -> Config {
        let env_path = |n: &str| var(n).map(PathBuf::from);
        let env_str = |n: &str| var(n).and_then(|v| v.into_string().ok());
        let home = env_path("HOME").unwrap_or_else(|| PathBuf::from("."));
        Config {
            listen: ([127, 0, 0, 1], 8443).into(),
            dist,
            data_dir: env_path("BRISK_DATA_DIR").unwrap_or_else(|| home.join(".local/share/brisk")),
            steam_dir: env_path("BRISK_STEAM_DIR")
                .unwrap_or_else(|| home.join(".local/share/Steam")),
            steam_key_file: env_path("BRISK_STEAM_KEY_FILE")
                .unwrap_or_else(|| home.join(".config/brisk/steam-api-key")),
            steam_api_base: env_str("BRISK_STEAM_API_BASE")
                .unwrap_or_else(|| "https://api.steampowered.com".into()),
            steam_store_base: env_str("BRISK_STEAM_STORE_BASE")
                .unwrap_or_else(|| "https://store.steampowered.com".into()),
            bridge_bin: env_path("BRISK_BRIDGE_BIN").unwrap_or_else(|| "brisk-bridge".into()),
            bridge_listen: env_str("BRISK_LISTEN").filter(|v| !v.is_empty()),
            tls_dir: env_path("BRISK_TLS_DIR").unwrap_or_else(|| home.join(".config/brisk/tls")),
            allowed_hosts: list(&env_str("BRISK_ALLOWED_HOSTS").unwrap_or_default()),
            allowed_users: list(&env_str("BRISK_ALLOWED_USERS").unwrap_or_default()),
            app_id: env_str("BRISK_APP")
                .and_then(|v| v.parse().ok())
                .unwrap_or(870779643),
            secret: new_secret(),
            service: var("INVOCATION_ID").is_some(),
            downloads_dir: env_path("BRISK_DOWNLOADS_DIR")
                .unwrap_or_else(|| home.join("Downloads/brisk")),
            games_dir: env_path("BRISK_GAMES_DIR").unwrap_or_else(|| home.join("Games")),
            scan_on_start: env_str("BRISK_SCAN_ON_START").is_some_and(|v| v == "1"),
            proc_dir: "/proc".into(),
            machines_file: env_path("BRISK_MACHINES_FILE")
                .unwrap_or_else(|| home.join(".config/brisk/machines.json")),
            machines_explicit: var("BRISK_MACHINES_FILE").is_some(),
            dolphin_dir: env_path("BRISK_DOLPHIN_DIR").unwrap_or_else(|| {
                env_path("XDG_CONFIG_HOME")
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or_else(|| home.join(".config"))
                    .join("dolphin-emu")
            }),
            // Dolphin's own rule: ~/.dolphin-emu if it exists, else XDG data home.
            dolphin_user_dir: env_path("BRISK_DOLPHIN_USER_DIR").unwrap_or_else(|| {
                let legacy = home.join(".dolphin-emu");
                if legacy.is_dir() {
                    return legacy;
                }
                env_path("XDG_DATA_HOME")
                    .filter(|p| p.is_absolute())
                    .unwrap_or_else(|| home.join(".local/share"))
                    .join("dolphin-emu")
            }),
            playtime_poll: crate::playtime::poll_from(env_str("BRISK_PLAYTIME_POLL_S").as_deref()),
            ra_key_file: env_path("BRISK_RA_KEY_FILE")
                .unwrap_or_else(|| home.join(".config/brisk/retroachievements")),
            ra_base: env_str("BRISK_RA_BASE")
                .unwrap_or_else(|| crate::retroachievements::DEFAULT_BASE.into()),
            ra_min_interval: std::time::Duration::from_secs(1),
        }
    }
}

/// hostd's startup line. Under systemd (`service`) it goes to the journal, so no secret in it.
pub fn page_line(secret: &str, service: bool) -> String {
    if service {
        "page\thttp://localhost:8443/ (link: scripts/brisk-service.sh link)".into()
    } else {
        format!("page\thttp://localhost:8443/?s={secret}")
    }
}

/// 128 random bits, hex.
pub(crate) fn new_secret() -> String {
    let mut s = [0u8; 16];
    OsRng.fill_bytes(&mut s);
    s.iter().map(|b| format!("{b:02x}")).collect()
}

/// Comma or newline separated; trimmed, `#` comments and empty entries dropped.
pub fn list(s: &str) -> Vec<String> {
    // A `#` comment runs to the end of its line, commas included.
    s.lines()
        .flat_map(|l| l.split('#').next().unwrap_or("").split(','))
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect()
}

/// `env` (`BRISK_SECRET`, for tests) if set, else the secret in `file`, created once (0600, its
/// directory 0700) so a bookmarked link survives restarts. Shorter than 16 characters is refused.
pub fn load_secret(env: Option<String>, file: &Path) -> std::io::Result<String> {
    use std::io::{Error, ErrorKind, Write};
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
    let check = |s: String| {
        if s.len() >= 16 && s.bytes().all(|b| b.is_ascii_graphic()) {
            Ok(s)
        } else {
            Err(Error::new(
                ErrorKind::InvalidData,
                format!(
                    "the secret ({}) is empty, too short or not printable",
                    file.display()
                ),
            ))
        }
    };
    if let Some(s) = env.filter(|s| !s.is_empty()) {
        return check(s);
    }
    match std::fs::read_to_string(file) {
        Ok(s) => {
            if std::fs::metadata(file)?.permissions().mode() & 0o077 != 0 {
                std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o600))?;
                tracing::warn!("{} was readable by others; now 0600", file.display());
            }
            check(s.trim().to_string())
        }
        Err(e) if e.kind() == ErrorKind::NotFound => {
            if let Some(dir) = file.parent() {
                std::fs::DirBuilder::new()
                    .recursive(true)
                    .mode(0o700)
                    .create(dir)?;
            }
            let s = new_secret();
            // create_new: of two hostds starting at once only one writes; the other reads it.
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(file)
            {
                Ok(mut f) => {
                    f.write_all(format!("{s}\n").as_bytes())?;
                    f.sync_all()?;
                    Ok(s)
                }
                Err(e) if e.kind() == ErrorKind::AlreadyExists => load_secret(None, file),
                Err(e) => Err(e),
            }
        }
        Err(e) => Err(e),
    }
}

/// One login per line (`#` comments ok). Ignored, with a warning, unless it is a regular file
/// (not a symlink) owned by us that only we can write (0600 or 0644): anyone else editing it
/// could let themselves in.
pub fn load_users(file: &Path) -> Vec<String> {
    load_users_as(file, rustix::process::getuid().as_raw())
}

/// `load_users` for an expected owner `uid`. Opened with O_NOFOLLOW (a symlink fails to open) and
/// checked with fstat on that same descriptor, so nothing can be swapped in between check and read.
pub fn load_users_as(file: &Path, uid: u32) -> Vec<String> {
    use std::io::Read;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
    let f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK) // NONBLOCK: a FIFO must not hang startup
        .open(file);
    let warn = || {
        tracing::warn!(
            "ignoring {} (must be a regular file, not a symlink, owned by uid {uid} and writable only by it: 0600 or 0644)",
            file.display()
        );
        vec![]
    };
    let mut f = match f {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return vec![],
        Err(_) => return warn(),
    };
    let Ok(m) = f.metadata() else {
        return warn();
    };
    if !m.file_type().is_file() || m.uid() != uid || m.permissions().mode() & 0o022 != 0 {
        return warn();
    }
    let mut s = String::new();
    if f.read_to_string(&mut s).is_err() {
        return warn();
    }
    list(&s)
}
