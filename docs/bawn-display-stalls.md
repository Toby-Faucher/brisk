# bawn display stalls (the "1 second freeze")

Symptom: video freezes for ~0.7 s (sometimes ~1.7 s) every few seconds, in Brisk **and** stock
Moonlight, with any content (game, plain Chrome animation, no stream at all).

## Root cause (found 2026-10-25)
Intel i915 gets a **long HPD pulse on PIN 11 (DDI TC3/PHY D = DP-2 / HDMI-A-2, the ArkKVM port)**
every few seconds. Each pulse runs `intel_dp_detect` + `intel_hdmi_detect` on both connectors, the
DPCD/AUX read times out (~0.67 s, `Too many retries, giving up. First error: -110`) while holding the
DRM modeset lock, so sway's page flip blocks in `drm_modeset_lock` (thread state D) and no frame is
flipped. Measured: sampler gaps == `drm_vblank_event` gaps (0.67/0.67/1.73/0.67 s); hardware vblank
kept running (frame counter +40 across a 0.68 s stall).
Not the cause: Brisk, WebTransport, laptop Chrome, network, Sunshine capture, the game, direct scanout
(already disabled), gamescope (not viable here: `drmModeAddFB2WithModifiers failed`).

## How it was measured (reusable)
- `grim` loop hashing the screen: a screencopy blocks while sway commits nothing, so gaps = stalls.
- tracefs `drm:drm_vblank_event` / `i915:*` (needs root; `trace_clock=boot` to align with /proc/uptime).
- `/proc/<sway>/task/<pid>/stack` while state D, then all tasks' stacks for the lock holder.
- `echo 0x4 > /sys/module/drm/parameters/debug` for 25 s -> HPD lines in dmesg (reset to 0 after).
- Page side: `SAME PICTURE for N frames` lines in the stats box (tiny frames < 300 B = unchanged picture).

## Runtime fix (applied, NOT persistent; gone after reboot)
    echo 5 > /sys/kernel/debug/dri/0000:00:02.0/i915_hpd_storm_ctl   # default 50; DP-2 then goes to polling
    echo 0 > /sys/module/drm_kms_helper/parameters/poll               # stop the 10 s poll of that connector
Result: stalls 8-10/min (incl. 1.7 s) -> 0-2/min; Brisk+Mewgenics loop 6 long runs -> 1 (stream start).
Revert: `echo 50 > .../i915_hpd_storm_ctl`, `echo 1 > .../poll`, or reboot.
Side effect: no hotplug detection on DP-2/HDMI-A-2 (they are hidden from sway anyway, `status=off` in the
getty drop-in). HDMI-A-3 (forced virtual display) is a different pin and is unaffected.

## Permanent (installed 2026-10-25)
`/usr/local/sbin/bawn-hpd-fix` + `/etc/systemd/system/bawn-hpd-fix.service` (oneshot, enabled; same pattern as
`bawn-virtual-display.service`) apply the two settings above at every boot. Not yet verified across a reboot.
Undo: `systemctl disable --now bawn-hpd-fix`, then the revert commands above. Surest fix remains unplugging the
ArkKVM / fixing its HPD line.
