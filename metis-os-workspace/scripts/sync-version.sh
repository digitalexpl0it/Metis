#!/usr/bin/env bash
# Sync Cargo workspace version with a GitHub/deb release version.
#
# GitHub tags use a four-part product scheme (v0.1.0.13, v0.1.0.13a) that is
# not valid Cargo SemVer. Mapping:
#   0.1.0.N      →  0.1.N
#   0.1.0.Na     →  0.1.N-a   (letter suffix → pre-release)
#   0.1.0 / 1.2.3 → unchanged (already SemVer)
#
# Usage:
#   ./scripts/sync-version.sh 0.1.0.13
#   ./scripts/sync-version.sh v0.1.0.13a
#   VERSION=0.1.0.14 ./scripts/sync-version.sh
#   ./scripts/sync-version.sh --print 0.1.0.13   # print Cargo version only
#
# Writes [workspace.package].version in metis-os-workspace/Cargo.toml.
# All crates use version.workspace = true.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE="$(cd "$SCRIPT_DIR/.." && pwd)"
ROOT_TOML="$WORKSPACE/Cargo.toml"

PRINT_ONLY=0
if [[ "${1:-}" == "--print" ]]; then
  PRINT_ONLY=1
  shift
fi

RAW="${1:-${VERSION:-}}"
if [[ -z "$RAW" ]]; then
  echo "Usage: $0 [--print] <version>| VERSION=… $0" >&2
  exit 1
fi
RAW="${RAW#v}"

# Map product / GitHub version → Cargo SemVer.
cargo_semver_from_product() {
  local v="$1"
  # Four-part: X.Y.Z.N or X.Y.Z.N + letter pre-release (e.g. 0.1.0.13a)
  if [[ "$v" =~ ^([0-9]+)\.([0-9]+)\.([0-9]+)\.([0-9]+)([A-Za-z][A-Za-z0-9]*)?$ ]]; then
    local major="${BASH_REMATCH[1]}"
    local minor="${BASH_REMATCH[2]}"
    local n="${BASH_REMATCH[4]}"
    local pre="${BASH_REMATCH[5]:-}"
    if [[ -n "$pre" ]]; then
      printf '%s.%s.%s-%s\n' "$major" "$minor" "$n" "$pre"
    else
      printf '%s.%s.%s\n' "$major" "$minor" "$n"
    fi
    return 0
  fi
  # Already SemVer-ish (with optional pre-release / build)
  if [[ "$v" =~ ^[0-9]+\.[0-9]+\.[0-9]+([.-][A-Za-z0-9][A-Za-z0-9.-]*)?$ ]]; then
    printf '%s\n' "$v"
    return 0
  fi
  echo "ERROR: cannot map '$v' to Cargo SemVer" >&2
  return 1
}

CARGO_VER="$(cargo_semver_from_product "$RAW")"

if [[ "$PRINT_ONLY" == "1" ]]; then
  printf '%s\n' "$CARGO_VER"
  exit 0
fi

if [[ ! -f "$ROOT_TOML" ]]; then
  echo "ERROR: missing $ROOT_TOML" >&2
  exit 1
fi

python3 - "$ROOT_TOML" "$CARGO_VER" <<'PY'
import re, sys
path, ver = sys.argv[1], sys.argv[2]
lines = open(path).read().splitlines(keepends=True)
in_wp = False
found = False
for i, line in enumerate(lines):
    if line.startswith("["):
        in_wp = line.startswith("[workspace.package]")
    if in_wp and re.match(r"^version\s*=", line):
        nl = "\n" if line.endswith("\n") else ""
        lines[i] = f'version = "{ver}"{nl}'
        found = True
        break
if not found:
    # Insert after [workspace.package] header.
    for i, line in enumerate(lines):
        if line.startswith("[workspace.package]"):
            nl = "\n" if line.endswith("\n") else "\n"
            lines.insert(i + 1, f'version = "{ver}"\n')
            found = True
            break
if not found:
    sys.exit("ERROR: [workspace.package] section not found")
open(path, "w").write("".join(lines))
print(f'ok: [workspace.package] version = "{ver}"')
PY

printf '==> Cargo workspace version = %s (from product/release %s)\n' "$CARGO_VER" "$RAW"
printf '==> Wrote %s\n' "$ROOT_TOML"
