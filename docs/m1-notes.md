# M1 notes

## Status (2026-10-24)

| milestone | status |
|---|---|
| M1 (receive + control) | complete: live gate PASS (60 s `brisk-probe stream`, loopback, LAN IP and Tailscale IP of bawn itself; docs/m0-baseline.md) |
| M0 (latency baseline) | partial: host side done (capture+encode, jitter, IDR response); network RTT, client decode and present rows PENDING (Toby) |

Verified live: control stream, encrypted RTSP, video packet parsing and frame assembly, audio
parsing and AES-CBC decryption (`SS_ENC_AUDIO`, encryption 0x5), IDR requests. Verified offline
against real Sunshine bytes: video Reed-Solomon and the audio parity matrix (erasures of the
recorded parity blocks, brisk-proto tests `recorded_*_fec_*`).
Still unverified: frames spanning several FEC blocks (none recorded), FEC under real loss (the live
runs saw none), input packet bytes (until the `--input-demo` live run), rumble, HEVC,
`SS_ENC_VIDEO`, any real network path.

Purity: `scripts/check-pure-rust.sh` fails if ring, aws-lc-sys, openssl-sys or cc is reachable
through normal/build dependencies on any target (2026-10-24: passes).

## Control stream (Task 8)

### ENet: own minimal sans-io peer, rusty_enet only as a test-side host

Decision: `brisk_proto::enet` is our own ENet 1.3 client subset (about 600 lines); the
`rusty_enet` crate is used only as a dev-dependency, as the host side of the fake Sunshine in
`crates/brisk-client/tests/control.rs`.

What Sunshine needs, read from the pinned sources: Sunshine builds the ENet fork vendored in
moonlight-common-c (`Sunshine/cmake/dependencies/common.cmake:14`), ENet 1.3.17
(`enet/include/enet/enet.h:26-28`). Neither side sets a checksum or a compressor
(`host.c:91,109`; Sunshine `network.cpp:222`, moonlight `ControlStream.c:1771`), so the wire is plain
ENet 1.3. The protocol header (`enet/include/enet/protocol.h`) is the same as upstream. The fork's
changes are host-side only (QoS, dual stack, a 4096 max MTU); none of them touch the wire. Sunshine's
host is 128 peers, channel limit 0 (= 255), unlimited bandwidth. moonlight connects 0x30 channels with
connect `data` = `X-SS-Connect-Data`; Sunshine matches it in `get_session` (`stream.cpp:659-690`,
needs `ML_FF_SESSION_ID_V1` in `x-ml-general.featureFlags`). Every Sunshine control message is under
100 bytes, so fragmentation is not needed.

rusty_enet 0.4.0 (checked 2026-10-24): pure Rust, MIT, wire-compatible ENet 1.3.18. It was
machine-translated from C: about 5.7 k lines under `src/c/` with raw-pointer lists and 150+
`unsafe` sites. It is a whole host: it owns its peers and drives time through a callback and I/O
through a `Socket` trait. A sans-io wrapper would mean a fake socket plus a fake clock around an
unsafe core. Its default MTU is 1392, not the fork's 900. Its last release was April 2025. Our
subset is small enough to own and to read against protocol.c, so the design-doc default stands.
Because rusty_enet is a separate implementation, it is a good *test* peer: the fake's ENet layer
does not mirror our reading of the ENet code.

Not implemented (all `shortcut:` or documented in `enet.rs`): fragments (received ones are dropped
without an ACK), compression, checksum, bandwidth and throttle adaptation, unsequenced send. Caps:
256 unacked reliable commands, 64-deep reorder per channel, datagrams over 4096 bytes rejected.

### Control-protocol facts (Gen7Enc, Sunshine v2026.914.233613)

- Encrypted envelope (`controlProtocolType` 13 = SDP `useReliableUdp:13`, `SS_ENC_CONTROL_V2` = 1):
  `0x0001 LE | length LE (=4+16+n) | seq u32 LE | tag[16] | AES-128-GCM(rikey, no AAD)` of
  `type u16 LE | payloadLength u16 LE | payload` (stream.cpp:291-308, 576-617, 1238-1300).
  The IV is 12 bytes: `seq LE` in [0..4], then zeros, then `'C','C'` (client) or `'H','C'` (host)
  in [10..11]. Each direction counts from 0. Sunshine drops unencrypted messages
  (stream.cpp:733). It stops the session on a bad tag (stream.cpp:1277-1284).
- Packet types (stream.cpp:57-75): Start A 0x0305, Start B 0x0307, RFI 0x0301, loss stats 0x0201,
  frame stats 0x0204, input 0x0206, rumble 0x010b, termination 0x0109, periodic ping 0x0200,
  IDR 0x0302, encrypted 0x0001, HDR 0x010e, Sunshine extensions 0x5500-0x5504.
- **moonlight's Gen7Enc table puts the IDR request (0x0302) in the Start A slot**
  (ControlStream.c:203-217, 287-290). At stream start it sends 0x0302 `{0,0}` and then 0x0307
  `{0}`, both reliable on channel 0 (CTRL_CHANNEL_GENERIC, ControlStream.c:1869-1912). Later
  on-demand IDR requests go on channel 1 (URGENT, ControlStream.c:1524-1527). It never sends 0x0305.
- Video/audio keepalive: Sunshine's video and audio threads start at PLAY (stream.cpp:2289-2293) and
  wait `ping_timeout` (10 s) for a UDP datagram containing `X-SS-Ping-Payload` on each media port.
  Without one, `session::stop` runs and the ENet peer is disconnect_now'd (stream.cpp:2092-2163,
  1336-1349). The datagram's source port becomes the media peer. moonlight sends
  `SS_PING { payload[16] ASCII, seq u32 BE from 1 }` (20 bytes) every 500 ms to the video and audio
  ports from its media sockets (Video.h:51-54, VideoStream.c:55-82, AudioStream.c:38-64). The
  payload is the 16-character `X-SS-Ping-Payload` string from the audio/video SETUP reply. Brisk
  sends these from `AvPing`, starting before the ENet connect.
- Periodic ping: 0x0200 every 100 ms, reliable, channel 0. The 8-byte payload starts
  `04 00 00 00 00 00`; moonlight leaves the last 2 bytes uninitialised and we send zeros
  (ControlStream.c:1388-1440). Sunshine ends the session after 10 s with no ENet event
  (stream.cpp:1329, config.cpp:815).
- Termination (host to client): 0x0109 with `ec u32 BE`; graceful is 0x80030023
  (stream.cpp:1385-1408). It is followed by `enet_peer_disconnect_now` at host teardown
  (network.cpp:232-239).
- Rumble: 0x010b, payload `0xC0FFEE u32 (native LE) | id u16 LE | low u16 LE | high u16 LE`
  (stream.cpp:203-212, 1010-1025; moonlight skips the first 4 bytes, ControlStream.c:1044-1051).
- Input: 0x0206 with the plaintext NV_INPUT packet. Sunshine strips the 4-byte V2 header and
  passes the rest through (stream.cpp:1295-1298). Channels: keyboard 0x02, mouse (move, button,
  scroll) 0x03, gamepad 0x10+index. All are reliable (InputStream.c:738-899, 1073;
  Limelight-internal.h:57-66).
- RFI: 0x0301, 24 bytes `first u32 LE, 0, last u32 LE, 0,0,0`, on channel 1. Sunshine reads it as
  two int64 values (Video.h:80-86, stream.cpp:1199-1210).
- ENet details moonlight sets: MTU 900, peer timeout `(2, 10000, 10000)` (ControlStream.c:1837),
  graceful disconnect = `disconnect_later` + 2 s linger (Misc.c:45-80).

## Live control run (2026-10-24, bawn, Sunshine v2026.914.233613)
Command: `brisk-probe control --host 127.0.0.1 --app 870779643 --secs 30`. Result: ENet handshake OK, encrypted control OK both ways (297/297 pings acked, 0 bad host messages), IDR at start and at 15 s, host message 0x010e once, no termination, clean cancel, Sunshine FREE after. A/V keepalive pings accepted: video 24437 pkts (34.4 MB) and audio 8991 pkts (3.0 MB) received in 30 s (about 9.2 Mbit/s video on a static Steam Big Picture screen; [U] the bitrate depends on content and Sunshine's configured bitrate). At the time not verified live: video/audio packet parsing and FEC (parsing since verified by the stream runs, see Status), input packet bytes (no observable effect yet), rumble.

## Stream receive (Task 12)

`brisk-probe stream` feeds every datagram on the A/V ping sockets through brisk_proto
(`ControlSession::run_until_with` hands them to `brisk_client::stream::StreamRx`). Facts read
from the pinned sources; the audio-encryption ones are confirmed by the live stream runs
(encryption 0x5, 0 parse errors), video encryption is not (never negotiated):

- Audio is encrypted only when `SS_ENC_AUDIO` is negotiated (`encode_audio(session->config.encryptionFlagsEnabled & SS_ENC_AUDIO, ...)`, stream.cpp:352-360, 1889; legacy nvFeatureFlags 0x20 also turns it on, rtsp.cpp:1163-1166). The live control run negotiated 0x1, so its audio was plain. `stream` asks for `SS_ENC_AUDIO` (Moonlight's default, [U] for moonlight-qt) so the audio path and the recorded samples are AES-CBC; Sunshine always advertises it as supported (rtsp.cpp:931).
- Audio key: `rikey`; IV base `avRiKeyId` = the launch `rikeyid` (nvhttp.cpp:523 stores it big-endian, stream.cpp:2372 reads it back big-endian).
- Video stays plain unless `SS_ENC_VIDEO` is negotiated (LAN encryption mode "never" by default: Sunshine does not even advertise it, rtsp.cpp:939-948). Both paths are handled.
- Gate: parse errors (video decrypt/parse/assembler + audio FEC/decrypt) = 0, frames >= 0.95 x 60 x secs, and an IDR frame within 2 s of each IDR request (sent at secs/3 and 2 secs/3). A request in the last 2 s of the run with no IDR yet counts as unanswered. A frame lost beyond FEC also triggers an IDR request (Sunshine's GOP is infinite), at most one per 500 ms; those are counted in `idr_requests ... on frame loss` but not gated.

## Receive buffers

`brisk-probe stream` asks for SO_RCVBUF of 2 MiB (video) and 256 KiB (audio); the summary prints the effective sizes as `rcvbuf video/audio`. Linux doubles the requested value and clamps it to `net.core.rmem_max`, so a small sysctl shows up as a smaller number there. We do not change the sysctl (no root).

## Live input smoke (2026-10-24 13:08, bawn, `brisk-probe control --host 127.0.0.1 --app 870779643 --secs 20 --input-demo`)
Sunshine's debug log (min_log_level = debug) decoded what brisk sent, at 5.3 s into the run:
- relative mouse move: `deltaX [5] deltaY [0]`, then `deltaX [-5] deltaY [0]` (exact).
- keyboard: `keyAction [3]` then `[4]` (down, up), `keyCode [8010]` (VK_SHIFT 0x10 with moonlight's 0x8000 flag), modifiers 00, flags 00.
- gamepad 0 (neutral attach/detach): Sunshine allocated "Gamepad 0 ... X-Box Series Controller" (so the active_mask path works).
- 6 packets sent, `inputs_sent 6`, 0 bad host messages, no termination, clean cancel; the IDR request at 10 s was answered.
Not verified live: gamepad button/stick bytes (Sunshine logs controller packets only at `verbose`; log level is `debug`), mouse buttons, scroll, rumble back-channel, more than one pad. Those need either `min_log_level = verbose` or a game that reads the pad.
