# shellcheck shell=bash disable=SC2034 # sets variables for the test that sources it
# Shared setup for the stubbed scripts/test-*.sh (sourced; check.sh runs only test-*.sh, so not
# this). Makes $t (a temp dir, removed on exit) with $t/bin first on PATH, and $L, the log every
# stub appends its call to. Setup steps stop the test on failure (set -e); the test turns it off
# with set +e before its cases.
set -eu
t=$(mktemp -d); trap 'rm -rf "$t"' EXIT
L=$t/log
mkdir -p "$t/bin"; : > "$L"
export PATH=$t/bin:$PATH
bad=0

# fail MSG: report it with the line of the failing case and the stub log, then keep going; the
# test ends with `exit $bad`. BASH_LINENO[-2] is the test script's own line, also from a helper.
fail() { echo "FAIL (line ${BASH_LINENO[-2]}): $*"; sed 's/^/    log: /' "$L" 2>/dev/null; bad=1; }

# stub NAME [BODY]: a bash stub on PATH that logs "NAME args" to $L, then runs BODY.
stub() { printf '#!/bin/bash\necho "%s $*" >> "%s"\n%s\n' "$1" "$L" "${2:-}" > "$t/bin/$1"; chmod +x "$t/bin/$1"; }

# The memory cap's tools. nice and ionice pass the command through (not logged). systemd-run
# fails when SR_FAIL is set, answers cap_check's memory.max probe with MEMMAX (default 9G, the
# enforced cap), else runs the command after the options.
stub_cap() {
  printf '#!/bin/sh\nshift 2; exec "$@"\n' > "$t/bin/nice"; cp "$t/bin/nice" "$t/bin/ionice"; chmod +x "$t/bin/nice" "$t/bin/ionice"
  # shellcheck disable=SC2016 # expanded by the stub
  stub systemd-run '[ -z "${SR_FAIL:-}" ] || exit 1
while [ "$1" != -c3 ]; do shift; done; shift
[ "$1" = sh ] && { echo "${MEMMAX:-9663676416}"; exit 0; }
exec "$@"'
}

# curl as Sunshine's serverinfo: GAME (default 0) and STATE (default FREE); NOANSWER=1: no answer.
stub_serverinfo() {
  # shellcheck disable=SC2016 # expanded by the stub
  printf '%s\n' '#!/bin/sh' '[ -n "${NOANSWER:-}" ] && exit 0' \
    'echo "<currentgame>${GAME:-0}</currentgame><state>${STATE:-SUNSHINE_SERVER_FREE}</state>"' > "$t/bin/curl"
  chmod +x "$t/bin/curl"
}

# fake_hostd SECS: a fake brisk-up hostd in the background (a sleep whose process name is
# brisk-hostd); returns once it has exec'd (its name is the parent's until then). Its pid is $!.
fake_hostd() {
  [ -e "$t/brisk-hostd" ] || ln -s "$(command -v sleep)" "$t/brisk-hostd"
  "$t/brisk-hostd" "$1" &
  for _ in $(seq 100); do [ "$(cat "/proc/$!/comm" 2>/dev/null)" = brisk-hostd ] && return 0; sleep 0.05; done
  echo "fake hostd $! did not start"
}
