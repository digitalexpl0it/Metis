#!/usr/bin/env bash
# Launch Metis — Wayland compositor + shell.
#
# Usage (from this directory):
#   ./run-metis.sh --session    # start Metis compositor + shell (full desktop)
#   ./run-metis.sh --session -- -c foot   # session + spawn a client app
#   ./run-metis.sh --session --import-env # also route D-Bus/systemd-activated
#                                         # apps into the nested session (dev)
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
# wallpaper.json holds the Settings app's selection; let the compositor resolve it
# (so a user pick survives restarts) and only fall back to a default here.
WALLPAPER_JSON="${XDG_CONFIG_HOME:-$HOME/.config}/metis/wallpaper.json"
if [[ -z "${METIS_WALLPAPER:-}" ]] && [[ -z "${METIS_NO_WALLPAPER:-}" ]]; then
    if [[ -f "$WALLPAPER_JSON" ]]; then
        : # compositor reads wallpaper.json directly — don't override it
    elif [[ -f "$WALLPAPER_CONFIG" ]]; then
        export METIS_WALLPAPER="$WALLPAPER_CONFIG"
    elif [[ -f "$WALLPAPER_DEFAULT" ]]; then
        export METIS_WALLPAPER="$WALLPAPER_DEFAULT"
    fi
fi

LOG_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/metis/logs"
PID_FILE="${XDG_STATE_HOME:-$HOME/.local/state}/metis/metis.pid"
mkdir -p "$LOG_DIR" "$(dirname "$PID_FILE")"

# --- launch audit -----------------------------------------------------------
# Records who invoked this script (PID + full parent chain) on every run. If a
# Metis session ever relaunches itself "automatically", this log names the exact
# process responsible — the script/compositor have no respawn logic of their own,
# so any reopen comes from an external invoker (IDE task, shell trap, systemd,…).
AUDIT_LOG="${XDG_STATE_HOME:-$HOME/.local/state}/metis/launch-audit.log"
{
    printf '[%s] invoked pid=%s ppid=%s args=[%s]\n' "$(date '+%F %T')" "$$" "$PPID" "$*"
    p="$PPID"
    depth=0
    while [[ -n "$p" && "$p" -gt 1 && "$depth" -lt 10 ]]; do
        cmd="$(tr '\0' ' ' < "/proc/$p/cmdline" 2>/dev/null)"
        printf '    parent[%s] pid=%s : %s\n' "$depth" "$p" "${cmd:-<gone>}"
        p="$(awk '{print $4}' "/proc/$p/stat" 2>/dev/null)"
        depth=$((depth + 1))
    done
} >>"$AUDIT_LOG" 2>/dev/null || true

FORCE_BUILD=0
SESSION=0
IMPORT_ENV=0
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
        --import-env) IMPORT_ENV=1 ;;
        --foreground) FOREGROUND=1 ;;
        --stop) DO_STOP=1 ;;
        --verify) DO_VERIFY=1 ;;
        --verify-grid) DO_VERIFY_GRID=1 ;;
        -h|--help)
            sed -n '2,25p' "$0"
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
    stopped=0
    if [[ -f "$PID_FILE" ]]; then
        pid="$(cat "$PID_FILE")"
        if kill -0 "$pid" 2>/dev/null; then
            kill "$pid" && echo "Stopped Metis shell (PID $pid)."
            stopped=1
        else
            echo "Metis shell not running (stale PID $pid)."
        fi
        rm -f "$PID_FILE"
    fi
    if comp_pid="$(pgrep -x metis-compositor 2>/dev/null)"; then
        kill "$comp_pid" 2>/dev/null && echo "Stopped Metis compositor (PID $comp_pid)."
        stopped=1
    fi
    if [[ "$stopped" -eq 0 && ! -f "$PID_FILE" ]]; then
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

    # Stale-binary check WITHOUT executing the binary. We must NOT run the binary
    # to probe it: `metis-compositor` ignores `--help` (its arg parser only knows
    # `-c`/`--command`) and would boot a full nested compositor window — which
    # looked exactly like the session "closing and auto-reopening". Just verify
    # the ELF interpreter still exists on disk.
    if [[ -n "$interp" && ! -f "$interp" ]]; then
        log "Stale binary: interpreter missing ($interp) — rebuild required."
        return 0
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

        # GNOME (and most host compositors) grab Super globally, so nested dev
        # sessions default to Alt for compositor shortcuts. Override with
        # METIS_MOD=super if you've disabled the conflicting host bindings.
        if [[ -z "${METIS_MOD+set}" ]]; then
            export METIS_MOD=alt
        fi
    fi
    log "XDG_SESSION_TYPE=${XDG_SESSION_TYPE:-unset}"
    log "WAYLAND_DISPLAY=${WAYLAND_DISPLAY:-unset}"
    log "XDG_CURRENT_DESKTOP=${XDG_CURRENT_DESKTOP:-unset}"
    log "DESKTOP_SESSION=${DESKTOP_SESSION:-unset}"
    if [[ -n "${METIS_MOD:-}" ]]; then
        log "METIS_MOD=${METIS_MOD} (compositor shortcuts use this modifier)"
    fi
    if [[ -n "${METIS_WALLPAPER:-}" ]]; then
        log "Wallpaper: $METIS_WALLPAPER"
    elif [[ -n "${METIS_NO_WALLPAPER:-}" ]]; then
        log "Wallpaper: disabled (METIS_NO_WALLPAPER)"
    fi

    if [[ "$IMPORT_ENV" -eq 1 ]] && [[ "$SESSION" -eq 0 ]]; then
        log "WARN: --import-env only applies to a full session; ignoring (add --session)."
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
    log "Controls: Metis edge bar · Mod+F maximize · Mod+Q close (see METIS_MOD)"
    log "Stop shell: ./run-metis.sh --stop"
    echo

    if [[ "$SESSION" -eq 1 ]]; then
        export METIS_SHELL_BIN="$SHELL_BIN"

        # Opt-in: redirect the user D-Bus/systemd activation environment at the
        # nested compositor so D-Bus-activated and single-instance apps open
        # inside Metis. The compositor performs (and reverts) the import once it
        # knows its auto-assigned socket name. Heads-up: while active this
        # temporarily points the logged-in user's activation env at Metis.
        if [[ "$IMPORT_ENV" -eq 1 ]]; then
            export METIS_IMPORT_ACTIVATION_ENV=1
            log "Activation-env import ENABLED — D-Bus/systemd apps will target the nested session."
        fi

        SESSION_DIR="${XDG_RUNTIME_DIR:-/tmp}/metis"
        # Avoid `session.lock` — unrelated apps (e.g. Claude Desktop's VM service)
        # have been observed to open that path and block flock even when Metis
        # is not running.
        LOCK_FILE="$SESSION_DIR/compositor-session.flock"
        LAST_EXIT_FILE="$SESSION_DIR/session.last-exit"
        mkdir -p "$SESSION_DIR" 2>/dev/null || true

        # Single-instance lock. A stray relaunch that OVERLAPS a live session
        # can't stack a second nested compositor — it hits this guard and bails.
        exec 9>"$LOCK_FILE"
        if ! flock -n 9; then
            holder="$(cat "$LOCK_FILE" 2>/dev/null)"
            lock_note=""
            if ! pgrep -x metis-compositor >/dev/null 2>&1; then
                lock_holder="$(fuser "$LOCK_FILE" 2>/dev/null | tr -s ' ' || true)"
                if [[ -n "$lock_holder" ]]; then
                    lock_note=" (compositor not running; lock held by PID(s): ${lock_holder})"
                else
                    lock_note=" (compositor not running; stale lock)"
                fi
            fi
            log "ERROR: a Metis session is already running (lock held${holder:+ by PID $holder})${lock_note}."
            log "       Refusing to start a second session. Stop the first: ./run-metis.sh --stop"
            if [[ -n "$lock_note" ]]; then
                log "       If Metis is not actually open, close the app holding the lock or restart it."
            fi
            log "       If you didn't start this, see launch audit: $AUDIT_LOG"
            exit 1
        fi
        printf '%s\n' "$BASHPID" >&9

        # Rapid-relaunch cooldown. Stops an instant auto-reopen after you close
        # the window (a session exiting and immediately respawning). A genuine
        # quick restart can override with METIS_FORCE=1.
        COOLDOWN="${METIS_SESSION_COOLDOWN:-4}"
        if [[ -z "${METIS_FORCE:-}" && -f "$LAST_EXIT_FILE" ]]; then
            now="$(date +%s)"
            last="$(cat "$LAST_EXIT_FILE" 2>/dev/null || echo 0)"
            delta=$(( now - last ))
            if (( delta >= 0 && delta < COOLDOWN )); then
                log "ERROR: a Metis session exited ${delta}s ago — refusing rapid auto-relaunch (cooldown ${COOLDOWN}s)."
                log "       This breaks an automatic close→reopen loop. To restart on purpose:"
                log "       METIS_FORCE=1 ./run-metis.sh --session   (or wait ${COOLDOWN}s)"
                log "       Who launched this run is recorded in: $AUDIT_LOG"
                exit 1
            fi
        fi

        log "Starting Metis compositor session (spawns shell automatically) …"
        if [[ ${#COMP_ARGS[@]} -gt 0 ]]; then
            log "Compositor args: ${COMP_ARGS[*]}"
        fi
        # Run as a child (not exec) so we can stamp the exit time for the cooldown
        # guard above. The lock on FD 9 is held by this subshell for the session's
        # lifetime and released when it returns.
        "$COMP_BIN" "${COMP_ARGS[@]}"
        session_rc=$?
        date +%s >"$LAST_EXIT_FILE" 2>/dev/null || true
        exit "$session_rc"
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
