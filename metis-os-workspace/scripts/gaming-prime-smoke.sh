#!/usr/bin/env bash
# Hybrid PRIME smoke test for Metis gaming (Phase 11).
#
# Verifies that a Vulkan app can render on the discrete GPU while the compositor
# scans out on the integrated panel — the standard NVIDIA/AMD offload path.
#
# Usage:
#   ./scripts/gaming-prime-smoke.sh [vulkan-test-app]
#
# Default app: vulkaninfo (must be installed). Set RUST_LOG=metis_compositor=trace
# on the compositor to see `scanout_promoted=true` when direct scanout succeeds.
set -euo pipefail

APP="${1:-vulkaninfo}"
if ! command -v "$APP" >/dev/null 2>&1; then
    echo "ERROR: $APP not found — install vulkan-tools or pass another Vulkan binary." >&2
    exit 1
fi

if [[ -z "${WAYLAND_DISPLAY:-}" ]]; then
    echo "ERROR: run inside a Metis Wayland session (WAYLAND_DISPLAY unset)." >&2
    exit 1
fi

echo "== Metis hybrid PRIME smoke =="
echo "Session: WAYLAND_DISPLAY=$WAYLAND_DISPLAY"
echo "Render GPU env (if hybrid):"
env | grep -E '^(DRI_PRIME|__NV_PRIME|MESA_VK_DEVICE_SELECT|METIS_GAME_GPU)=' || true

echo ""
echo "Launching $APP on discrete GPU (when present)…"
if command -v gamemoderun >/dev/null 2>&1; then
    exec gamemoderun env DRI_PRIME=1 __NV_PRIME_RENDER_OFFLOAD=1 __NV_PRIME_RENDER_OFFLOAD_PROVIDER=NVIDIA-G0 "$APP" 2>/dev/null || \
        exec env DRI_PRIME=1 __NV_PRIME_RENDER_OFFLOAD=1 "$APP"
else
    exec env DRI_PRIME=1 __NV_PRIME_RENDER_OFFLOAD=1 "$APP"
fi
