# Tap fixtures

`NNN.bin` are the first 60 messages the brisk-patched Sunshine (v2026.914.233613 +
patches/sunshine/0001-brisk-tap.patch) sent on the tap socket, byte for byte as received: the
44-byte little-endian `BTAP` v1 header (crates/brisk-proto/src/tap.rs) and then the Annex B
access unit. One message per frame, frame_index 1..60, 000.bin is the opening IDR (SPS, PPS,
slice).

Recorded 2026-10-24 on bawn, loopback, static Steam Big Picture screen, 1080p60 H.264 (VAAPI):

    brisk-probe stream --host 127.0.0.1 --app 870779643 --secs 60 --tap --record-samples DIR

then `DIR/tap/*.bin` copied here. Each message was passed through `brisk_proto::scrub::scrub`
with the session's rikey before writing (the tap carries no key material; the scrub is a guard).
`connect_data` in the header is the per-session `X-SS-Connect-Data` of a finished session, not a
secret. The payload is encoded video of the host screen at that moment.

Replay: `cargo test -p brisk-proto --test tap` (`recorded_tap_frames_parse`).
