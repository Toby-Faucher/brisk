# Mic Passthrough Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A mic button in the stream page sends the viewer's voice to a virtual "Brisk mic" on the game host.

**Architecture:** Chrome captures the mic, an AudioWorklet hands 128-frame blocks to the main thread, which
packs 10 ms of s16 mono PCM per WebTransport datagram. The bridge reads the browser's datagrams, checks
them, and pipes the PCM into a `pw-cat` child that PipeWire shows as a source. One `pw-cat` per viewer,
started on the first packet, killed when that viewer's connection ends.

**Tech Stack:** Rust (quinn, tokio `process`), TypeScript + Svelte 5, Web Audio, WebTransport datagrams.

**Spec:** `docs/superpowers/specs/2026-11-14-mic-passthrough-design.md`

## Global Constraints

- Packet: `[0x30][seq u16 LE][480 x s16 LE]`, exactly 963 bytes. 48000 Hz, mono, 10 ms.
- Host side: `pw-cat --playback --raw --format s16 --rate 48000 --channels 1 --latency 20ms -P '{ media.class=Audio/Source node.name=brisk-mic node.description="Brisk mic" }' -`
  (checked on games-arch 2026-11-14: `Audio/Source` shows up and becomes the default source; `Audio/Source/Virtual` does not become default).
- Mic is off by default; on only after a click in this session. Reconnect, quality change, disconnect, leaving the page: mic off and tracks stopped.
- A broken mic path never ends or slows the stream: every mic error is logged or toasted, then dropped.
- No new crates or npm packages. tokio gains the `process` feature in `brisk-bridge` only. Pure Rust stays pure.
- Web strings pass `scripts/check-wording.sh`. Rust follows `docs/rust-style.md`; no `unsafe`.
- Builds on bawn: `. scripts/capped.sh; capped cargo ...`. Do not build while Toby is gaming.

## Review Focus

1. Mic off then on again restarts the browser's seq at 0. The bridge must take it, not drop a second of packets as "old". (Task 1, `order_takes_a_restart`.)
2. `pw-cat` missing or dying: the stream carries on, one warning, later packets dropped. (Task 1, `missing_program_disables_quietly`.)
3. A permission prompt the user denies, or a `getUserMedia` that resolves after the connection was replaced: no hot mic, button stays off, a toast says why. (Task 3, `late_mic_after_reconnect_is_stopped`, `denied_mic_notifies_and_stays_off`.)
4. Datagrams with the wrong length or kind (an old or future page): dropped, no panic. (Task 1, `parse_rejects_bad_packets`.)
5. A full pipe to `pw-cat`: packets dropped, the read loop never waits on it. (Task 1, `full_queue_drops`.)

---

### Task 1: Bridge mic sink (`mic.rs`)

**Files:**
- Create: `crates/brisk-bridge/src/mic.rs` (with `#[cfg(test)] mod tests`)
- Modify: `crates/brisk-bridge/src/lib.rs` (add `pub mod mic;`), `crates/brisk-bridge/Cargo.toml` (tokio feature `process`)

**Interfaces:**
- Produces:
  - `pub const KIND_MIC: u8 = 0x30; pub const MIC_FRAMES: usize = 480; pub const MIC_PACKET_LEN: usize = 963;`
  - `pub fn parse(b: &[u8]) -> Option<(u16, &[u8])>`: `(seq, pcm)` for a 963-byte packet of kind 0x30, else `None`.
  - `pub struct Order` (`Default`), `pub fn fresh(&mut self, seq: u16) -> bool`: true for the first seq, for a seq ahead of the last (`seq.wrapping_sub(last) as i16 > 0`), and for one more than 100 behind (a restart). Updates `last` when true.
  - `pub fn pw_cat() -> tokio::process::Command`: the command from Global Constraints, stdin piped, stdout and stderr null, `kill_on_drop(true)`.
  - `pub struct MicOut`, `pub fn new(cmd: tokio::process::Command) -> Self` (spawns nothing yet), `pub fn push(&mut self, pcm: &[u8])`: spawns on first call, then `try_send`s a copy to a writer task (channel of 8) that `write_all`s to stdin. Spawn or write failure: one `tracing::warn!`, then every later push is a no-op. Prints `mic\tstarted` once on spawn.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn parse_rejects_bad_packets() {
    let mut p = vec![KIND_MIC, 7, 1]; p.resize(MIC_PACKET_LEN, 0);
    assert_eq!(parse(&p).map(|(s, b)| (s, b.len())), Some((0x0107, 960)));
    assert!(parse(&p[..962]).is_none());
    let mut q = p.clone(); q.push(0); assert!(parse(&q).is_none());
    let mut k = p.clone(); k[0] = 0x31; assert!(parse(&k).is_none());
    assert!(parse(&[]).is_none());
}
#[test] fn order_drops_old_and_wraps() {
    let mut o = Order::default();
    assert!(o.fresh(65534)); assert!(o.fresh(65535)); assert!(o.fresh(0)); assert!(o.fresh(2));
    assert!(!o.fresh(1)); assert!(!o.fresh(2));
}
#[test] fn order_takes_a_restart() {
    let mut o = Order::default();
    assert!(o.fresh(30000)); assert!(!o.fresh(29950)); assert!(o.fresh(0));
}
#[tokio::test] async fn writes_pcm_in_order() { /* MicOut over `sh -c 'cat > "$F"'` (F = tempdir file), push [1,2] then [3,4], drop, wait for the file: == [1,2,3,4] */ }
#[tokio::test] async fn missing_program_disables_quietly() { /* MicOut over Command::new("/nonexistent/pw-cat"): two pushes do not panic */ }
#[tokio::test] async fn full_queue_drops() { /* MicOut over `sleep 5` (never reads stdin): 1000 pushes of 960 bytes return at once (under 100 ms) */ }
```

- [ ] **Step 2: Run, expect FAIL (module missing):** `. scripts/capped.sh; capped cargo nextest run -p brisk-bridge mic::`
- [ ] **Step 3: Implement `mic.rs` per the Interfaces block; add `process` to brisk-bridge's tokio features.** Use `tempfile` only if it is already a dev-dependency; otherwise `std::env::temp_dir()` plus the test name.
- [ ] **Step 4: Run, expect PASS:** same command.
- [ ] **Step 5: Commit:** `bridge: mic sink, PCM packets into a pw-cat source`

### Task 2: Bridge reads the browser's datagrams

**Files:**
- Modify: `crates/brisk-bridge/src/wt.rs` (new `Session::read_datagram`, helper, test), `crates/brisk-bridge/src/mic.rs` (`serve`), `crates/brisk-bridge/src/session.rs:627-646` (spawn it beside `input::serve`), module doc of `wt.rs` (browser datagrams are mic audio)

**Interfaces:**
- Consumes: Task 1 `parse`, `Order`, `MicOut`, `pw_cat`.
- Produces:
  - `fn strip_header<'a>(h: &[u8], d: &'a [u8]) -> Option<&'a [u8]>` in `wt.rs`: payload when `d` starts with `h`.
  - `pub async fn read_datagram(&self) -> Result<Bytes, quinn::ConnectionError>` on `wt::Session`: loops `conn.read_datagram()`, skips any whose prefix is not this session's `datagram_header`, returns the payload (`Bytes::slice`).
  - `pub async fn serve(s: Arc<wt::Session>)` in `mic.rs`: `while let Ok(d) = s.read_datagram().await`, parse, `Order::fresh`, `MicOut::push`. Ends with the connection (dropping `MicOut` kills `pw-cat`).

- [ ] **Step 1: Failing test in `wt.rs` tests:** `strip_header_checks_the_prefix`: `strip_header(&[0x01], &[0x01, 9, 9]) == Some(&[9, 9][..])`, `strip_header(&[0x01], &[0x02, 9]) == None`, `strip_header(&[0x40, 0x40], &[0x40]) == None`.
- [ ] **Step 2: Run, expect FAIL:** `capped cargo nextest run -p brisk-bridge wt::`
- [ ] **Step 3: Implement `strip_header`, `read_datagram`, `mic::serve`; in `forward`'s `Ev::Viewer` arm add `tokio::spawn(mic::serve(s.clone()));` next to `input::serve`.** quinn's datagram receive buffer is on by default; leave `endpoint()` as is.
- [ ] **Step 4: Run, expect PASS:** `capped cargo nextest run -p brisk-bridge` and `capped cargo clippy -p brisk-bridge --all-targets -- -D warnings`
- [ ] **Step 5: Commit:** `bridge: read the viewer's datagrams as mic audio`

### Task 3: Browser mic capture (`lib/mic.ts`)

**Files:**
- Create: `web/src/lib/mic.ts`, `web/src/lib/mic-worklet.ts`, `web/src/lib/mic.test.ts`

**Interfaces:**
- Produces:
  - `export const KIND_MIC = 0x30, MIC_FRAMES = 480, MIC_PACKET_LEN = 963;`
  - `export function packMic(seq: number, pcm: Float32Array): Uint8Array`: header plus `Math.round(clamp(x, -1, 1) * 32767)` as s16 LE; `seq & 0xffff`.
  - `export class MicPacker { push(block: Float32Array): Uint8Array[] }`: buffers blocks, returns one packet per full 480 frames, seq from 0 and wrapping at 65536.
  - `export type Mic = { stop(): void }`
  - `export async function startMic(t: WebTransport, workletUrl: string): Promise<Mic>`: creates `new AudioContext({ sampleRate: 48000, latencyHint: 'interactive' })` synchronously first (keeps the click's activation), then `getUserMedia({ audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true, autoGainControl: true } })`, `addModule(workletUrl)`, source -> `AudioWorkletNode(ctx, 'brisk-mic')` -> `ctx.destination` (its output stays silent; the connection makes Chrome pull it). Each posted block goes through `MicPacker`; each packet is written with one `t.datagrams.writable.getWriter()`, not awaited, errors ignored. On any failure before it resolves, stop the tracks and close the context, then throw. `stop()` stops every track, disconnects, closes the context, releases the writer; calling it twice is safe.
  - `mic-worklet.ts`: processor `brisk-mic` posts a copy of `inputs[0][0]` each quantum when present (same declare block as `audio-worklet.ts`).
- [ ] **Step 1: Failing tests (`mic.test.ts`):**
  - `packMic writes the header and clamps`: `packMic(0x10203, f)` with f = [0, 1, -1, 2, ...zeros to 480] gives length 963, bytes 0..3 = [0x30, 0x03, 0x02], sample 1 = 32767, sample 2 = -32767, sample 3 = 32767.
  - `MicPacker makes 10 ms packets from 128-frame blocks`: 4 blocks (512 frames) give 1 packet with seq 0; 3 more blocks (896 total) give 1 more with seq 1; the second packet's first sample is frame 480 of the input.
  - `MicPacker seq wraps`: after 65536 packets the next has seq 0.
- [ ] **Step 2: Run, expect FAIL:** `npm --prefix web exec -- vitest run --project unit src/lib/mic.test.ts`
- [ ] **Step 3: Implement both files.**
- [ ] **Step 4: Run, expect PASS:** same command, plus `npm --prefix web run check` (type check).
- [ ] **Step 5: Commit:** `web: mic capture, 10 ms PCM datagrams`

### Task 4: Mic button and session wiring

**Files:**
- Modify: `web/src/modules/stream/session.ts`, `web/src/modules/stream/session.test.ts`, `web/src/modules/stream/GhostBar.svelte`, `web/src/modules/stream/GhostBar.browser.test.ts`, `web/src/modules/stream/Stream.svelte`, `web/src/ui/Icon.svelte` (icons `mic`, `micoff`, 24x24 like `vol`/`mute`)

**Interfaces:**
- Consumes: Task 3 `startMic`, `Mic`.
- Produces:
  - `Deps.startMic?(t: Transport): Promise<{ stop(): void }>`; `Conn.mic?: { stop(): void }`.
  - `StreamSession.micOn = writable(false)`; `async toggleMic(): Promise<void>`: off when on. When on is asked: no-op without a live `conn`; else `await d.startMic(conn.t)`; if `conn !== this.conn` by then, stop it and stay off; on error `notify('Could not use the mic: <errMsg>', true)` and stay off. `end()` stops `conn.mic` and sets `micOn` false.
  - GhostBar props `micOn: boolean`, `onmic: () => void`. Button right after the volume slider: `aria-pressed={micOn}`, `aria-label={micOn ? 'Turn mic off' : 'Turn mic on'}`, tip `micOn ? 'Mic on: the host hears you' : 'Mic off'`, icon `micOn ? 'mic' : 'micoff'`.
  - Stream.svelte: `startMic: (t) => startMic(t as WebTransport, micWorkletUrl)` with `import micWorkletUrl from '../../lib/mic-worklet.ts?worker&url'`; pass `micOn={$micOn}` and `onmic={() => session.toggleMic()}`.
- [ ] **Step 1: Failing tests:**
  - session.test.ts `toggleMic starts and stops`: fake `startMic` returns `{ stop: spy }`; after connect, toggle: `micOn` true, startMic called with the conn's transport; toggle again: stop called once, `micOn` false.
  - `reconnect turns the mic off`: mic on, then the transport's `closed` resolves: stop called, `micOn` false.
  - `late_mic_after_reconnect_is_stopped`: startMic resolves after a `changeQuality`: its stop is called, `micOn` false.
  - `denied_mic_notifies_and_stays_off`: startMic rejects `new Error('Permission denied')`: notify called with `'Could not use the mic: Permission denied', true`, `micOn` false.
  - GhostBar.browser.test.ts `mic button shows its state and calls onmic`: with `micOn: false` the button named "Turn mic on" has `aria-pressed="false"`; click calls `onmic`; rerender with `micOn: true` gives "Turn mic off", pressed.
- [ ] **Step 2: Run, expect FAIL:** `npm --prefix web exec -- vitest run --project unit src/modules/stream` and `npm --prefix web exec -- vitest run --project browser src/modules/stream/GhostBar`
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run, expect PASS:** same, then `npm --prefix web run ci`.
- [ ] **Step 5: Commit:** `web: mic button in the stream bar`

### Task 5: Full checks, deploy, live test

**Files:** none new (PR body only).

- [ ] **Step 1:** `. scripts/capped.sh; scripts/check.sh`. Expected: every step passes.
- [ ] **Step 2:** Push the branch, open a PR (pr skill), link it.
- [ ] **Step 3: Ask Toby before deploying** (he must not be gaming; the hub restart drops a live stream). Then: build release (`capped cargo build --release -p brisk-bridge -p brisk-hostd`, `npm --prefix web run build`), copy the bridge and web build to games-arch, rebuild the hub image with the new web (docs/hosting-runbook.md, "Machine hub"; tag `brisk-hub:prev` first).
- [ ] **Step 4: Live test with Toby:** start a stream from the laptop, click the mic button, allow the mic. On games-arch: `pactl get-default-source` prints `brisk-mic`, and `timeout 3 parec --latency-msec=20 --raw --format=s16le --rate=48000 --channels=1 | python3 -c '...peak...'` shows a peak above 1000 while Toby talks. Then Toby checks Peak's voice settings show the mic.
- [ ] **Step 5:** Merge only on Toby's "merge N".
