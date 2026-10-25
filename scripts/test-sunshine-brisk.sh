#!/bin/bash
# Stubbed test for sunshine-brisk.sh; touches nothing real (systemctl/sudoq/systemd-run/cmake/curl are stubs).
set -u
s=${SB_SCRIPT:-$(cd "$(dirname "$0")" && pwd)/sunshine-brisk.sh} # SB_SCRIPT: run against another copy
t=$(mktemp -d); trap 'rm -rf "$t"' EXIT
mkdir -p "$t/bin" "$t/p/bin" "$t/src/.git"; : > "$t/p/bin/sunshine"; chmod +x "$t/p/bin/sunshine"
printf '#!/bin/sh\necho "cmake $*" >> "%s/log"\ntouch "%s/cmake.marker"\n' "$t" "$t" > "$t/bin/cmake"
for c in sudoq getcap ionice nice; do printf '#!/bin/sh\necho "%s $*" >> "%s/log"\n' "$c" "$t" > "$t/bin/$c"; done
# git: logs; `describe` reports the checkout's tag ($GIT_TAG, default the pinned one)
printf '#!/bin/sh\necho "git $*" >> "%s/log"\ncase "$*" in *describe*) echo "${GIT_TAG:-v2026.914.233613}" ;; esac\nexit 0\n' "$t" > "$t/bin/git"
# systemctl: `show` reports $EXEC (a path) as the running ExecStart
printf '#!/bin/sh\necho "systemctl $*" >> "%s/log"\n[ "$2" = is-active ] && [ -n "${INACTIVE:-}" ] && exit 1\n[ "$2" = show ] && echo "ExecStart={ path=${EXEC:-/usr/bin/sunshine} ; }"\nexit 0\n' "$t" > "$t/bin/systemctl"
# systemd-run: fails if SR_FAIL, else reports the enforced memory.max
printf '#!/bin/sh\necho "systemd-run $*" >> "%s/log"\n[ -z "${SR_FAIL:-}" ] || exit 1\nwhile [ "$1" != -c3 ]; do shift; done; shift\n[ "$1" = sh ] && { echo "${MEMMAX:-9663676416}"; exit 0; }\nexec "$@"\n' "$t" > "$t/bin/systemd-run"
printf '#!/bin/sh\nif [ -n "${NOANSWER:-}" ]; then exit 0; fi\necho "<currentgame>0</currentgame><state>${STATE:-SUNSHINE_SERVER_FREE}</state>"\n' > "$t/bin/curl"
printf '#!/bin/sh\nshift 2; exec "$@"\n' > "$t/bin/ionice"; printf '#!/bin/sh\nshift 2; exec "$@"\n' > "$t/bin/nice"
chmod +x "$t"/bin/*
export PATH=$t/bin:$PATH SB_PREFIX=$t/p SB_DROPIN_DIR=$t/d SB_SRC=$t/src
chk() { grep -qF -- "$1" "$2" || { echo "FAIL: '$1' not in $2"; cat "$2"; exit 1; }; }
no() { ! grep -q -- "$1" "$2" || { echo "FAIL: unexpected '$1'"; cat "$2"; exit 1; }; }
fail() { echo "FAIL: $*"; exit 1; }
L=$t/log; B=$t/p/bin/sunshine
CAP="-p MemoryHigh=7G -p MemoryMax=9G -p MemorySwapMax=0 -p CPUWeight=20 nice -n 19 ionice -c3"

$s install >/dev/null
chk "sudoq run --why KMS capture caps for patched sunshine -- setcap cap_sys_admin,cap_sys_nice+p $B" $L
printf '[Service]\nExecStart=\nExecStart=%%h/.local/opt/sunshine-brisk/bin/sunshine\n' | cmp - "$t/d/brisk.conf" || fail dropin

# enable: reset-failed precedes restart
: > $L; $s enable >/dev/null
chk "systemctl --user restart app-dev.lizardbyte.app.Sunshine.service" $L
[ "$(grep -n reset-failed $L | head -1 | cut -d: -f1)" -lt "$(grep -n ' restart ' $L | head -1 | cut -d: -f1)" ] || fail "reset-failed must precede restart"
# enable no-op when already running brisk
: > $L; EXEC=$B $s enable >/dev/null; no restart $L
# busy / no answer refuse
: > $L; STATE=SUNSHINE_SERVER_BUSY $s enable >/dev/null 2>&1 && fail "busy should refuse"; no restart $L
: > $L; NOANSWER=1 $s enable >/dev/null 2>&1 && fail "no answer should refuse"; no restart $L
: > $L; NOANSWER=1 FORCE=1 $s enable >/dev/null 2>&1 || fail "FORCE=1 should allow"; chk " restart " $L
# rollback: removes dropin, restarts (even with no answer); second call is a no-op
: > $L; NOANSWER=1 $s rollback >/dev/null; [ ! -e "$t/d/brisk.conf" ] || fail "dropin remains"; chk " restart " $L
: > $L; $s rollback >/dev/null; no restart $L; no daemon-reload $L
# rollback must not strand: busy refuses without deleting; conf-gone-but-brisk-running still restarts
printf x > "$t/d/brisk.conf"; : > $L
STATE=SUNSHINE_SERVER_BUSY $s rollback >/dev/null 2>&1 && fail "busy rollback should refuse"; [ -f "$t/d/brisk.conf" ] || fail "refused rollback deleted conf"; no " restart " $L
STATE=SUNSHINE_SERVER_BUSY FORCE=1 $s rollback >/dev/null 2>&1 || fail "FORCE rollback"; chk " restart " $L
printf x > "$t/d/brisk.conf"; $s rollback >/dev/null; [ ! -e "$t/d/brisk.conf" ] || fail "FREE rollback keeps conf"
: > $L; EXEC=$B $s rollback >/dev/null; chk " restart " $L
# enable recovers from a failed (inactive) unit even if ExecStart already brisk
printf x > "$t/d/brisk.conf"; : > $L; EXEC=$B INACTIVE=1 $s enable >/dev/null; chk " restart " $L
$s status | grep -q "serverinfo: answers" || fail status

# build: every cmake step goes through the capped scope, jobs <= 6, default 3
: > $L; $s build >/dev/null 2>&1 || fail "build failed"
for step in "cmake -S $t/src" "cmake --build $t/src/build -j3" "cmake --install $t/src/build"; do chk "$CAP $step" $L; done
SB_JOBS=6 $s build >/dev/null 2>&1 || fail "-j6 should be allowed"
# refusals: systemd-run failing, unenforced cap, bad jobs, brisk binary running
: > $L; rm -f $t/cmake.marker; SR_FAIL=1 $s build >/dev/null 2>&1 && fail "build must refuse when systemd-run fails"
[ "$(grep -c '^systemd-run' $L)" = 1 ] && ! grep '^systemd-run' $L | grep -q cmake || fail "only the cap probe may run before refusal"; [ ! -e $t/cmake.marker ] || fail "cmake ran"
: > $L; rm -f $t/cmake.marker; MEMMAX=max $s build >/dev/null 2>&1 && fail "build must refuse when cap not enforced"
[ ! -e $t/cmake.marker ] && ! grep -q '^cmake' $L || fail "cmake ran despite unenforced cap"
SB_JOBS=20 $s build >/dev/null 2>&1 && fail "-j20 accepted"
EXEC=$B $s build >/dev/null 2>&1 && fail "build must refuse while brisk binary runs"
# a checkout at another tag, or the read-only reference tree: refuse before patching or cmake
: > $L; rm -f $t/cmake.marker; GIT_TAG=v2025.1.1 $s build >/dev/null 2>&1 && fail "build must refuse a checkout at another tag"
no "apply" $L; [ ! -e $t/cmake.marker ] || fail "cmake ran on the wrong base"
: > $L; mkdir -p "$t/h/recon-pinned/Sunshine/.git"
HOME=$t/h SB_SRC=$t/h/recon-pinned/Sunshine $s build >/dev/null 2>&1 && fail "build must refuse ~/recon-pinned"
no "apply" $L; no "^cmake" $L
# bad SB_JOBS must not break rollback/status
SB_JOBS=zz $s status >/dev/null || fail "status broke on bad SB_JOBS"
echo "all passed"
