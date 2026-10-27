#!/usr/bin/env bash
# Copy check: fails when user-facing text (README, docs, the web app's strings) contains a phrase
# from scripts/wording-banned.txt, the stock words that make writing sound machine-made.
# Checked: *.md and web/src/**/*.{svelte,astro,ts}, minus tests, plans, mockups, research, fixtures
# and agent instructions (CLAUDE.md). A real use of a listed phrase goes in scripts/wording-allow.txt
# as `path | text on the line | reason`.
# Usage: scripts/check-wording.sh   (WORDING_ROOT overrides the repo root, for the test)
set -u
root=${WORDING_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}
cd "$root" || exit 2
banned=${WORDING_BANNED:-scripts/wording-banned.txt}
allow=${WORDING_ALLOW:-scripts/wording-allow.txt}
pat=$(grep -v '^[[:space:]]*\(#\|$\)' "$banned" | paste -sd'|')
[ -n "$pat" ] || { echo "no patterns in $banned" >&2; exit 2; }
bad=0
while IFS= read -r f; do
  while IFS= read -r hit; do
    ok=0
    while IFS='|' read -r ap atext _; do
      ap=$(echo "$ap" | xargs); atext=$(echo "$atext" | xargs)
      case $ap in ''|'#'*) continue ;; esac
      [ "$ap" = "$f" ] && [ -n "$atext" ] && [[ $hit == *"$atext"* ]] && ok=1 && break
    done < "$allow"
    [ $ok = 1 ] || { echo "$f:$hit"; bad=1; }
  done < <(grep -nEi -- "$pat" "$f")
done < <(find . -type f \( -name '*.md' -o -path './web/src/*' \( -name '*.svelte' -o -name '*.astro' -o -name '*.ts' \) \) \
  -not -path './node_modules/*' -not -path './web/node_modules/*' -not -path './target/*' -not -path './.git/*' \
  -not -path './docs/plans/*' -not -path './docs/ui-mockups/*' -not -path './research/*' \
  -not -path '*/fixtures/*' -not -path './.superpowers/*' -not -path './.claude/*' -not -name 'CLAUDE.md' \
  -not -name '*.test.ts' -not -name '*.browser.test.ts' -not -name '*.fail.test.ts' | sed 's|^\./||' | sort)
if [ $bad = 1 ]; then
  echo "wording check failed: rewrite the lines above in plain words, or allow a real use in scripts/wording-allow.txt" >&2
  exit 1
fi
echo "wording ok"
