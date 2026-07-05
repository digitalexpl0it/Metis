#!/usr/bin/env bash
# Send a runtime command to the running Metis shell.
set -euo pipefail
RUNTIME="${XDG_RUNTIME_DIR:?XDG_RUNTIME_DIR not set — run under a Wayland session (./run-metis.sh --session)}"
CMD_FILE="$RUNTIME/metis/command"
CMD_LOG="$RUNTIME/metis/cmd.log"
mkdir -p "$(dirname "$CMD_FILE")"
if [[ $# -lt 1 ]]; then
    echo "Usage: metis-cmd.sh {close-popovers|reload-bar|reload-theme|reload-weather|reload-calendars|show-onboarding|settings [page]}" >&2
    echo "  settings page: appearance | weather | network | calendars | remote | …" >&2
    exit 2
fi
# Forward the full command line (e.g. "settings network") as a single line.
printf '%s\n' "$*" >"$CMD_FILE"
printf '%s %s\n' "$(date -Iseconds)" "$*" >>"$CMD_LOG"
