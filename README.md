# Metis

> **Beta** — Metis is under active development. Expect rough edges: session setup,
> window management, and configuration formats may change between releases. Bug
> reports and feedback are welcome.

> **Metis** is a next-generation Wayland desktop environment built in Rust. The
> **Metis compositor** owns the Wayland session, the window grid, and the
> wallpaper; the **Metis shell** is a GTK4 layer-shell edge bar (plus on-demand
> popovers, Notification Center, Control Center, and optional desktop widgets)
> spawned by the compositor.

New to Metis? Start with the **[User Guide](docs/USER_GUIDE.md)**.

## Screenshots

**Desktop** — edge bar, workspaces, weather, and server-side window decorations on a
theme-aware wallpaper.

![Metis desktop with edge bar, workspaces, and tiled windows](Screenshots/metis_desktop.png)

**Control Center** — pull-down system monitor with live charts and a searchable process
list (Settings → Control Center to configure).

![Metis Control Center with CPU, memory, and process list](Screenshots/metis_control_center.png)

**Settings** — grouped sidebar for display, appearance, connectivity, input, gaming,
and system configuration.

![Metis Settings control center](Screenshots/metis_settings.png)

## Philosophy

- **Performance first** — idiomatic, low-overhead Rust with `tokio` async and damage-driven rendering.
- **Compositor-first** — a Smithay compositor owns the session; the shell is spawned by it.
- **On-demand shell** — `wlr-layer-shell` overlays (edge bar, launcher, popovers,
  Notification Center, Control Center, optional desktop widgets) summoned when
  needed and torn down cleanly.

## Workspace layout

```
.
├── metis-os-workspace/          # Cargo workspace
│   ├── assets/                  # Wallpapers, portal registration, session launcher
│   ├── metis-capture/           # Shared Wayland ext-image-copy-capture client (shell + portal)
│   ├── metis-compositor/        # Smithay Wayland compositor (winit nested backend for dev)
│   ├── metis-config/            # Shared config + theme-token types (serde, no GTK)
│   ├── metis-gaming/            # Flatpak optimizer, health checks, metis-gamingd daemon
│   ├── metis-grid/              # Window grid / tiling + scrolling layout engine (pure logic)
│   ├── metis-portal/            # xdg-desktop-portal backend (Settings, Screenshot, ScreenCast)
│   ├── metis-protocol/          # Shared JSON IPC contracts between compositor and shell
│   ├── metis-remote/            # Desktop sharing orchestrator (gnome-remote-desktop RDP)
│   ├── metis-secrets/           # Shared freedesktop Secret Service (oo7) wrapper
│   ├── metis-settings/          # GTK4 settings app (display, desktop, devices, system)
│   ├── metis-shell/             # GTK4 layer-shell: edge bar, panels, desktop widgets
│   └── scripts/                 # package-deb.sh + packaging / smoke helpers
├── Screenshots/                 # README showcase images
└── docs/                        # User guide + development setup
```

## Technology stack

- **Language:** Rust (stable), `tokio` async, `serde`/`serde_json` for JSON contracts.
- **Compositor:** [Smithay](https://github.com/Smithay/smithay) with a `winit` nested backend for development; `calloop` event loop; `image` for wallpaper decode; XWayland for X11 apps.
- **Shell / UI:** GTK4 with [`gtk4-layer-shell`](https://github.com/wmww/gtk4-layer-shell); `zbus` for the freedesktop notification daemon.
- **IPC:** JSON over Unix sockets (`metis-protocol`) plus a runtime command file under `$XDG_RUNTIME_DIR/metis/`.
- **Configuration:** JSON under `~/.config/metis/`.

## Quick start

### Install from a `.deb` (Ubuntu 24.04)

Download the latest `metis_*_amd64.ubuntu24.04.deb` from
[GitHub Releases](https://github.com/digitalexpl0it/Metis/releases), then open it
in your package installer, or:

```bash
sudo apt install ./metis_VERSION-1_amd64.ubuntu24.04.deb
# or: sudo dpkg -i metis_VERSION-1_amd64.ubuntu24.04.deb && sudo apt-get install -f
```

An apt `_apt` / `Permission denied` notice from `~/Downloads` is harmless — the
package still installs. See [`docs/PACKAGING.md`](docs/PACKAGING.md) for
Depends/Suggests, local packaging, and CI release tags.

Log out and pick **Metis** at the greeter.

### Build from source (dev)

See [`docs/UBUNTU_DEV.md`](docs/UBUNTU_DEV.md) for full system-package setup (Ubuntu 24.04+).

```bash
# Install GTK4 + layer-shell dev packages (Ubuntu example)
sudo apt install -y build-essential pkg-config libssl-dev \
  libgtk-4-dev libadwaita-1-dev libgtk-4-layer-shell-dev

# Build and launch a nested dev session
cd metis-os-workspace/metis-shell
./run-metis.sh --build --session
```

The nested session runs inside your existing Wayland session via the winit
backend. Session mode disables the wallpaper and login briefing by default;
re-enable them with:

```bash
METIS_NO_WALLPAPER= METIS_NO_BRIEFING= ./run-metis.sh --session
```

To simulate multiple monitors in the dev session, split the window into N
side-by-side virtual outputs:

```bash
METIS_VIRTUAL_OUTPUTS=2 ./run-metis.sh --session
```

### Standalone session (real TTY/GPU)

Metis also runs as a real desktop session on its own GPU via a DRM/KMS + libseat
+ libinput backend (autodetected when no parent Wayland/X11 session is present).
Install the login entry and pick **Metis** from your display manager, just like
Hyprland:

```bash
./run-metis.sh --install-session   # build release + install the session entry
```

Or test it directly from a free VT with `./run-metis.sh --session --drm`. See
[`docs/UBUNTU_DEV.md`](docs/UBUNTU_DEV.md) for details and escape hatches
(Ctrl+Alt+Backspace to quit, Ctrl+Alt+F<n> to switch VT).

## Using Metis

Full walkthrough in the **[User Guide](docs/USER_GUIDE.md)**. The essentials:

- **Edge bar** — app launcher, taskbar dock, workspaces, weather, battery,
  Bluetooth (when an adapter is present), network, volume, system tray, removable
  volumes (USB / SD / optical / ISO — open, mount/unlock, eject), and clock
  (opens Notification Center). Right-click dock icons to pin/close.
- **Desktop widgets** *(optional)* — free-floating wallpaper panels (Folders,
  Apps, Clock, System, Weather, Equalizer). Off by default; enable in
  Settings → Desktop widgets. Edit mode to move/resize; configure via the gear
  on each instance. Writes `desktop-widgets.json` (live reload).
- **Control Center** — pull the edge bar toward the desktop (or click the grid
  icon beside the workspace dots) for a system monitor: CPU/memory/network/disk
  charts, temperature gauges, and a searchable process list with right-click
  actions. Configure in Settings → Control Center.
- **Windows** — every app gets a server-side titlebar with close / minimize /
  maximize. Drag the titlebar to move; drag to a screen edge to snap
  (half / quarter / maximize); drag a border to resize. On the default desktop
  layout, windows reopen at the position and size you last left them. Settings →
  Display includes a **Graphics profile** (Auto / Compatibility / Normal) for
  VM-safe GTK rendering.
- **Workspaces** — `Super`+`1`..`9` switch, `Super`+`Shift`+`1`..`9` move the
  focused window, `Super`+`Alt`+`←`/`→` cycle workspaces (wraps). Each monitor
  has its own workspaces (configurable).
- **Cross-output moves** — drag a window onto another monitor (or snap it there)
  and it follows that display's desk; on grid workspaces `Super`+`Shift`+`←`/`→`
  sends the focused window to the adjacent monitor.
- **Scrolling layout** — toggle any workspace into a niri/PaperWM-style scrolling
  strip with `Super`+`\`; navigate with `Super`+arrows.
- **Settings** — launch from the app launcher, or `metis-cmd settings`. Grouped
  sidebar (Displays, Desktop, Connectivity, Input, System) with search. Pages
  include Display, Appearance, Background, Edge bar, Windows, **Desktop widgets**,
  Metis Menu, Weather, Network, Calendars, Input, Bluetooth, Printers, Power,
  Sound, **Gaming**, **Control Center**, and **Remote access**.
- **Gaming** — hybrid-GPU routing (`gaming.json`), Flatpak Steam/Lutris/Heroic
  overrides, health checklist, and `metis-gamingd` for auto performance profile
  + GameMode while gaming. See the [User Guide — Steam & Proton](docs/USER_GUIDE.md#steam-proton--steamos-class-gaming).
- **Screenshots** — **PrtSc** opens a native Metis overlay (Selection / Full screen /
  Window); **Shift+PrtSc** captures the full screen instantly; **Ctrl+PrtSc** starts in
  Window mode. **Esc** dismisses without capturing. Third-party apps (Flameshot, etc.)
  still use the xdg-desktop-portal Screenshot interface via `metis-portal`.
- **Notification Center** — click the clock for a right-side panel (notifications,
  calendar events, world clocks / timer / alarms). Toasts appear top-right with a
  close button.

| Shortcut | Action |
|----------|--------|
| `PrtSc` | Interactive screenshot overlay |
| `Shift`+`PrtSc` | Instant full-screen capture (no overlay) |
| `Ctrl`+`PrtSc` | Screenshot overlay starting in Window mode |
| `Esc` | (screenshot overlay) Dismiss without capturing |
| `Super`+`1`..`9` | Switch workspace (on the monitor under the pointer) |
| `Super`+`Shift`+`1`..`9` | Move focused window to a workspace |
| `Super`+`Alt`+`←` / `→` | Cycle to previous / next workspace (wraps at 1..=count) |
| `Super`+`Shift`+`←` / `→` | (grid) Move focused window to adjacent monitor |
| `Super`+`Ctrl`+`Shift`+`←` / `→` | Move active workspace to adjacent monitor (independent mode) |
| `Super`+`F` | Toggle maximize for the focused window (below the edge bar) |
| `Super`+`Q` | Close the focused window |
| `Esc` | Exit fullscreen / immersive (focused window) |
| `Super`+`\` | Toggle the active workspace between grid and scrolling |
| `Super`+arrows | (scrolling) Move focus across columns / within a stack |
| `Super`+`Shift`+arrows | (scrolling) Move the column / window |
| `Super`+`,` / `Super`+`.` | (scrolling) Consume into / expel from a column |
| `Super`+`-` / `Super`+`=` | (scrolling) Cycle the focused column width |

## Configuration

Configuration lives in `~/.config/metis/`. On first run the shell writes these
defaults:

| File | Purpose |
|------|---------|
| `bar.json` | Edge bar position/size/opacity/blur, widget order, workspaces, window/titlebar borders, default layout |
| `clock.json` | World clocks and alarms |
| `calendars.json` | Calendar accounts (local / CalDAV / Thunderbird / Microsoft 365) |
| `themes/dark.json`, `themes/light.json` | Design tokens — accents, semantic status colors, `text_on_accent`, shadows/glows |

Other files are created on demand:

| File | Created when | Purpose |
|------|--------------|---------|
| `config.json` | You change a preference | Active theme (defaults to dark), graphics profile, onboarding state, briefing-on-login |
| `menu.json` | You set launcher defaults / pins | App launcher: terminal + file-manager choices (kitty preferred on auto-detect), pinned apps |
| `wallpaper.json` | You pick a background | Wallpaper picture / colour / gradient (+ per-output overrides) |
| `weather.json` | You configure weather | Bar weather: unit, auto-detect / IP-geolocation, saved locations |
| `desk.json` | The compositor persists its layout | Compositor window-grid layout (app tiles) |
| `desktop-widgets.json` | You enable Desktop widgets | Wallpaper widgets: enable, edit mode, chrome, Folders / Apps / Clock / System / Weather / Equalizer instances |
| `dismissed.json` | You dismiss a calendar reminder | Dismissed reminder IDs |
| `briefing.json` | You create it (optional) | Login-briefing weather coordinates + RSS feed |
| `input.json` | You configure input devices | Mouse, touchpad, keyboard (compositor live-reload) |
| `keybinds.json` | You edit Shortcuts | Desktop chords → actions (Settings → Keyboard) |
| `power.json` | You configure power settings | Power profile (`powerprofilesctl`), idle blank/suspend, lid-close |
| `remote.json` | You configure Remote access | Live-session RDP sharing via gnome-remote-desktop |
| `dashboard.json` | You configure Control Center | Enable, widget order, max height %, refresh interval, confirm-before-kill |
| `gaming.json` | You configure gaming | Graphics mode, auto performance/GameMode, Flatpak GPU env |
| `gaming-flatpak.json` | Gaming setup runs | Record of applied Flatpak gaming overrides |
| `screenshot.json` | You configure screenshots | Default mode, pointer toggle, delay, after-capture, save dir |
| `outputs.json` | You configure displays | Per-output scale, resolution/refresh, layout, `display_mode` / `mirror_source`, night-light prefs |

Edit `bar.json`, `themes/*.json`, or `desktop-widgets.json` while the shell runs —
changes apply live (widgets rebuild or update chrome in place). Set bar `opacity`
< 1 for a see-through bar and `blur: true` (with an optional `blur_radius`,
default 18) for a compositor Gaussian backdrop blur. See the
[User Guide](docs/USER_GUIDE.md#10-configuration-reference) for the full field
reference.

## Status

- **Phase 1 — Edge bar:** complete. App launcher, dock, workspaces, weather,
  tray, removable volumes, token-driven theming with live reload, transparency,
  and backdrop blur. Clock opens Notification Center (Phase 13).
- **Phase 2 — Settings app + window decorations:** complete. Standalone
  `metis-settings`, compositor SSD titlebars, edge snapping, XWayland support,
  Appearance light/dark sync for session GTK apps.
- **Phase 3 — Multi-monitor, workspaces & tiling:** largely complete. Per-output
  bars and desks; independent or linked workspaces; optional scrolling layout.
- **Phase 4 — System settings expansion:** complete (Input, Bluetooth, Printers,
  Power, Sound, Display).
- **Phase 5 — display pipeline (VRR / colour / HDR):** in progress — resolution /
  refresh, arrangement, and duplicate mode on DRM; VRR / HDR remain upcoming.
- **Phase 6 — Flatpak, Steam & gaming (v1):** **complete** (2026-07-05).
- **Phase 7 — Remote access:** live-session RDP via `gnome-remote-desktop`
  (Settings → Remote access).
- **Phase 8 — Internationalization:** hybrid gettext (shell/settings) + Fluent
  (compositor); Settings Language & region; onboarding language step. See
  [`docs/I18N.md`](docs/I18N.md).
- **Phase 9 — Onboarding:** **complete** (2026-07-04); language step with Phase 8.
- **Phase 10 — Control Center:** **complete** (2026-07-07; process tree + monitor
  picker 2026-07-11).
- **Phase 11 — Gaming Platform 2.0:** **complete** (2026-07-07).
- **Phase 12 — Native Screenshot Tool:** **complete** (2026-07-09).
- **Phase 13 — Notification Center:** **complete** (2026-07-10).
- **Phase 14 — Desktop Widgets:** **complete** (2026-07-18) — optional wallpaper
  panels (Folders, Apps, Clock, System, Weather, Equalizer); Settings list +
  configure dialogs; chrome and text style. Extension API deferred.
- **Configurable shortcuts:** Settings → Keyboard → Shortcuts + `keybinds.json`.
- **Portal capture:** Screenshot + ScreenCast (SHM; dmabuf zero-copy deferred).

Optional follow-up: dmabuf screencast perf, Deck-class hardware verification,
compositor **dim on battery** hook, desktop-widget extension API.

See [`metis-os-workspace/TODO.md`](metis-os-workspace/TODO.md) for the detailed
roadmap, [`CHANGELOG.md`](CHANGELOG.md) for recent changes, and
[`docs/PERF_AUDIT.md`](docs/PERF_AUDIT.md) for performance and binary-size notes.

## License

Licensed under the [MIT License](LICENSE).
