#!/usr/bin/env bash
# Installs the Dolphin gamepad profile (deploy/dolphin) for the streamed Sunshine pad. Safe to rerun.
# A missing file is written. A file that differs is left alone unless --force, which first saves
# the old one as <name>.before-brisk. Dolphin.ini only gets the [Input] key added if it is absent.
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
key=$(grep '^BackgroundInput' "$src/Dolphin.ini.fragment")
if [ ! -e "$d" ]; then
  printf '[Input]\n%s\n' "$key" > "$d"; echo "Dolphin.ini: written"
elif grep -q '^BackgroundInput[ =]' "$d"; then
  echo "Dolphin.ini: BackgroundInput already set"
elif grep -q '^\[Input\]' "$d"; then
  awk -v k="$key" '{print} /^\[Input\]/ {print k}' "$d" > "$d.tmp" && mv "$d.tmp" "$d"
  echo "Dolphin.ini: added BackgroundInput to [Input]"
else
  printf '[Input]\n%s\n' "$key" >> "$d"; echo "Dolphin.ini: added [Input] with BackgroundInput"
fi
echo "Dolphin reads these at start: restart it to pick them up."
