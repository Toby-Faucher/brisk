#!/bin/sh
# Stand-in for brisk-bridge in tests. Logs "start <pid> <argv>" and "term <pid>" to $FAKE_BRIDGE_LOG.
# FAKE_BRIDGE_MODE: early_exit (dies before the page line), quit (exits 0.3 s after tap),
# exit_after_page (exits 0 right after the page line), slow_term (takes 1 s to exit after SIGTERM, logging "term" when it does), else runs until SIGTERM.
# On exit it prints a real bridge end-of-session report (bridge-report.txt without the hostd prefix).
log=${FAKE_BRIDGE_LOG:?}
report() { cut -f2- "$(dirname "$0")/bridge-report.txt"; }
echo "start $$ $*" >> "$log"
trap 'report; echo "term $$" >> "$log"; exit 0' TERM
[ "$FAKE_BRIDGE_MODE" = slow_term ] && trap 'sleep 1; echo "term $$" >> "$log"; exit 0' TERM
if [ "$FAKE_BRIDGE_MODE" = early_exit ]; then
  echo "error: bind: address in use" >&2
  exit 1
fi
listen=
while [ $# -gt 0 ]; do
  [ "$1" = --listen ] && listen=$2
  shift
done
printf 'listening\t%s (udp)\n' "$listen"
printf 'page\thttp://localhost:8443/?wt=https://%s/tok%s&hash=ab12cd\n' "$listen" "$$"
[ "$FAKE_BRIDGE_MODE" = exit_after_page ] && exit 0
sleep 0.3 >/dev/null
printf 'tap\tconnected at 0.3s\n'
if [ "$FAKE_BRIDGE_MODE" = quit ]; then
  sleep 0.3 >/dev/null
  report; exit 0
fi
while :; do sleep 0.1 >/dev/null & wait $!; done
