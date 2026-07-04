# Metis performance audit

Audit date: 2026-06-28. Scope: compositor hot path, shell/bar overhead, portal
capture, binary footprint, and recommended follow-ups.

---

## Executive summary

| Area | Rating | Notes |
|------|--------|-------|
| Idle CPU (compositor) | **Good** | Damage-gated render; ~60 fps cap; near-zero work when idle |
| Interactive latency | **Good–OK** | Pointer throttling, partial damage; 6k-line `state.rs` monolith |
| DRM session | **OK** | Vblank + damage-gated flips; single-GPU only |
| Shell / edge bar | **OK** | Background poll thread; subprocess I/O every 400 ms–6 s |
| Screen capture | **Early** | Full-scene GL render + CPU SHM copy per screenshot |
| Gaming / Steam | **Improving** | Pointer lock + relative motion; vblank-driven present; scanout feedback wired — direct scanout still needs on-hardware verification |
| Install footprint | **Improved** | Release profiles use LTO + strip; optional `release-small` |

Metis is **past prototype** on compositor fundamentals (no busy loops, deliberate
throttles, async portal warm-up). It is **not yet gaming- or streaming-optimized**.

---

## Compositor — what is already optimized

### Damage-driven rendering

- Global `damaged` flag; winit/DRM skip GL when nothing changed.
- **16 ms heartbeat** caps nested dev at ~60 fps and avoids unbounded
  `RedrawRequested` loops (`winit.rs`).
- **`OutputDamageTracker`** for partial repaints.
- DRM: `drm_dispatch_damage()` only flips outputs with `pending && !queued`.

### Input & housekeeping throttles

- **Pointer motion** forwarded at most ~48 ms / 3 px unless grab or bar hit
  (`state.rs::should_forward_pointer_motion`) — prevents GTK hover storms.
- **`input.json`** reload throttled to ~1 s.
- **Wallpaper decode** debounced off the render path.
- **Portal stack** started on a detached thread (login no longer blocks 10+ s).

### Cheap bar blur

- Backdrop blur samples **wallpaper texture under the bar**, not a full
  framebuffer capture (`blur.rs`) — avoids transform hazards and heavy readback.

### Shared logic

- **`metis-grid`** — pure layout/reflow, no I/O in hot path.
- **`metis-protocol`** — JSON IPC for control plane only (windows, workspaces).

---

## Hotspots & risks (priority order)

### P0 — ScreenCast / continuous capture (when implemented)

**File:** `metis-compositor/src/image_capture.rs`, `metis-portal/src/capture/`

Each screenshot today:

1. Rebuilds full render element list for the output.
2. Renders to an offscreen GLES buffer.
3. `copy_framebuffer` → CPU, then SHM write.

Fine for occasional PNGs; **unacceptable at 30–60 Hz** without dmabuf export and
a PipeWire zero-copy path.

**Recommendation:** dmabuf capture session + register with PipeWire; avoid
`pixels().to_vec()` in the portal client loop.

### P1 — No fullscreen direct scanout

Games and Proton (XWayland) are **always composited**. No unredirect / direct
lease for full-screen clients.

**Impact:** Extra latency and GPU fill rate vs Gamescope or mature compositors.

**Recommendation:** Phase 6 — optional per-game Gamescope wrapper; long-term
evaluate direct scanout for true fullscreen XWayland/Wayland clients.

### P2 — `state.rs` monolith (~6k lines)

Single `MetisState` holds windowing, workspaces, scroll layout, IPC, wallpaper,
decorations, grabs, etc.

**Impact:** Compile time, cache locality, harder to profile isolated subsystems.

**Recommendation:** Incremental split (input routing, workspace, render prep) when
touching those areas — not urgent for runtime if damage gating stays correct.

### P3 — Shell bar subprocess polling

**File:** `metis-shell/src/services/poll.rs`

Background thread (~400 ms) runs `nmcli`, `bluetoothctl`, `pactl`, `upower`,
optional `solaar` (~2 s, cached 20 s).

**Impact:** Low average CPU; occasional latency spikes; not on compositor thread.

**Recommendation:** Keep as-is for bar; consider D-Bus subscriptions for Wi-Fi/BT
later if profiling shows wakeups.

### P4 — Default Cairo shell renderer

`METIS_SHELL_GSK_RENDERER=cairo` in session — **software GTK** for reliability on
fresh DRM sessions.

**Impact:** Shell CPU only; games unaffected.

**Recommendation:** Document `METIS_SHELL_GSK_RENDERER=gl` when drivers are stable.

### P5 — Dependency feature bloat

| Crate | Issue | Action taken |
|-------|--------|--------------|
| `metis-shell` | `tokio` `full` | Trimmed to `rt`, `rt-multi-thread`, `macros`, `time`, `sync` |
| `metis-compositor` | Smithay `renderer_multi` | Needed for future multi-GPU; keep until split |
| `metis-shell` | `rusqlite bundled` | SQLite embedded in shell binary (~size) — acceptable for calendar cache |

---

## Binary footprint

Measured on 2026-06-28 (x86_64, after profile + tokio trim):

| Binary | Stock release (before) | **`release`** (LTO + strip) | **`release-small`** |
|--------|------------------------|----------------------------|---------------------|
| metis-compositor | 16 MB | **11 MB** (−31%) | 9.2 MB |
| metis-shell | 21 MB | **15 MB** (−29%) | **9.5 MB** (−55%) |
| metis-portal | 9.7 MB | **5.7 MB** (−41%) | **3.2 MB** (−67%) |
| metis-settings | 14 MB | **8.6 MB** (−39%) | **5.0 MB** (−64%) |
| **Total** | **~61 MB** | **~40 MB** (−34%) | **~27 MB** (−56%) |

Dominant contributors: **Smithay + GLES**, **GTK4 + layer-shell**, **ashpd/zbus**,
embedded SQLite in shell.

### Build profiles (`metis-os-workspace/Cargo.toml`)

| Profile | Use | Settings |
|---------|-----|----------|
| **`release`** (default) | `./run-metis.sh --release`, `--install-session` | `opt-level=3`, `lto=thin`, `codegen-units=1`, `strip=symbols` |
| **`release-small`** | `./run-metis.sh --release-small --install-session` | `opt-level=s`, `lto=fat`, `panic=abort`, strip; **compositor stays `opt-level=3`** |

Rebuild after changing profiles:

```bash
cd metis-os-workspace/metis-shell
./run-metis.sh --build --release          # balanced
./run-metis.sh --build --release-small    # smallest install
ls -lh ../target/release/metis-compositor ../target/release-small/metis-compositor
```

Further size wins (not yet applied):

- `panic = "abort"` on default `release` (saves ~100 KB–1 MB; loses backtraces on panic)
- Split calendar/SNI into optional features on `metis-shell`
- System SQLite instead of `rusqlite/bundled` where distros allow

---

## Measurement checklist

Run under a real Metis DRM session when validating changes:

```bash
# Idle CPU (should be ~0–2% compositor on one core)
top -p $(pgrep metis-compositor)

# Frame timing — watch for sustained 100% compositor without input
perf top -p $(pgrep metis-compositor)

# Binary sizes after profile change
ls -lh metis-os-workspace/target/{release,release-small}/metis-*

# Capture cost (one-shot)
/usr/bin/time -f '%e sec' metis-portal --capture-test /tmp/t.png
```

---

## Recommended roadmap (perf)

1. **ScreenCast** with dmabuf + PipeWire (P0).
2. **Inhibit portal** — prevent idle dim during games (correctness + fewer wakeups).
3. **Gamescope** launch-option testing on Metis (P1 gaming path).
4. **Split `state.rs`** when refactoring (P2 maintainability).
5. **Phase 5 VRR** — latency/smoothness on supported panels.

See also [`TODO.md`](../metis-os-workspace/TODO.md) Phase 5–6.
