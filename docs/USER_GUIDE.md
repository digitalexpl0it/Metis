# Metis User Guide

Welcome to **Metis** — a Wayland desktop environment built on a custom Smithay
compositor with a GTK4 layer-shell edge bar. This guide covers everyday use:
launching a session, the edge bar, managing windows, workspaces, the scrolling
layout, keyboard shortcuts, and the Settings app.

For installation and build prerequisites, see [`UBUNTU_DEV.md`](UBUNTU_DEV.md).

---

## 1. Launching Metis

Metis currently runs as a **nested session** inside your existing Wayland session
(via the winit backend) — ideal for development and trying it out.

```bash
cd metis-os-workspace/metis-shell

./run-metis.sh --session            # start the compositor + shell
./run-metis.sh --build --session    # rebuild first, then start
./run-metis.sh --stop               # stop a running session
```

The compositor opens a window that *is* your Metis desktop. The shell (edge bar)
is spawned automatically.

**Wallpaper & briefing.** A nested dev session turns the wallpaper and login
briefing off by default. Turn them on for the run:

```bash
METIS_NO_WALLPAPER= METIS_NO_BRIEFING= ./run-metis.sh --session
```

**Multiple monitors (simulated).** Split the session window into N side-by-side
virtual outputs to test multi-monitor behaviour:

```bash
METIS_VIRTUAL_OUTPUTS=2 ./run-metis.sh --session
```

Each virtual output gets its own edge bar, wallpaper, and workspaces (subject to
your settings).

---

## 2. The desktop at a glance

- **Edge bar** — a thin bar anchored to one screen edge (top by default). It
  holds the app launcher, a taskbar dock of running apps, workspace dots, and
  status widgets (weather, battery, network, volume, notifications, clock).
- **Windows** — every app gets a compositor-drawn **titlebar** with close,
  minimize, and maximize buttons, plus a border. Windows tile into a grid by
  default — opening or closing an app re-splits the area below desk widgets among
  visible tiled windows. You can float, snap, maximize, or switch a workspace into a
  scrolling layout.
- **Popovers** — clicking a bar widget opens an on-demand popover (calendar,
  Wi-Fi, volume, notifications, weather forecast, app launcher). Clicking
  elsewhere dismisses it.

---

## 3. The edge bar

Widgets appear in the order set by `bar.json#widgets`. The defaults:

| Widget | What it does |
|--------|--------------|
| **App launcher** | The brand icon at the start of the bar. Opens the launcher panel (see §4). |
| **Tasks (dock)** | Icons for running (and pinned) apps on this output's current workspace. Click to focus/minimize; right-click to pin/close. |
| **Workspaces** | One dot per workspace; the active one is highlighted. Click a dot to switch (see §6). |
| **Weather** | Condition icon + temperature. Click for a forecast popover with hourly strip and saved locations. |
| **Battery** | Charge level and state (hidden on desktops without a battery). |
| **Network** | Wired/Wi-Fi status. Click for a Wi-Fi popover (scan, connect, forget). |
| **Volume** | Current output volume. Click for a slider + mute. |
| **Notifications** | Bell with an unread badge. Click for grouped notifications; clear all with a slide-out. |
| **Clock** | Date/time. Click for a tabbed popover: calendar, world clocks, stopwatch, timer, and alarms. |

**Per-output bars.** With multiple outputs you can show the bar on **all
displays** (each is independent and live) or **the primary display only** —
configured in Settings → Appearance → Edge bar → *Show bar on*.

**Live editing.** Edit `~/.config/metis/bar.json` while Metis runs; bar changes
apply within about a second. Theme edits (`themes/*.json`) re-apply live too.

---

## 4. Launching and managing apps

### App launcher

Click the brand icon (or the launcher widget) to open the launcher panel. It has:

- **Quick launchers + power actions** — a rail with your terminal, file manager,
  Settings, and power actions. The terminal and file manager are configurable in
  Settings → Metis Menu.
- **App list** — a Frequent/alphabetical list. Just start typing to search
  (no need to click the search box first).
- **Pinnable apps grid** — pin favourites for quick access.

Selecting an app launches it and dismisses the panel.

### Taskbar dock

The dock shows apps running on the **current output and workspace**, grouped by
app identity. A dot marks running apps; the focused app is highlighted; minimized
apps are dimmed.

- **Left-click** — focus the window (or minimize it if already focused). If an
  app has several windows, a picker popover appears. Pinned-but-not-running apps
  launch.
- **Right-click** — pin/unpin the app, or close its window(s).
- The dock scrolls horizontally if it outgrows the bar.

---

## 5. Window management

Metis draws **server-side decorations**, so every window (Wayland or XWayland)
gets a consistent titlebar and border that follow your theme.

- **Move** — drag the titlebar.
- **Close / minimize / maximize** — the three titlebar buttons (× / − / +).
- **Resize** — drag a window border or corner; tiled windows float out of the
  grid when you resize them, and the new geometry is remembered.
- **Snap zones** — drag a window to a screen edge for a live translucent preview,
  then drop to snap into half / quarter / maximize regions. Snapping respects the
  bar, so the top zone clears it.
- **Maximize** — `Super`+`F` toggles maximize for the focused window (fills the
  area below the edge bar, same as the titlebar + button or top-edge snap);
  `Escape` exits maximize, fullscreen, or grid tile mode.
- **Close** — `Super`+`Q`.
- **Geometry memory** — on the default desktop layout, a window you've moved or
  resized reopens at the same position and size next time you launch it (saved per
  app in `~/.config/metis/windows.json`). Off-screen saved positions are pulled
  back on-screen; grid/scrolling workspaces tile instead.

**Auto-hiding titlebars.** Maximized and edge-snapped windows hide their titlebar
so the client fills the space; hover the top strip to reveal it as a translucent
overlay.

Titlebar translucency, the title "pill" border, and the window frame border are
all configurable in Settings → Appearance → Windows.

---

## 6. Workspaces

Each workspace is a separate set of app windows. Switch between them with the bar
dots or the keyboard:

- `Super`+`1` .. `Super`+`9` — switch to that workspace.
- `Super`+`Shift`+`1` .. `9` — move the focused window to that workspace.
- Click a workspace dot in the bar to switch.

Keybinds and clicks act on the monitor under the pointer.

### Per-output behaviour

With multiple displays you choose how workspaces relate, in
Settings → Appearance → Edge bar → *Workspaces*:

- **Independent per display** (default) — each monitor keeps its own set of
  workspaces and its own active one. The dots on each bar reflect that monitor.
- **Linked across displays** — switching a workspace moves every monitor at once,
  so all displays stay on the same virtual desktop number.

The taskbar dock always follows its own bar's output and active workspace.

### Moving windows between monitors

With multiple displays (or `METIS_VIRTUAL_OUTPUTS=2` in a dev session):

- **Drag** — titlebar-drag a window onto another monitor and release; Metis
  re-homes its desk tile to that output automatically (snapping on a secondary
  monitor does the same).
- **Keyboard** — on a **grid** workspace, `Super`+`Shift`+`←` / `→` moves the
  focused window to the adjacent monitor (left-to-right order). On a **scrolling**
  workspace those keys still move columns instead.

The window keeps its workspace number on the destination output (e.g. workspace 2
on monitor A becomes workspace 2 on monitor B). If that workspace is not active
on the destination, the window is stashed until you switch to it there.

**Move the whole workspace** — with independent per-output workspaces,
`Super`+`Ctrl`+`Shift`+`←` / `→` moves every window on the active workspace
(under the pointer) to the same workspace number on the adjacent monitor, including
scroll layout state.

---

## 7. Scrolling layout (niri / PaperWM style)

Any workspace can be a **grid** (the default tiling) or a **scrolling** layout —
an infinite horizontal strip of full-height columns (niri / PaperWM / paneru
style). Each column is one window (or a vertical stack), and the strip extends to
the right as you open more. The viewport scrolls to keep the focused column in
view; off-screen columns are clipped to the current display, so a column scrolled
past the edge never bleeds onto an adjacent monitor.

Opening a new window **never resizes the windows already on the strip** — it just
appends a column. New windows open at half-width.

### Resizing columns

- **Mouse** — drag a window's **right** border to set its width; everything to the
  right slides over to make room. Dragging the **left** border resizes the
  previous window. Columns are full-height, so there's no vertical resize.
- **Keyboard** — `Super`+`-` / `Super`+`=` snaps the focused column to full width,
  then back to half.

### Turning it on

- **Per workspace** — `Super`+`\` toggles the active workspace between grid and
  scrolling.
- **Everywhere** — Settings → Appearance → Edge bar → *New workspace layout*.
  Choosing Grid tiling or Scrolling applies to **every** workspace on **every**
  output immediately (it acts as a global on/off switch).

### Navigating a scrolling workspace

| Shortcut | Action |
|----------|--------|
| `Super`+`←` / `Super`+`→` | Move focus to the previous / next column |
| `Super`+`↑` / `Super`+`↓` | Move focus up / down within the focused column's stack |
| `Super`+`Shift`+`←` / `→` | Move the focused column left / right |
| `Super`+`Shift`+`↑` / `↓` | Move the focused window up / down in its stack |
| `Super`+`,` | Consume: pull the next window into the focused column |
| `Super`+`.` | Expel: push the focused window out into its own column |
| `Super`+`-` / `Super`+`=` | Snap the focused column to full width / back to half (or drag a border to resize) |
| `Super`+`\` | Toggle this workspace back to grid |

These scrolling keybinds are only active while the focused workspace is in
scrolling mode; in grid mode they're inert.

---

## 8. Keyboard shortcuts reference

| Shortcut | Action |
|----------|--------|
| `Super`+`1`..`9` | Switch to workspace 1–9 (monitor under the pointer) |
| `Super`+`Shift`+`1`..`9` | Move the focused window to workspace 1–9 |
| `Super`+`Shift`+`←` / `→` | (grid) Move the focused window to the adjacent monitor |
| `Super`+`Ctrl`+`Shift`+`←` / `→` | Move the active workspace to the adjacent monitor (independent mode) |
| `Super`+`F` | Toggle maximize for the focused window (fills the area below the edge bar) |
| `Super`+`Q` | Close the focused window |
| `Escape` | Exit fullscreen / immersive |
| `Super`+`\` | Toggle the active workspace between grid and scrolling |
| `Super`+`←` `→` `↑` `↓` | (scrolling) Move focus between/within columns |
| `Super`+`Shift`+arrows | (scrolling) Move the column / window |
| `Super`+`,` / `Super`+`.` | (scrolling) Consume into / expel from a column |
| `Super`+`-` / `Super`+`=` | (scrolling) Snap the focused column to full / half width |

**Nested in GNOME?** `./run-metis.sh --session` sets `METIS_MOD=alt` — read **Super** as **Alt** in the table above, and click the Metis window first so it has keyboard focus. On a real Metis session, **Super** is the logo / Windows key.

---

## 9. The Settings app

Launch Settings from the app launcher's quick-launch rail, or from a terminal:

```bash
metis-cmd settings            # open Settings
metis-cmd settings appearance # open a specific page
```

Pages:

- **Appearance** — Light/Dark style; accent, secondary, and semantic status
  colors; bar opacity and backdrop blur. A **background picker** with three
  types: Picture (bundled + imported images, "Add Picture…"), Solid colour, and
  Gradient (start/end + direction) — applied live and remembered, with per-output
  overrides. The **Edge bar** card covers position (top/bottom/left/right),
  distance from the edge, the bar border, *Show bar on* (all displays / primary
  only), *Workspaces* (independent vs linked), and *New workspace layout* (grid
  vs scrolling). The **Windows** card covers titlebar opacity, the title pill
  border, and the window frame border.
- **Metis Menu** — choose your default **terminal** and **file manager** (from
  auto-detected installs or a custom binary path), plus the launcher panel
  opacity. Saved to `menu.json`.
- **Weather** — manual location override + search, multiple saved locations
  (reorder/remove), °F/°C unit, and an IP-geolocation toggle.
- **Network** — wired/NIC config (DHCP vs static) and Wi-Fi scan/connect/forget.
- **Calendars** — calendar accounts (local / CalDAV / Thunderbird / Microsoft
  365) used by the clock popover.

Most appearance and bar changes apply live; some device-backed settings only take
full effect under a real (DRM) session.

---

## 10. Configuration reference

All configuration lives in `~/.config/metis/` as JSON. You can edit files by
hand — `bar.json` and `themes/*.json` reload while Metis runs.

### Nested dev sessions (GNOME / host compositor)

When Metis runs inside another desktop (the default `./run-metis.sh --session`
winit window), the **host grabs Super** for its own shortcuts. Metis shortcuts
won't fire with Super unless you reconfigure the host.

**Default:** nested sessions set `METIS_MOD=alt`, so every shortcut below that
says **Super** means **Alt** instead — e.g. **Alt+1** switches workspace,
**Alt+Shift+←** moves a window to the adjacent monitor. **Click the Metis
session window first** so it has keyboard focus.

Override with `METIS_MOD=super` or `METIS_MOD=ctrl` if you prefer. On a real
Metis session (future DRM backend), the default is Super.

| File | Purpose |
|------|---------|
| `bar.json` | Edge bar position/size/opacity/blur, widget order, workspaces, borders, default layout |
| `clock.json` | World clocks and alarms |
| `calendars.json` | Calendar accounts |
| `themes/dark.json`, `themes/light.json` | Design tokens — accents, semantic colors, `text_on_accent`, shadows/glows |
| `config.json` | Active theme, onboarding state, briefing-on-login |
| `menu.json` | App launcher terminal / file-manager defaults and pinned apps |
| `wallpaper.json` | Background picture / colour / gradient, plus per-output overrides |
| `weather.json` | Bar weather: unit, auto-detect, IP-geolocation, saved locations |
| `desk.json` | Compositor window-grid layout (widget tiles) |
| `dismissed.json` | Dismissed calendar reminder IDs |
| `briefing.json` | Login-briefing weather coordinates + RSS feed (optional) |

### Key `bar.json` fields

| Field | Meaning |
|-------|---------|
| `position` | `top` / `bottom` / `left` / `right` |
| `height` / `width` | Bar thickness on the long / short axes |
| `margin_top` / `margin_h` | Gap from the anchored edge / along the edge |
| `full_width` | Stretch the bar across the whole edge |
| `opacity` | Bar background opacity (`< 1` = see-through) |
| `menu_opacity` | App launcher panel opacity |
| `blur` / `blur_radius` | Compositor backdrop blur behind the bar |
| `displays` | Show the bar on all displays or the primary only |
| `workspace_mode` | Workspaces independent per display, or linked across displays |
| `default_layout` | Default workspace layout: grid or scrolling (live global switch) |
| `titlebar_opacity` | Window titlebar background opacity |
| `titlebar_pill_border` / `window_border` / `bar_border` | Border style (accent gradient / solid / custom gradient + width) |
| `widgets` | Ordered list of bar widgets |
| `taskbar_pinned` | Apps pinned to the dock |

Prefer the Settings app for most of these — it writes the same files and applies
changes live.

---

## 11. Troubleshooting

| Symptom | Try |
|---------|-----|
| Bar or popovers don't appear | Confirm a Wayland session (`echo $WAYLAND_DISPLAY`) and that `libgtk-4-layer-shell` is installed |
| Session won't start / behaves oddly | `./run-metis.sh --stop` then `./run-metis.sh --build --session` |
| Theme looks wrong | Delete `~/.config/metis/themes/*.json` and restart to regenerate |
| Verify the shell is reachable | `./run-metis.sh --verify` |
| Compare compositor vs shell grid | `./run-metis.sh --verify-grid` |

Logs are written to `~/.local/state/metis/logs/` (`latest.log` points at the most
recent run).

---

Questions, roadmap, and recent changes: see
[`../metis-os-workspace/TODO.md`](../metis-os-workspace/TODO.md) and
[`../CHANGELOG.md`](../CHANGELOG.md).
