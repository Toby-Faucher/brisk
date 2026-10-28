# Machine hub deployment

One `brisk-hostd` with a `machines.json`, run as a Docker stack (Dockge) on a small VM and exposed with
`tailscale serve`. Everything else (build steps, Proxmox token, config, troubleshooting) is in
[docs/hosting-runbook.md](../../docs/hosting-runbook.md), section Machine hub. Contract:
[docs/plans/machine-hub.md](../../docs/plans/machine-hub.md).

- `Dockerfile`: runtime image around a prebuilt `brisk-hostd` (nothing is compiled in the image).
- `compose.yaml`: the stack. Host networking, config mounted read-only, one data volume.
- `machines.example.json`: copy to the config directory as `machines.json` and edit.
