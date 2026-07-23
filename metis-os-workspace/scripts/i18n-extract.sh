#!/usr/bin/env bash
# Extract gettext strings from metis-shell / metis-settings into the English .po template.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PO_DIR="$ROOT/assets/locale/en/LC_MESSAGES"
mkdir -p "$PO_DIR"
POT="$PO_DIR/metis.pot"

if ! command -v xgettext >/dev/null 2>&1; then
  echo "xgettext not found — install gettext package" >&2
  exit 1
fi

mapfile -t FILES < <(find "$ROOT/metis-shell/src" "$ROOT/metis-settings/src" \
  -name '*.rs' -type f | sort)

xgettext \
  --language=C \
  --keyword=tr \
  --keyword=trn:1,2 \
  --from-code=UTF-8 \
  --add-comments=TRANSLATORS \
  --package-name=metis \
  --package-version=0.1 \
  --output="$POT" \
  "${FILES[@]}"

# Merge into en/metis.po (identity catalog).
if [[ -f "$PO_DIR/metis.po" ]]; then
  msgmerge --update --backup=none "$PO_DIR/metis.po" "$POT"
else
  msginit --no-translator --locale=en --input="$POT" --output-file="$PO_DIR/metis.po"
fi

echo "Updated $PO_DIR/metis.po"
echo "Note: Fluent compositor strings live in assets/locale/*/compositor/metis.ftl (edit by hand)."
