//! Powering a machine on and off. The hub talks to Proxmox with a token that can only power one VM.
//! Blocking (ureq, like steam.rs); async callers go through `spawn_blocking`.

use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// Power settings for one machine.
#[derive(Clone, Debug, Deserialize)]
pub struct PowerCfg {
    pub proxmox: ProxmoxCfg,
    /// Minutes with nothing happening before the hub shuts the machine down; absent = never.
    #[serde(default)]
    pub idle_shutdown_min: Option<u64>,
    /// URL answering `{"cpuTempC": number}` (Proxmox has no temperatures); checked in machines.rs.
    #[serde(default)]
    pub temp_url: Option<String>,
}

/// How to reach the Proxmox VM that backs a machine.
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

/// Whether the VM is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VmState {
    Running,
    Stopped,
}

/// VM load from Proxmox `status/current`: `cpu` is a 0..1 fraction, memory is in bytes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VmStats {
    pub cpu: f64,
    pub mem_used: u64,
    pub mem_total: u64,
}

/// Power control of one machine.
pub trait Power: Send + Sync {
    /// Whether the machine is running.
    fn status(&self) -> Result<VmState, String>;
    /// Returns once Proxmox accepted the request, not when the guest is up.
    fn start(&self) -> Result<(), String>;
    /// ACPI shutdown (never a hard stop).
    fn shutdown(&self) -> Result<(), String>;
    /// Load figures; not every backend has them.
    fn stats(&self) -> Result<VmStats, String> {
        Err("unsupported".into())
    }
}

/// Power control through the Proxmox API.
pub(crate) fn proxmox(cfg: &ProxmoxCfg) -> Arc<dyn Power> {
    Arc::new(Proxmox { cfg: cfg.clone() })
}

/// The checks that keep a Proxmox URL from changing shape; `machines::parse` runs them at startup.
pub(crate) fn check(c: &ProxmoxCfg) -> Result<(), String> {
    if !crate::steam::base_ok(c.api.trim_end_matches('/')) {
        return Err("proxmox api must be https (or http to loopback)".into());
    }
    if c.node.is_empty()
        || !c
            .node
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
    {
        return Err("proxmox node is not a plain node name".into());
    }
    if c.token_id.is_empty()
        || c.token_id
            .chars()
            .any(|ch| ch.is_whitespace() || ch.is_control())
    {
        return Err("proxmox token_id is not valid".into());
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Method {
    Get,
    Post,
}

struct Proxmox {
    cfg: ProxmoxCfg,
}

impl Power for Proxmox {
    fn status(&self) -> Result<VmState, String> {
        match self.current()?["status"].as_str() {
            Some("running") => Ok(VmState::Running),
            Some("stopped") => Ok(VmState::Stopped),
            _ => Err("unexpected VM status".into()),
        }
    }

    fn stats(&self) -> Result<VmStats, String> {
        let d = self.current()?;
        let cpu = d["cpu"].as_f64().filter(|c| c.is_finite() && *c >= 0.0);
        let (used, total) = (d["mem"].as_f64(), d["maxmem"].as_f64());
        let bytes = |f: Option<f64>| f.filter(|f| f.is_finite() && *f >= 0.0).map(|f| f as u64);
        match (cpu, bytes(used), bytes(total)) {
            // A VM with pinned memory (GPU passthrough) reports host-side use that can pass maxmem.
            (Some(cpu), Some(mem_used), Some(mem_total)) => Ok(VmStats {
                cpu,
                mem_used: mem_used.min(mem_total),
                mem_total,
            }),
            _ => Err("unexpected VM stats".into()),
        }
    }

    /// Already running counts as started (status is checked first).
    fn start(&self) -> Result<(), String> {
        if self.status()? == VmState::Running {
            return Ok(());
        }
        self.call(Method::Post, "status/start").map(drop)
    }

    /// Already stopped counts as shut down (status is checked first).
    fn shutdown(&self) -> Result<(), String> {
        if self.status()? == VmState::Stopped {
            return Ok(());
        }
        self.call(Method::Post, "status/shutdown").map(drop)
    }
}

impl Proxmox {
    /// The `data` object of `status/current`.
    fn current(&self) -> Result<serde_json::Value, String> {
        let body = self.call(Method::Get, "status/current")?;
        let mut v: serde_json::Value =
            serde_json::from_slice(&body).map_err(|_| "response is not JSON")?;
        Ok(v["data"].take())
    }

    /// The URL of `suffix` under this VM; every part is validated, so it cannot change shape.
    fn url(&self, suffix: &str) -> Result<String, String> {
        let c = &self.cfg;
        check(c)?;
        let api = c.api.trim_end_matches('/');
        Ok(format!(
            "{api}/api2/json/nodes/{}/qemu/{}/{suffix}",
            c.node, c.vmid
        ))
    }

    /// One request (GET, or POST with an empty body). The secret is read here and only goes into
    /// the Authorization header; errors carry only the kind of failure.
    fn call(&self, method: Method, suffix: &str) -> Result<Vec<u8>, String> {
        use ureq::tls::{Certificate, RootCerts, parse_pem};
        let url = self.url(suffix)?;
        let secret = std::fs::read_to_string(&self.cfg.token_file).map_err(|_| "no token file")?;
        let secret = secret.trim();
        if secret.is_empty() || secret.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err("token file is not a plain secret".into());
        }
        let roots = match &self.cfg.ca_file {
            None => RootCerts::PlatformVerifier,
            Some(p) => {
                let pem = std::fs::read(p).map_err(|_| "no CA file")?;
                let certs = parse_pem(&pem)
                    .filter_map(|i| match i {
                        Ok(ureq::tls::PemItem::Certificate(c)) => Some(Ok(c)),
                        Ok(_) => None,
                        Err(_) => Some(Err(())),
                    })
                    .collect::<Result<Vec<Certificate<'static>>, ()>>()
                    .map_err(|_| "CA file is not valid PEM")?;
                if certs.is_empty() {
                    return Err("CA file has no certificate".into());
                }
                RootCerts::Specific(Arc::new(certs))
            }
        };
        let agent = crate::net::agent(
            roots,
            Some(Duration::from_secs(5)),
            Duration::from_secs(20),
            true,
        );
        let auth = format!("PVEAPIToken={}={secret}", self.cfg.token_id);
        let res = if method == Method::Post {
            agent.post(&url).header("Authorization", &auth).send_empty()
        } else {
            agent.get(&url).header("Authorization", &auth).call()
        };
        let mut r = res.map_err(|e| crate::net::safe_err(&e))?;
        r.body_mut()
            .with_config()
            .limit(1 << 20)
            .read_to_vec()
            .map_err(|e| crate::net::safe_err(&e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Mutex;

    const SECRET: &str = "11111111-2222-3333-4444-555555555555";

    type Log = Arc<Mutex<Vec<(String, String, Option<String>)>>>;

    /// Fake Proxmox on loopback: answers `(status, body)` from `reply(method, path)` and records
    /// (method, path, Authorization) per request.
    fn fake(reply: impl Fn(&str, &str) -> (u16, String) + Send + 'static) -> (String, Log) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        let log: Log = Default::default();
        let seen = log.clone();
        std::thread::spawn(move || {
            for mut s in l.incoming().flatten() {
                let mut buf = Vec::new();
                let mut b = [0u8; 1024];
                while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    match s.read(&mut b) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => buf.extend_from_slice(&b[..n]),
                    }
                }
                let text = String::from_utf8_lossy(&buf).into_owned();
                let mut lines = text.lines();
                let mut first = lines.next().unwrap_or("").split(' ');
                let (m, p) = (first.next().unwrap_or(""), first.next().unwrap_or(""));
                let auth = lines
                    .find_map(|l| {
                        l.strip_prefix("authorization: ")
                            .or(l.strip_prefix("Authorization: "))
                    })
                    .map(str::to_string);
                seen.lock().unwrap().push((m.into(), p.into(), auth));
                let (code, body) = reply(m, p);
                let _ = write!(
                    s,
                    "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        (base, log)
    }

    fn cfg(dir: &tempfile::TempDir, api: &str) -> ProxmoxCfg {
        let token_file = dir.path().join("secret");
        std::fs::write(&token_file, format!("{SECRET}\n")).unwrap();
        ProxmoxCfg {
            api: api.into(),
            node: "pve-1.example".into(),
            vmid: 110,
            token_id: "hub@pve!power".into(),
            token_file,
            ca_file: None,
        }
    }

    fn running(state: &'static str) -> impl Fn(&str, &str) -> (u16, String) + Send + 'static {
        move |m, _| match m {
            "GET" => (200, format!(r#"{{"data":{{"status":"{state}"}}}}"#)),
            _ => (200, r#"{"data":"UPID:x"}"#.into()),
        }
    }

    const BASE: &str = "/api2/json/nodes/pve-1.example/qemu/110";

    #[test]
    fn status_parses_and_sends_exact_request() {
        let dir = tempfile::tempdir().unwrap();
        for (state, want) in [("running", VmState::Running), ("stopped", VmState::Stopped)] {
            let (api, log) = fake(running(state));
            assert_eq!(proxmox(&cfg(&dir, &api)).status(), Ok(want));
            let l = log.lock().unwrap();
            assert_eq!(l.len(), 1);
            assert_eq!(l[0].0, "GET");
            assert_eq!(l[0].1, format!("{BASE}/status/current"));
            assert_eq!(
                l[0].2.as_deref(),
                Some(format!("PVEAPIToken=hub@pve!power={SECRET}").as_str())
            );
        }
    }

    #[test]
    fn stats_parses_and_rejects_bad_values() {
        let dir = tempfile::tempdir().unwrap();
        let (api, log) = fake(|_, _| {
            (
                200,
                r#"{"data":{"status":"running","cpu":0.256,"mem":1024,"maxmem":4096}}"#.into(),
            )
        });
        assert_eq!(
            proxmox(&cfg(&dir, &api)).stats(),
            Ok(VmStats {
                cpu: 0.256,
                mem_used: 1024,
                mem_total: 4096
            })
        );
        assert_eq!(log.lock().unwrap()[0].1, format!("{BASE}/status/current"));
        let (api, _) = fake(|_, _| {
            (
                200,
                r#"{"data":{"status":"running","cpu":0.1,"mem":5000,"maxmem":4096}}"#.into(),
            )
        });
        assert_eq!(proxmox(&cfg(&dir, &api)).stats().unwrap().mem_used, 4096);
        for body in [
            r#"{"data":{"status":"stopped"}}"#,
            r#"{"data":{"cpu":-1,"mem":1,"maxmem":2}}"#,
            r#"{"data":{"cpu":0.1,"mem":-5,"maxmem":2}}"#,
            r#"{"data":{"cpu":"x","mem":1,"maxmem":2}}"#,
        ] {
            let (api, _) = fake(move |_, _| (200, body.into()));
            assert!(proxmox(&cfg(&dir, &api)).stats().is_err(), "{body}");
        }
    }

    #[test]
    fn start_posts_when_stopped_and_noops_when_running() {
        let dir = tempfile::tempdir().unwrap();
        let (api, log) = fake(running("stopped"));
        proxmox(&cfg(&dir, &api)).start().unwrap();
        let l = log.lock().unwrap();
        assert_eq!(l.len(), 2);
        assert_eq!(
            (l[1].0.as_str(), l[1].1.as_str()),
            ("POST", format!("{BASE}/status/start").as_str())
        );
        assert!(l[1].2.as_deref().unwrap().ends_with(SECRET));
        drop(l);

        let (api, log) = fake(running("running"));
        proxmox(&cfg(&dir, &api)).start().unwrap();
        assert_eq!(log.lock().unwrap().len(), 1, "only the status GET");
    }

    #[test]
    fn shutdown_posts_when_running_and_noops_when_stopped() {
        let dir = tempfile::tempdir().unwrap();
        let (api, log) = fake(running("running"));
        proxmox(&cfg(&dir, &api)).shutdown().unwrap();
        let l = log.lock().unwrap();
        assert_eq!(
            (l[1].0.as_str(), l[1].1.as_str()),
            ("POST", format!("{BASE}/status/shutdown").as_str())
        );
        drop(l);

        let (api, log) = fake(running("stopped"));
        proxmox(&cfg(&dir, &api)).shutdown().unwrap();
        assert_eq!(log.lock().unwrap().len(), 1);
    }

    #[test]
    fn http_errors_are_mapped_and_hide_the_secret() {
        let dir = tempfile::tempdir().unwrap();
        for code in [401u16, 500] {
            let (api, _) = fake(move |_, _| (code, format!("{{\"errors\":\"{SECRET}\"}}")));
            let c = cfg(&dir, &api);
            let e = proxmox(&c).status().unwrap_err();
            assert_eq!(e, format!("HTTP {code}"));
            assert!(!e.contains(SECRET) && !e.contains("127.0.0.1"));
        }
        // POST failing after a good status.
        let (api, _) = fake(|m, _| match m {
            "GET" => (200, r#"{"data":{"status":"stopped"}}"#.into()),
            _ => (500, "{}".into()),
        });
        assert_eq!(proxmox(&cfg(&dir, &api)).start().unwrap_err(), "HTTP 500");
        // Unexpected payloads are errors, not a guess.
        let (api, _) = fake(|_, _| (200, r#"{"data":{"status":"paused"}}"#.into()));
        assert!(proxmox(&cfg(&dir, &api)).status().is_err());
        let (api, _) = fake(|_, _| (200, "nope".into()));
        assert!(proxmox(&cfg(&dir, &api)).status().is_err());
    }

    #[test]
    fn bad_config_is_rejected_without_a_request() {
        let dir = tempfile::tempdir().unwrap();
        let (api, log) = fake(running("running"));
        let good = cfg(&dir, &api);
        let bad: Vec<ProxmoxCfg> = vec![
            ProxmoxCfg {
                api: "http://example.invalid:8006".into(),
                ..good.clone()
            },
            ProxmoxCfg {
                api: "ftp://example.invalid".into(),
                ..good.clone()
            },
            ProxmoxCfg {
                node: "a/../b".into(),
                ..good.clone()
            },
            ProxmoxCfg {
                node: "".into(),
                ..good.clone()
            },
            ProxmoxCfg {
                token_id: "hub@pve!x\nHost: y".into(),
                ..good.clone()
            },
            ProxmoxCfg {
                token_id: "hub@pve!a b".into(),
                ..good.clone()
            },
            ProxmoxCfg {
                token_file: dir.path().join("missing"),
                ..good.clone()
            },
        ];
        for c in bad {
            let e = proxmox(&c).status().unwrap_err();
            assert!(!e.contains(SECRET), "{e}");
        }
        std::fs::write(&good.token_file, " \n").unwrap();
        assert!(proxmox(&good).status().is_err());
        assert!(log.lock().unwrap().is_empty());
    }

    #[test]
    fn garbage_ca_file_is_a_clean_error() {
        let dir = tempfile::tempdir().unwrap();
        let (api, log) = fake(running("running"));
        let ca = dir.path().join("ca.pem");
        for text in [
            "not a pem",
            "-----BEGIN CERTIFICATE-----\n!!!\n-----END CERTIFICATE-----\n",
        ] {
            std::fs::write(&ca, text).unwrap();
            let c = ProxmoxCfg {
                ca_file: Some(ca.clone()),
                ..cfg(&dir, &api)
            };
            let e = proxmox(&c).status().unwrap_err();
            assert!(e.starts_with("CA file"), "{e}");
        }
        assert!(log.lock().unwrap().is_empty());
    }
}
