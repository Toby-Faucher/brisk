<div align="center">

<img src="assets/logo.png" alt="Brisk" width="360">

### Your Sunshine games, in a browser tab. As close to local as physics allows.

![Rust](https://img.shields.io/badge/Rust-2024-5eead4?style=flat-square&logo=rust&logoColor=04201b&labelColor=121419)
![Astro + Svelte](https://img.shields.io/badge/UI-Astro%20%2B%20Svelte-5eead4?style=flat-square&labelColor=121419)
![Chrome](https://img.shields.io/badge/browser-Chrome-5eead4?style=flat-square&logo=googlechrome&logoColor=5eead4&labelColor=121419)
![Sunshine](https://img.shields.io/badge/host-Sunshine-5eead4?style=flat-square&labelColor=121419)
![Status](https://img.shields.io/badge/status-personal%20project-818cf8?style=flat-square&labelColor=121419)

</div>

---

Brisk is a web Moonlight client. It plays games from a [Sunshine](https://github.com/LizardByte/Sunshine) host in Chrome over WebTransport and WebCodecs, with a pure-Rust protocol client and bridge behind it.

It exists because the existing web clients felt laggy and clunky. Brisk aims at **low input-to-photon latency**, **steady frame pacing** (a stable frame time beats a low average) and **controller input first**, with rumble coming back. Desktop mouse and keyboard come second.

## What you get

| | |
|---|---|
| **Fast path** | A small Sunshine patch (the "tap") sends encoded frames straight to the bridge, skipping FEC and AES on the host. |
| **Pure Rust** | The protocol client, bridge and host agent are Rust, with no C or C++ in the bridge. `scripts/check-pure-rust.sh` enforces it. |
| **Controller first** | Low-jitter gamepad input with rumble, plus a controller test page and a configurable deadzone. |
| **A real UI** | Library with filters and search, session history, machines, settings, and drag to reorder on Home. |
| **Clips and replay** | Save instant replays and clips, with a configurable replay length. |
| **Honest numbers** | Latency is measured and shown in the UI, and the project measures before it optimises. |
| **Accents** | Mint, indigo, pink, orange, lime or sky. Mint is the default. |

## How it fits together

```
 laptop (Chrome)                                bawn (Sunshine host)
┌──────────────────┐   https, tailnet only    ┌─────────────────────────────┐
│ Brisk web UI     │◄────────────────────────►│ tailscale serve (TLS)       │
│ WebCodecs decode │                          │   ├─ brisk-hostd  (agent)   │
│ WebTransport     │◄════ video/audio/input ══►│   └─ brisk-bridge (QUIC)    │
└──────────────────┘                          │        ▲                    │
                                              │        │ tap (encoded NALs) │
                                              │   Sunshine (patched)        │
                                              └─────────────────────────────┘
```

| Crate | Job |
|---|---|
| `brisk-proto` | Sans-IO protocol: RTSP, RTP, ENet, control messages, audio, pairing. No sockets. Fuzzed. |
| `brisk-client` | Session client and the `brisk-probe` CLI. |
| `brisk-bridge` | Runs beside Sunshine and speaks QUIC and WebTransport to the browser. |
| `brisk-hostd` | Host agent: serves the UI and API, auth, the Steam and emulator game library, starts the bridge per stream, restarts Sunshine. With a `machines.json` it is the machine hub. |

## Status

| | |
|---|---|
| **Works today** | Streaming from a Sunshine host in Chrome with video, audio, keyboard, mouse and gamepad (with rumble), clips and replay, session history, a game library with search and per-game quality. |
| **Steam** | A game panel with store info, achievements, news, friends online, your screenshots, and your own notes and colored tags. |
| **Emulator games** | Wii and GameCube games through Dolphin: uploads from the browser or a games folder scan, a controller mappings editor, launch options, playtime, a read-only saves list and stream screenshots. |
| **Art and progress** | SteamGridDB art, GameTDB details, and RetroAchievements progress for emulator games. |
| **Machine hub** | One page for several streaming machines, with power on and idle shutdown for a Proxmox VM. See [`docs/hosting-runbook.md`](docs/hosting-runbook.md#machine-hub). |
| **Browsers** | Chrome is the target. Firefox 158+ on desktop connects and streams but is rough (about 100 ms latency, 30-40 fps, no HEVC, no Keyboard Lock). |
| **Not planned** | Safari, mobile, non-Sunshine hosts, public internet exposure. |

## Prerequisites

- **Rust** through rustup. `rust-toolchain.toml` pins the version, so rustup installs the right one on the first `cargo` call.
- **Node 26** and npm for the web UI in `web/`.
- **Sunshine with the tap patch** on the host. `scripts/sunshine-brisk.sh build` clones, patches and builds it; see [`patches/sunshine/`](patches/sunshine/README.md).
- **Tailscale** on the host and the laptop. The bridge listens on the host's tailnet address.

## Run locally

- `scripts/brisk-up.sh` builds the bridge and hostd, switches to the patched Sunshine and runs hostd in the foreground. It prints the page link. `scripts/brisk-down.sh` puts stock Sunshine back.
- `scripts/check.sh` runs every check (Rust and web) in order. All of them must pass.
- In `web/`: `npm ci`, then `npm run dev` for a dev server on 127.0.0.1, `npm run ci` for the web checks and `npm run fmt` to format.

How it is built and checked: [`docs/dev.md`](docs/dev.md). Hosting it for real, with the systemd user service and Tailscale, is in [`docs/hosting-runbook.md`](docs/hosting-runbook.md).

## Read more

- [`docs/design.md`](docs/design.md): goals, non-goals and the architecture decisions
- [`docs/architecture.md`](docs/architecture.md): processes, crates, data files and the path of a stream
- [`docs/ui-notes.md`](docs/ui-notes.md): the UI, the API and how each part behaves
- [`docs/dev.md`](docs/dev.md): toolchain, lints, the full list of checks, fuzzing
- [`docs/hosting-runbook.md`](docs/hosting-runbook.md): standing it up on a host
- [`docs/env.md`](docs/env.md): every `BRISK_*` environment variable
- [`docs/moonlight-compare.md`](docs/moonlight-compare.md): how it compares with Moonlight
- [`docs/fuzz-inventory.md`](docs/fuzz-inventory.md): what is fuzzed
- [`CONTRIBUTING.md`](CONTRIBUTING.md): how to send a change

## Scope

Brisk is a personal project, built around one setup: a Sunshine host, Chrome clients and a Tailscale network. Safari, mobile, non-Sunshine hosts and public internet exposure are out of scope for now, and Firefox is best effort (see Status).

Licensed under the [MIT License](LICENSE). The Sunshine tap patch in `patches/sunshine/` is a derivative of Sunshine and is under Sunshine's GPL-3.0 license.
