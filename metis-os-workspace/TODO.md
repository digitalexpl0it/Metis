# Metis Shell — Edge Bar (v2)

**Current phase:** Phase 1 — Configurable edge bar on the Metis Smithay compositor.

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
- [ ] Theme file watcher (live `themes/*.json` reload)
- [ ] Freedesktop notification D-Bus subscription
- [ ] WiFi / audio popover controls

---

## Config

Config lives under `~/.config/metis/` (falls back to `~/.config/metis/`, then
`$HOME/.config/metis`). Defaults are written on first run:

| File | Purpose |
|------|---------|
| `config.json` | Active theme, onboarding state, briefing-on-login |
| `bar.json` | Edge bar layout and widgets |
| `desk.json` | Compositor window-grid layout |
| `themes/dark.json`, `themes/light.json` | Design tokens |
| `briefing.json` | Weather coordinates + RSS feed URL |

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
  "widgets": [
    "workspaces",
    "spacer",
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
| `opacity` / `blur` | Pill background opacity and blur |
| `widgets` | Ordered list; `spacer` pushes following widgets apart |
| `clock.time_format` / `date_format` | `chrono` format strings |
| `clock.timezones` | Extra zones listed in the calendar popover |
| `workspace_count` | Number of workspace indicator dots (1–12) |

Edit `bar.json` while the shell runs — changes apply within ~1s. Legacy layouts
are migrated to the current defaults automatically.

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
| Notifications | Runtime in-bar store (grouped, icons); freedesktop D-Bus next |

> Tip: set `METIS_DEMO_NOTIFICATIONS=1` before launching to seed demo notifications
> for testing the notification popup.
