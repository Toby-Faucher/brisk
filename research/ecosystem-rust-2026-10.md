# Research: the Rust ecosystem in Oct 2026, fit for Brisk

Date 2026-10-25. Research only: the repo (Cargo.toml, Cargo.lock, target) was not touched. Builds ran in a scratch copy with `CARGO_TARGET_DIR` outside the repo, capped by `scripts/capped.sh`, 4 jobs; scratch deleted at the end. Sources: crates.io API (version, date, licence, downloads, MSRV), GitHub API (pushed date, contributors), the RustSec advisory-db clone that `cargo-audit 0.22.2` fetched (1352 advisories, DB commit of 2026-10-24), blog.rust-lang.org, RELEASES.md of rust-lang/rust, and `cargo tree` probes of each candidate with default features unless stated. "Pure" below means: no `cc`, `-sys`, `ring`, `aws-lc-sys`, `openssl-sys`, `bindgen`, `cmake` in `cargo tree -e normal,build` on x86_64-linux (`linux-raw-sys` is pure Rust and ignored). Downloads are the crates.io 90-day figure. "Top author" is the share of the top 100 GitHub contributors' commits, a rough bus-factor proxy. [U] = not verified from a primary source.

## TL;DR

1. **The big 2026 change for the pure-Rust rule: `graviola` (ctz, a rustls maintainer) + `rustls-graviola`.** No C compiler, no `cc`, no `ring`; formally verified s2n-bignum asm via Rust `global_asm`, builds in 1 s. I proved it here: `ureq 3.5` + `rustls-graviola 0.4` + platform verifier fetched `api.steampowered.com` over HTTPS, 34 crates, `check-pure-rust.sh`'s four package names absent on all six targets. bawn's i7-12700 has every required CPU flag (aes, avx2, adx, bmi2, pclmulqdq). Caveat: v0.4, "very new", x86_64/aarch64 only, **no QUIC support yet** (draft PR #156 stalled since March; quinn also needs patches).
2. **Adopt next, ranked:** (1) `ureq` + `rustls-graviola` replaces `curl` for Steam; (2) `tower-http` 0.7 `ServeDir` (Range, precompressed, ETag, If-None-Match, all verified in source) replaces `static_file`; (3) `sd-notify` 0.5 (3 crates: Type=notify, watchdog, `listen_fds`); (4) `rustix` pidfd (`pidfd_open` + `pidfd_send_signal` exist in 1.1.5) fixes the pid-reuse race and replaces `kill` and libc `unsafe`; (5) `cargo-fuzz` on the brisk-proto parsers; (6) `cargo-nextest` + `cargo-deny` in CI; (7) `rust-embed` for single-binary (with the module); (8) edition 2024 + `[workspace.lints]` + pin `rust-toolchain.toml` to 1.99.1.
3. **Replace what we hand-rolled:** static serving -> tower-http; `curl` -> ureq; `kill` spawn -> rustix pidfd; `systemctl`/`tailscale` spawn -> keep `tailscale status --json` (the local API is a unix socket, `tailscale-localapi` 0.6 has 10k downloads, skip), use `zbus` only when you need signals not polling; arg parsing -> `lexopt` (2 crates); /proc parsing -> keep by hand (procfs last released Aug 2025; `sysinfo` MSRV 1.95 and 87% one author).
4. **Audit of OUR graph (cargo-audit 0.22.2, 218 crates): 7 findings, none reachable.** Six are `rustls-webpki 0.102.8` dragged in by `rustls-rustcrypto 0.0.2-alpha`, which only uses its `alg_id` constants (the real chain verifier is rustls' own 0.103.16). One is `rsa 0.9.10` Marvin (RUSTSEC-2023-0071, no fix, 0.10 still rc). Plus `crypto-bigint 0.5.5` timing (RUSTSEC-2026-0387, published 2 days ago) and `paste` unmaintained. All five come from the RustCrypto 0.x generation that `rustls-rustcrypto` pins.
5. **Skip:** `ort`/`fastembed` (ONNX Runtime C++, plus openssl-sys, ring), `candle-core` (hard-enables `tokenizers/onig`, a C regex lib), `usearch` (C++ via cxx), `rusqlite` and `heed` (C), `turso`/`libsql` (339/217 crates, bindgen, cc, mimalloc-sys, zstd-sys), `reqwest` (104 crates), `wtransport`/`web-transport-quinn` defaults (ring/aws-lc), `hickory` (RUSTSEC-2026-0118 unpatched), Leptos/Dioxus, askama/maud, `bacon` (AGPL), `bincode` (dead).
6. **Keep:** `quic_crypto.rs` (832 lines). Nothing in the ecosystem can replace it today: quinn's rustls glue needs ring/aws-lc, and graviola has no QUIC yet. Revisit when graviola #156 or quinn changes land.

## 0. Purity check: what it really allows

`scripts/check-pure-rust.sh` fails if `ring`, `aws-lc-sys`, `openssl-sys` or `cc` appear in `cargo tree --workspace -e normal,build` for six targets (linux/mac/windows, x64/arm64). It does **not** reject `-sys` crates in general (the brief says it does; the script lists four names). `cc` is the catch-all for "something compiles C at build time", which is a proxy, not a proof. Facts from probes:

- **`cc` in the graph but no C compiled:** `wayland-backend 0.3.17` (via `wayland-client`, `wl-clipboard-rs 0.9.4`) lists `cc` as an unconditional build-dependency, but `build.rs` calls it only when its `log` feature is on (`log_shim.c`). `cargo tree -e features` shows `log` is **not** enabled by wl-clipboard-rs; the crate builds. So the answer to "can the purity check be satisfied now": no, `cc` still appears (and `wayland-sys`, a binding crate with no C unless `client_system` is on). Options: allow `cc` only when its sole parent is `wayland-backend` and `wayland-backend/log` is off (script change of about 6 lines), or keep shelling out to `wl-copy`. 44 crates for wl-clipboard-rs. Verdict: keep `wl-copy`/`wl-paste` spawning until you want clipboard without a process per event; revisit then.
- **`cc` for asm kernels:** `tract-linalg` (cc build-dep for asm), so tract fails the check.
- **Assembly is not C:** graviola uses `global_asm!`/intrinsics inside Rust. It passes the script, but it is not "no assembly"; that is a taste call. `rustls-pure` (new, 107 downloads, TLS 1.3 only, ECDSA/Ed25519 only) claims no C or asm; too young to use, and Sunshine's pairing cert is RSA.

## 1. Core / web

| Crate | Version (date) | Licence | 90d dl | Bus / last push | Pure, deps, MSRV | Verdict |
|---|---|---|---|---|---|---|
| axum | 0.8.9 (2026-04-14) | MIT | 141M | tokio-rs, top author 821 commits; pushed 2026-10-20 | pure; 58 crates with defaults; 1.80 | **Keep.** No 0.9 yet, no new major in 2026. We already use `default-features=false`. Add `ws` feature only for the web terminal. |
| hyper | 1.12.0 (2026-10-06) | MIT | 249M | top author 69%; pushed 2026-10-13 | pure; 1.63 | Transitive only. Skip direct use. |
| tower | 0.5.3 (2026-01-12) | MIT | 210M | tower-rs | pure; 1.64 | Keep (dev-dep for `oneshot` tests). |
| tower-http | **0.7.1** (2026-08-31) | MIT | 189M | top author 25%; pushed 2026-10-25 | pure; 39 crates with fs, gzip, trace, cors, set-header; 1.65 | **Replace what we have (static).** 0.7.1 `ServeDir` source has `Range` (single-range, `ignore_multi_range_requests` option), `precompressed_gzip/br/zstd/deflate`, `ETag`, `If-None-Match`, `If-Modified-Since`, `Last-Modified`. Ours (`static_file`, http.rs:495) reads the whole file, no range, no cache headers, no compression, 9 MIME types. Needs a small wrapper for our SPA rule (missing path with an extension = 404, else index.html): `not_found_service` alone returns index for `foo.js`. Disk-only, so not for embedded. Add `trace` (request logs) and `set-header` (CSP) for free. |
| rust-embed | 8.13.0 (2026-10-07) | MIT | 19M | repo **moved off GitHub** to pyrossh.dev (github.com/pyrossh/rust-embed returns 404) | pure; 27 crates; 1.80 | **Later, with single-binary deploy.** Reads from disk in debug builds, embeds in release; optional compression. Mind the new forge: pin exact and vendor-check. You write the 40-line handler (ETag from the embedded hash it exposes). |
| include_dir | 0.7.4 (2024-06-17) | MIT | 20M | stale 16 months | pure | Skip (rust-embed is the maintained one). |
| memory-serve | 2.4.0 (2026-09-17) | MIT/Apache | 29k | small | pure but **92 crates** (brotli at build) | Skip. Too heavy for what rust-embed + tower gives. |
| axum-extra | 0.12.6 (2026-04-14) | MIT | 14M | same team | pure; 31 | Skip until a need (typed headers, cookie jar). |
| askama 0.16.1 / maud 0.27 | 2026-09 / 2025-02 | MIT/Apache | 15M / 2.8M | | pure | Skip: Astro/Svelte front end already exists. |
| ureq | 3.5.0 (2026-10-21) | MIT/Apache | 75M | top author 63%; pushed 2026-10-22 | **verified pure** with `rustls-no-provider,platform-verifier`: 24-34 crates; 1.85 | **Adopt now** for Steam (blocking, run in `spawn_blocking`). Needs the graviola provider handed in; first-party defaults pull ring/aws-lc, so use `default-features=false`. Tested live: 21 KB response, 200 OK, 4.6 MB unstripped release binary. |
| reqwest | 0.13.5 (2026-09-08) | MIT/Apache | 231M | top author 59% | pure with `rustls-no-provider`, but **104 crates** | Skip: 3-4x ureq for one GET. |
| hyper-util + tokio-rustls | 0.1.21 / 0.26 | MIT | | | pure with graviola | Only if hostd needs an async HTTPS client for many calls (media metadata, Home Assistant). More glue than ureq. |
| rustls | 0.23.46 (2026-10-14) | Apache/ISC/MIT | 249M | top author 49%; pushed 2026-10-23 | pure core | See §1a. **0.24 is still `0.24.0-dev.2` on main**; a tracking issue (#2400) is mostly ticked. From 0.24 you must pass a crypto provider explicitly and `ring`/`aws-lc-rs` move to separate `rustls-ring`/`rustls-aws-lc-rs` crates, which suits Brisk. Five RUSTSEC advisories since 2026-09-14, all fixed in 0.23.46 (we have it). |
| hickory-resolver | 0.26.3 (2026-09-10) | MIT/Apache | 29M | | pure; 110 | Skip. `hickory-proto` has RUSTSEC-2026-0118 (DNSSEC NSEC3 loop) **with no patched version**. System resolver is fine on a tailnet. |
| quinn / quinn-proto | 0.11.12 / 0.11.19 | MIT/Apache | 133M | 40%; pushed 2026-10-23 | pure core; default features pull `rustls-ring` | **Keep, no default features (as now).** Eight quinn-proto advisories 2026-03 to 2026-09 (remote panics, memory exhaustion); fixed at 0.11.18+. We have 0.11.19. Expect more: it is a network-facing parser. Run `cargo update -p quinn-proto` whenever advisories drop. |
| wtransport | 0.7.2 (2026-08-11) | MIT/Apache | 510k | 79% one author | default has **ring**; `--no-default-features --features quinn` pure but supplies no provider; 131 crates | Skip. Same quinn glue blocker. |
| web-transport-quinn | 0.13.5 (2026-10-24) | MIT/Apache | 1.3M | moq-dev, 70% one author; pushed today | defaults pull **aws-lc-sys, cmake**; `--no-default-features` pure, 123 crates | Skip for now; the most active WT stack (moq.dev). Same glue blocker. |
| web-transport-proto | 0.6.3 (2026-10-23) | MIT/Apache | 1.5M | same | pure, but its `url` dependency brings the **ICU/idna tree: 35 of our 240 crates** | **Keep, but note the cost.** We use only `ConnectRequest`, `ConnectResponse`, `Settings`, `StreamUni`, `VarInt` (wt.rs:19). Either send an upstream PR making `url` optional [U] or hand-roll those five types (about 200 lines, we already own the QUIC layer). |
| h3-webtransport | 0.1.2 (2025-05-06) | MIT | 179k | | | Skip (stale, experimental). |
| s2n-quic | 1.91.0 (2026-10-14) | Apache-2.0 | 212k | AWS | own crypto via s2n-tls (C) or rustls; MSRV 1.95 | Skip (C by default). |
| quiche | (Cloudflare, C/BoringSSL) | | | | C | Reject (rule). |
| webrtc (webrtc-rs) / str0m | 0.21.1 / 0.24.1 | MIT/Apache | 3.2M / 1.2M | | str0m default pulls **aws-lc-sys, cmake** | Skip. Design says WebTransport only; no WebRTC in goals. |

### 1a. Pure-Rust TLS providers, Oct 2026

| Provider | State | QUIC suites | Verdict |
|---|---|---|---|
| `rustls-rustcrypto` 0.0.2-alpha (RustCrypto org) | **No release since 2024-04-24**; README says "DO NOT USE IN PRODUCTION"; git main only has dependabot bumps (last 2026-08-28), version string still 0.0.2-alpha; pins `rustls-webpki 0.102` | no | **Replace** (we exact-pin it, so no surprise bump, but it is the root of 6 advisories + paste + 3 duplicate stacks). |
| `rustls-graviola` 0.4.0 / `graviola` 0.4.1 (ctz) | 2026-06; 315 stars; pushed today; 238k downloads; Apache/ISC/MIT-0; RSA, ECDSA P-256/384, Ed25519, X25519, ML-KEM-768, AES-GCM, ChaCha20; x86_64 + aarch64 only, requires AVX2/ADX/BMI2/PCLMUL | **no** (issue #98 since 2025, PR #156 draft since 2026-03; ctz said quinn needs patches too) | **Adopt for non-QUIC TLS** (Steam HTTPS, and brisk-client's Sunshine HTTPS, which uses RSA client certs: graviola does RSA PKCS#1/PSS sign+verify and 2048+ keygen, "no RSA encryption"). Maintainer is the rustls co-lead, which is the best bus factor in this category. Risk: "very new, exercise due caution". [U] whether Sunshine's TLS 1.2 ciphers are all covered; spike with a pairing run. |
| `ring` 0.17.14 (2025-03-11) | Author hiatus, rustls team does security-only maintenance (RUSTSEC-2025-0007 update). C + asm via build.rs | yes | Stays banned by the rule. In the lock only via quinn-proto's wasm-only edge. |
| `aws-lc-rs` 1.18.2 | Dominant default; `aws-lc-sys` had 5 advisories in March 2026 | yes | Banned. |
| `rustls-pure` 0.15.3, `rustls-scytale` 0.10.0-dev | New (listed in PROVIDERS.md on 2026-10-22), 107 and 9 downloads | no | Watch only. |
| RustCrypto "1.0" | Not 1.0. sha2 0.11.1, aes-gcm 0.11.1, hkdf 0.13, p256 0.14, x509-cert 0.3.0, pem-rfc7468 1.0.0 are stable releases; **rsa 0.10.0-rc.19** still rc after 2 years; crypto-bigint 0.7.10 | | See §8: bump our direct crypto crates to this generation; it removes the duplicate chacha20/cpufeatures/rand_core. |

## 2. Async / runtime / system

| Crate | Version (date) | Licence | 90d dl | Bus / last push | Pure, deps, MSRV | Verdict |
|---|---|---|---|---|---|---|
| tokio | 1.54.1 (2026-10-21) | MIT | 273M | top author 20%; pushed 2026-10-25 | pure; 1.85 | **Keep, and never sit on 1.54.0**: RUSTSEC-2026-0384 (`Interval` race) fixed in 1.54.1; RUSTSEC-2026-0386 (`AsyncFd::new` I/O safety) fixed in 1.53.3 / 1.54. Our tap.rs uses `AsyncFd::register`; we are patched. |
| tokio-util | 0.7.20 (2026-10-09) | MIT | 207M | | pure | Later: `CancellationToken`, `TaskTracker` for the Agents module and graceful shutdown. |
| async-trait / native async fn in traits | 0.1.92 / language | MIT/Apache | 182M | | | Native `async fn` in traits is stable since 1.75; **dyn-compatible async traits are still not stable** [U, not re-checked in 1.99 notes]. Use native where static dispatch works; keep our `Runner` trait fakes sync + `spawn_blocking`. Skip async-trait unless `dyn`. |
| tokio-console / console-subscriber | 0.1.14 / 0.5.0 (2025-10-30) | MIT | 10M | no release in 12 months | needs `tokio_unstable` | Skip until a stall bug needs it. |
| futures | 0.3.34 | MIT/Apache | 200M | | pure | Keep `futures-util` dev only. |
| rustix | 1.1.5 (2026-09-16) | Apache-2.0 WITH LLVM-exception OR Apache OR MIT | 316M | 85% one author (Dan Gohman successor team, Bytecode Alliance); pushed 2026-10-15 | pure, **4 crates** (linux-raw-sys backend); 1.65 | **Adopt now (replaces libc `unsafe`).** Provides `getuid/geteuid`, `kill_process`, `pidfd_open`, `pidfd_send_signal`, `set_parent_process_death_signal` (our pre_exec prctl), `statvfs`. Leaves zero `unsafe` in our code for those five sites (stream.rs:234, clipboard.rs:146/257, running.rs:144/241, files.rs:70). |
| nix | 0.31.3 (2026-05-11) | MIT | 217M | 25% | pure; 6 crates | Skip: rustix covers it with fewer deps and safe fds. |
| libc | 0.2.191 | MIT/Apache | 434M | | | Keep only for tap.rs `clock_gettime`/`recvmmsg`-style calls; the rest goes to rustix. |
| procfs | 0.18.0 (**2025-08-30**) | MIT/Apache | 20M | 65%; pushed 2026-06-26 | pure; 18 | Skip. 14 months since release; our hand-parsing of a few /proc files is smaller than its tree. |
| sysinfo | 0.39.6 (2026-07-09) | MIT | 56M | **87% one author**; MSRV **1.95** | pure; 4 crates | **Later (host metrics module).** CPU/mem/disks/temps/components in one API, pure on Linux. GPU is not covered (read i915/xe sysfs by hand). The 1.95 MSRV is fine with our floating `stable`. |
| zbus | 6.1.0 (2026-10-23) | MIT | 35M | now at `z-galaxy/zbus`; zeenix 4636 commits, elmarco 688; pushed 2026-10-25 | pure; 36 crates with `--features tokio`; 1.89 | **Later, with the module** (logind session/lock, systemd user manager signals, desktop notifications, MPRIS for the media player). Not worth it just to replace `systemctl --user restart` (host.rs:272); one spawn per rare action is fine. 5.x (5.19) and 6.x both in the registry: pin 6. |
| sd-notify | 0.5.0 (2026-03-09) | MIT/Apache | 3.7M | one maintainer (lnicola, 57 commits) | pure; **3 crates** (just libc); 1.82 | **Adopt now.** `notify(&[Ready])`, `watchdog_enabled()`, `listen_fds()` (socket activation) in one file. Replaces the need for `libsystemd`/`systemd` crates. |
| libsystemd 0.7.2 / systemd 0.10.1 | 2025 | MIT / **LGPL-2.1** | 1M / 0.5M | | `systemd` links libsystemd (`libsystemd-sys`) | Skip. |
| listenfd | 1.0.2 (2025-01) | Apache-2.0 | 1.4M | | pure | Skip: sd-notify's `listen_fds` covers it. |
| notify | 8.2.1 (2026-10-25) | **CC0-1.0** | 49M | top author 20%; pushed today | pure; 11 crates (`inotify-sys` is a libc binding) | **Later** (Steam manifests, sessions dir, hot reload). CC0 is fine for a personal repo. |
| inotify | 0.11.5 (2026-08-14) | ISC | 50M | | pure; 20 | Alternative to notify if you want only inotify. |
| signal-hook | 0.4.5 (2026-10-04) | MIT/Apache | 66M | 77% one author | pure; 5 | Skip: `tokio::signal` is already used and enough. |
| pidfd (crate) | 0.2.4 (2019) | | | | | Skip; use rustix. |
| evdev 0.13.2 / input-linux 0.7.1 / uinput 0.1.3 | 2025-09 / 2024-08 / 2018 | | 0.8M / 39k / 23k | | pure | Not needed: Sunshine owns uinput. If the bridge ever creates virtual pads, `evdev` (has `uinput::VirtualDevice`) is the maintained one. Skip `uinput` (2018). |
| wayland-client 0.31.15 / wayland-backend 0.3.17 | 2026-07 / 2026-08 | MIT | 24M / 22M | smithay, 58% | `cc` present unconditionally in build-deps; see §0 | See §0. **Unchanged since you last looked.** |
| wl-clipboard-rs | 0.9.4 (2026-09-28) | MIT/Apache | 10.5M | 86% one author | 44 crates; `cc` in graph, C not compiled | Later; needs the script exception in §0. |
| smithay-clipboard | 0.7.3 (2025-11) | MIT | 7M | | needs a display-owning window (winit use case) | Skip: not for a headless daemon. |
| portable-pty | 0.9.0 (**2025-02-11**) | MIT | 14M | lives in the wezterm monorepo, 89% one author | pure; 21 crates | Fallback only. |
| pty-process | 0.5.4 (2026-10-19) | MIT | 1.5M | **one author (98%)**, 33 stars | pure; **5 crates**, tokio feature | **Later (web terminal).** Smallest async PTY spawn. Alternative: `rustix` with `pty` feature (`openpt/grantpt/unlockpt`) and spawn via `std::process::Command` + `setsid` yourself: zero extra crates [U]. |
| alacritty_terminal | 0.26.0 (2026-04-06) | Apache-2.0 | 1.3M | alacritty org | pure; 40 crates | Later, **only if** you want server-side screen state (reconnect shows the current screen). Otherwise stream raw PTY bytes to xterm.js and skip it. |
| vte | 0.15.0 (2025-02) | Apache/MIT | 23M | | pure; 4 crates | Only for parsing escapes server-side. |
| tmux-like sessions | | | | | | Use real `tmux`/`screen` as the PTY child: persistence for free; the web terminal attaches to `tmux attach`. No crate. |

## 3. Data / storage / search

| Crate | Version (date) | Licence | 90d dl | Bus / last push | Pure? deps | Verdict |
|---|---|---|---|---|---|---|
| serde / serde_json | 1.0.229 / 1.0.154 | MIT/Apache | 381M | dtolnay | pure | Keep. |
| toml | 1.1.8 (2026-10-09) | MIT/Apache | 276M | | pure; 8 | **Now stable 1.x with TOML 1.1.** Use for config; `toml_edit` 0.25.17 only if you write config back preserving comments. |
| postcard | 1.1.3 (2025-07) | MIT/Apache | 33M | 50% | pure; 25 w/ std | Later: compact binary for series files if JSONL gets big. |
| rmp-serde | 1.3.1 (2025-12) | MIT | 29M | | pure | Skip (no reason over postcard/JSON). |
| **bincode** | 3.0.0 (2025-12-16) | MIT | 73M | **RUSTSEC-2025-0141: unmaintained, team ceased permanently after a doxxing/harassment incident**; 1.3.3 is declared "complete" | | Not in our tree. If you need it: `wincode` (anza, Apache-2.0, 0.6.3, 2026-10-13, 11 crates, MSRV 1.89), `postcard`, `bitcode` 0.6.9 or `rkyv` 0.8.18 (5 advisories in 2026, fixed at 0.8.17: careful). |
| ciborium | 0.2.2 (2024-01) | Apache-2.0 | 73M | enarx, quiet | pure | Skip. |
| rusqlite | 0.41.0 (2026-10-24) | MIT | 49M | | **C** (`libsqlite3-sys`; bundled adds cc) | Reject by rule. |
| **turso** (Rust SQLite rewrite, ex-Limbo) | 0.8.3 stable (2026-10-12), 0.9.0-pre.1 (2026-10-15) | **MIT** | 0.7M | tursodatabase, 25k stars, 1550 open issues; pushed today | **Not pure by our rule**: 339 crates, `bindgen`, `cc`, `libmimalloc-sys`, `zstd-sys`, `clang-sys` at default features (258 crates with no default features, still bindgen+cc) | **Skip.** README: "not yet 1.0", "keep independent backups". `limbo` crate is a dead 0.0.22 (2025-06) name. libSQL 0.10 (217 crates, ring, cmake) is the older C fork; also skip. |
| redb | 4.4.0 (2026-10-13) | MIT/Apache | 6.2M | 90% one author (cberner), 2 open issues; pushed today | pure; **3 crates**; MSRV 1.90 | **Later** (key-value for Agents/memory metadata). Best fit for the taste: copy-on-write, ACID, no deps. |
| fjall | 3.1.13 (2026-10-25) | MIT/Apache | 0.7M | 95% one author | pure; 48 crates; MSRV 1.90 | Only if write-heavy LSM (series/log ingestion) matters. redb first. |
| sled | 0.34.7 (2021); 1.0 alpha stuck at 2024-10 | MIT/Apache | 3.8M | dormant | | Skip. |
| heed (LMDB) | 0.23.0 (2026-10-22) | MIT | 2.8M | meilisearch | **C** (`lmdb-master-sys`) | Reject. |
| tantivy | 0.27.0 (2026-10-17) | MIT | 5M | quickwit-oss, 493 open issues; pushed 2026-10-23 | **pure only with `--no-default-features --features mmap,lz4-compression`** (129 crates). Defaults pull `zstd-sys` (C). | **Later (Agents memory/search).** Verified: no cc with those features. Compile cost: about 130 crates; expect 30-60 s extra cold build [U, not timed]. Alternative for under ~50k small notes: plain scan or a hand-rolled inverted index; tantivy earns its keep above that. |
| sqlite-vec | | | | | C extension | Irrelevant (C). |
| usearch | 2.26.6 (2026-10-25) | Apache-2.0 | 0.7M | | **C++ via cxx** | Reject. |
| hnsw_rs | 0.3.5 (2026-10-09) | MIT/Apache | 0.5M | 87% one author; 255 stars | pure; 84 crates | Later if you build vector recall; for under ~100k vectors brute-force cosine over a `Vec<f32>` is simpler and exact. |
| instant-distance | 0.6.1 (2023-06) | MIT/Apache | 0.4M | | pure | Dormant; skip. |
| candle-core | 0.11.0 (2026-06-26) | MIT/Apache | 4M | HF, 63% one author | **fails**: unconditional `tokenizers` with `onig` feature (C regex, `onig_sys`) | Reject for the rule. (`tokenizers` itself is pure with `fancy-regex`; candle does not let you pick.) |
| burn | 0.22.1 (2026-10-20) | MIT/Apache | 0.8M | tracel-ai, 23% top | pure with `ndarray`+`std`: 173 crates; MSRV 1.95 | The only pure ML runtime that passed. Heavy; run a small embedding model on CPU [U on speed]. Alternative: call a local Ollama/llama.cpp server over HTTP (process boundary keeps C out of the build graph). |
| tract-onnx | 0.23.9 (2026-10-15) | MIT/Apache | 1.4M | | `cc` in graph (asm kernels) | Fails check. |
| ort 2.0.0-rc.15 / fastembed 7.5.0 | 2026-10 | MIT/Apache; Apache-2.0 | 9M / 2.4M | | **ONNX Runtime C++, openssl-sys, ring, cc**: 334 crates | Reject. fastembed uses ort, confirmed. |

## 4. Observability / errors / CLI / config / small utilities

| Crate | Version (date) | Licence | 90d dl | Bus | Pure, deps | Verdict |
|---|---|---|---|---|---|---|
| tracing | 0.1.44 (2025-12) | MIT | 237M | tokio-rs, 59% | pure | We use it in bridge/client with `std` only. Add in hostd (it uses `eprintln!` today). |
| tracing-subscriber | 0.3.23 (2026-03-13) | MIT | 171M | | pure; 13 | **Adopt with hostd systemd work.** RUSTSEC-2025-0055 (ANSI escape injection in logs) fixed in 0.3.20; use >=0.3.23. |
| tracing-journald | 0.3.2 (2025-11-26) | MIT | 2.2M | | pure; 11 | Adopt with systemd: structured fields into journald (`journalctl -o json`). stderr alone also lands in the journal, so only if you want fields/priority. |
| log / env_logger | 0.4.34 / 0.11.11 | MIT/Apache | 344M / 90M | | pure | Skip (tracing). |
| thiserror | 2.0.21 | MIT/Apache | 457M | dtolnay, 96% | pure | Adopt when error enums grow (hostd uses `BoxResult` and `String` errors). |
| anyhow | 1.0.104 | MIT/Apache | 257M | | pure | **Needs >=1.0.103**: RUSTSEC-2026-0190 unsound `downcast_mut`. Apps only (hostd/bridge main). |
| eyre / color-eyre / miette / snafu | 0.6.14 / 0.6.5 (2025-05) / 7.6.0 (2025-04) / 0.9.2 | | | | | Skip. |
| clap | 4.6.7 (2026-09-14) | MIT/Apache | 265M | 49% | pure; 20 with derive | Overkill for 2 binaries; skip. |
| **lexopt** | 0.3.2 (2026-02-28) | MIT | 3M | one author, but tiny and finished | pure; **2 crates** | **Replace hand-parsing** in `parse_from` (bridge main.rs:54) if you want a `--help` and `--flag=value` for free; it is a tokenizer, you still write the match. Honest verdict: our 40-line parser is tested; do it only when a third binary arrives. argh 0.1.19 (BSD-3, derive) and bpaf 0.9.28 also fine; `pico-args` last release 2022. |
| figment 0.10.19 (2024-05) / config 0.15.27 | | MIT/Apache | | | | Skip: `toml` + serde + env overrides by hand. |
| directories 6.0.0 / xdg 3.0.0 / etcetera 0.11.0 | | | | | pure | Skip: Linux-only; `$XDG_*` env + fallback is 5 lines (we already do `/run/user/{uid}`). |
| **jiff** | 0.2.38 (2026-10-06) | Unlicense/MIT | 94M | BurntSushi, 87% | pure; **3 crates** | **The 2026 default for new date/time code**: IANA zones, `Timestamp`, civil time, human durations, serde. `time` 0.3.55 (RUSTSEC-2026-0009 fixed >=0.3.47) and `chrono` 0.4.45 remain fine. Adopt jiff in session history timestamps if you hand-format dates today. |
| uuid 1.29.0 / ulid 3.0.0 | | | | | pure | Skip: a random u64 hex from `getrandom` is enough; `uuid` v7 if you want sortable ids (needs MSRV 1.89). |
| rand 0.10.3 (2026-09-20) / getrandom 0.4.3 | | MIT/Apache | 550M / 701M | | pure | **We pin rand_core 0.6.4 (getrandom 0.2) while quinn-proto pulls rand 0.10 / getrandom 0.4**: two RNG stacks. RUSTSEC-2026-0097 (unsound) fixed in 0.10.1. Bump direct usage to `getrandom 0.4` directly where possible. |
| bytes 1.12.1 / smallvec 1.16.4 / arc-swap 1.9.3 | | | | | pure | bytes: RUSTSEC-2026-0007 fixed >=1.11.1 (ok). arc-swap: Later for config hot-reload. smallvec: skip. |
| dashmap 6.2.1 / parking_lot 0.13.0 | | MIT | | | | Skip both: `std::sync::Mutex` is fast since 1.62 and a `Mutex<HashMap>` suffices. parking_lot 0.13 now MSRV 1.95. |

## 5. Testing / quality / supply chain

| Tool | Version (date) | Licence | Notes | Verdict |
|---|---|---|---|---|
| cargo-nextest | 0.9.149 (2026-10-20) | Apache/MIT | 48% one author but a funded org project; MSRV 1.91 for building it (use the prebuilt binary) | **Adopt** (per-test processes, retries for the flaky live tests, JUnit). |
| cargo-llvm-cov | 0.9.1 (2026-09-06) | Apache/MIT | taiki-e, 80% | Adopt on demand to find untested parser branches. |
| proptest | 1.12.0 (2026-10-10) | MIT/Apache | 37%; 29 crates | **Adopt (dev-dep)** for FEC encode/decode roundtrips, RTP reorder, rtsp roundtrip. quickcheck 1.1.0 is fine but proptest has shrinking by strategy. |
| insta | 1.49.0 (2026-10-03) | Apache-2.0 | mitsuhiko | Later (golden captures from real Sunshine are already your fixtures; insta helps for JSON API responses). |
| loom | 0.7.2 (2024-04) | MIT | tokio-rs, quiet | Skip (little hand-written concurrency; tokio's `test-util` paused clock covers the watchdog). |
| mockall | 0.15.0 (2026-06-28) | MIT/Apache | 95% one author; 18 crates | **Skip.** Hand-rolled trait fakes (`Runner`) are smaller and clearer for 3-4 traits. |
| wiremock 0.6.5 (2025-08, 108 crates) / httpmock 0.9.0 (2026-10-20, 76 crates) | | MIT | | **Replace the python mock server only if** you move Steam fetch to Rust (ureq) and want in-process tests. httpmock is current. Cheaper alternative: a 30-line tokio `TcpListener` fake in tests (no TLS needed with `steam_api_base` http). |
| **cargo-deny** | 0.20.2 (2026-07-09) | MIT/Apache | Embark, 77% | **Adopt.** `[bans] deny = [ring, aws-lc-sys, openssl-sys, cc]` + `skip-tree` + `[advisories]` + `[licenses]` + `[sources] unknown-registry = "deny"`. It can replace most of `check-pure-rust.sh` in one file, with graph-wide wrapper allow-lists (`wrappers = ["wayland-backend"]` for `cc`). |
| cargo-audit | 0.22.2 (2026-06-05) | Apache/MIT | rustsec org | Installed into scratch, worked. Use cargo-deny's advisory check instead if you adopt deny, or schedule `cargo audit` in a systemd user timer (the host is always on). |
| cargo-vet | 0.10.3 (2026-10-22) | Apache/MIT | Mozilla | Skip for one person; import Google/Mozilla/Bytecode Alliance audits only if you want a third source. Heavy process for a hobby repo. |
| cargo-auditable | 0.7.7 (2026-10-02) | MIT/Apache | | Later: embeds the dep list in the shipped binary so you can audit the single deployed binary. |
| cargo-machete 0.9.2 / cargo-udeps 0.1.61 | | MIT | | machete (fast, regex): run it once; udeps needs nightly. |
| cargo-semver-checks 0.51.0 | | | | Skip: no published crates. |
| cargo-dist 0.32.0 / cross 0.2.5 (2023!) / cargo-zigbuild 0.24.0 | | | | Skip all: one host, one target (x86_64-linux-gnu). |
| cargo-watch 8.5.3 (2024-10, archived-ish) / bacon 3.26.0 | | CC0 / **AGPL-3.0** | | cargo-watch is dead-ish; bacon is AGPL (fine to run locally, avoid linking). Use `cargo nextest` + `watchexec`. |
| taplo-cli 0.10.0 | 2025-05 | MIT | | Optional. `cargo fmt` for TOML is not a thing yet [U]. |
| **cargo-fuzz** | 0.13.2 (2026-06-09) | MIT/Apache | rust-fuzz | **Adopt (needs nightly for sanitizers; the C++ libFuzzer is inside `libfuzzer-sys`, but it lives in a separate `fuzz/` crate outside the workspace, so the purity rule is untouched).** |
| libfuzzer-sys / afl / arbitrary / bolero | 0.4.13 / 0.18.2 / 1.5.0 / 0.13.7 | | | arbitrary for structured inputs; bolero gives one harness that runs as a plain test, proptest, or fuzzer [U on ergonomics]. |
| Miri | in rustup nightly | | | Useful for brisk-proto if it ever gets `unsafe`; for our blocks (pre_exec/prctl/geteuid/statvfs/FFI libc) Miri cannot run syscalls. Moving them to rustix removes the need. Note the 2026-09-21 Rust blog post: **Miri stores all env vars in `target/`, so cached `target/` in GitHub Actions can leak secrets to PRs.** Irrelevant to a local repo; relevant if you ever add CI cache. |
| ASAN | nightly `-Zsanitizer=address` | | | Only for fuzz runs. |

## 6. Language / toolchain

- **Stable today: 1.99.1 (2026-10-15)**; 1.99.0 was 2026-10-01. Cadence: 1.98 (Aug 20), 1.97 (Jul 9), 1.96 (May 28), 1.95 (Apr 16). Our `rust-toolchain.toml` is `channel = "stable"` (floating): it works, but builds are not reproducible and a point release can change behaviour (1.99.1 fixed an x86_64 miscompile of FFI `bool` returns, rust-lang/rust#163911). **Suggest `channel = "1.99.1"`, `profile = "minimal"`, `components = ["clippy","rustfmt"]`** and bump deliberately each ~6 weeks after `cargo update` + tests. Your "exact-pinned" taste argues for this. Nightly only for cargo-fuzz.
- **Edition**: workspace is `edition = "2021"`. 2024 has been stable since 1.85 (Feb 2025); no 2027 edition yet [U]. Moving gives let-chains (stable in 2024 since 1.88), `if let` guards on match arms (1.95), RPIT lifetime capture rules, `unsafe extern`, `gen` is reserved but `gen` blocks are **still unstable** [U]. New in 1.94 era Cargo: edition-2024 members may override `default-features` of workspace deps (RFC 3945). Run `cargo fix --edition` per crate; expect `unsafe_op_in_unsafe_fn` and `tail_expr_drop_order` churn. Cost: small (13k lines).
- **Async closures** stable since 1.85. Native `async fn` in traits since 1.75.
- **Linker**: since 1.90, `rust-lld` is the default linker on `x86_64-unknown-linux-gnu`: **no need for mold or `-fuse-ld`**; drop any `RUSTFLAGS` for it. Parallel front end (`-Zthreads`) is still nightly [U]. Cranelift is a nightly rustup component; skip. `cargo build --timings` works: my debug build of the whole workspace on 4 jobs took **17 s wall (62 CPU-seconds, 233 units)**; slowest: `num-bigint-dig` 3.7 s (the `opt-level=3` override), tokio 2.8, rustls 2.8, quinn-proto 2.1, **both syn 3.0.9 and syn 2.0.119 (about 2 s each, a duplicate)**, rsa 1.9. The compile cost is not a problem.
- **Cargo news that matters**: `build.warnings` config (1.97) replaces `-Dwarnings`; `resolver.lockfile-path`; incremental off by default in CI; pubtime in the registry index (stable 1.94), the groundwork for time-based dependency resolution, i.e. a future "don't take versions younger than N days" (the arrayref defence, §9) [U on when the resolver lands]; new `debug` profile preparing dev-profile changes.
- **Workspace lints** (add to root Cargo.toml and `[lints] workspace = true` in each crate): `unsafe_op_in_unsafe_fn = "deny"`, `unused_must_use = "deny"`, `missing_debug_implementations = "warn"` for brisk-proto; clippy: `clippy::dbg_macro`, `todo`, `unimplemented`, `undocumented_unsafe_blocks`, `unwrap_used` (warn, allow in tests), `cast_possible_truncation` + `indexing_slicing` for parser crates (brisk-proto) only. Skip full pedantic: too noisy.
- **RUSTFLAGS**: leave empty. `target-cpu=native` would be fine on bawn (single host), but graviola's runtime detection already picks AVX2/SHA-NI.

## 7. What changed since mid-2026 that we may not know

- **graviola 0.4.0/0.4.1 (June 2026)** and its entry as first provider in rustls' `PROVIDERS.md` (§1a). rustls PROVIDERS.md also gained `rustls-pure` and `rustls-scytale` on 2026-10-22.
- **rustls 0.24 is `0.24.0-dev.2`** (main, 2026-10): explicit provider required; `rustls-ring` is already a separate crate at 0.24.0-dev.2; `rustls-webpki 0.104.0-alpha.8` in flight.
- **Rust**: 1.97 (Jul 9), 1.98 (Aug 20), 1.99 (Oct 1), 1.99.1 (Oct 15, FFI-bool miscompile fix). Rustup 1.29.2 (Oct 20) updated rustls-webpki for CVE-2026-111943 / RUSTSEC-2026-0382 (**eMudhra distrusted intermediates accepted**). Update rustup (`rustup self update`).
- **rustls security wave**: five advisories 2026-09-14 and 2026-10-14 (RUSTSEC-2026-0285, 0346-0349: handshake across encryption levels, `Debug` leaks of session secrets, ECH rejection data, Brotli cert decompression 1 GiB, downgrade of PQ kex). We are on 0.23.46 which fixes all.
- **quinn-proto**: eight advisories Mar-Sep 2026, latest fixed in 0.11.18 (2026-09-14). We are on 0.11.19.
- **tokio 1.54.0 had a bad `Interval` race (advisory only, not confirmed yanked), fixed 1.54.1 on 2026-10-21**; 1.53.3 fixed `AsyncFd` I/O safety on 2026-10-13.
- **rand 0.10**, getrandom 0.4, sha2 0.11, aes-gcm 0.11, hkdf 0.13, p256 0.14, x509-cert 0.3, pem-rfc7468 1.0: the RustCrypto "next generation" is out (stable) except `rsa` (0.10.0-rc.19).
- **toml 1.x** (TOML 1.1), **zbus 6**, **hickory 0.26**, **reqwest 0.13**, **ureq 3**, **tower-http 0.7**, **rusqlite 0.41**, **tantivy 0.27**, **redb 4**, **fjall 3**, **leptos 0.9-rc** (0.9.0-rc0 on 2026-10-15), **dioxus 0.8-alpha**, **tokio-tungstenite 0.30**, **notify 8.2**, **bollard 0.21**.
- **Maintainers in Residence**: Rust Foundation Maintainers Fund now pays full-time for the Cargo team (Scott Schafer, 2026-09-22): faster Cargo features likely.
- **Bincode ended** (Dec 2025) and `wincode` (Anza) is the replacement people moved to.
- **Rust blog 2026-09-17: targeted attacks on prominent Rustaceans** (fake video-call jobs).
- **Turso** is the intended SQLite direction of the company; **libSQL** is in maintenance feel (0.10.0 still released 2026-10-13).
- Repo moves: `rust-embed` left GitHub; `zbus` moved to `z-galaxy/zbus`; `web-transport` moved to `moq-dev`; `rustls-mbedtls` crate name does not exist.

## 8. Risks in our current dependency set (cargo-audit 0.22.2 on a scratch copy of the lock, 218 crates, DB of 2026-10-24)

`cargo audit` exit: 7 vulnerabilities + 1 warning (`paste`). Reachability checked by reading `rustls-rustcrypto-0.0.2-alpha/src`.

| Finding | Where it comes from | Reachable? | Action |
|---|---|---|---|
| RUSTSEC-2026-0049, 0098, 0099, 0104, 0382 on `rustls-webpki 0.102.8` | `rustls-rustcrypto` pins the old 0.102 line | **No.** It imports only `webpki::alg_id` constants (verify/ecdsa.rs, eddsa.rs, rsa.rs). Certificate and CRL logic runs in `rustls` -> `rustls-webpki 0.103.16` (patched), and `brisk-client` replaces verification with its own `PinnedCert`. | Remove the root cause (replace rustls-rustcrypto, §1a). Meanwhile add `ignore` entries with a one-line reason so CI stays green. |
| RUSTSEC-2023-0071 `rsa 0.9.10` (Marvin timing, no fix; 0.10 still rc) | brisk-proto, brisk-client (pairing signatures, client cert) and rustls-rustcrypto | Local-timing attack on a tailnet-only client key; low. | Spike: use graviola's RSA signing (constant-time asm) or `rsa 0.10` once stable. |
| RUSTSEC-2026-0387 `crypto-bigint 0.5.5` (timing, fixed >=0.7.6, published 2026-10-23) | via rsa/p256/ecdsa 0.1x chain | Same class. | Fixed by moving the RustCrypto generation forward; blocked by `rsa` 0.9 and `rustls-rustcrypto`. |
| `paste 1.0.15` unmaintained (RUSTSEC-2024-0436) | `rustls-rustcrypto` | proc-macro, build-time only. | Disappears with rustls-rustcrypto. |

Other facts from `cargo tree`:
- **Duplicates** (`cargo tree -d`): chacha20 0.9/0.10, cpufeatures 0.2/0.3, getrandom 0.2/0.4, rand 0.8/0.10 (0.8.8 only via num-bigint-dig), rand_core 0.6/0.10, rustls-webpki 0.102/0.103, **syn 2/3**. All caused by the RustCrypto 0.x generation + rustls-rustcrypto + quinn-proto's rand 0.10. Replacing rustls-rustcrypto and bumping `aes 0.9`, `aes-gcm 0.11`, `hkdf 0.13`, `hmac 0.13`, `sha2 0.11`, `x509-cert 0.3`, `rand_core 0.10` would remove the old line: 9 direct deps are behind (`cargo update --dry-run --verbose`). `matchit 0.8.4` -> 0.9.2 is axum's choice, ignore.
- **Heavy**: 240 crates in the normal+build graph. **35 are the ICU/url/idna tree from `web-transport-proto` alone.** num-bigint-dig is the slowest compile (RSA keygen needs it at opt-level 3, per the root Cargo.toml).
- **No `cc`, `ring`, `aws-lc-sys`, `openssl-sys`** on the host target (and the script passes on six targets per its own comment). No other `-sys` crates in the host graph (only `libc`).
- **Version ranges, not exact pins**: most `Cargo.toml` lines are `"1"`, `"0.8"` (only `rustls-rustcrypto` and `rusty_enet` use `=`). Reproducibility rests on `Cargo.lock`; keep building with `--locked`. If "exact-pinned" is a hard rule, `cargo add --exact` them or document that the lock is the pin.
- **Hostd has one dev-only mock** (python) and uses `eprintln!` instead of tracing; `unsafe` sites: tap.rs (clock_gettime, AsyncFd register, recv into uninit slice), stream.rs (pre_exec prctl), clipboard.rs, running.rs (getuid, geteuid), files.rs (statvfs). After rustix, only tap.rs's uninit-buffer and register remain.
- **`rusty_enet 0.4.0`** (dev-dep, ENet 1.3, MIT, last release 2025-04, 12k downloads): tiny bus factor but it is test-only and exact-pinned. Fine.
- No `cargo deny`/`cargo audit` configured in the repo today. No RustSec `unmaintained` crate in the lock other than `paste`.

## 9. Security: supply chain, 2025-2026

- **2025-09-24** `faster_log` and `async_println` (typosquats of `fast_log`) scanned source files for Solana/Ethereum keys; runtime payload, about 8.4k downloads, deleted (blog.rust-lang.org/2025/09/24).
- **2026-06** a campaign against prominent Rust developers (fake job/video-call lures) and **2026-08-20 `arrayref` 0.3.10, `internment` 0.8.7, `append-only-vec` 0.1.9** were published from compromised maintainer accounts with a dependency on typosquatted `proc-macro1`, whose `build.rs` downloaded a payload. Live 86-107 minutes; arrayref 0.3.10 downloaded 2,285 times (RUSTSEC-2026-0260, 0262, 0266). The Rust project's 2026-09-17 post says the campaign is ongoing. I checked bawn: none of those crates are in `~/.cargo/registry/cache`, and `arrayref` is not in the Brisk lock.
- **2026-09-21** Miri puts the full environment in `target/`; cached `target/` in GitHub Actions can leak secrets to later PRs.
- **Mitigations that fit one person**: (1) commit `Cargo.lock` and build with `--locked` (done; a 107-minute window only hits you if you `cargo update` in it); (2) `cargo update` only after a short cooling-off, reading `cargo update --dry-run` first (the registry now carries `pubtime`, so you can check how fresh a version is) [U on a cargo flag that enforces it]; (3) `cargo deny` with `[sources]` locked to crates.io and `[bans]` wrapper lists; (4) prefer crates with few build scripts: check `cargo tree -e build`, since both 2026 attacks ran in `build.rs`; our own graph has build scripts from the usual proc-macro/`libc`/`rustix`/`wayland-sys` set; (5) build in the capped systemd scope (you already do) and ideally without network (`cargo build --offline` after `cargo fetch`); (6) crates.io **trusted publishing** (GitHub OIDC) exists for publishers; as a consumer it makes a compromised long-lived token less likely, and you can prefer crates that use it [U which of our deps do]; (7) cargo-vet only if you want imported audits, not needed here.

## 10. Discovery beyond the list

- **(a) Home-lab / agent dashboards in Rust**: `bollard` 0.21.1 (Docker API, Apache-2.0, 19M/90d, **pure, 103 crates**; use for Dockge/Docker status via the unix socket, no TLS needed) is the one to use; Proxmox has no maintained client crate [U], call its REST API with ureq + API token. `rumqttc` 0.25.1 (MQTT for smart home) pulls aws-lc/ring by default (60 crates): use `--no-default-features` with plain TCP over the tailnet, or talk to Home Assistant's REST/WebSocket. Wake-on-LAN: `wake-on-lan` crate is 2018; a magic packet is 6x`0xFF` + 16x MAC over a UDP broadcast socket with `socket2` (already a dep), about 12 lines. `notify-rust` 4.18 (88 crates, zbus): later, or just `notify-send`. Metrics: `sysinfo` + sysfs. `iroh` 1.4.1 (n0, p2p QUIC; 320 crates, ring) is interesting but redundant with Tailscale; skip. `hls_m3u8` 0.7.0 parses/builds playlists if you do HLS by hand; ffmpeg itself stays a supervised child process (`ffmpeg-next` links C; `gstreamer` links C).
- **(b) Low-latency media/network**: `reed-solomon-simd` 3.1.0 (pure, 6 crates, MIT+BSD-3, SIMD GF(2^16), 0.5M/90d) is the modern FEC library; Sunshine/Moonlight use GF(2^8) Reed-Solomon with their own parameters (nanors), so it cannot be used directly for interop, but is relevant for Brisk's own bridge->browser FEC, if you ever add one over datagrams [U on matching the Moonlight shard layout]. `raptorq` 2.0.1 (pure RFC 6330) for fountain codes. `io-uring` 0.7.15 (raw bindings, pure) and `tokio-uring` (dormant since 2024-05): the UDP rates here (a few thousand pps) do not need them. `socket2` 0.6 is what we use for `SO_RCVBUF`/DSCP. `rtp` 0.17.2 (webrtc-rs) and `rtsp-types` 0.1.3 (2024) exist; our brisk-proto owns its own sans-io RTP/RTSP and that is right given the Moonlight dialect. HEVC/AV1/4K: the bridge forwards encoded frames, so decode is WebCodecs in the browser; Rust-side we only need NAL/OBU boundary parsing (hand-write; `rav1e` is an encoder, `openh264` wraps C). `mimalloc` is C; skip, the glibc allocator is fine.
- **(c) Embedded web UIs**: Leptos 0.8.22 (0.9.0-rc0 out), Dioxus 0.7.10 (0.8 alpha). One-paragraph verdict: skip both. They would put a WASM build (target `wasm32-unknown-unknown`, which brings `ring` back into any rustls/quinn tree through quinn-proto's wasm-only edge, the exact case your purity script carves out) and a second toolchain into a repo whose UI is Astro/Svelte with WebCodecs/WebTransport/Gamepad glue that is JS-API-heavy anyway; Rust-in-the-browser buys nothing for a latency path that is the browser's own decoder.
- **(d) What makes pure Rust easier/harder in 2026**: easier: graviola (first credible fast pure provider), rustls 0.24's explicit-provider design, `ring` in security-only mode (so the ecosystem is shifting off it), RustCrypto's next generation going stable, `ureq` 3 being small and provider-agnostic. Harder: **QUIC still blocks pure providers** (quinn glue), `aws-lc-rs` is the default for most crates (`reqwest`, `rumqttc`, `str0m`, `web-transport-quinn` all pull it by default, so you always need `default-features = false`), `wayland-backend` keeps `cc` in the graph, ML ecosystem is C/C++ (`ort`, candle's onig).
- **(e) Home dashboards/agent orchestration**: `tokio-util` `CancellationToken`/`TaskTracker`, `redb` for state, `tantivy` for recall, `pty-process` for each Claude Code agent's terminal, `sd-notify` for the service, `bollard` for Docker, `sysinfo` for metrics. That is about 8 added crates for the whole parked roadmap.

## 11. Recommended actions (ordered)

1. Add `cargo-deny` config with the bans above (and a `wrappers` exception for `cc` via `wayland-backend` when you want clipboard); keep `check-pure-rust.sh` until deny is proven.
2. Spike graviola in a branch: (a) `brisk-hostd` Steam fetch with ureq (verified here); (b) `brisk-client` TLS to Sunshine + RSA pairing against the real host; (c) see whether the bridge handshake works with `rustls-graviola` suites under the existing `quic_crypto.rs`. If (b) works, delete `rustls-rustcrypto`, `rsa`, `num-bigint-dig`, `paste` from the lock and the 6+2 advisories with them.
3. Swap `static_file` for `ServeDir` + a 10-line SPA fallback; later `rust-embed` for the single binary.
4. `sd-notify` (Ready + watchdog; also `listen_fds` if you want socket activation: with `tailscale serve` proxying to 127.0.0.1:8443 you do not need it).
5. `rustix` for pidfd: open pidfd, re-read `/proc/<pid>/cmdline` and starttime, only then `pidfd_send_signal`. Removes the `kill` child and 5 unsafe blocks.
6. `cargo-fuzz` targets for `brisk-proto`: rtsp, rtp, enet, control, fec (a parser fed network bytes), plus `proptest` roundtrips.
7. Pin `rust-toolchain.toml` to 1.99.1; move to edition 2024; add `[workspace.lints]`.
8. Replace `web-transport-proto` or get `url` made optional upstream: saves 35 crates.
9. Bump `tracing`+`tracing-subscriber` into hostd and `tracing-journald` once it runs under systemd.

## Definitely skip

`ort`, `fastembed`, `candle-core` (as of 0.11), `usearch`, `rusqlite`, `heed`, `libsql`, `turso` (for now), `reqwest`, `wtransport` / `web-transport-quinn` (until a pure QUIC path exists), `quiche`, `s2n-quic`, `webrtc`/`str0m`, `hickory-*`, `sled`, `bincode`, `bacon` (AGPL), `cross`, `cargo-watch`, `mockall`, `eyre`/`miette`/`snafu`, `clap` for two binaries, `dashmap`, `parking_lot`, `askama`/`maud`, Leptos/Dioxus, `libsystemd`/`systemd` (LGPL + C), `uinput` (2018), `include_dir`, `memory-serve`.

## Open questions

1. Does graviola cover every cipher suite Sunshine's HTTPS (OpenSSL) negotiates with our client (TLS 1.2 ECDHE-RSA, client-cert auth)? The spike answers it. [U]
2. Can `quic_crypto.rs` sit on `rustls-graviola` suites, or does it assume `rustls-rustcrypto` types? Read the bridge's use of `rustls_rustcrypto` before deciding. [U]
3. Is a shipped `asm` provider acceptable under the "no C" rule, or should the script list graviola as an allowed exception explicitly (my reading: yes, since no compiler is invoked)?
4. Is `rust-embed`'s new home (pyrossh.dev) acceptable for supply chain, or do you prefer a 40-line `build.rs` + `include_bytes!` generator (zero deps)?
5. Which agent memory size do you expect: if under 50k notes, tantivy may be more than needed (a plain scan of Markdown files with `grep`-style search is zero-dep).
6. Do you want `tracing-journald` fields, or is plain stderr into the journal enough?
7. rsa 0.10: still rc after two years; do you want to wait or move pairing signatures to graviola?
8. Is there a cargo flag in 1.99 that refuses versions younger than N days (pubtime-based)? I found the index field but not the resolver option. [U]
