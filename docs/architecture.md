# Architecture

One page on how the parts fit. Decisions and goals: [design.md](design.md). Hosting: [hosting-runbook.md](hosting-runbook.md).
Every env var: [env.md](env.md).

## Processes

```
 Chrome (laptop)
   │ https, tailnet only (tailscale serve :8443)       WebTransport https://<tailnet IPv4>:4433/<token>
   ▼                                                   │
 brisk-hostd  127.0.0.1:8443  ── spawns per stream ──▶ brisk-bridge ── RTSP/ENet/RTP ──▶ Sunshine (patched)
   page + /api, Steam, emulator library,                     ▲                              │
   machine hub proxy (/api/m/<id>/...)                       └── tap: unix SEQPACKET socket ┘
```

- **brisk-hostd** is the host agent. A systemd user service in normal use (`deploy/brisk-hostd.service`), or
  run by `scripts/brisk-up.sh` while developing. It binds 127.0.0.1:8443 only and serves the built web UI and
  the JSON API. With a `machines.json` it is a **hub**: the same binary, which forwards `/api/m/<id>/...` to
  each machine's own hostd and can power a Proxmox VM on and off.
- **brisk-bridge** is started by hostd for each stream (at most one at a time). It runs a Sunshine session
  through `brisk-client`, takes the encoded frames from the tap socket, and sends video and audio to the
  browser over WebTransport (QUIC). Input and rumble go the other way.
- **Sunshine**, with `patches/sunshine/0001-brisk-tap.patch`. A session that asks for the tap sends its
  encoded frames to the bridge's unix socket instead of RTP with FEC and AES.
- **brisk-probe** (in `brisk-client`) drives Sunshine by hand: pairing, a test stream, fixtures.
- **Web UI** (`web/`): Astro + Svelte 5, built to `web/dist` and served by hostd (or compiled into it with
  the `embed` feature). WebCodecs decodes the video.

## Crates

| Crate | Modules |
|---|---|
| `brisk-proto` | Sans-IO protocol, no sockets or clocks. `http` (GameStream XML), `pair`, `rtsp`, `sdp`, `enet`, `control` (control-stream messages), `rtp` (video reassembly), `fec` (Reed-Solomon), `audio`, `input`, `tap` (tap framing), `scrub` (blanks secrets in fixtures). |
| `brisk-client` | The IO around it. `identity` (client cert and pinned server cert), `session` (HTTP(S) to Sunshine), `rtsp` (handshake driver), `control` (ENet session), `stream` (A/V receive), `tap` (the socket listener), `recorder` (fixtures), `bin/brisk-probe.rs`. |
| `brisk-bridge` | `main.rs` (arguments, one session, the exit report), `wt` (WebTransport over HTTP/3 on quinn), `quic_crypto` (pure-Rust QUIC crypto), `session` (frame pacing, drops, latency counters), `input` (browser input to Sunshine, rumble back). |
| `brisk-hostd` | See below. |

`brisk-hostd` modules, by job:

- **Server and auth**: `main` (startup, the `add` and `wrap` subcommands), `http` (the router, every
  `/api` handler and the guard), `auth` (Host allowlist, tailnet login check), `config` (env vars and
  defaults), `site` and `embed` (static files), `notify` (systemd readiness and watchdog).
- **Streaming**: `stream` (the bridge supervisor: start, stop, the wt URL), `sessions` (stream history and
  the bridge's end report), `host` (host status, rate-limited Sunshine restart), `running` (running Steam
  games and their windows), `clipboard`, `files` (file drop).
- **Games**: `game` (the one game model: ids and caps), `source` (where a game comes from), `steam`
  (installed and owned games), `steam_info/` (store info, achievements, news, friends, local screenshots,
  the warm task), `emu` (the emulator library), `registry` (the table of emulators brisk can run),
  `migrate` (old data files to the current ids), `prefs` (per-game quality), `notes` (notes and tags),
  `playtime`.
- **Emulator games**: `ingest` and `ingest/` (games folder scan, add, remove), `uploads` (resumable
  uploads), `discid` (identify a disc by its header), `launch`, `launchstore` and `wrap` (launch options and
  the Steam launch shim), `controls` (Dolphin controller files), `saves` (read-only saves list), `shots`
  (stream screenshots), `art`, `steamgriddb`, `gametdb` (art and details), `retroachievements`,
  `ra_game`, `rahash` (achievement progress).
- **Hub**: `machines` (machines.json, the proxy, the power route), `power` (Proxmox), `idle` (the idle
  shutdown clock).

## Data files

hostd keeps everything in two places. Paths are the defaults; [env.md](env.md) has the overrides.

- `~/.config/brisk/`: `secret` (API secret, 0600), `allowed-users`, `steam-api-key`,
  `steamgriddb-api-key`, `retroachievements`, `tls/` (the bridge certificate, made by hostd), and on a
  hub `machines.json`.
- `~/.local/share/brisk/` (the data dir): `prefs.json`, `sessions.jsonl`, `owned.json`, `notes.json`,
  `library.json` (emulator games), `removed.json`, `launch.json`, `playtime.json`, `emu-played.json`,
  `steam/` (Steam caches), `art/`, `shots/`.
- `~/Games/<system>/` holds the emulator game files; `.uploads/` there holds partial uploads.

## A stream, start to finish

1. The page calls `POST /api/stream` with the game and quality. The guard checks the Host header and the
   secret (`x-brisk-secret`) or an allowed tailnet login.
2. hostd resolves the bridge address (`BRISK_LISTEN`, else the Tailscale IPv4 on :4433), makes the
   certificate if needed, stops any old bridge and spawns `brisk-bridge`.
3. The bridge connects to Sunshine with the client identity paired once beforehand (`brisk-probe`), runs the RTSP handshake with the tap SDP attribute, and
   opens the control stream. Sunshine connects to the tap socket and sends encoded frames.
4. The bridge prints a `page` line with the WebTransport URL (it carries a session token) and the
   certificate hash. hostd reads it, and the page gets it from `GET /api/stream`.
5. The page opens WebTransport straight to the bridge (never through the hub), decodes with WebCodecs and
   sends input back on the same session.
6. On stop, hostd ends the bridge. The bridge prints its end report, which hostd stores in
   `sessions.jsonl` for the history page.
