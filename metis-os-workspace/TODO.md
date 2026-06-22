# Metis Shell — Edge Bar (v2)

**Current phase:** Phase 1 (edge bar, incl. the ArcMenu-style app launcher)
complete; Phase 2 (`metis-settings` app + server-side window decorations) complete,
including decoration polish (rounded button glyphs + focus-aware dimming),
theme-aware + translucent titlebars with an auto-hide overlay for maximized /
edge-snapped windows, and XWayland support. A taskbar / running-apps dock has
landed on the edge bar (live window state over IPC). Next: Phase 3 — broader
window management (multi-monitor, workspaces, richer tiling).

---

## Phase 1 — Edge bar

- [x] `bar.json` config — position (top/left/right), height/width, opacity, widget order
- [x] Live reload when `bar.json` changes
- [x] Workspace indicator (single Metis desktop for now)
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

- [ ] **Output-agnostic refactor** — remove the `space.outputs().next()` /
      single-monitor assumptions; thread an output id through placement, grid,
      snapping, decorations, and IPC
- [ ] **Per-output state** — each output owns its own usable area, grid, and
      wallpaper; the bar (and dock) become per-output
- [ ] **Per-output workspaces** — Hyprland-style: each workspace is its own grid;
      switch/move-to-workspace keybinds + IPC; the workspaces widget drives them
- [ ] **Virtual outputs under winit** — simulate multiple monitors in the nested
      dev session for testing before real DRM/udev
- [ ] **Cross-output moves** — move windows (and whole workspaces) between outputs
- [ ] **Automatic dynamic tiling** — richer reflow beyond the manual grid
- [ ] Taskbar follows: filter the dock to the current output + active workspace
      (the `WindowInfo.output`/`workspace` fields are already reserved)
- [ ] DRM/udev backend (real multi-seat sessions) — deferred until the above lands
- [ ] **Settings portal (`org.freedesktop.portal.Settings`)** — once Metis runs as
      a standalone DE (not nested in GNOME), serve an empty GTK decoration layout so
      stubborn CSD apps (e.g. GTK3 Cheese) drop their own close/min/max buttons and
      only Metis's server-side controls show; also a clean handoff for theme / font /
      dark-mode prefs to clients. (Avoided while nested — it would alter the host
      GNOME session live.)

---

## Config

Config lives under `~/.config/metis/`. Written on first run: `bar.json`,
`clock.json`, `calendars.json`, `themes/dark.json`, `themes/light.json`. Created
on demand: `config.json` (on preference change), `dismissed.json`, `desk.json`
(compositor), `briefing.json` (optional, user-created).

| File | Purpose |
|------|---------|
| `bar.json` | Edge bar layout and widgets |
| `clock.json` | World clocks and alarms |
| `calendars.json` | Calendar accounts |
| `config.json` | Active theme, onboarding state, briefing-on-login |
| `desk.json` | Compositor window-grid layout |
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
  "taskbar_pinned": []
}
```

| Field | Meaning |
|-------|---------|
| `position` | `top`, `left`, or `right` edge |
| `height` | Bar thickness when `position: top` |
| `width` | Bar thickness when `position: left`/`right` |
| `margin_top` | Gap between the bar and the screen edge |
| `margin_h` | Margin along the bar's long axis |
| `full_width` | Span the entire edge vs. hug content |
| `opacity` | Pill background opacity (0–1); enables a see-through bar |
| `menu_opacity` | App launcher panel background opacity (0–1); text/icons stay opaque |
| `titlebar_opacity` | Server-side titlebar background opacity (0–1); title/buttons stay opaque |
| `blur` | Enable the compositor Gaussian backdrop blur behind the bar |
| `blur_radius` | Blur strength in pixels (1–64) when `blur` is on |
| `widgets` | Ordered list; `spacer` pushes following widgets apart. Includes `tasks` (the running-apps dock) |
| `clock.time_format` / `date_format` | `chrono` format strings |
| `clock.timezones` | Extra zones listed in the calendar popover |
| `workspace_count` | Number of workspace indicator dots (1–12) |
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
| Workspaces | Metis compositor (single desktop stub) |
| Tasks | Running-apps dock — live compositor window state (`services/windows.rs`), per-app grouping, pin/minimize |
| Clock | `chrono` + `GtkCalendar`, tabbed popover (world clocks, stopwatch, timer, alarms) |
| Battery | `/sys/class/power_supply/BAT*` |
| Network | `nmcli` or sysfs fallback |
| Volume | `pactl` (scroll on widget to adjust) |
| Notifications | Freedesktop D-Bus daemon → runtime in-bar store (grouped, icons) |
| Weather | Open-Meteo (keyless); IP-geolocation auto-detect (timezone fallback), override in Settings |

> Tip: set `METIS_DEMO_NOTIFICATIONS=1` before launching to seed demo notifications
> for testing the notification popup.
