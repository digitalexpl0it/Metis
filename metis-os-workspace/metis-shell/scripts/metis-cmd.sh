#!/usr/bin/env bash
# Send a runtime command to the running Metis shell.
set -euo pipefail
RUNTIME="${XDG_RUNTIME_DIR:?XDG_RUNTIME_DIR not set — run under a Wayland session (./run-metis.sh --session)}"
CMD_FILE="$RUNTIME/metis/command"
CMD_LOG="$RUNTIME/metis/cmd.log"
mkdir -m 700 -p "$(dirname "$CMD_FILE")"
chmod 700 "$(dirname "$CMD_FILE")" 2>/dev/null || true
if [[ $# -lt 1 ]]; then
    echo "Usage: metis-cmd.sh {close-popovers|reload-bar|reload-dashboard|reload-desktop-widgets|reload-theme|reload-weather|reload-calendars|reload-gaming|optimize-gaming --yes|show-onboarding|screenshot|settings [page]}" >&2
    echo "  settings page: appearance | weather | network | calendars | remote | …" >&2
    echo "  optimize-gaming requires --yes (applies Flatpak --device=all overrides)" >&2
    exit 2
fi

if [[ "$1" == "optimize-gaming" ]]; then
    if [[ "${2:-}" != "--yes" && -z "${METIS_GAMING_OPTIMIZE_YES:-}" ]]; then
        echo "metis-cmd: refusing optimize-gaming without --yes (Flatpak sandbox overrides)." >&2
        echo "  Use Settings → Gaming → Optimize now, or: metis-cmd optimize-gaming --yes" >&2
        exit 2
    fi
    printf '%s\n' "optimize-gaming yes" >"$CMD_FILE"
    chmod 600 "$CMD_FILE" 2>/dev/null || true
    printf '%s %s\n' "$(date -Iseconds)" "optimize-gaming yes" >>"$CMD_LOG"
    exit 0
fi

# Forward the full command line (e.g. "settings network") as a single line.
printf '%s\n' "$*" >"$CMD_FILE"
chmod 600 "$CMD_FILE" 2>/dev/null || true
printf '%s %s\n' "$(date -Iseconds)" "$*" >>"$CMD_LOG"
