#!/usr/bin/env bash
# Launch Metis — Wayland compositor + shell.
#
# Usage (from this directory):
#   ./run-metis.sh --session    # start Metis compositor + shell (full desktop)
#   ./run-metis.sh --session -- -c foot   # session + spawn a client app
#   ./run-metis.sh              # shell only (compositor must already run)
#   ./run-metis.sh --build      # force rebuild before run
#   ./run-metis.sh --release    # optimized binaries
#   ./run-metis.sh --stop       # stop background shell process
#   ./run-metis.sh --verify-grid # compare compositor vs shell grid layouts
#
# Logs:
#   ~/.local/state/metis/logs/metis-YYYYMMDD-HHMMSS.log
#   ~/.local/state/metis/logs/latest.log  -> most recent run

set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE="$(cd "$ROOT/.." && pwd)"
cd "$WORKSPACE"

unset CARGO_TARGET_DIR
export CARGO_TARGET_DIR="$WORKSPACE/target"

WALLPAPER_DEFAULT="$WORKSPACE/assets/wallpapers/default.jpg"
WALLPAPER_CONFIG="${XDG_CONFIG_HOME:-$HOME/.config}/metis/wallpaper.jpg"
if [[ -z "${METIS_WALLPAPER:-}" ]] && [[ -z "${METIS_NO_WALLPAPER:-}" ]]; then
    if [[ -f "$WALLPAPER_CONFIG" ]]; then
        export METIS_WALLPAPER="$WALLPAPER_CONFIG"
    elif [[ -f "$WALLPAPER_DEFAULT" ]]; then
        export METIS_WALLPAPER="$WALLPAPER_DEFAULT"
    fi
fi

LOG_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/metis/logs"
PID_FILE="${XDG_STATE_HOME:-$HOME/.local/state}/metis/metis.pid"
mkdir -p "$LOG_DIR" "$(dirname "$PID_FILE")"

FORCE_BUILD=0
SESSION=0
FOREGROUND=0
DO_STOP=0
DO_VERIFY=0
DO_VERIFY_GRID=0
PROFILE="dev"
COMP_ARGS=()

while [[ $# -gt 0 ]]; do
    case "$1" in
        --)
            shift
            COMP_ARGS=("$@")
            break
            ;;
        --build) FORCE_BUILD=1 ;;
        --release) PROFILE="release" ;;
        --session) SESSION=1 ;;
        --foreground) FOREGROUND=1 ;;
        --stop) DO_STOP=1 ;;
        --verify) DO_VERIFY=1 ;;
        --verify-grid) DO_VERIFY_GRID=1 ;;
        -h|--help)
            sed -n '2,23p' "$0"
            exit 0
            ;;
        *)
            echo "Unknown option: $1 (try --help)" >&2
            exit 2
            ;;
    esac
    shift
done

log() {
    printf '[%s] %s\n' "$(date '+%H:%M:%S')" "$*"
}

run_section() {
    log "--- $* ---"
}

verify_keybind_chain() {
    local ok=0
    local cmd_script="$ROOT/scripts/metis-cmd.sh"
    local runtime="${XDG_RUNTIME_DIR:-}"

    run_section "Metis compositor + shell verification"
    log "Run this while Metis is active (./run-metis.sh --session or shell attached to compositor)."

    if [[ -z "$runtime" ]]; then
        log "FAIL: XDG_RUNTIME_DIR is not set."
        ok=1
    else
        log "OK:   XDG_RUNTIME_DIR=$runtime"
    fi

    if [[ ! -x "$cmd_script" ]]; then
        log "FAIL: $cmd_script missing or not executable (run ./run-metis.sh once to install)."
        ok=1
    else
        log "OK:   metis-cmd.sh is executable"
    fi

    if [[ -S "${XDG_RUNTIME_DIR:-/tmp}/metis/compositor.sock" ]]; then
        log "OK:   Metis compositor IPC socket present"
    else
        log "FAIL: compositor socket missing — start with ./run-metis.sh --session"
        ok=1
    fi

    if [[ -f "$PID_FILE" ]]; then
        local pid
        pid="$(cat "$PID_FILE")"
        if kill -0 "$pid" 2>/dev/null; then
            log "OK:   Metis daemon running (PID $pid)"
        else
            log "FAIL: Metis pid file exists but process $pid is dead — run ./run-metis.sh --stop && ./run-metis.sh"
            ok=1
        fi
    else
        log "FAIL: Metis is not running — Super+Space has nothing to talk to. Run ./run-metis.sh"
        ok=1
    fi

    if [[ -n "$runtime" && -x "$cmd_script" && -f "$PID_FILE" ]]; then
        local pid
        pid="$(cat "$PID_FILE")"
        if kill -0 "$pid" 2>/dev/null; then
            run_section "IPC smoke test"
            rm -f "$runtime/metis/command"
            if bash "$cmd_script" close-popovers; then
                sleep 0.15
                if [[ ! -f "$runtime/metis/command" ]]; then
                    log "OK:   metis-cmd close-popovers was consumed by the shell"
                else
                    log "FAIL: command file still present — shell is not polling $runtime/metis/command"
                    ok=1
                fi
            else
                log "FAIL: metis-cmd.sh close-popovers exited with error"
                ok=1
            fi
        fi
    fi

    echo
    if [[ "$ok" -eq 0 ]]; then
        log "All checks passed. Runtime commands reach the edge bar."
    else
        log "Some checks failed. Fix the FAIL lines above, then: ./run-metis.sh --stop && ./run-metis.sh --build"
    fi
    return "$ok"
}

if [[ "$DO_STOP" -eq 1 ]]; then
    if [[ -f "$PID_FILE" ]]; then
        pid="$(cat "$PID_FILE")"
        if kill -0 "$pid" 2>/dev/null; then
            kill "$pid" && echo "Stopped Metis (PID $pid)."
        else
            echo "Metis not running (stale PID $pid)."
        fi
        rm -f "$PID_FILE"
    else
        echo "Metis is not running (no pid file)."
    fi
    exit 0
fi

verify_grid_layout() {
    local ok=0
    local runtime="${XDG_RUNTIME_DIR:-}"
    local socket="${runtime}/metis/compositor.sock"
    local shell_layout
    shell_layout="$(python3 - <<'PY'
import json, os
from pathlib import Path
home = Path(os.environ.get("HOME", ""))
for path in [
    home / ".config/metis/desk.json",
]:
    if path.exists():
        data = json.loads(path.read_text())
        ids = sorted(t["id"] for t in data.get("tiles", []))
        print(json.dumps(ids))
        break
else:
    print("[]")
PY
)"

    run_section "Grid layout verification"
    if [[ ! -S "$socket" ]]; then
        log "FAIL: compositor socket missing — start with ./run-metis.sh --session"
        return 1
    fi

    local comp_layout
    comp_layout="$(python3 - <<'PY'
import json, socket, os
path = os.path.join(os.environ["XDG_RUNTIME_DIR"], "metis/compositor.sock")
payload = json.dumps({"cmd": "get_layout"}) + "\n"
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.connect(path)
s.sendall(payload.encode())
data = s.recv(65536).decode()
line = data.strip().splitlines()[0]
evt = json.loads(line)
ids = sorted(t["id"] for t in evt.get("layout", {}).get("tiles", []))
print(json.dumps(ids))
PY
)" || true

    if [[ -z "$comp_layout" ]]; then
        log "FAIL: could not read layout from compositor (GetLayout IPC)"
        ok=1
    else
        log "Shell tile ids:      $shell_layout"
        log "Compositor tile ids: $comp_layout"
        if [[ "$shell_layout" == "$comp_layout" ]]; then
            log "OK:   compositor and shell tile lists match"
        else
            log "FAIL: compositor/shell tile lists diverge"
            ok=1
        fi
    fi

    if [[ "$comp_layout" == *"app-"* ]]; then
        log "OK:   compositor layout contains app-* tile(s)"
    else
        log "WARN: no app-* tiles in compositor layout (launch foot to verify)"
    fi

    return "$ok"
}

if [[ "$DO_VERIFY_GRID" -eq 1 ]]; then
    verify_grid_layout
    exit $?
fi

if [[ "$DO_VERIFY" -eq 1 ]]; then
    verify_keybind_chain
    exit $?
fi

STAMP="$(date +%Y%m%d-%H%M%S)"
LOG_FILE="$LOG_DIR/metis-$STAMP.log"
ln -sfn "$LOG_FILE" "$LOG_DIR/latest.log"

# From here on, log() output is also tee'd to LOG_FILE via the block at end of script.
log_tee() {
    printf '[%s] %s\n' "$(date '+%H:%M:%S')" "$*"
}

log() {
    log_tee "$@"
}

run_section() {
    log "--- $* ---"
}

binary_needs_rebuild() {
    local bin="$1"
    [[ ! -x "$bin" ]] && return 0

    local workspace="$WORKSPACE"
    local newest_src
    newest_src="$(find "$workspace/metis-compositor" "$workspace/metis-shell" "$workspace/metis-grid" "$workspace/metis-protocol" \
        "$workspace/metis-config" "$workspace/metis-secrets" "$workspace/metis-settings" \
        -name '*.rs' -newer "$bin" 2>/dev/null | head -1)"
    if [[ -n "$newest_src" ]]; then
        log "Source changed since last build ($newest_src) — rebuild required."
        return 0
    fi

    local interp
    interp="$(readelf -l "$bin" 2>/dev/null | awk '/Requesting program interpreter/{print $NF}' | tr -d '[]')"
    if [[ -n "$interp" && "$interp" == /nix/store/* && ! -f "$interp" ]]; then
        log "Stale binary: linked against Nix glibc ($interp) — rebuild required."
        return 0
    fi

    if ! "$bin" --help >/dev/null 2>&1; then
        # Binary may not support --help; try a dry run via file check only
        if [[ -n "$interp" && ! -f "$interp" ]]; then
            log "Stale binary: interpreter missing ($interp) — rebuild required."
            return 0
        fi
    fi

    return 1
}

check_build_deps() {
    local missing=0
    for pc in gtk4 gtk4-layer-shell-0; do
        if pkg-config --exists "$pc" 2>/dev/null; then
            log "pkg-config $pc: $(pkg-config --modversion "$pc")"
        else
            log "ERROR: pkg-config '$pc' not found."
            missing=1
        fi
    done

    if [[ "$missing" -eq 1 ]]; then
        log ""
        log "Install build dependencies, then re-run:"
        log "  sudo apt install -y libgtk-4-dev libgraphene-1.0-dev pkg-config"
        log "  # gtk4-layer-shell: see docs/UBUNTU_DEV.md (build from source on 24.04)"
        log "  export PKG_CONFIG_PATH=/usr/local/lib/x86_64-linux-gnu/pkgconfig:\$PKG_CONFIG_PATH"
        log "  ./run-metis.sh --build"
        return 1
    fi
    return 0
}

# --- environment -----------------------------------------------------------

if [[ -f "$HOME/.cargo/env" ]]; then
    # shellcheck source=/dev/null
    source "$HOME/.cargo/env"
fi

# gtk4-layer-shell from source (common on Ubuntu 24.04)
for pc_dir in \
    /usr/local/lib/x86_64-linux-gnu/pkgconfig \
    /usr/local/lib/pkgconfig \
    /usr/lib/x86_64-linux-gnu/pkgconfig; do
    if [[ -d "$pc_dir" ]]; then
        PKG_CONFIG_PATH="${pc_dir}${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
    fi
done
export PKG_CONFIG_PATH

export RUST_BACKTRACE=1
export RUST_LOG="${RUST_LOG:-metis_shell=info,metis_compositor=info,warn}"

# --- preflight -------------------------------------------------------------

{
    run_section "Metis Shell launch"
    log "Project:  $ROOT"
    log "Log file: $LOG_FILE"
    log "Profile:  $PROFILE"
    echo

    run_section "Session"
    if [[ "$SESSION" -eq 1 ]]; then
        # Nested dev sessions default wallpaper + briefing OFF. Distinguish an
        # explicit empty value (METIS_NO_WALLPAPER=, meaning "enable") from an
        # unset var ("use the session default of disabled"). Using ${VAR-...}
        # (no colon) treats an explicit empty value as set.
        if [[ -z "${METIS_NO_WALLPAPER+set}" ]]; then
            export METIS_NO_WALLPAPER=1   # unset → disable
            unset METIS_WALLPAPER
        elif [[ -z "$METIS_NO_WALLPAPER" ]]; then
            unset METIS_NO_WALLPAPER       # explicit empty → enable
        else
            unset METIS_WALLPAPER          # non-empty → disable
        fi

        if [[ -z "${METIS_NO_BRIEFING+set}" ]]; then
            export METIS_NO_BRIEFING=1     # unset → disable
        elif [[ -z "$METIS_NO_BRIEFING" ]]; then
            unset METIS_NO_BRIEFING        # explicit empty → enable
        fi
    fi
    log "XDG_SESSION_TYPE=${XDG_SESSION_TYPE:-unset}"
    log "WAYLAND_DISPLAY=${WAYLAND_DISPLAY:-unset}"
    log "XDG_CURRENT_DESKTOP=${XDG_CURRENT_DESKTOP:-unset}"
    log "DESKTOP_SESSION=${DESKTOP_SESSION:-unset}"
    if [[ -n "${METIS_WALLPAPER:-}" ]]; then
        log "Wallpaper: $METIS_WALLPAPER"
    elif [[ -n "${METIS_NO_WALLPAPER:-}" ]]; then
        log "Wallpaper: disabled (METIS_NO_WALLPAPER)"
    fi

    if [[ "$SESSION" -eq 0 ]] && [[ -z "${WAYLAND_DISPLAY:-}" ]]; then
        log "ERROR: WAYLAND_DISPLAY is not set."
        log "Start the full session: ./run-metis.sh --session"
        exit 1
    fi

    if [[ "$SESSION" -eq 1 ]]; then
        # Nested compositor: Cairo renderer avoids blank/hung GTK layer-shell on some drivers.
        export GDK_BACKEND="${GDK_BACKEND:-wayland}"
        export GSK_RENDERER="${GSK_RENDERER:-cairo}"
        # Nested session: ignore stale IPC sockets from a prior crashed run.
        rm -f "${XDG_RUNTIME_DIR:-/tmp}/metis/compositor.sock" \
              "${XDG_RUNTIME_DIR:-/tmp}/metis/compositor-events.sock" 2>/dev/null || true
    elif [[ -S "${XDG_RUNTIME_DIR:-/tmp}/metis/compositor.sock" ]]; then
        log "Compositor IPC: connected"
    elif [[ "$SESSION" -eq 0 ]]; then
        log "WARN: Metis compositor not running — use ./run-metis.sh --session"
    fi

    if ! command -v cargo >/dev/null 2>&1; then
        log "ERROR: cargo not in PATH. Install Rust: https://rustup.rs"
        exit 1
    fi
    log "Cargo:    $(cargo --version) ($(command -v cargo))"
    log "Rustc:    $(rustc --version 2>/dev/null || echo 'not found')"
    log "PKG_CONFIG_PATH=${PKG_CONFIG_PATH:-unset}"

    run_section "Build dependencies"
    if ! check_build_deps; then
        exit 1
    fi
    echo

    TARGET_DIR="$CARGO_TARGET_DIR"
    if [[ "$PROFILE" == "release" ]]; then
        SHELL_BIN="$TARGET_DIR/release/metis-shell"
        COMP_BIN="$TARGET_DIR/release/metis-compositor"
        BUILD_CMD=(cargo build --release -p metis-shell -p metis-compositor -p metis-settings)
    else
        SHELL_BIN="$TARGET_DIR/debug/metis-shell"
        COMP_BIN="$TARGET_DIR/debug/metis-compositor"
        BUILD_CMD=(cargo build -p metis-shell -p metis-compositor -p metis-settings)
    fi

    if [[ "$FORCE_BUILD" -eq 1 ]] || binary_needs_rebuild "$SHELL_BIN" || binary_needs_rebuild "$COMP_BIN"; then
        run_section "Build"
        log "Running: ${BUILD_CMD[*]} (compiler warnings are OK)"
        build_log="$(mktemp)"
        if ! "${BUILD_CMD[@]}" 2>&1 | tee "$build_log"; then
            log "ERROR: cargo build failed — see log above."
            rm -f "$build_log"
            exit 1
        fi
        warn_count="$(grep -c '^warning:' "$build_log" 2>/dev/null || true)"
        rm -f "$build_log"
        log "Build OK: $SHELL_BIN"
        log "Build OK: $COMP_BIN"
        if [[ "${warn_count:-0}" -gt 0 ]]; then
            log "Note: $warn_count compiler warning(s) — safe to ignore for dev builds."
        fi
        log "Interpreter: $(readelf -l "$SHELL_BIN" 2>/dev/null | awk '/Requesting program interpreter/{print $NF}' | tr -d '[]')"
    else
        run_section "Build"
        log "Using existing binaries (pass --build to rebuild)"
        log "  shell: $SHELL_BIN"
        log "  compositor: $COMP_BIN"
    fi
    echo

    # --- run -----------------------------------------------------------------

    if [[ -f "$PID_FILE" ]]; then
        old_pid="$(cat "$PID_FILE")"
        if kill -0 "$old_pid" 2>/dev/null; then
            log "Metis already running (PID $old_pid). Stop it first: ./run-metis.sh --stop"
            exit 1
        fi
        rm -f "$PID_FILE"
    fi

    run_section "Run"
    log "Controls: Metis edge bar · Super+F fullscreen · Super+Q close"
    log "Stop shell: ./run-metis.sh --stop"
    echo

    if [[ "$SESSION" -eq 1 ]]; then
        export METIS_SHELL_BIN="$SHELL_BIN"
        log "Starting Metis compositor session (spawns shell automatically) …"
        if [[ ${#COMP_ARGS[@]} -gt 0 ]]; then
            log "Compositor args: ${COMP_ARGS[*]}"
        fi
        exec "$COMP_BIN" "${COMP_ARGS[@]}"
    fi

    if [[ "$FOREGROUND" -eq 1 ]]; then
        log "Foreground shell — Ctrl+C stops Metis shell."
        exec "$SHELL_BIN"
    fi

    log "Starting metis-shell in background …"
    nohup "$SHELL_BIN" >>"$LOG_FILE" 2>&1 &
    metis_pid=$!
    echo "$metis_pid" >"$PID_FILE"
    disown "$metis_pid" 2>/dev/null || true
    log "Metis running (PID $metis_pid). This terminal is free — open apps as usual."
    log "Controls: search pill at top · Super+Space command bar · Super+D desk grid"
    sleep 2
    if kill -0 "$metis_pid" 2>/dev/null; then
        if grep -q "panicked at" "$LOG_FILE" 2>/dev/null; then
            log "ERROR: Metis crashed on startup — see $LOG_FILE"
            grep "panicked at" "$LOG_FILE" | tail -1 || true
            rm -f "$PID_FILE"
            exit 1
        fi
        log "Post-start check: Metis still alive (PID $metis_pid)."
    else
        log "ERROR: Metis exited immediately — see $LOG_FILE"
        grep -E "panicked at|ERROR|error" "$LOG_FILE" 2>/dev/null | tail -5 || true
        rm -f "$PID_FILE"
        exit 1
    fi
    log "Verify keybinds anytime: ./run-metis.sh --verify"
    log "Verify grid sync:       ./run-metis.sh --verify-grid"
} 2>&1 | tee -a "$LOG_FILE"
