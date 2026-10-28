# Machine hub

One URL, several streaming machines. A **hub** is a `brisk-hostd` that has a `machines.json`. It serves
the UI, forwards machine-scoped API calls to the machine you picked, and can power a machine on/off
(Proxmox). Video does not go through the hub: `POST /api/stream` on the machine still answers with
its own bridge `wt` URL and cert `hash`, and the browser connects there directly.

Without a `machines.json` hostd behaves exactly as before (single host, `GET /api/machines` is 404).

## machines.json

Path: `BRISK_MACHINES_FILE`, default `~/.config/brisk/machines.json`. Secrets are read from files at
use, never stored in `Config`, never logged.

```json
{ "machines": [
  { "id": "bawn", "name": "bawn",
    "url": "https://bawn.<tailnet>.ts.net:8443", "secret_file": "/run/secrets/bawn" },
  { "id": "games", "name": "games-arch",
    "url": "https://games-arch.<tailnet>.ts.net:8443", "secret_file": "/run/secrets/games",
    "power": { "proxmox": { "api": "https://<pve>:8006", "node": "<node>", "vmid": 110,
                            "token_id": "brisk@pve!hub", "token_file": "/run/secrets/pve",
                            "ca_file": "/run/secrets/pve-ca.pem" },
               "idle_shutdown_min": 15 } } ] }
```

Rules: `id` is `[a-z0-9-]{1,32}` and unique; `url` is https (or http on loopback, tests), no trailing
slash; `idle_shutdown_min` optional (absent = never auto shut down); at least one machine.
A bad file stops hostd at startup with a clear error (do not silently fall back to single host).

## Hub API (only when machines.json exists)

All under the existing guard (Host allowlist + secret or tailnet login).

- `GET /api/machines` -> `{ "machines": [Machine] }`
  `Machine = { id, name, state, canPower, sunshine, host }`
  - `state`: `"off" | "starting" | "stopping" | "ready" | "unreachable"`
    - power machine: Proxmox stopped -> `off`; running and the machine's `/api/host` says
      `sunshine == "ready"` -> `ready`; running otherwise -> `starting`; a shutdown requested in the
      last 120 s and still running -> `stopping`; Proxmox unreachable -> `unreachable`.
    - no power: host reachable -> `ready`, else `unreachable`.
  - `canPower`: has a `power` block. `sunshine`: `"ready"|"busy"|"down"|null`. `host`: the machine's
    `/api/host` body (HostInfo) or null.
  - Answers fast: per-machine probes run in parallel with a 3 s timeout and are cached 3 s.
- `POST /api/machines/{id}/power` body `{"action":"start"|"shutdown"}` ->
  `202 {"state": ...}`. Start on a running VM and shutdown on a stopped one are 200 no-ops.
  404 unknown id; 409 `{"error":"..."}` no power control; 502 `{"error":"..."}` Proxmox failed
  (message never contains the token).
- `ANY /api/m/{id}/{*path}` -> forwarded to `{url}/api/{path}?{query}` with the machine's secret as
  `x-brisk-secret` and `x-brisk-csrf: 1`. Forwards method, `content-type`, `accept`, the body and
  these response headers: status, `content-type`, `x-brisk-seq`. Body cap 16 MiB each way.
  `files/*` -> 501 `{"error":"file drop is not available through the hub"}`.
  Unknown id 404; machine powered off 503 `{"error":"machine is off"}`; unreachable/timeout 502
  `{"error":"machine did not answer"}` (timeout 40 s: `/api/quit` can take 25 s).
- Everything else under `/api/*` stays the hub's own (prefs and notes are user-level and live on the hub).

## Idle shutdown

For a power machine with `idle_shutdown_min`, the hub checks every 30 s: if it is `ready` and the
machine reports no live stream (`GET /api/stream` state `idle`), no running game (`/api/games`
`running`), and no Steam update in progress (`update == "running"`), an idle clock runs; after N
minutes the hub shuts the VM down (ACPI shutdown, never a hard stop). Any activity, or being unable
to observe the machine, resets the clock. The clock starts when the machine becomes `ready`.

## Web

- `web/src/state/machines.ts` (stores): `hub: Readable<boolean|null>` (null = not probed yet),
  `machines: Readable<Machine[]>` (polled every 5 s while a hub is detected, every 1.5 s while one is
  `starting`), `selected: Writable<string|null>` (persisted in localStorage `brisk.machine`, falls back
  to the first machine), `selectMachine(id)`, `powerMachine(id, action)`, `ensureReady(id)` (starts
  the machine when `off`, resolves when `ready`, rejects after 5 min; exposes progress through the
  `wake` store: `{ id, phase: "starting"|"waiting"|"failed", since, error? } | null`).
- `web/src/state/api.ts`: `hub` mode routes machine-scoped calls through `/api/m/{selected}/...`.
  Machine-scoped: host, games*, friends, stream, launch, install, quit, sessions*, sunshine/*,
  clipboard, files. User-level (stay on the hub): prefs*, notes*. Single-host mode is unchanged.
  `api.machines()` and `api.power(id, action)` are always unrouted.
- UI: Machines page lists every machine (state dot, Sunshine, temps, Start / Shut down / Use);
  the shell host pill shows the selected machine and opens a switcher; Ctrl+K gets "Switch to X";
  `WakeOverlay` shows "Starting games-arch (GPU, Steam, Sunshine)..." while `wake` is set.
  Picking an `off` machine, or pressing Play/Stream while the selected machine is off, calls
  `ensureReady` first. Opening the app with an off machine selected does NOT start it.

## Out of scope for v1

Merged library across machines, per-game preferred machine, file drop through the hub, WoL.
