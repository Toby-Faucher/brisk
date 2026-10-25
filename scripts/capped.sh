# Resource cap shared by sunshine-brisk.sh and brisk-up.sh (sourced): an OOM kills the build, not
# the box. bawn hung once on an uncapped build.
capped() {
  systemd-run --user --scope -q -p MemoryHigh=7G -p MemoryMax=9G -p MemorySwapMax=0 -p CPUWeight=20 \
    nice -n 19 ionice -c3 "$@"
}

# Succeeds only if the kernel really enforces MemoryMax inside the scope (9G = 9663676416);
# otherwise says why on stderr. Callers refuse to build when it fails.
cap_check() {
  local v
  v=$(capped sh -c 'cat /sys/fs/cgroup$(cut -d: -f3 /proc/self/cgroup)/memory.max') \
    || { echo "capped scope failed; refusing to build uncapped" >&2; return 1; }
  [ "$v" = 9663676416 ] || { echo "memory cap not enforced (memory.max='$v'); refusing to build" >&2; return 1; }
}
