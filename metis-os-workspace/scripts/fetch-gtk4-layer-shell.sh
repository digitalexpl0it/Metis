#!/usr/bin/env bash
# Fetch gtk4-layer-shell sources into DEST (default: /tmp/gtk4-layer-shell).
# Prefers a GitHub release tarball with retries (more resilient than git clone
# when DNS/network blips), then falls back to shallow git clone.
#
# Usage:
#   GTK4_LAYER_SHELL_TAG=v1.3.0 ./scripts/fetch-gtk4-layer-shell.sh [/tmp/gtk4-layer-shell]
#
# Env:
#   GTK4_LAYER_SHELL_TAG  tag/branch (default v1.3.0)
#   FETCH_RETRIES         attempts per method (default 5)
#   FETCH_RETRY_SLEEP     seconds between attempts (default 5)

set -euo pipefail

TAG="${GTK4_LAYER_SHELL_TAG:-v1.3.0}"
DEST="${1:-/tmp/gtk4-layer-shell}"
RETRIES="${FETCH_RETRIES:-5}"
SLEEP_SECS="${FETCH_RETRY_SLEEP:-5}"
REPO="https://github.com/wmww/gtk4-layer-shell"
ARCHIVE_URL="${REPO}/archive/refs/tags/${TAG}.tar.gz"

rm -rf "$DEST"
mkdir -p "$(dirname "$DEST")"

fetch_tarball() {
  local tmp tarball
  tmp="$(mktemp -d)"
  tarball="$tmp/gtk4-layer-shell.tar.gz"
  echo "==> Downloading $ARCHIVE_URL"
  if ! curl -fsSL --retry 3 --retry-delay 2 --retry-connrefused \
    --connect-timeout 20 --max-time 120 \
    -o "$tarball" "$ARCHIVE_URL"; then
    rm -rf "$tmp"
    return 1
  fi
  if [[ ! -s "$tarball" ]]; then
    echo "ERROR: empty download" >&2
    rm -rf "$tmp"
    return 1
  fi
  mkdir -p "$tmp/extract"
  if ! tar -xzf "$tarball" -C "$tmp/extract"; then
    rm -rf "$tmp"
    return 1
  fi
  # Archive extracts to gtk4-layer-shell-<tag-without-v>/ or gtk4-layer-shell-<tag>/
  local extracted
  extracted="$(find "$tmp/extract" -mindepth 1 -maxdepth 1 -type d | head -n1)"
  if [[ -z "$extracted" ]]; then
    echo "ERROR: empty archive extract" >&2
    rm -rf "$tmp"
    return 1
  fi
  mv "$extracted" "$DEST"
  rm -rf "$tmp"
}

fetch_git() {
  echo "==> git clone --depth 1 --branch $TAG"
  git clone --depth 1 --branch "$TAG" "$REPO.git" "$DEST"
}

attempt() {
  local label="$1"
  shift
  local i=1
  while (( i <= RETRIES )); do
    echo "==> $label (attempt $i/$RETRIES)"
    if "$@"; then
      return 0
    fi
    rm -rf "$DEST"
    if (( i == RETRIES )); then
      break
    fi
    echo "==> retry in ${SLEEP_SECS}s…"
    sleep "$SLEEP_SECS"
    i=$((i + 1))
  done
  return 1
}

if command -v curl >/dev/null 2>&1; then
  if attempt "tarball" fetch_tarball; then
    echo "==> gtk4-layer-shell sources ready at $DEST"
    exit 0
  fi
  echo "==> tarball fetch failed; trying git clone"
fi

if attempt "git clone" fetch_git; then
  echo "==> gtk4-layer-shell sources ready at $DEST"
  exit 0
fi

echo "ERROR: could not fetch gtk4-layer-shell ($TAG) from GitHub" >&2
exit 1
