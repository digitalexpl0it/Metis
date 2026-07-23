#!/usr/bin/env bash
# Compile .po catalogs to .mo for gettext runtime.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
LOCALE_ROOT="$ROOT/assets/locale"

if ! command -v msgfmt >/dev/null 2>&1; then
  echo "msgfmt not found — install gettext package" >&2
  exit 1
fi

count=0
while IFS= read -r -d '' po; do
  mo="${po%.po}.mo"
  msgfmt -o "$mo" "$po"
  echo "compiled $mo"
  count=$((count + 1))
done < <(find "$LOCALE_ROOT" -path '*/LC_MESSAGES/*.po' -print0)

echo "Compiled $count catalog(s)."
