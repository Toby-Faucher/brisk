//! Powering a machine on and off. The hub talks to Proxmox with a token that can only power one VM.
//! Blocking (ureq, like steam.rs); async callers go through `spawn_blocking`.

use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone, Debug, Deserialize)]
pub struct PowerCfg {
    pub proxmox: ProxmoxCfg,
    /// Minutes with nothing happening before the hub shuts the machine down; absent = never.
    #[serde(default)]
    pub idle_shutdown_min: Option<u64>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ProxmoxCfg {
    /// `https://host:8006`
    pub api: String,
    pub node: String,
    pub vmid: u32,
    /// `user@realm!tokenname`
    pub token_id: String,
    /// File holding the token secret (a UUID); read per call.
    pub token_file: PathBuf,
    /// PEM of the CA (or self-signed cert) that signed the Proxmox certificate.
    #[serde(default)]
    pub ca_file: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VmState {
    Running,
    Stopped,
}

pub trait Power: Send + Sync {
    fn status(&self) -> Result<VmState, String>;
    /// Returns once Proxmox accepted the request, not when the guest is up.
    fn start(&self) -> Result<(), String>;
    /// ACPI shutdown (never a hard stop).
    fn shutdown(&self) -> Result<(), String>;
}

pub fn proxmox(_cfg: &ProxmoxCfg) -> Arc<dyn Power> {
    Arc::new(Unwired)
}

struct Unwired;
impl Power for Unwired {
    fn status(&self) -> Result<VmState, String> {
        Err("not implemented".into())
    }
    fn start(&self) -> Result<(), String> {
        Err("not implemented".into())
    }
    fn shutdown(&self) -> Result<(), String> {
        Err("not implemented".into())
    }
}
