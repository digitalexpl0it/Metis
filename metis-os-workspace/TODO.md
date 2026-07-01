# Metis Shell — Edge Bar (v2)

**Current phase:** Phase 3 is complete except the deferred **full multi-GPU**
compositing item. **Phase 4** (settings-app expansion) is complete for the planned
Device + System pages. Next major tracks: **Phase 5** (display mode-setting,
resolution/mirror/extend, VRR, colour management, HDR), **Phase 6** (Flatpak, Steam
& gaming), and **Phase 7** (remote access / full desktop sharing).

---

## Phase 1 — Edge bar

- [x] `bar.json` config — position (top/bottom/left/right), height/width, opacity, widget order
- [x] Live reload when `bar.json` changes (cosmetic edits apply in place; full
      rebuild only when widgets, position, displays, clock format, or workspace
      count change)
- [x] Bar position — top/bottom/left/right dropdown (Settings → Appearance → Edge bar);
      exclusive zone, pill flush-to-edge, and popover/menu open direction adapt per side
- [x] Distance from edge — slider for the gap between the bar and its anchored screen edge
- [x] Edge-bar border — `bar_border` (accent gradient / solid / custom gradient + width,
      0 disables); rounded gradient via layered `background-clip`, flows along the long axis
- [x] Workspace indicator — live virtual workspaces (click a dot or `Super`+`n` to switch)
- [x] Clock popover — tabbed: calendar, world clocks, stopwatch, timer, alarms
- [x] World Clocks — inline searchable timezone picker (up to 3 zones)
- [x] Stopwatch with scrollable lap list
- [x] Timer — movable, always-on-top layer-shell HUD with pause/close
- [x] Alarms — segmented sound selector
- [x] Battery, network, volume indicators
- [x] Bluetooth bar widget — appears when an adapter is present; popover lists
      connected devices with battery level and charging icon (UPower / HID sysfs /
      optional Solaar for Logitech HID++; BlueZ fallback); low-battery alerts
- [x] Wi-Fi icon stability — holds the last known active network during rescans so
      the signal icon does not flicker offline
- [x] Notifications popup — badge, grouped duplicates + count badge, per-kind icons,
      clear-all with slide-out animation, scrollbar, in-bar alert routing
- [x] **Clipboard history widget** — edge-bar popover listing recent clipboard
      entries (text previews + image thumbnails); click a row to recall via
      compositor IPC; history persisted to `~/.local/state/metis/clipboard.json`
      (max 50 entries; 10 MB image cap; clear-history button)
- [x] WiFi / audio popover controls
- [x] Weather widget — icon + temperature with a forecast popover (see Phase 2);
      cached snapshot re-applied after bar rebuilds (no empty flash)
- [x] System tray — collapsed popover vs pinned-on-bar modes; readable tooltips
      (SNI title / bus-name fallback); light-mode pixmap/symbolic icon rendering
- [x] Light-mode bar popover styling — theme-token entries, buttons, and icon
      actions in dropdown panels (clock calendar, clipboard, network, notifications)
- [x] Bundled default wallpapers (`default.png` … `default9.png`) listed in
      Settings → Appearance background picker
- [x] Bar symbolic icons — bluetooth, clipboard, and notifications match other bar
      icons (GTK symbolic + theme text color)
- [x] Theme file watcher (live `themes/*.json` reload)
- [x] Freedesktop notification D-Bus daemon (`org.freedesktop.Notifications`)
- [x] Themeable token palette — accent + secondary accent, semantic status colors,
      `text_on_accent`, configurable shadows/glows drive the stylesheet
- [x] Bar transparency (`opacity`) + compositor backdrop blur (`blur`/`blur_radius`)
- [x] App launcher — ArcMenu-style popover off the brand icon: quick launchers +
      power actions rail, Frequent/alphabetical app list with apps-only search,
      and a pinnable apps grid; translucent panel (`menu_opacity`) with in-surface
      tooltips, dismissed synchronously on launch
  - [x] Dedicated **Settings · Metis Menu** page — Quick launchers (configurable
        Terminal / File-manager from auto-detected installs or a custom binary path,
        persisted to `menu.json`; shell falls back to env hint → known candidates →
        `xdg-open`) plus the menu panel opacity (moved out of the Edge bar card)
  - [x] Input robustness — compositor gives the bar popover top pointer priority so
        the app list scrolls even over a window behind it; click-outside (desktop or
        another window's titlebar) reliably dismisses; type-to-search works without
        clicking the search box first (`SearchEntry` key capture, no focus grab); and
        the scroll gutter/scrollbar stay flat in dark mode (GTK variant synced to the
        active theme)

---

## Phase 2 — Settings app + Window decorations

A standalone `metis-settings` GTK4 toplevel (the first real window-managed
client) with a sidebar and per-domain pages, plus compositor-drawn server-side
decorations so it (and every app) gets a real titlebar.

### Shared crates
- [x] `metis-config` — pure (serde + fs, no GTK) config + theme-token types and
      stylesheet builder, shared by the shell and settings app
- [x] `metis-secrets` — shared freedesktop Secret Service (`oo7`) wrapper

### `metis-settings` shell
- [x] New `metis-settings` binary — sidebar nav + content stack, `--page` preselect
- [x] Launch from the bar launcher icon and via `metis-cmd settings [page]`
- [x] Appearance page — Light/Dark style chooser, accent/secondary/semantic
      colors, opacity, blur + blur radius (writes `themes/*.json` + `bar.json`,
      live reload)
- [x] Appearance page — background picker with three types: Picture (bundled +
      imported grid, "Add Picture…"), Solid colour, and Gradient (start/end +
      direction); live switch via `ApplyBackground` IPC, persisted to
      `wallpaper.json`
- [x] Network page — wired/NIC config (DHCP vs static), Wi-Fi scan/connect/forget;
      bar "wired-only" network click opens this page
- [x] Calendars page — moved CalDAV/MS365 account config out of the clock popover

### Window decorations (server-side)
- [x] `zxdg_decoration_manager_v1` + `XdgDecorationHandler`; force SSD so GTK
      omits its client-side headerbar
- [x] Frame vs client geometry split — titlebar + border replace the old undrawn
      44 px grid tile-header inset
- [x] Compositor-drawn titlebar (title via `fontdue`), border, and close /
      minimize / maximize buttons
- [x] Decoration input — button actions + titlebar drag (`MoveSurfaceGrab`)
- [x] Border resize — compositor-side edge/corner hit-test (`RESIZE_MARGIN_PX`)
      starts `ResizeSurfaceGrab`, floats tiled windows out of the grid, persists
      the new geometry, and shows directional resize cursors on hover
- [x] Resize edge occlusion — auto-hide, maximized, and fullscreen windows block
      lower-window resize bands (client geometry + 12 px halo) without stealing
      titlebar/decoration input
- [x] Auto-hide titlebar pointer routing — revealed overlay chrome and permanent
      SSD border strips own pointer hits above the mapped client rect so hover and
      clicks cannot fall through to windows below
- [x] **Terminal right-click / primary-selection paste** — right/middle press
      syncs data-device + primary-selection focus before chrome handlers so
      context menus and middle-click paste work in kitty/foot on tiled, floating,
      maximized, and auto-hide-titlebar layouts
- [x] Per-app geometry memory — free-layout windows save their position/size per
      `app_id` to `~/.config/metis/windows.json` and restore it on reopen (placement
      defers until `app_id` is known so the restore isn't lost to a centered default)
- [x] Polish — rounded, anti-aliased control buttons with glyphs (× / + / −) and
      focus-aware dimming (traffic-light colors when focused, gray when not)
- [x] Snap zones / hot-spots — drag a window to a screen edge for a live
      translucent preview that drops it into half / quarter / maximize regions
      (`metis-grid::pixel_snap_target`, wired through `MoveSurfaceGrab` + winit
      overlay; computed against the usable area so the top zone clears the bar)
- [x] Theme-aware + translucent titlebar — palette follows the active light/dark
      theme (live ~1s poll), background opacity via `titlebar_opacity`, rounded
      top corners + a border that wraps under the titlebar; text/buttons stay opaque
- [x] Title pill border — flat solid pill plate ringed by a thin stroke on the
      focused window; configurable via `titlebar_pill_border` (accent gradient /
      solid / custom gradient + width) from Settings → Appearance → Windows
- [x] Window frame border — independent of the pill (`window_border`): accent
      gradient / solid / custom gradient, vertical top→bottom ramp, with a
      configurable thickness that also insets the client body (live-applied via a
      runtime `metis_grid::set_app_tile_border_px` + relayout)
- [x] Auto-hide titlebar — maximized and left/right/top-corner snaps hide the
      titlebar (client fills the footprint) and reveal it as a borderless
      translucent overlay on top-strip hover; oversized clients are re-anchored so
      the screen-edge gap survives
- [x] XWayland — spawn/manage an X11 server (`X11Wm`/`XwmHandler`) so X11-only
      apps run in the nested session alongside Wayland clients

### Weather
Backend: **Open-Meteo** (keyless) — reuse/extend `briefing/connectors/weather.rs`
for `current` + `hourly` + `daily`, plus the Open-Meteo geocoding API for city
search. Attribution shown in the popover footer.

- [x] Bar widget — condition icon + temperature (matches reference screenshot)
- [x] Popover — current conditions (temp, label, H/L), hourly strip, saved
      locations list, attribution footer
- [x] Auto-detect location — IP geolocation (city-level, keyless ipwho.is) with
      an offline system-timezone (`zoneinfo`) fallback; 12s HTTP timeouts, cached,
      retries every 30s on failure
- [x] `weather.json` config (unit, `auto_detect`, `ip_geolocation`, locations)
- [x] Settings → Weather page — manual location override + search, multiple saved
      locations (reorder/remove), °F/°C unit toggle, IP-geolocation toggle

---

## Phase 2.5 — Taskbar / running-apps dock

A dock-style `tasks` widget on the edge bar, driven by live compositor window
state. Built for a single output now, but designed forward-compatible with the
Phase 3 per-output bars + per-output workspaces.

- [x] Protocol — `WindowInfo` gains `minimized`/`focused` (+ `output`/`workspace`
      placeholders for Phase 3); new `SetMinimized`/`ActivateWindow` commands and a
      `WindowMinimized` event; `WindowFocused` now also pushed on the event bus
- [x] Compositor — minimize/restore/activate by id (grid tiles routed through
      `set_tile_mode`, floating windows directly), focus emitted on pointer focus
- [x] Shell window-state cache (`services/windows.rs`) — folds the compositor event
      stream into a snapshot, seeded by `list_windows()` with a slow reconcile
- [x] Per-app grouping — windows grouped by resolved app identity (desktop id /
      `StartupWMClass`, case-insensitive `app_id` match, fallback icon)
- [x] Running indicator dot + focus highlight; minimized apps dimmed
- [x] Click — toggle focus/minimize (single window), window picker popover
      (multi-window), launch (pinned-but-not-running)
- [x] Right-click — pin/unpin (separate `taskbar_pinned` list in `bar.json`) and
      close window(s)
- [x] Overflow — horizontal scroll when the dock outgrows the bar

---

## Phase 3 — Multi-monitor, workspaces & tiling

Broaden window management beyond the single-output, single-desktop stub. Staged
so each milestone is shippable on its own:

- [x] **Virtual workspaces (single output)** — fixed-N workspaces on the one
      output, each its own set of tiled app windows with shared desk widgets.
      `SwitchWorkspace`/`MoveWindowToWorkspace` commands + `WorkspaceChanged`
      event; `Super`+`1..9` / `Super`+`Shift`+`1..9` keybinds; `Super`+`Alt`+`←`/`→`
      cycles workspaces in order (wraps at 1..=count); bar dots wired
      through IPC; `WindowInfo.workspace` populated. Count from `bar.json`
      `workspace_count`. (Per-output split comes with the refactor below.)
- [x] **Output-agnostic refactor** — remove the `space.outputs().next()` /
      single-monitor assumptions; thread an output id through placement, grid,
      snapping, decorations, and IPC. Per-output placement/snap routing, absolute-
      pointer mapping across outputs, `ListOutputs` IPC (name, geometry, primary
      flag, sorted left-to-right).
- [x] **Virtual outputs under winit** — `METIS_VIRTUAL_OUTPUTS=2` tiles the
      nested window into two side-by-side logical outputs (dedicated full-window
      render output + multi-output layer/blur/frame loop). The test rig for the
      rest of the refactor; cursor + cross-output drag now work on it.
- [x] **Per-output edge bar** — one bar per output (`gtk4-layer-shell`
      `set_monitor`), rebuilt on monitor hotplug; `bar.json` `displays`
      (`all`/`primary`) + Settings control to limit it to the primary output.
- [x] **Per-output state** — each output owns its own usable area, grid, and
      wallpaper. Per-output wallpaper — each display is cover-cropped to its
      own resolution and can carry its own picture via `wallpaper.json` `per_output`
      (Settings · Appearance · Per-display background; outputs discovered via the
      `ListOutputs` IPC); per-output usable area drives floating placement,
      snapping, and maximize; per-output grid/tiling landed with per-output
      workspaces below; per-output dock landed with "Taskbar follows" below.
- [x] **Per-output workspaces** — Hyprland-style: each output owns an independent
      set of workspaces, its own active workspace, and its own grid of app
      windows. Compositor state is now per-output (`OutputDesk` per output: grid +
      active workspace + stashed tiles); windows are tagged with their output and
      map only while their output's active workspace matches. `Super`+`n` /
      `Super`+`Shift`+`n` act on the output under the pointer; `SwitchWorkspace` /
      `WorkspaceChanged` carry an output id; each per-output bar drives and
      reflects its own output's workspaces (matched via the GDK monitor connector).
- [x] **Workspace mode toggle** — Settings → Appearance → Edge bar → Workspaces
      chooses `separate` (independent per output, default) or `linked` (every output
      switches to the same workspace at once). `bar.json#workspace_mode`; the
      compositor routes `Super`+`n` / `SwitchWorkspace` through `switch_workspace_routed`,
      fanning out to all outputs in linked mode (each emits its own `WorkspaceChanged`).
- [x] **Cross-output moves** — drag a window onto another monitor (or snap it
      there) and its desk tile + scroll membership follow automatically
      (`maybe_adopt_window_output` on drag-drop / snap). `Super`+`Shift`+`←`/`→`
      moves the focused window to the adjacent output on grid workspaces
      (scroll mode keeps those keys for column moves). `MoveWindowToOutput` IPC.
      Move a whole workspace with `Super`+`Ctrl`+`Shift`+`←`/`→` (independent
      per-output mode) or `MoveWorkspaceToOutput` IPC.
- [x] **Automatic dynamic tiling** — grid workspaces auto-split among open app
      windows on open/close and workspace switch (below desk widgets; master stack
      for three; focused window gets the primary slot)
- [x] **Scrolling layout option** — niri/PaperWM/paneru-style horizontally
      scrolling workspace, selectable per-workspace as a second mode in `metis-grid`
      (`scroll.rs`: `ScrollState` of columns, each a vertical window stack). App
      tiles stay the membership/stash source of truth; scroll mode only overrides
      pixel placement + hit-testing. Toggle live with `Super`+`\`; settings default
      for new workspaces via `bar.json#default_layout`. Keybinds (scroll workspace
      only): `Super`+arrows focus, `Super`+`Shift`+arrows move, `Super`+`,`/`.`
      consume/expel a window into/out of a column stack, `Super`+`-`/`=` snap the
      column to full/half width. `SetWorkspaceLayout` IPC.
  - Full-height columns with **continuous, mouse-resizable widths** (drag the
        right border to grow a window and push the rest over; left border resizes
        the previous column — `ScrollResizeGrab`). Width is a fraction of the
        viewport, not fixed presets.
  - Opening a window appends a column and **never resizes existing windows**;
        the strip reflows on open/close so columns slide into place.
  - Off-screen columns are clipped to their own output (`CropRenderElement`) so a
        column scrolled past the edge never bleeds onto an adjacent monitor; fully
        off-screen columns stay unmapped; scroll offset is clamped to strip width.
  - Viewport pan **eases** toward the focused column (`advance_scroll_animation`);
        client surfaces stay mapped at the target offset and render with an X nudge
        (`scroll_x_target - scroll_x`) so resize-averse clients are not reconfigured
        every frame.
- [x] **Taskbar follows** — each output's dock shows only the windows on that
      output's currently-visible workspace (pinned launchers persist everywhere).
      `WindowInfo.output` carries the monitor name; the dock filters by
      `(output, active workspace)`, repaints on workspace switch, and dedups per
      bar. The "Per-output state" per-output dock item is now also covered.
- [x] **DRM/udev backend (real standalone session)** — DRM/KMS + libseat +
      libinput backend selected alongside the nested winit dev path
      (`METIS_BACKEND` / autodetect). Damage-gated per-output page-flips on the
      primary GPU's `GlesRenderer`, dmabuf global for EGL clients, libinput input
      with VT-switch / safe-quit / suspend-resume, software/HW cursor (XCursor +
      client surfaces), and live connector hotplug with output re-packing.
      Login-manager entry installable via `run-metis.sh --install-session`
      (`metis-session` + `metis.desktop`); `--session --drm` runs from a TTY.
  - [ ] **Full multi-GPU** — render each output on its own GPU via
        `GpuManager`/`MultiRenderer`. Blocked on making the custom GL **blur
        shader** (`BlurElement`, a `GlesTexProgram` drawn through
        `GlesFrame::render_texture_from_to`) work through `MultiFrame`, which the
        pinned smithay API does not expose; needs a blur rework (or a
        per-renderer fallback) + multi-GPU hardware to validate. Single-GPU and
        hybrid single-output (direct EGL/PRIME import) work today.
- [x] **Settings portal (`org.freedesktop.portal.Settings`)** — `metis-portal`
      serves color-scheme, gtk-theme, and empty decoration/button layouts from
      `metis-config` so GTK clients pick up Metis light/dark prefs and drop CSD
      chrome; registered via `metis.portal` + `metis-portals.conf`, started by
      the compositor before `xdg-desktop-portal` in the DRM session.
- [x] **Screenshot portal** — compositor exposes
      `ext-image-copy-capture-v1` / `ext-image-capture-source-v1`; `metis-portal`
      serves `org.freedesktop.impl.portal.Screenshot` via a native Wayland capture
      client (SHM buffer + PNG encode). Compositor retains capture `Session` objects
      for the client lifetime. Verified with Flameshot via `xdg-desktop-portal`.
      Registered in `metis.portal` + `metis-portals.conf`.
- [x] **ScreenCast portal (live streaming)** — `metis-portal` registers
      `org.freedesktop.impl.portal.ScreenCast`; persistent
      `ext-image-copy-capture-v1` session + ~30 Hz frame pump pushes BGRx frames
      into a real PipeWire output stream node (`pipewire` crate on a dedicated
      thread). Verify with OBS “Video Capture Device (PipeWire)” under a live
      session. Post-Phase-3 follow-up: dmabuf zero-copy export (see
      `docs/PERF_AUDIT.md` P0).
- [ ] **ScreenCast dmabuf zero-copy** — export dmabuf from compositor capture +
      PipeWire memfd import (perf pass; SHM pump is functional at 1080p30 today).

---

## Phase 4 — System settings expansion

Grow `metis-settings` from appearance / menu / weather / network / calendars into a
proper control center. Most pages need real device or service backends (libinput,
D-Bus services, PipeWire) that only work under the DRM/udev session — under the
nested winit dev session they degrade to read-only or no-op. Group the sidebar into
**Input**, **Devices**, and **System** sections as it grows.

### Input devices (libinput / xkb)
Compositor applies device config from a new `input.json`; settings writes it and the
compositor live-reloads (mirrors the `bar.json` watcher pattern).

- [x] **Mouse** — pointer speed / acceleration, acceleration profile (flat vs
      adaptive), natural scroll, primary button (left/right-handed), scroll
      wheel speed multiplier; compositor applies via libinput + axis scaling from
      `input.json` with live reload
- [x] **Touchpad** — tap-to-click, tap-and-drag, natural scroll,
      disable-while-typing, pointer speed + acceleration profile, scroll speed;
      stored in `input.json`, applied when a touchpad is present
- [x] **Keyboard** — layout(s) + variant, repeat delay + rate, compose key,
      Caps/Esc/Control remap via xkb options; applied via Smithay xkb config +
      `wl_keyboard` `repeat_info`

### Devices (D-Bus services)
- [x] **Bluetooth** — adapter on/off, scan (toggle + auto-stop), pair / connect /
      trust / remove, battery level and charging state where reported; via BlueZ
      (`bluetoothctl`) with UPower and optional Solaar (Logitech HID++) overlays.
      Bar indicator appears only when an adapter is present
      (`BarWidgetId::Bluetooth`); popover lists connected devices with battery
      icons; low-battery in-bar alerts (≤20%, hysteresis, suppressed while
      charging)
- [x] **Printers** — list queues via CUPS (`lpstat`); launcher for
      `system-config-printer` / CUPS web UI

### System
- [x] **Power / Battery** — power profiles (power-saver / balanced / performance) via
      `powerprofilesctl`, battery details via sysfs, idle-dim / blank /
      suspend timeouts + lid-close action persisted to `power.json` (logind
      best-effort); battery widget links to Power settings; **Connected devices**
      section lists Bluetooth peripherals with battery + charging status
- [x] **Sound** — output / input device selection via `pactl` default sink/source;
      volume readout on the settings page (bar volume widget unchanged)
- [x] **Display (settings UI)** — Settings → Display: monitor picker chips, per-output
      scale/enabled, night-light prefs in `outputs.json`; scale applies live via
      `ReloadOutputs` IPC + compositor file poll. Resolution / mirror / turn-off /
      night-light compositor pipeline remain Phase 5 below.

---

## Phase 5 — Display pipeline (mode-setting, HDR / VRR / colour)

Advanced output features, all gated on the real DRM/udev backend (deferred in
Phase 3) — none of these are possible under the nested winit dev session.

### A. Display settings & mode-setting (Settings → Display)

- [x] **Wire `outputs.json` scale to the compositor** — `ReloadOutputs` IPC +
      ~1s file poll; fractional scale applied live; window reflow + wallpaper relayout
- [ ] **Per-output enable/disable** — honour the Display page “Enabled” switch
      (unmap output, reclaim desktop space, hot-plug safe)
- [ ] **Display picker UX** — basic monitor chips + per-display panel (make/model
      from EDID when available); refresh list on hotplug
- [ ] **Resolution & refresh rate** — list DRM modes per output; let the user pick
      resolution and refresh; apply via Smithay `DrmOutput` mode-setting
- [ ] **Multi-monitor layout** — extend desktop (independent positions, current
      default), mirror/clone displays, turn off / disable an output

### B. Colour pipeline

- [ ] **VRR / adaptive sync** — enable per-output via Smithay's DRM VRR support;
      opt-in toggle surfaced in the Display settings page
- [ ] **Colour management** — ICC / per-output colour profiles and the
      `wp_color_management` protocol; groundwork shared with HDR
- [ ] **Night light / colour temperature** — scheduled warm-shift (UI toggle exists
      in Display settings today; compositor colour pipeline not wired yet)
- [ ] **HDR** — wide-gamut / 10-bit GLES render path + DRM colour pipeline on top of
      colour management (long-term; gated on protocol + Smithay maturity). A genuinely
      rare thing for a lightweight DE — worth doing right rather than fast

---

## Phase 6 — Flatpak, Steam & gaming

Sandboxed apps (Flatpak), **Steam / Proton**, and native games share the same
Wayland session and **xdg-desktop-portal** stack. Metis does not ship gaming-specific
code today — apps work incidentally when the host has the right packages, portal
daemons, and device permissions. This phase makes Flatpak and **SteamOS-class gaming**
(first-class Steam client, Proton, controllers, optional Gamescope) explicit and
fills portal gaps launchers expect.

**Target:** a Metis session where you can install Steam (`.deb` or Flatpak), launch
Proton titles, use Steam Input / common controllers, and optionally wrap games in
**Gamescope** — the same building blocks Valve uses on SteamOS (minus replacing
SteamOS's own Gamescope gaming-mode session).

**Permissions model (three layers):**

1. **Flatpak manifest / overrides** — `socket=wayland`, `device=dri`, `device=all`
   (gamepads), `share=network`, etc. (`flatpak override --user …`).
2. **Portal runtime prompts** — screenshot, screencast, file access; persisted by
   system **`xdg-permission-store`** (not `metis-portal`).
3. **Metis portal backends** — only interfaces registered in `metis.portal` /
   `metis-portals.conf` (Settings, Screenshot, ScreenCast today; gtk default for
   file dialogs and notifications).

**Gamepads:** Wayland has no standard gamepad protocol on `wl_seat`. Games read
`/dev/input/event*` via SDL/Proton/evdev. The compositor seat is keyboard +
pointer only; libinput gamepad events are not forwarded. Flatpak games typically
need `--device=all` (or equivalent overrides), not a compositor gamepad driver.

### A. Flatpak session integration

- [ ] **Host prerequisites** — document `flatpak`, `xdg-desktop-portal`,
      `xdg-desktop-portal-gtk`, user in `input` / `video` / `render` groups
- [ ] **Session env** — verify `XDG_DATA_DIRS` includes Flatpak exports
      (`~/.local/share/flatpak/exports`, `/var/lib/flatpak/exports`) through
      `metis-session` activation env
- [ ] **App launcher** — discover Flatpak `.desktop` entries alongside native apps
- [ ] **Window identity** — optional `StartupWMClass` / `X-Flatpak` hints for Steam
      and common game clients

### B. Portal completeness for sandboxed apps

- [ ] **Inhibit portal** — block idle blank / suspend / lock while a game or media
      app holds an inhibit request (high value for gaming)
- [ ] **ScreenCast live pump** — ~~finish Phase 3 PipeWire frame streaming~~ done
      (OBS / Discord / browser share); remaining: dmabuf zero-copy perf pass
- [ ] **Background / PowerProfile / GameMode** — route or stub via portal (can no-op
      initially; GameMode may integrate `gamemoded` later)
- [ ] **Permission UX docs** — `flatpak permission-show`, portal permission reset,
      `~/.local/share/xdg-desktop-portal/` permission files

### C. Controllers & input (host + Flatpak)

- [ ] **Flatpak override guide** — document `flatpak override --user --device=all`
      for controller-heavy games; `--device=dri` alone is often insufficient
- [ ] **Settings → Gaming / Input** — read-only list of connected gamepads
      (`/proc/bus/input/devices` or libudev); no compositor evdev grab that blocks
      games
- [ ] **libinput audit** — confirm compositor does not exclusively grab gamepad
      nodes opened via libinput
- [ ] **Touch (optional)** — `wl_touch` on seat for touchscreen Flatpak apps
      (separate from gamepads)

### D. Steam & Proton (SteamOS-class desktop gaming)

Steam on Metis runs as a **normal Wayland client** (native `.deb` or Flatpak
`com.valvesoftware.Steam`). Games launch as child processes with Proton/Wine;
most do **not** go through compositor gamepad protocols — they use evdev, Steam
Input, and SDL. SteamOS Desktop Mode uses KDE today; Metis aims to be a viable
alternative desktop with the same Steam/Proton stack.

- [ ] **Native Steam (.deb)** — document Valve repo install on Ubuntu/Debian;
      prerequisites: 32-bit (`i386`), Vulkan (`mesa-vulkan-drivers`,
      `lib32-mesa-vulkan-drivers`), PipeWire/Pulse, `steam-devices` udev rules
- [ ] **Flatpak Steam** — document `com.valvesoftware.Steam` from Flathub;
      pressure-vessel / `~/.steam` layout; portal and `device` permissions
- [ ] **Proton** — verify Proton Experimental / GE-Proton launch; document common
      failures (missing i386, wrong default GPU on hybrid laptops → `METIS_DRM_DEVICE`
      / DRI_PRIME)
- [ ] **Gamescope (optional)** — per-game launch option
      (`gamescope -W … -H … -- %command%`) as nested compositor; session-wide
      wrapper for Big Picture-style use; verify focus, overlay, and multi-monitor
- [ ] **Big Picture / `-gamepadui`** — `.desktop` / menu entry; fullscreen and
      controller navigation without keyboard
- [ ] **Steam Input & hardware** — Steam Controller, Deck controls, Switch Pro,
      etc. via Steam's user-space mapping; confirm Metis/libinput does not grab
      exclusive evdev access on gamepad nodes
- [ ] **Steam overlay** — audit XWayland + native Wayland games (shift+tab);
      fullscreen unredirect / focus issues
- [ ] **Steam Remote Play / Link** — depends on ScreenCast portal + PipeWire pump
      (Phase 3 / §B); host-side streaming encode is out of scope initially
- [ ] **Power while gaming** — Inhibit portal + logind idle/sleep block (Steam sets
      these during gameplay); tie-in with Power settings performance profile

### E. SteamOS & handheld compatibility (optional / stretch)

Running **Metis on SteamOS** (replacing Desktop Mode) or on Deck-class hardware
is a stretch goal — SteamOS is immutable/read-only and ships Gamescope for Gaming
Mode. Track compatibility either way:

- [ ] **Steam Deck / handheld inputs** — SD card reader, volume buttons, gyro
      (where exposed as evdev) documented or passed through to Steam Input
- [ ] **SteamOS host notes** — if experimenting on SteamOS Desktop: read-only
      root, `steamos-readonly disable`, where to install `metis-compositor` without
      breaking Valve updates (document only; not officially supported initially)
- [ ] **Gamescope vs Metis** — clarify roles: Metis = session compositor;
      Gamescope = optional per-game nested compositor (SteamOS Gaming Mode uses
      Gamescope *instead of* a full DE, not alongside one)

### F. Gaming polish (optional)

- [ ] **gamemoded** — CPU governor / scheduler hints via GameMode portal or D-Bus
      (Steam can invoke `gamemoderun` in launch options)
- [ ] **Fullscreen / pointer confinement** — audit Proton / XWayland game behaviour
- [ ] **XWayland game notes** — document games that still require X11 socket vs
      native Wayland
- [ ] **MangoHud / vkBasalt** — document `%command%` prefix patterns in Steam
      launch options (no Metis code required)

---

## Phase 7 — Remote access (full desktop sharing)

Let you **remote into a Metis machine from another device** (laptop, tablet,
phone) with full interactive control — not just “share screen” in a call.
ScreenCast (Phase 3) covers **local video capture** for portal apps; remote
desktop also needs **remote input injection**, **network transport**, and
**session security**. Third-party servers (RustDesk, RDP) may work incidentally
today when installed on the host; this phase makes remote access a documented,
tested, and optionally Metis-integrated capability.

**Target:** from another machine on LAN or over the internet, connect to a Metis
session and see + control the desktop (windows, bar, games) with acceptable
latency and clear setup docs.

### A. Third-party remote desktop (document + verify)

- [ ] **RustDesk** — document host install (`rustdesk` server / headless), firewall
      ports, and Wayland capture path (portal ScreenCast / PipeWire vs RustDesk’s
      own capture); verify remote mouse/keyboard into Metis compositor + XWayland
      clients under the DRM session
- [ ] **RDP (GNOME Remote Desktop / xrdp)** — document enabling RDP on Metis
      (`gnome-remote-desktop`, `xrdp` + XWayland fallback); note which stack works
      on pure-Wayland vs XWayland apps; test from Windows/macOS/Linux RDP clients
- [ ] **Other tools** — spot-check AnyDesk, Chrome Remote Desktop, TigerVNC /
      `wayvnc` where relevant; capture known-good / known-broken matrix in dev docs
- [ ] **Settings → System → Remote access** (optional) — read-only status: which
      services are installed/running, port hints, link to setup docs (no secrets in
      the UI)

### B. Metis-native / portal integration (longer term)

- [ ] **Remote input path** — audit compositor input routing so injected pointer/
      keyboard events from a remote server reach focused Wayland/XWayland clients
      reliably (multi-monitor, per-output workspaces, layer-shell bar)
- [ ] **ScreenCast as capture backend** — optional: remote servers that consume
      portal PipeWire streams use `metis-portal` instead of brittle screencopy;
      follow-up: dmabuf zero-copy (Phase 3 perf item) for lower latency
- [ ] **First-party remote option** (stretch) — lightweight Metis remote viewer/
      host or official RustDesk/RDP preset in `metis-session` (TBD; depends on
      security review and maintenance cost)

### C. Security & session policy

- [ ] **Firewall / LAN-only defaults** — document ufw/nftables rules; warn against
      exposing RDP/RustDesk to the open internet without VPN or strong auth
- [ ] **Session lock** — remote session behaviour when Metis is locked / idle
      (Inhibit portal + logind integration from Phase 6)
- [ ] **Multi-user / VT** — clarify behaviour when switching TTYs or multiple seats

---

## Config

Config lives under `~/.config/metis/`. Written on first run: `bar.json`,
`clock.json`, `calendars.json`, `themes/dark.json`, `themes/light.json`. Created
on demand: `config.json` (on preference change), `menu.json` (launcher defaults /
pins), `wallpaper.json` (background pick), `weather.json` (weather setup),
`dismissed.json`, `desk.json` (compositor), `briefing.json` (optional, user-created).

| File | Purpose |
|------|---------|
| `bar.json` | Edge bar layout, widgets, workspaces, borders, default layout |
| `clock.json` | World clocks and alarms |
| `calendars.json` | Calendar accounts |
| `config.json` | Active theme, onboarding state, briefing-on-login |
| `menu.json` | App launcher: terminal + file-manager defaults, pinned apps |
| `wallpaper.json` | Wallpaper picture / colour / gradient (+ per-output overrides) |
| `desk.json` | Compositor window-grid layout (widget tiles) |
| `themes/dark.json`, `themes/light.json` | Design tokens (accent + secondary accent, semantic colors, `text_on_accent`, shadows/glows); live-reloaded |
| `briefing.json` | Weather coordinates + RSS feed URL |
| `weather.json` | Bar weather: unit, auto-detect, IP-geolocation, saved locations |
| `input.json` | Mouse, touchpad, and keyboard settings (compositor live-reload) |
| `power.json` | Power profile, idle blank/suspend timeouts, lid-close action |
| `outputs.json` | Per-output scale, enabled, night-light prefs (UI writes today; compositor apply + resolution/mirror/extend in Phase 5) |

### `bar.json` (defaults)

```json
{
  "position": "top",
  "height": 36,
  "width": 48,
  "margin_top": 8,
  "margin_h": 10,
  "full_width": true,
  "opacity": 0.92,
  "menu_opacity": 0.92,
  "titlebar_opacity": 1.0,
  "titlebar_pill_border": {
    "mode": "accent",
    "color": "#00F2FE",
    "gradient": ["#00F2FE", "#4FACFE", "#A24BFF"],
    "width_px": 1.0
  },
  "window_border": {
    "mode": "accent",
    "color": "#00F2FE",
    "gradient": ["#00F2FE", "#4FACFE", "#A24BFF"],
    "width_px": 1.0
  },
  "bar_border": {
    "mode": "accent",
    "color": "#00F2FE",
    "gradient": ["#00F2FE", "#4FACFE", "#A24BFF"],
    "width_px": 1.0
  },
  "blur": true,
  "blur_radius": 18.0,
  "widgets": [
    "workspaces",
    "tasks",
    "spacer",
    "weather",
    "battery",
    "network",
    "volume",
    "notifications",
    "clock"
  ],
  "clock": {
    "time_format": "%I:%M %p",
    "date_format": "%a %b %d",
    "timezones": ["UTC"]
  },
  "workspace_count": 4,
  "workspace_mode": "separate",
  "default_layout": "grid",
  "taskbar_pinned": []
}
```

| Field | Meaning |
|-------|---------|
| `position` | `top`, `bottom`, `left`, or `right` edge (Settings → Appearance → Edge bar) |
| `height` | Bar thickness when `position: top`/`bottom` |
| `width` | Bar thickness when `position: left`/`right` |
| `margin_top` | Distance from the anchored screen edge (all positions) |
| `margin_h` | Margin along the bar's long axis |
| `full_width` | Span the entire edge vs. hug content |
| `opacity` | Pill background opacity (0–1); enables a see-through bar |
| `menu_opacity` | App launcher panel background opacity (0–1); text/icons stay opaque |
| `titlebar_opacity` | Server-side titlebar background opacity (0–1); title/buttons stay opaque |
| `titlebar_pill_border.mode` | Focused title-pill border: `accent` (theme accent gradient), `solid`, or `gradient` |
| `titlebar_pill_border.color` | Stroke color (`#rrggbb`) when `mode: solid` |
| `titlebar_pill_border.gradient` | Stops (`#rrggbb`), left→right, when `mode: gradient` |
| `titlebar_pill_border.width_px` | Stroke thickness in pixels (0–8) |
| `window_border.mode` | Focused window frame border: `accent`, `solid`, or `gradient` (independent of the pill) |
| `window_border.color` | Frame stroke color (`#rrggbb`) when `mode: solid` |
| `window_border.gradient` | Stops (`#rrggbb`), top→bottom, when `mode: gradient` |
| `window_border.width_px` | Frame thickness in pixels (0–16); also insets the client body |
| `bar_border.mode` | Edge-bar pill border: `accent` (theme accent gradient), `solid`, or `gradient` |
| `bar_border.color` | Stroke color (`#rrggbb`) when `mode: solid` |
| `bar_border.gradient` | Stops (`#rrggbb`), along the bar's long axis, when `mode: gradient` |
| `bar_border.width_px` | Stroke thickness in pixels (0 disables the border) |
| `blur` | Enable the compositor Gaussian backdrop blur behind the bar |
| `blur_radius` | Blur strength in pixels (1–64) when `blur` is on |
| `widgets` | Ordered list; `spacer` pushes following widgets apart. Includes `tasks` (the running-apps dock) |
| `clock.time_format` / `date_format` | `chrono` format strings |
| `clock.timezones` | Extra zones listed in the calendar popover |
| `workspace_count` | Number of workspace indicator dots (1–12) |
| `workspace_mode` | Multi-monitor workspace behavior: `separate` (each output independent) or `linked` (all outputs switch together). Settings → Appearance → Edge bar → Workspaces |
| `default_layout` | Layout mode: `grid` (tiling) or `scroll` (niri-style strip). Changing it in Settings → Appearance → Edge bar → New workspace layout applies live to every workspace; `Super`+`\` toggles a single workspace |
| `taskbar_pinned` | App ids pinned to the `tasks` dock, in order (independent of `menu.json` launcher pins) |

Edit `bar.json` while the shell runs — changes apply within ~1s. Cosmetic fields
(opacity, tray mode, margins, blur, borders, taskbar pins) update in place without
closing popovers; structural changes (widgets, position, displays, clock format,
workspace count) trigger a full widget rebuild. The compositor also re-reads
`blur`/`blur_radius` live. Legacy layouts are migrated to the current defaults
automatically. Editing `themes/dark.json` / `themes/light.json` re-applies the
active theme live as well.

---

## Run

```bash
cd metis-os-workspace/metis-shell
./run-metis.sh --build --session
```

| Flag | Effect |
|------|--------|
| `--session` | Start Metis compositor + shell (nested dev session) |
| `--import-env` | (With `--session`) route D-Bus/systemd-activated apps into the nested session, restored on exit |
| `--build` | Force a rebuild before running |
| `--release` | Use optimized binaries |
| `--foreground` | Run in the foreground instead of backgrounding the shell |
| `--stop` | Stop the background shell process |
| `--verify` | Check keybind / socket health while running |
| `--verify-grid` | Compare compositor vs shell grid layouts |
| `-- -c <app>` | Spawn a client app inside the session |

Session mode disables wallpaper and briefing by default (`METIS_NO_WALLPAPER=1`, `METIS_NO_BRIEFING=1`). Re-enable with:

```bash
METIS_NO_WALLPAPER= METIS_NO_BRIEFING= ./run-metis.sh --session
```

Send runtime commands to a running shell with `scripts/metis-cmd.sh {close-popovers|reload-bar}`.

---

## Widget map

| Widget | Source |
|--------|--------|
| Workspaces | Metis compositor — live virtual workspaces (single output) |
| Tasks | Running-apps dock — live compositor window state (`services/windows.rs`), per-app grouping, pin/minimize |
| Clock | `chrono` + `GtkCalendar`, tabbed popover (world clocks, stopwatch, timer, alarms) |
| Battery | `/sys/class/power_supply/BAT*` |
| Bluetooth | BlueZ (`bluetoothctl`) + UPower + optional Solaar (Logitech HID++); bar popover lists connected devices |
| Network | `nmcli` (timeouts + scan grace for stable Wi-Fi icon) |
| Volume | `pactl` (scroll on widget to adjust) |
| Notifications | Freedesktop D-Bus daemon → runtime in-bar store (grouped, icons) |
| Weather | Open-Meteo (keyless); IP-geolocation auto-detect (timezone fallback), override in Settings |

> Tip: set `METIS_DEMO_NOTIFICATIONS=1` before launching to seed demo notifications
> for testing the notification popup.
