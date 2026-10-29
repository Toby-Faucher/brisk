# Sunshine brisk tap patch

Adds an opt-in "tap": a session whose ANNOUNCE SDP contains `a=x-brisk.tap:1` gets its encoded
frames sent to a unix SEQPACKET socket (44-byte LE header + Annex B) instead of RTP/FEC/AES.
Design: `research/m2-tap-design.md` sections 1 and 2.

- Base tag: `v2026.914.233613`
- Base commit: `63d35f702ee9e362e43263742981836ec0710384`
- Touches: `src/stream.h`, `src/stream.cpp`, `src/rtsp.cpp` (98 added lines)

## Apply

The patch path is relative to the brisk checkout, not to the Sunshine tree, so set `BRISK_REPO` first.

    BRISK_REPO=/path/to/brisk
    git clone --branch v2026.914.233613 https://github.com/LizardByte/Sunshine ~/build/sunshine-brisk
    cd ~/build/sunshine-brisk
    git rev-parse HEAD            # must start with 63d35f7
    git submodule update --init --recursive
    git apply --check "$BRISK_REPO/patches/sunshine/0001-brisk-tap.patch"
    git apply "$BRISK_REPO/patches/sunshine/0001-brisk-tap.patch"

`scripts/sunshine-brisk.sh build` does all of this for you.

## License

The patch changes Sunshine source, so it is a derivative work of Sunshine and is licensed under
Sunshine's GPL-3.0. The rest of brisk is MIT. The bridge does not link Sunshine: it only reads the socket.

## Behaviour

- Socket path: `$BRISK_TAP_SOCK`, else `$XDG_RUNTIME_DIR/brisk/tap.sock`. The bridge listens.
- Sunshine connects after the first video ping; if that fails it logs a warning and the session uses RTP.
- Header flags: b0 idr, b1 after_rfi, b2 no_capture_ts, b3 gap (this is the recovery IDR after drops).
- Send is `MSG_DONTWAIT|MSG_NOSIGNAL`. `EAGAIN`: drop frame, set `tap_need_idr`, raise `idr_events`,
  count it; non-IDR frames are skipped until an IDR passes. Any other error (EPIPE): `session::stop`.
- `session::join` calls `shutdown(SHUT_RDWR)` on the socket (bridge sees EOF); the fd is closed in `~session_t`, so the shared broadcast thread never sees a closed or reused fd.
- Linux-only (AF_UNIX SOCK_SEQPACKET, MSG_NOSIGNAL).

## [U] list (from the design note, still unverified)

- bawn's VAAPI SPS has a VUI or not (log the first SPS).
- AF_UNIX SEQPACKET send is atomic (whole message or EAGAIN); exact SO_SNDBUF limit formula;
  `net.unix.max_dgram_qlen` applying to seqpacket; IDR sizes of 100-500 KB fit.
- Extra kernel copy costs tens of microseconds.
- libstdc++ `steady_clock` is `CLOCK_MONOTONIC`.
- Impact of the shared broadcast thread when a normal Moonlight client streams at the same time.
- ffmpeg emits in-band SPS/PPS as Moonlight expects; build with gcc 16 needs `-DBUILD_WERROR=OFF`.
- This patch has not been fully compiled; only the two new functions were syntax-checked against stubs.
