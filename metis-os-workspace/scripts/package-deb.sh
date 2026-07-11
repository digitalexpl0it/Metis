#!/usr/bin/env bash
# Build a Metis .deb for Ubuntu (default 24.04 amd64).
#
# Usage:
#   VERSION=0.1.0 ./scripts/package-deb.sh
#   VERSION=0.1.0 UBUNTU_SUITE=24.04 SKIP_BUILD=1 ./scripts/package-deb.sh
#
# Stages the same files as `run-metis.sh --install-session`, but under /usr
# (proper packaging prefix) instead of /usr/local.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE="$(cd "$SCRIPT_DIR/.." && pwd)"
ASSETS_DIR="$WORKSPACE/assets"
# Prefer an explicit CARGO_TARGET_DIR; otherwise use the workspace target/
# (ignore sandbox/env overrides that point elsewhere when packaging locally).
if [[ -n "${METIS_CARGO_TARGET_DIR:-}" ]]; then
    CARGO_TARGET_DIR="$METIS_CARGO_TARGET_DIR"
elif [[ -x "$WORKSPACE/target/release/metis-compositor" ]]; then
    CARGO_TARGET_DIR="$WORKSPACE/target"
else
    CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$WORKSPACE/target}"
fi

VERSION="${VERSION:-}"
UBUNTU_SUITE="${UBUNTU_SUITE:-24.04}"
SKIP_BUILD="${SKIP_BUILD:-0}"
ARCH="${ARCH:-amd64}"
PKG_NAME="metis"
REVISION="${DEB_REVISION:-1}"

if [[ -z "$VERSION" ]]; then
    echo "ERROR: set VERSION (e.g. VERSION=0.1.0)" >&2
    exit 1
fi

# Strip leading v from tags.
VERSION="${VERSION#v}"

DIST_ROOT="${DIST_ROOT:-$WORKSPACE/dist}"
STAGE="$DIST_ROOT/${PKG_NAME}-stage"
DEB_OUT="$DIST_ROOT/${PKG_NAME}_${VERSION}-${REVISION}_${ARCH}.ubuntu${UBUNTU_SUITE}.deb"

log() { printf '==> %s\n' "$*"; }

ensure_cargo() {
    if ! command -v cargo >/dev/null 2>&1; then
        echo "ERROR: cargo not in PATH" >&2
        exit 1
    fi
}

build_binaries() {
    if [[ "$SKIP_BUILD" == "1" ]]; then
        log "Skipping cargo build (SKIP_BUILD=1)"
        return
    fi
    ensure_cargo
    log "Building release binaries…"
    (
        cd "$WORKSPACE"
        cargo build --release \
            -p metis-compositor \
            -p metis-shell \
            -p metis-settings \
            -p metis-portal \
            -p metis-remote \
            -p metis-gaming
    )
}

require_bin() {
    local path="$1"
    if [[ ! -x "$path" ]]; then
        echo "ERROR: missing binary: $path (build first or unset SKIP_BUILD)" >&2
        exit 1
    fi
}

# Ubuntu 24.04 has no libgtk-4-layer-shell* package — ship the shared library
# that the shell was linked against (system or /usr/local from source builds).
stage_gtk4_layer_shell() {
    local libdir="$STAGE/usr/lib/x86_64-linux-gnu"
    mkdir -p "$libdir"
    local found=""
    local candidate
    for candidate in \
        /usr/local/lib/x86_64-linux-gnu/libgtk4-layer-shell.so.0 \
        /usr/lib/x86_64-linux-gnu/libgtk4-layer-shell.so.0 \
        /usr/local/lib/libgtk4-layer-shell.so.0 \
        /usr/lib/libgtk4-layer-shell.so.0; do
        if [[ -e "$candidate" ]]; then
            found="$candidate"
            break
        fi
    done
    if [[ -z "$found" ]]; then
        # Resolve via ldd on the shell binary.
        local shell_bin="$CARGO_TARGET_DIR/release/metis-shell"
        if [[ -x "$shell_bin" ]]; then
            found="$(ldd "$shell_bin" 2>/dev/null | awk '/libgtk4-layer-shell\.so/ {print $3; exit}')"
        fi
    fi
    if [[ -z "$found" || ! -e "$found" ]]; then
        echo "ERROR: libgtk4-layer-shell.so.0 not found." >&2
        echo "  Ubuntu 24.04 does not package gtk4-layer-shell; build it from" >&2
        echo "  https://github.com/wmww/gtk4-layer-shell and install to /usr/local," >&2
        echo "  or set PKG_CONFIG_PATH so the shell links against it." >&2
        exit 1
    fi
    local real
    real="$(readlink -f "$found")"
    local base
    base="$(basename "$real")"
    log "Bundling gtk4-layer-shell: $real"
    install -Dm755 "$real" "$libdir/$base"
    # Keep the SONAME symlink the dynamic linker expects.
    ln -sfn "$base" "$libdir/libgtk4-layer-shell.so.0"
    if [[ "$base" != "libgtk4-layer-shell.so" ]]; then
        ln -sfn "$base" "$libdir/libgtk4-layer-shell.so"
    fi
}

stage_tree() {
    log "Staging package tree under $STAGE…"
    rm -rf "$STAGE"
    mkdir -p \
        "$STAGE/DEBIAN" \
        "$STAGE/usr/bin" \
        "$STAGE/usr/lib/x86_64-linux-gnu" \
        "$STAGE/usr/share/wayland-sessions" \
        "$STAGE/usr/share/xdg-desktop-portal/portals" \
        "$STAGE/usr/share/applications" \
        "$STAGE/usr/share/icons/hicolor/48x48/apps" \
        "$STAGE/usr/share/icons/hicolor/256x256/apps" \
        "$STAGE/etc/pam.d"

    local rel="$CARGO_TARGET_DIR/release"
    for bin in metis-compositor metis-shell metis-settings metis-portal metis-remote metis-gamingd; do
        require_bin "$rel/$bin"
        install -Dm755 "$rel/$bin" "$STAGE/usr/bin/$bin"
    done
    stage_gtk4_layer_shell
    install -Dm755 "$ASSETS_DIR/metis-session" "$STAGE/usr/bin/metis-session"
    install -Dm644 "$ASSETS_DIR/metis.desktop" "$STAGE/usr/share/wayland-sessions/metis.desktop"
    install -Dm644 "$ASSETS_DIR/metis.portal" "$STAGE/usr/share/xdg-desktop-portal/portals/metis.portal"
    install -Dm644 "$ASSETS_DIR/metis-portals.conf" "$STAGE/usr/share/xdg-desktop-portal/metis-portals.conf"
    install -Dm644 "$ASSETS_DIR/metis-settings.desktop" "$STAGE/usr/share/applications/metis-settings.desktop"
    install -Dm644 "$ASSETS_DIR/metis-settings-48.png" "$STAGE/usr/share/icons/hicolor/48x48/apps/metis-settings.png"
    install -Dm644 "$ASSETS_DIR/metis-settings.png" "$STAGE/usr/share/icons/hicolor/256x256/apps/metis-settings.png"
    install -Dm644 "$ASSETS_DIR/pam-metis" "$STAGE/etc/pam.d/metis"
}

# Runtime Depends for Ubuntu 24.04 (noble). Validated against typical ldd
# linkage of release binaries; keep in sync with docs/PACKAGING.md.
write_control() {
    local installed_size
    installed_size="$(du -sk "$STAGE" | awk '{print $1}')"

    cat >"$STAGE/DEBIAN/control" <<EOF
Package: ${PKG_NAME}
Version: ${VERSION}-${REVISION}
Section: x11
Priority: optional
Architecture: ${ARCH}
Installed-Size: ${installed_size}
Maintainer: Metis Developers <metis@localhost>
Homepage: https://github.com/digitalexpl0it/Metis
Depends: libgtk-4-1, libadwaita-1-0, libglib2.0-0t64 | libglib2.0-0, libpango-1.0-0, libcairo2, libgraphene-1.0-0, libseat1, libinput10, libudev1, libgbm1, libdrm2, libegl1, libgles2, libwayland-client0, libwayland-server0, libxkbcommon0, libpipewire-0.3-0, libssl3t64 | libssl3, libpam0g, libdisplay-info1, libeis1, xdg-desktop-portal
Recommends: gnome-keyring, xdg-desktop-portal-gtk
Suggests: gnome-remote-desktop, gamemode, flatpak, bluez, bluetooth, cups, system-config-printer
Description: Metis Wayland desktop environment
 Metis is a Wayland desktop environment built in Rust: a Smithay compositor,
 GTK4 edge bar (shell), Settings app, and xdg-desktop-portal backend.
 .
 After installing, log out and pick "Metis" from your display manager's
 session menu (GDM, SDDM, and other Wayland-capable greeters).
 .
 Ships libgtk4-layer-shell (not packaged on Ubuntu 24.04). Optional features
 (remote desktop, Flatpak, GameMode, Bluetooth, printers) are Suggests —
 enable them from the first-run Optional software step or with apt.
 See https://github.com/digitalexpl0it/Metis.
EOF
}

write_postinst() {
    cat >"$STAGE/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -f -t /usr/share/icons/hicolor >/dev/null 2>&1 || true
fi
exit 0
EOF
    chmod 0755 "$STAGE/DEBIAN/postinst"
}

build_deb() {
    log "Building $DEB_OUT…"
    mkdir -p "$DIST_ROOT"
    rm -f "$DEB_OUT"
    if command -v fakeroot >/dev/null 2>&1; then
        fakeroot dpkg-deb --build "$STAGE" "$DEB_OUT"
    else
        dpkg-deb --build "$STAGE" "$DEB_OUT"
    fi
    log "Done: $DEB_OUT"
    dpkg-deb --info "$DEB_OUT" || true
    ls -lh "$DEB_OUT"
}

build_binaries
stage_tree
write_control
write_postinst
build_deb
