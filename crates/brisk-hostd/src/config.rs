use rand_core::{OsRng, RngCore};
use std::net::SocketAddr;
use std::path::PathBuf;

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
    pub bridge_bin: PathBuf,
    pub bridge_listen: String,
    pub app_id: u32,
    pub secret: String,
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from)
}

impl Config {
    /// Defaults plus the `BRISK_*` overrides; a fresh per-run secret (128 random bits, hex).
    pub fn from_env(dist: PathBuf) -> Config {
        let home = env_path("HOME").unwrap_or_else(|| PathBuf::from("."));
        let mut s = [0u8; 16];
        OsRng.fill_bytes(&mut s);
        Config {
            listen: ([127, 0, 0, 1], 8443).into(),
            dist,
            data_dir: env_path("BRISK_DATA_DIR").unwrap_or_else(|| home.join(".local/share/brisk")),
            steam_dir: env_path("BRISK_STEAM_DIR")
                .unwrap_or_else(|| home.join(".local/share/Steam")),
            steam_key_file: env_path("BRISK_STEAM_KEY_FILE")
                .unwrap_or_else(|| home.join(".config/brisk/steam-api-key")),
            steam_api_base: std::env::var("BRISK_STEAM_API_BASE")
                .unwrap_or_else(|_| "https://api.steampowered.com".into()),
            bridge_bin: env_path("BRISK_BRIDGE_BIN").unwrap_or_else(|| "brisk-bridge".into()),
            // Same names as scripts/brisk-up.sh. [U] 127.0.0.1 default: brisk-up.sh passes the tailscale IP.
            bridge_listen: std::env::var("BRISK_LISTEN")
                .unwrap_or_else(|_| "127.0.0.1:4433".into()),
            app_id: std::env::var("BRISK_APP")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(870779643),
            secret: s.iter().map(|b| format!("{b:02x}")).collect(),
        }
    }
}
