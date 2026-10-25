# Shared by brisk-up.sh and brisk-down.sh (sourced; needs $RUN).
# sunshine-brisk.sh's drop-in (same SB_DROPIN_DIR override).
BRISK_DROPIN=${SB_DROPIN_DIR:-$HOME/.config/systemd/user/app-dev.lizardbyte.app.Sunshine.service.d}/brisk.conf
serverinfo_free() {
  curl -s --max-time 5 http://127.0.0.1:47989/serverinfo | grep -q '<state>SUNSHINE_SERVER_FREE</state>'
}
brisk_running() {
  systemctl --user show -p ExecStart app-dev.lizardbyte.app.Sunshine.service | grep -qF sunshine-brisk/bin/sunshine
}
# Sunshine restarts at least 100 s apart (the last one is remembered in $RUN/last-restart).
spaced_restart() {
  local last now wait
  last=$(cat "$RUN/last-restart" 2>/dev/null || echo 0); now=$(date +%s); wait=$((last + 100 - now))
  if [ "$wait" -gt 0 ]; then echo "waiting ${wait}s (Sunshine restarts stay 100 s apart)"; sleep "$wait"; fi
  "$@"
  date +%s > "$RUN/last-restart"
}
# brisk-up's EXIT trap. After Ctrl-C the bridge is still cancelling its Sunshine session: keep
# bridge.pid until that process is gone (up to $BRISK_EXIT_WAIT s, default 90; the cancel retries
# can take 45 s), so brisk-down waits for it instead of restarting Sunshine under it.
bridge_exit_cleanup() {
  local pid
  pid=$(cat "$RUN/bridge.pid" 2>/dev/null || true)
  if [ -n "$pid" ]; then
    for _ in $(seq "${BRISK_EXIT_WAIT:-90}"); do kill -0 "$pid" 2>/dev/null || break; sleep 1; done
    kill -0 "$pid" 2>/dev/null || rm -f "$RUN/bridge.pid"
  fi
  if [ -f "$RUN/page.pid" ]; then kill "$(cat "$RUN/page.pid")" 2>/dev/null || true; rm -f "$RUN/page.pid"; fi
}
