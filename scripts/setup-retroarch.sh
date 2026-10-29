#!/usr/bin/env bash
# Sets RetroArch up for brisk: the Sunshine pad profile (deploy/retroarch) and the retroarch.cfg
# keys in deploy/retroarch/retroarch.cfg.fragment, and creates the saves/states/system folders.
# A key already in retroarch.cfg is left alone unless --force, which replaces its value; a profile
# that differs is left alone unless --force, which first saves the old one as <name>.before-brisk.
# Run it with RetroArch closed: RetroArch rewrites its config on exit.
# Usage: scripts/setup-retroarch.sh [--force] [--dir DIR] [--data DIR]
#   DIR  default ${XDG_CONFIG_HOME:-~/.config}/retroarch
#   DATA default ${BRISK_RETROARCH_DIR:-${BRISK_DATA_DIR:-~/.local/share/brisk}/retroarch} (hostd reads saves there)
set -euo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
src=$here/deploy/retroarch
dir=${XDG_CONFIG_HOME:-$HOME/.config}/retroarch
data=${BRISK_RETROARCH_DIR:-${BRISK_DATA_DIR:-$HOME/.local/share/brisk}/retroarch}
force=0
while [ $# -gt 0 ]; do
  case $1 in
    --force) force=1 ;;
    --dir) [ $# -ge 2 ] || { echo "--dir needs a value" >&2; exit 2; }; dir=$2; shift ;;
    --data) [ $# -ge 2 ] || { echo "--data needs a value" >&2; exit 2; }; data=$2; shift ;;
    *) echo "usage: $0 [--force] [--dir DIR] [--data DIR]" >&2; exit 2 ;;
  esac
  shift
done
mkdir -p "$dir/autoconfig" "$data/saves" "$data/states" "$data/system"

p=Sunshine_X-Box_Series.cfg
f=$dir/autoconfig/$p
if [ ! -e "$f" ]; then
  cp "$src/$p" "$f"; echo "$p: written"
elif cmp -s "$src/$p" "$f"; then
  echo "$p: already up to date"
elif [ "$force" = 1 ]; then
  cp "$f" "$f.before-brisk"; cp "$src/$p" "$f"; echo "$p: replaced, old one saved as $p.before-brisk"
else
  echo "$p: differs, left alone (use --force to replace)"
fi

c=$dir/retroarch.cfg
touch "$c"
# '|' is the sed delimiter: refuse a path that holds one or a backslash or ampersand.
case $data$dir in *[\|\\\&]*) echo "paths with | \\ or & are not supported" >&2; exit 2 ;; esac
grep -v '^#' "$src/retroarch.cfg.fragment" | grep . | sed "s|@DATA@|$data|g; s|@CONFIG@|$dir|g" |
while IFS= read -r line; do
  key=${line%% =*}
  if ! grep -q "^$key *=" "$c"; then
    printf '%s\n' "$line" >> "$c"; echo "retroarch.cfg: added $key"
  elif [ "$force" = 1 ]; then
    awk -v k="$key" -v l="$line" '$0 ~ "^" k " *=" {print l; next} {print}' "$c" > "$c.tmp" && mv "$c.tmp" "$c"
    echo "retroarch.cfg: set $key"
  else
    echo "retroarch.cfg: $key already set"
  fi
done
echo "RetroArch reads these at start: restart it to pick them up."
