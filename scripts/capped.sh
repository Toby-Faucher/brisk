# shellcheck shell=bash
# Resource cap shared by sunshine-brisk.sh, brisk-up.sh, brisk-service.sh and check.sh (sourced):
# an OOM kills the build, not the box. bawn hung once on an uncapped build.
# BRISK_NO_CAP=1 turns the cap off: CI runners are not bawn and have no user systemd.
if [ -n "${BRISK_NO_CAP:-}" ]; then
  capped() { "$@"; }
  cap_check() { :; }
else
  CAP_MAX_GIB=9 # MemoryMax; cap_check reads it back in bytes
  capped() {
    systemd-run --user --scope -q -p MemoryHigh=7G -p MemoryMax="${CAP_MAX_GIB}G" -p MemorySwapMax=0 -p CPUWeight=20 \
      nice -n 19 ionice -c3 "$@"
  }

  # Succeeds only if the kernel really enforces MemoryMax inside the scope; otherwise says why on
  # stderr. Callers refuse to build when it fails.
  cap_check() {
    local v
    # shellcheck disable=SC2016 # expanded by the sh inside the scope
    v=$(capped sh -c 'cat /sys/fs/cgroup$(cut -d: -f3 /proc/self/cgroup)/memory.max') \
      || { echo "capped scope failed; refusing to build uncapped" >&2; return 1; }
    [ "$v" = $((CAP_MAX_GIB << 30)) ] || { echo "memory cap not enforced (memory.max='$v'); refusing to build" >&2; return 1; }
  }
fi
