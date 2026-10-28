#!/usr/bin/env bash
# Installs the Dolphin gamepad profile (deploy/dolphin) for the streamed Sunshine pad. Safe to rerun.
# A missing file is written. A file that differs is left alone unless --force, which first saves
# the old one as <name>.before-brisk. Dolphin.ini only gets BackgroundInput and ConfirmStop added, each if absent.
# Usage: scripts/setup-dolphin.sh [--force] [--dir DIR]   (DIR default: ${XDG_CONFIG_HOME:-~/.config}/dolphin-emu)
set -euo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
src=$here/deploy/dolphin
dir=${XDG_CONFIG_HOME:-$HOME/.config}/dolphin-emu
force=0
while [ $# -gt 0 ]; do
  case $1 in
    --force) force=1 ;;
    --dir) [ $# -ge 2 ] || { echo "--dir needs a value" >&2; exit 2; }; dir=$2; shift ;;
    *) echo "usage: $0 [--force] [--dir DIR]" >&2; exit 2 ;;
  esac
  shift
done
mkdir -p "$dir"

f=$dir/WiimoteNew.ini
if [ ! -e "$f" ]; then
  cp "$src/WiimoteNew.ini" "$f"; echo "WiimoteNew.ini: written"
elif cmp -s "$src/WiimoteNew.ini" "$f"; then
  echo "WiimoteNew.ini: already up to date"
elif [ "$force" = 1 ]; then
  cp "$f" "$f.before-brisk"; cp "$src/WiimoteNew.ini" "$f"
  echo "WiimoteNew.ini: replaced, old one saved as WiimoteNew.ini.before-brisk"
else
  echo "WiimoteNew.ini: differs, left alone (use --force to replace)"
fi

d=$dir/Dolphin.ini
# add_key SECTION KEY: adds the fragment's "KEY = value" under [SECTION] if KEY is absent anywhere.
add_key() {
  local sec=$1 name=$2 line
  line=$(grep "^$name" "$src/Dolphin.ini.fragment")
  if [ ! -e "$d" ]; then
    printf '[%s]\n%s\n' "$sec" "$line" > "$d"; echo "Dolphin.ini: written with $name"
  elif grep -q "^$name[ =]" "$d"; then
    echo "Dolphin.ini: $name already set"
  elif grep -q "^\[$sec\]" "$d"; then
    awk -v s="[$sec]" -v k="$line" '{print} $0==s {print k}' "$d" > "$d.tmp" && mv "$d.tmp" "$d"
    echo "Dolphin.ini: added $name to [$sec]"
  else
    printf '\n[%s]\n%s\n' "$sec" "$line" >> "$d"; echo "Dolphin.ini: added [$sec] with $name"
  fi
}
add_key Input BackgroundInput
# Without this, closing the window asks "stop the current emulation?" and nobody sees it on a stream.
add_key Interface ConfirmStop
echo "Dolphin reads these at start: restart it to pick them up."
