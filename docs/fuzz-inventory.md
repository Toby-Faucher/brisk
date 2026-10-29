# Parsers of attacker-influenced input

Every function that reads bytes or text Brisk did not produce itself, as of 2026-10-26 (Rust
hardening part C). How to run the tests: docs/ui-notes.md, "Fuzzing and property tests".

Trust levels:
- **net**: unauthenticated network bytes (UDP from anyone who can reach the port, or the host
  before pairing). Treat as hostile.
- **host**: Sunshine on 127.0.0.1, after pairing (pinned TLS) or under AES-GCM with the launch key.
  Hostile only if Sunshine is compromised or something local races it, but still must not crash us.
- **local**: the patched Sunshine over the tap unix socket (peer uid checked).
- **peer**: a tailnet peer through WebTransport after the session-token check (the user's browser,
  or anyone holding the token).

Coverage after this pass: **P** = property test (file::test), **F** = cargo-fuzz target in fuzz/.

| Function | Crate / file | Input origin | Trust | Existing tests (before) | Coverage after this pass |
|---|---|---|---|---|---|
| `rtsp::parse_response`, `RtspReply::{cseq,session_id,server_port}` | brisk-proto src/rtsp.rs | RTSP replies | host (net for plain `rtsp://`) | tests/rtsp.rs fixtures, truncation | P prop_parsers::rtsp_parsers_take_any_bytes, rtsp_content_length_is_checked_against_the_bytes; F rtsp |
| `rtsp::parse_request` | brisk-proto src/rtsp.rs | RTSP requests (fake hosts in tests; shared parser) | host | tests/rtsp.rs round trip | P rtsp_request_round_trip, rtsp_parsers_take_any_bytes; F rtsp |
| `RtspCipher::open` | brisk-proto src/rtsp.rs | `rtspenc://` frames (length prefix, seq, tag) | host | tamper, wrong key, oversize, replay | P rtsp_frame_length_prefix_is_capped, rtsp_cipher_round_trip_and_rejects; F rtsp |
| `rtp::parse_video_packet` | brisk-proto src/rtp.rs | video UDP datagrams | net | recorded fixtures, caps, hostile hand-built | P video_parser_takes_any_bytes; F rtp_fec |
| `VideoCipher::open` | brisk-proto src/rtp.rs | SS_ENC_VIDEO datagrams | net | round trip, tamper, truncation | P video_cipher_round_trip_and_rejects_tampering, video_parser_takes_any_bytes; F rtp_fec |
| `FrameAssembler::push` (+ `finish`, frame header) | brisk-proto src/rtp.rs | parsed video packets | net | FEC recovery, wraps, bounds, duplicates | P frame_assembler_takes_any_packets, assembler_rebuilds_frames_under_loss_and_reordering; F rtp_fec |
| `fec::rs_decode`, `rs_decode_matrix` | brisk-proto src/fec.rs | shards from video/audio datagrams | net | nanors vectors (200+ cases) | P rs_recovers_any_loss_up_to_the_parity_count, rs_reports_too_many_losses_and_changes_nothing, audio_parity_matrix_recovers_any_two; F rtp_fec |
| `audio::parse_audio_payload`, `AudioCipher::open`, `parse_audio_packet` | brisk-proto src/audio.rs | audio UDP datagrams (AES-CBC, no MAC) | net | recorded fixtures, wrong key, malformed | P audio_parsers_take_any_bytes, audio_cbc_round_trip_and_wrong_key; F audio |
| `AudioFec::push` | brisk-proto src/audio.rs | audio data + FEC datagrams | net | recovery, windows, resync, odd sizes | P audio_fec_takes_any_datagrams, audio_fec_recovers_two_losses_per_block; F audio |
| `AudioRx::push`, `poll` | brisk-bridge src/session.rs | audio datagrams via AudioFec | net | tests/audio_fwd.rs | P prop_audio (both); F audio |
| `EnetPeer::handle_datagram` (+ `command`, `reliable`, `on_ack`) | brisk-proto src/enet.rs | control UDP datagrams (ENet layer is unauthenticated) | net | hand-built vectors, rusty_enet interop, garbage | P enet_peer_takes_any_datagrams, enet_reliable_delivery_is_in_order_and_exactly_once; F control_enet |
| `Control::decode`, `ControlMsg::parse` | brisk-proto src/control.rs | control messages (AES-GCM) | host (parse runs after the tag check) | round trip, tamper, unknown types, layout | P control_decoder_takes_any_bytes, control_parser_takes_any_plaintext, control_round_trip_both_directions; F control_enet |
| `ControlMsg::input`, `input::encode_input` | brisk-proto src/control.rs, src/input.rs | events built from browser input | peer (values already range-checked by the bridge) | byte layouts, clamps | P input_packets_are_well_formed; F input (found bug 1) |
| `http::{check_status, parse_field, parse_serverinfo, parse_applist, parse_launch}` (private `parse`) | brisk-proto src/http.rs | Sunshine HTTP(S) XML replies | net before pairing (serverinfo, pairing over HTTP), host after | fixtures, truncation, size cap, status | P http_parsers_take_any_text, http_parsers_take_any_xml_shape, serverinfo_and_applist_round_trip, deep_or_long_xml_paths_cost_no_quadratic_memory; F http_sdp (found bug 2) |
| `pair::unhex`, `aes_ecb_decrypt`, `PairingClient::on_*` | brisk-proto src/pair.rs | pairing reply fields (hex) | net (pairing runs over plain HTTP) | vectors, tamper, out of order, garbage | P hex_and_ecb_round_trip, sdp_attr_and_unhex_take_any_text; F http_sdp (unhex). Not fuzzed: `on_servercert`'s PEM/x509 parse (x509-cert crate; needs an RSA identity per run) |
| `sdp::sdp_attr` | brisk-proto src/sdp.rs | RTSP DESCRIBE body | host | describe fixture | P sdp_attr_and_unhex_take_any_text, sdp_build_round_trips_through_sdp_attr; F http_sdp |
| `tap::parse_tap` | brisk-proto src/tap.rs | tap SEQPACKET messages | local | round trip, magic, header_len, size, recorded | P tap_parser_takes_any_bytes, tap_header_round_trip; F tap |
| `TapConn::recv` / peek header | brisk-client src/tap.rs | tap socket (MSG_TRUNC length) | local | tests/tap_live.rs | via parse_tap (P/F above); the socket code itself is not fuzzable sans-io |
| `find_sps` (private) | brisk-client src/tap.rs | Annex B from the tap | local | table test + indirect (tap_live) | P tap::tests::find_sps_takes_any_annexb |
| `strip_filler` (private) | brisk-client src/tap.rs | Annex B from the tap | local | table tests | P tap::tests::strip_filler_takes_any_annexb (no panic, never longer, no filler left, idempotent, no copy without filler) |
| `http::response_body` | brisk-proto src/http.rs (called by brisk-client src/session.rs) | raw HTTP/1.1 responses from Sunshine | net before pairing, host after | response_body_checks_length_and_status | P http::tests::response_body_takes_any_response; F http_sdp |
| `rtsp::parse_session_url` | brisk-client src/rtsp.rs | `sessionUrl0` from the launch XML | host | none | P rtsp::tests::parse_session_url_takes_any_text; F http_sdp |
| `StreamRx::on_datagram` (video/audio counters) | brisk-client src/stream.rs | media UDP datagrams | net | tests/live_stream.rs | via parse_video_packet, FrameAssembler, AudioFec (P/F above) |
| `input::parse`, `InputQueue::on_message_from` and the queue | brisk-bridge src/input.rs | browser input frames (len u8 + body) | peer | tests/input_map.rs, gamepad.rs | P prop_input (4 tests: any body, round trip, bounded queue + release-all, final state wins); F bridge_input |
| `read_frame` | brisk-bridge src/input.rs | WebTransport bidi stream | peer | input_map.rs | length is a u8 into a 255-byte buffer; covered by construction |
| browser messages `MSG_WANT_KEY` / `MSG_REFRESH_KEY` | brisk-bridge src/session.rs | WebTransport uni streams (at most MAX_MSG bytes) | peer | wt.rs | exact byte compare only; nothing to fuzz |
| `wt::handshake` (`ConnectRequest::read`, settings), `browser_uni`, `is_webtransport_bi` | brisk-bridge src/wt.rs | HTTP/3 + WebTransport framing | net (any tailnet peer before the token check) | tests/wt.rs | not fuzzed: the parsing is web-transport-proto / quinn (dependencies); Brisk code reads varints and caps sizes |
| hostd: `http::parse` (JSON bodies), `auth` (Tailscale whois JSON, Host header), `steam::parse_vdf`/`parse_acf`, Steam API JSON, `running` (/proc stat, sway tree), `sessions` (JSONL), `prefs`, `stream::parse_page` | brisk-hostd src/* | HTTP API, local files, Steam API | peer / local / net (Steam API over HTTPS) | tests/api.rs etc. | out of scope for this pass (brisk-hostd is being changed in another branch); candidates for the next pass |

## Findings of this pass

1. **Fixed** (fuzz target `input`): `ControlMsg::input` computed `GAMEPAD_BASE + index` before
   `encode_input` rejected the index, so an index >= 240 overflowed u8: a panic in debug builds
   (release wrapped, then returned the error). Fix: encode (which rejects the index) before the
   channel sum; valid indices unchanged.
   Repro: brisk-proto tests/control.rs `control_input_gamepad_index_out_of_range_errors_not_panics`.
   Reachability: the bridge only passes slots 0..4, so not reachable from the browser today.
2. **Fixed** (property test with a counting allocator): the HTTP XML parser stored
   `stack.join("/")` for every text node, so a 64 KiB reply nested 16000 deep, or many text nodes
   under one 30 KB element name, allocated 257 MB and took 20 s (debug). Fix: track the path length
   and keep only text under paths of at most 64 bytes (every path a reply is read by is at most 27);
   results unchanged for any input. Repro: tests/prop_parsers.rs
   `deep_or_long_xml_paths_cost_no_quadratic_memory`. Reachability: any reply Sunshine (or, before
   pairing, anything answering on the host's HTTP port) sends, up to MAX_BODY.
3. **Not changed** (fuzz target `rtsp`): `RtspCipher::seal(&[])` makes a 24-byte frame that `open`
   refuses as `Truncated`. No RTSP message is empty, and accepting it would change what the client
   takes from the wire, so it is left as is.
4. **Fixed after review** (property `final_state_wins`): a key or mouse-button press whose send
   times out is dropped by design (`InputQueue::timed_out`), but the queue kept counting it as
   down, so after a 1 s control stall the page's repeats were swallowed (holding W did nothing)
   until the key was released and pressed again. `timed_out` now forgets the dropped press unless
   a newer event for the same key/button is still queued (then that one keeps it held, so its
   release is not ignored). Repro: brisk-bridge tests/input_map.rs
   `press_dropped_on_timeout_is_forwarded_on_the_next_repeat` (and
   `press_dropped_on_timeout_keeps_a_newer_queued_press`). Pad states are fine: one is dropped
   only when a newer queued state of that pad follows.
5. **Bounded but large**: `FrameAssembler` keeps up to 3 frames x 4 FEC blocks of shards; with
   0% FEC a block may have up to 1023 data shards of up to 4 KiB, about 50 MB in the worst case
   from forged UDP packets. Bounded, so not changed; worth a cap if video ever leaves loopback/LAN.
