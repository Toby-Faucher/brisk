#!/bin/sh
# Stand-in for brisk-bridge in tests. Logs "start <pid> <argv>" and "term <pid>" to $FAKE_BRIDGE_LOG.
# FAKE_BRIDGE_MODE: early_exit (dies before the page line), quit (exits 0.3 s after tap), else runs
# until SIGTERM.
log=${FAKE_BRIDGE_LOG:?}
echo "start $$ $*" >> "$log"
trap 'echo "term $$" >> "$log"; exit 0' TERM
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
sleep 0.3 >/dev/null
printf 'tap\tconnected at 0.3s\n'
if [ "$FAKE_BRIDGE_MODE" = quit ]; then
  sleep 0.3 >/dev/null
  echo "session	ended" ; exit 0
fi
while :; do sleep 0.1 >/dev/null & wait $!; done
