#!/usr/bin/env bash
# Send a runtime command to the running Metis shell.
set -euo pipefail
RUNTIME="${XDG_RUNTIME_DIR:?XDG_RUNTIME_DIR not set — run under a Wayland session (./run-metis.sh --session)}"
CMD_FILE="$RUNTIME/metis/command"
CMD_LOG="$RUNTIME/metis/cmd.log"
mkdir -p "$(dirname "$CMD_FILE")"
if [[ $# -lt 1 ]]; then
    echo "Usage: metis-cmd.sh {close-popovers|reload-bar}" >&2
    exit 2
fi
printf '%s\n' "$1" >"$CMD_FILE"
printf '%s %s\n' "$(date -Iseconds)" "$1" >>"$CMD_LOG"
