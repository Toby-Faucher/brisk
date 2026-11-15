# Mic passthrough: design

## Why

Toby wants to play Peak with friends through brisk. Peak's voice chat is proximity voice inside the
game, so the mic has to reach the machine running the game (games-arch). Today brisk only sends
audio from the host to the browser. Moonlight and Sunshine have no mic path either.

## Goals

- A mic button in the stream page. Click it and your voice reaches games-arch as a mic that games
  can pick. Click again and it stops.
- Low enough delay for live talk (aim: under 100 ms on the LAN or tailnet).
- Works in Chrome on a laptop, with a Linux host running PipeWire.

## Non-goals

- Push to talk, a level meter, mic choice inside brisk (Chrome's own picker covers it).
- Windows or macOS hosts, more than one viewer's mic at once, the legacy stream page.
- Opus or any codec. Raw PCM is small enough for a LAN or tailnet.

## Anti-goals

- The mic is never on unless the user clicked the button in this session. Leaving the stream page,
  a disconnect, or a reconnect turns it off and releases the mic.
- No lasting change to the host's audio settings (no saved default device).

## Approaches considered

1. Direct PipeWire RTP link from the laptop to games-arch. No code, but it lives outside brisk and
   has no mute in the stream page. Fine as a stopgap, not picked.
2. Mic support inside brisk (picked by Toby): browser to bridge over the WebTransport connection
   that already exists, then into a virtual PipeWire mic on the host.

## Design

**Browser** (`web/src/lib/mic.ts`, button in `GhostBar.svelte`, wired in `session.ts`):

- `getUserMedia({ audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true,
  autoGainControl: true } })`. Chrome asks for permission the first time.
- An `AudioContext` at 48 kHz and an AudioWorklet that posts 128-frame blocks. The main thread
  packs 480 frames (10 ms) as signed 16-bit little-endian PCM.
- Each packet is one WebTransport datagram: `[0x30 kind][seq u16 LE][960 bytes PCM]`, 963 bytes.
- Off by default. Stopping ends the tracks so Chrome's mic light goes out.

**Bridge** (`crates/brisk-bridge`):

- Read the browser's datagrams (`conn.read_datagram()`), check the session's quarter stream id
  prefix, then the kind and the exact length. Anything else is dropped. Drop a packet whose seq is
  not newer than the last one.
- On the first good packet, start
  `pw-cat --playback --raw --format s16 --rate 48000 --channels 1` with properties that make it a
  source named `brisk-mic` ("Brisk mic"), and write PCM to its stdin. Writes go through a small
  bounded queue; when it is full the packet is dropped, so a stuck pw-cat never slows the stream.
- The child is killed when the session ends. If pw-cat is missing or fails, log once and ignore mic
  packets; the stream carries on.
- First step of the build: check on games-arch that a `pw-cat` stream with
  `media.class=Audio/Source/Virtual` shows up as a mic. If not, fall back to a `pw-loopback`
  pair (a sink brisk writes to, a source games read from).

**Default mic:** games-arch has no real mic, so `brisk-mic` should be picked as the default while it
exists. Check that Peak (through Proton) uses it.

**No changes** to hostd, Sunshine, or the GameStream side.

## Testing

- Rust unit tests: datagram parse (good packet, wrong kind, wrong length, old seq).
- Web unit test: float to s16 packing and the packet header.
- All of `scripts/check.sh`.
- Live test with Toby before playing: talk, and check the level on games-arch with `pw-record`.
  Headphones are recommended; Chrome's echo cancellation may not remove game sound from speakers.

## Left to the builder

- Exact button placement and icon in the ghost bar.
- How the new bridge and web build reach games-arch and the hub (follow the hosting runbook).
