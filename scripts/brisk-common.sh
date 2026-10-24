# Shared by brisk-up.sh and brisk-down.sh (sourced; needs $RUN).
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
