#!/bin/sh
# M0a: path verdict (direct/DERP) + RTT min/median/p99 to a host.
# Tailnet name -> tailscale status/ping (-c 20). IPv4 literal that is not a peer -> plain ping -c 100 -i 0.2.
# Test hooks: TS_STATUS_JSON / TS_PING_OUT / PING_OUT point at fixture files instead of running the tools.
set -eu
host=${1:?usage: m0-netpath.sh <tailnet-host|lan-ip>}
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT

stats='
import sys,re
v=sorted(float(x) for x in sys.stdin.read().split())
if not v: sys.exit("no RTT samples")
n=len(v); med=(v[n//2] if n%2 else (v[n//2-1]+v[n//2])/2)
p99=v[min(n-1,-(-99*n//100)-1)]  # nearest-rank
print(f"samples={n} min={v[0]:.3f}ms median={med:.3f}ms p99={p99:.3f}ms max={v[-1]:.3f}ms")'

# Peer lookup in tailscale status (match HostName or DNSName label; Self counts too).
if [ -n "${TS_STATUS_JSON:-}" ]; then cp "$TS_STATUS_JSON" "$tmp/st"; else tailscale status --json > "$tmp/st" 2>/dev/null || : > "$tmp/st"; fi
verdict=$(python3 -I -c '
import json,sys
h=sys.argv[2].lower().rstrip(".")
try: d=json.load(open(sys.argv[1]))
except Exception: print("NOTAILSCALE"); sys.exit()
nodes=list((d.get("Peer") or {}).values())+[d.get("Self") or {}]
for p in nodes:
    names={(p.get("HostName") or "").lower(),(p.get("DNSName") or "").lower().rstrip("."),(p.get("DNSName") or "").lower().split(".")[0]}|set(a.lower() for a in p.get("TailscaleIPs") or [])
    if h in names:
        if p is d.get("Self"): print("SELF"); break
        cur,rel=p.get("CurAddr") or "",p.get("Relay") or ""
        print("direct CurAddr="+cur if cur else ("DERP Relay="+rel if rel else "idle (no CurAddr, no Relay; ping to establish)"))
        break
else: print("NOTPEER")
' "$tmp/st" "$host")

case $verdict in
  NOTAILSCALE|NOTPEER)
    echo "tailscale: $host is not a tailnet peer -> plain ping (LAN case)"
    if [ -n "${PING_OUT:-}" ]; then cp "$PING_OUT" "$tmp/p"; else ping -c 100 -i 0.2 "$host" > "$tmp/p"; fi
    sed -n 's/.*time=\([0-9.]*\) *ms.*/\1/p' "$tmp/p" | python3 -I -c "$stats"
    grep -E 'packet loss' "$tmp/p" || true ;;
  SELF) echo "$host is this node; tailscale ping cannot measure self. Run from the client machine." >&2; exit 2 ;;
  *)
    echo "status: $verdict"
    if [ -n "${TS_PING_OUT:-}" ]; then cp "$TS_PING_OUT" "$tmp/p"; else tailscale ping --until-direct=false -c 20 "$host" > "$tmp/p" || true; fi
    # Per-reply path: "via IP:port" (direct) or "via DERP(region)"
    sed -n 's/^pong from .* via \(.*\) in .*/path: \1/p' "$tmp/p" | sort | uniq -c
    sed -n 's/^pong from .* in \([0-9.]*\)ms$/\1/p' "$tmp/p" | python3 -I -c "$stats"
    echo "note: tailscale ping reports whole ms; resolution is coarse" ;;
esac
