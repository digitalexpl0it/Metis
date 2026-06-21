# Metis Shell — Edge Bar (v2)

**Current phase:** Phase 1 (edge bar) complete; Phase 2 (`metis-settings` app +
server-side window decorations) landed. Next: decoration polish (rounded button
glyphs, focus-aware styling, border resize) and broader window management.

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
- [ ] Polish — rounded button glyphs, focus-aware dimming
- [ ] Snap zones / hot-spots — half-screen + maximize overlays when dragging a
      window to a screen edge

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
  "blur": true,
  "blur_radius": 18.0,
  "widgets": [
    "workspaces",
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
  "workspace_count": 4
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
| `blur` | Enable the compositor Gaussian backdrop blur behind the bar |
| `blur_radius` | Blur strength in pixels (1–64) when `blur` is on |
| `widgets` | Ordered list; `spacer` pushes following widgets apart |
| `clock.time_format` / `date_format` | `chrono` format strings |
| `clock.timezones` | Extra zones listed in the calendar popover |
| `workspace_count` | Number of workspace indicator dots (1–12) |

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
| Clock | `chrono` + `GtkCalendar`, tabbed popover (world clocks, stopwatch, timer, alarms) |
| Battery | `/sys/class/power_supply/BAT*` |
| Network | `nmcli` or sysfs fallback |
| Volume | `pactl` (scroll on widget to adjust) |
| Notifications | Freedesktop D-Bus daemon → runtime in-bar store (grouped, icons) |
| Weather | Open-Meteo (keyless); IP-geolocation auto-detect (timezone fallback), override in Settings |

> Tip: set `METIS_DEMO_NOTIFICATIONS=1` before launching to seed demo notifications
> for testing the notification popup.
