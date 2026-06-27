# Metis Shell — Edge Bar (v2)

**Current phase:** Phase 1 (edge bar, incl. the ArcMenu-style app launcher) and
Phase 2 (`metis-settings` app + server-side window decorations) are complete —
decoration polish, theme-aware + translucent auto-hiding titlebars, edge snapping,
XWayland, and a taskbar / running-apps dock. Phase 3 (multi-monitor, workspaces &
tiling) is largely done: the output-agnostic refactor, per-output edge bars +
wallpaper + usable areas, independent **or** linked per-output virtual workspaces
(live bar dots + `Super`+`n`), a dock that follows the output + workspace, an
optional niri/PaperWM-style **scrolling layout** selectable per workspace
(`Super`+`\`), and **cross-output window moves** (drag/snap adopt, `Super`+`Shift`+`←`/`→`,
`MoveWindowToOutput` IPC). Remaining in Phase 3: whole-workspace cross-output
moves, richer automatic tiling, and the DRM/udev backend (deferred). Next: Phase 4
(settings-app expansion into a full control center) and Phase 5 (display pipeline:
VRR, colour management, HDR).

---

## Phase 1 — Edge bar

- [x] `bar.json` config — position (top/bottom/left/right), height/width, opacity, widget order
- [x] Live reload when `bar.json` changes
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
- [x] Notifications popup — badge, grouped duplicates + count badge, per-kind icons,
      clear-all with slide-out animation, scrollbar, in-bar alert routing
- [x] WiFi / audio popover controls
- [x] Weather widget — icon + temperature with a forecast popover (see Phase 2)
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
      event; `Super`+`1..9` / `Super`+`Shift`+`1..9` keybinds; bar dots wired
      through IPC; `WindowInfo.workspace` populated. Count from `bar.json`
      `workspace_count`. (Per-output split comes with the refactor below.)
- [~] **Output-agnostic refactor** — remove the `space.outputs().next()` /
      single-monitor assumptions; thread an output id through placement, grid,
      snapping, decorations, and IPC. _Started:_ centralized output-geometry
      helpers (`primary_output`/`output_rect`/`primary_monitor_rect`) now back
      `grid_metrics`/`usable_zone`/`placement_zone`/`set_fullscreen`/
      `arrange_layers`. _Done:_ absolute-pointer mapping spans all outputs
      (`desktop_bounds`) and titlebar drags clamp to the whole desktop; per-output
      placement/snap routing — new windows open on the output under the cursor,
      drag-to-edge snaps tile on the hovered output, maximize fills the window's
      own output, and floating windows clamp within their own monitor
      (`output_at`/`output_under_pointer`/`output_for_window` +
      `*_zone_for(output)` zone helpers). _Remaining:_ per-output grid/tiling
      (lands with per-output workspaces), `GetMonitors`-style protocol surface.
- [~] **Virtual outputs under winit** — `METIS_VIRTUAL_OUTPUTS=2` tiles the
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
      _Remaining (later):_ move a whole workspace between outputs.
- [ ] **Automatic dynamic tiling** — richer reflow beyond the manual grid
- [x] **Scrolling layout option** — niri/PaperWM/mango-style horizontally scrolling
      workspace, selectable per-workspace as a second mode in `metis-grid`
      (`scroll.rs`: `ScrollState` of columns, each a vertical window stack). App
      tiles stay the membership/stash source of truth; scroll mode only overrides
      pixel placement + hit-testing. Toggle live with `Super`+`\`; settings default
      for new workspaces via `bar.json#default_layout`. Keybinds (scroll workspace
      only): `Super`+arrows focus, `Super`+`Shift`+arrows move, `Super`+`,`/`.`
      consume/expel a window into/out of a column stack, `Super`+`-`/`=` cycle column
      width. `SetWorkspaceLayout` IPC. _Remaining (later):_ scroll animation, cross-
      output clamping of off-screen columns, vertical stacking polish.
- [x] **Taskbar follows** — each output's dock shows only the windows on that
      output's currently-visible workspace (pinned launchers persist everywhere).
      `WindowInfo.output` carries the monitor name; the dock filters by
      `(output, active workspace)`, repaints on workspace switch, and dedups per
      bar. The "Per-output state" per-output dock item is now also covered.
- [ ] DRM/udev backend (real multi-seat sessions) — deferred until the above lands
- [ ] **Settings portal (`org.freedesktop.portal.Settings`)** — once Metis runs as
      a standalone DE (not nested in GNOME), serve an empty GTK decoration layout so
      stubborn CSD apps (e.g. GTK3 Cheese) drop their own close/min/max buttons and
      only Metis's server-side controls show; also a clean handoff for theme / font /
      dark-mode prefs to clients. (Avoided while nested — it would alter the host
      GNOME session live.)

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

- [ ] **Mouse** — pointer speed / acceleration, acceleration profile (flat vs
      adaptive), natural scroll, primary button (left/right-handed), scroll speed
- [ ] **Touchpad** — shown only when a touchpad device is present: tap-to-click,
      tap-and-drag, two-finger / edge scroll, natural scroll, disable-while-typing,
      palm rejection, pointer speed
- [ ] **Keyboard** — layout(s) + switch shortcut (xkb), repeat delay + rate, compose
      key, Caps/Esc remap; applied via Smithay's xkb config + `wl_keyboard`
      `repeat_info`

### Devices (D-Bus services)
- [ ] **Bluetooth** — adapter on/off, scan, pair / connect / trust / remove, battery
      level where reported; via BlueZ (`org.bluez`) over D-Bus. Add a bar indicator
      that appears only when an adapter is present
- [ ] **Printers** — list / add / manage printers + queues via CUPS (IPP / D-Bus);
      may begin as a thin launcher for an existing tool, then go native

### System
- [ ] **Power / Battery** — power profiles (power-saver / balanced / performance) via
      power-profiles-daemon, battery details + health via UPower, idle-dim / blank /
      suspend timeouts, lid-close action; feeds the existing bar battery widget
- [ ] **Sound** — output / input device selection, per-device volume + mute, default
      sink / source, balance, and (later) per-app volume; via PipeWire / PulseAudio.
      Feeds the existing bar volume control
- [ ] **Display** — per-output resolution, refresh rate, scale, orientation,
      multi-monitor arrangement + primary, VRR toggle, night-light / colour
      temperature; writes output config (`outputs.json`). Depends on the Phase 3
      output-agnostic refactor (and the Phase 5 DRM backend for real mode-setting /
      VRR)

---

## Phase 5 — Display pipeline (HDR / VRR / colour)

Advanced output features, all gated on the real DRM/udev backend (deferred in
Phase 3) — none of these are possible under the nested winit dev session.

- [ ] **VRR / adaptive sync** — enable per-output via Smithay's DRM VRR support;
      opt-in toggle surfaced in the Display settings page
- [ ] **Colour management** — ICC / per-output colour profiles and the
      `wp_color_management` protocol; groundwork shared with HDR
- [ ] **Night light / colour temperature** — scheduled warm-shift (also exposed in
      Display settings); a simpler precursor that exercises the colour pipeline
- [ ] **HDR** — wide-gamut / 10-bit GLES render path + DRM colour pipeline on top of
      colour management (long-term; gated on protocol + Smithay maturity). A genuinely
      rare thing for a lightweight DE — worth doing right rather than fast

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

Edit `bar.json` while the shell runs — changes apply within ~1s (the compositor
also re-reads `blur`/`blur_radius` live). Legacy layouts are migrated to the
current defaults automatically. Editing `themes/dark.json` / `themes/light.json`
re-applies the active theme live as well.

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
| Network | `nmcli` or sysfs fallback |
| Volume | `pactl` (scroll on widget to adjust) |
| Notifications | Freedesktop D-Bus daemon → runtime in-bar store (grouped, icons) |
| Weather | Open-Meteo (keyless); IP-geolocation auto-detect (timezone fallback), override in Settings |

> Tip: set `METIS_DEMO_NOTIFICATIONS=1` before launching to seed demo notifications
> for testing the notification popup.
