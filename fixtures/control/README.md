# Control stream fixtures (stub)

No live capture yet. Everything in brisk_proto::{enet, control} and the fake host in
crates/brisk-client/tests/control.rs comes from reading the pinned C sources. The first live run
(`brisk-probe control`, with `tcpdump -w` on UDP 47999 if wanted) must confirm the byte offsets
below. Then scrub the capture (it carries the per-session connect data and ciphertext under the
rikey; the rikey itself must never be stored) and commit it here.

## Client to host (ENet CONNECT, datagram 1)

| offset | bytes | expect |
|---|---|---|
| 0 | 2 | `8F FF`: peerID 0xFFF and the sent-time flag |
| 4 | 4 | `82 FF 00 01`: CONNECT plus ACK flag, channel 0xFF, seq 1 |
| 12 | 4 | MTU 900 (`00 00 03 84`) |
| 20 | 4 | channel count 0x30 |
| 48 | 4 | connect data, big-endian (= `X-SS-Connect-Data`, decimal, from SETUP control) |

## Host to client (VERIFY_CONNECT)

- The header's peerID low 12 bits = 0, our incoming id. Session bits 12-13 = verify
  `incomingSessionID`. **[U]** We only read this from protocol.c; the live run checks it.
- `outgoingPeerID` (offset 4 of the command) is the host's slot for us. Throttle 5000/2/2 and our
  connectID are echoed; we refuse a mismatch.
- channelCount: 0x30 is expected (Sunshine channel limit 255).

## Encrypted control messages (inside ENet SEND_RELIABLE)

| offset | bytes | expect |
|---|---|---|
| 0 | 2 | `01 00` |
| 2 | 2 | length LE = 20 + plaintext length |
| 4 | 4 | seq LE: host counts 0,1,2..., client counts 0,1,2... |
| 8 | 16 | GCM tag (tag first, then ciphertext) |
| 24 | n | ciphertext of `type LE, payloadLength LE, payload` |

To confirm live, in order:

1. Sunshine accepts our first message (0x0302 IDR) and Start B. It logs `IDX_REQUEST_IDR_FRAME`
   and `IDX_START_B` at debug level, and there is no "Failed to verify tag". This proves the
   IV is `seq LE, 0.., 'C','C'`.
2. The first host message (likely HDR mode 0x010e when the encoder starts, or rumble) decodes
   with IV `'H','C'` and host seq 0, and later seqs increase by 1. We reject seq <= the last one
   seen (replay). **[U]** That stream.cpp's single `control.seq++` counter never reorders is our
   reading, not a measurement.
3. Termination on cancel (if Sunshine sends one before the ENet reset) is 0x0109 with a
   big-endian HRESULT, 0x80030023.
4. Pings are 0x0200, 8 bytes, every 100 ms. ENet ACKs come back for them (`pings acked`
   about equal to `pings sent`).
