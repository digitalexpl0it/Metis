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

**First-run setup.** On a fresh install (when `onboarding_complete` is false in
`config.json`), the shell shows an onboarding wizard after the startup splash:
theme, wallpaper, clock format, edge bar, weather, gaming, **optional software**
(Remote desktop, Flatpak, GameMode, Bluetooth, printers, and keyring if missing),
and a finish screen with keybind tips. Already-installed optionals are greyed out;
use toggles and **Install selected** to install the rest in one `pkexec apt-get`
pass (or skip and install later). Skip or Finish marks setup complete so it does
not appear again. Reopen it anytime from **Settings → Appearance → Run setup again**,
or with `metis-cmd.sh show-onboarding`. Disable for dev with
`METIS_NO_ONBOARDING=1`.

**Install Metis.** Prefer the Ubuntu `.deb` from GitHub Releases when available
([`docs/PACKAGING.md`](PACKAGING.md)); developers can still use
`./run-metis.sh --install-session` for a `/usr/local` greeter entry.

---

## 2. The desktop at a glance

- **Edge bar** — a thin bar anchored to one screen edge (top by default). It
  holds the app launcher, a taskbar dock of running apps, workspace dots, and
  status widgets (weather, battery, Bluetooth, network, volume, clipboard, clock).
- **Windows** — every app gets a compositor-drawn **titlebar** with close,
  minimize, and maximize buttons, plus a border. Windows tile into a grid by
  default — opening or closing an app re-splits the area below desk widgets among
  visible tiled windows. You can float, snap, maximize, or switch a workspace into a
  scrolling layout.
- **Popovers & panels** — clicking a bar widget opens an on-demand popover (Wi-Fi,
  volume, weather, app launcher). The **clock** opens the right-side **Notification
  Center** (notifications, calendar, world clocks, timer, alarms). Clicking
  elsewhere or pressing **Esc** dismisses it.

---

## 3. The edge bar

Widgets appear in the order set by `bar.json#widgets`. The defaults:

| Widget | What it does |
|--------|--------------|
| **App launcher** | The brand icon at the start of the bar. Opens the launcher panel (see §4). |
| **Tasks (dock)** | Icons for running (and pinned) apps on this output's current workspace. Click to focus/minimize; right-click to pin/close. |
| **Workspaces** | One dot per workspace; the active one is highlighted. Click a dot to switch (see §6). |
| **Weather** | Condition icon + temperature. Click for a forecast popover with hourly strip and saved locations. |
| **Battery** | Charge level and state (hidden on desktops without a battery). Click to open Power settings. |
| **Bluetooth** | Shown when a Bluetooth adapter is present. Click for connected devices (with battery level and charging icon when reported), plus a shortcut to Bluetooth settings. |
| **Network** | Wired/Wi-Fi status. Click for a Wi-Fi popover (scan, connect, forget). The signal icon stays stable during background rescans. |
| **Volume** | Current output volume. Click for a slider + mute. |
| **Notifications** | *(optional)* Legacy bell — opens the same Notification Center as the clock. Removed from the default bar layout in Phase 13. |
| **Clock** | Date/time with unread badge. Click opens the **Notification Center** (right panel): notifications, calendar events, and calendar/tools (world clocks, stopwatch, timer, alarms). **Esc** closes. |

**Per-output bars.** With multiple outputs you can show the bar on **all
displays** (each is independent and live) or **the primary display only** —
configured in Settings → Appearance → Edge bar → *Show bar on*.

**Live editing.** Edit `~/.config/metis/bar.json` while Metis runs; bar changes
apply within about a second. Theme edits (`themes/*.json`) re-apply live too.

### Notification Center

Click the **clock** to slide open a frosted panel from the right edge:

1. **Notifications** — grouped cards, Do Not Disturb, Clear all (hides when empty).
2. **Events** — selected-day calendar events (hides when empty).
3. **Calendar / tools** — month grid plus an icon rail for World clocks, Stopwatch,
   Timer, and Alarms.

Transient **toasts** still appear top-right (with a close button) and shift left
while the panel is open. Press **Esc** or click the clock again to dismiss.

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

### Screenshots

Metis ships a **native screenshot tool** (Phase 12) integrated into the shell.
Press **PrtSc** to open the interactive overlay (default **Selection** mode):

| Key | Action |
|-----|--------|
| **PrtSc** | Interactive overlay (Selection / Screen / Window) |
| **Shift+PrtSc** | Instant full-screen capture (no overlay) |
| **Ctrl+PrtSc** | Interactive overlay starting in **Window** mode |
| **Esc** | Close the overlay without capturing (all modes) |

The overlay uses your active Metis theme (dark/light/custom tokens): frosted
toolbar, accent **Capture** button, and dashed selection border all update live when
you change theme in Settings or edit `themes/*.json`.

**Modes** — toolbar icon toggles for **Selection** (drag a rectangle), **Full screen**
(entire monitor), and **Window** (hover to preview, **click to lock** a window, then
**Capture**). Press **Esc** at any time to close the overlay without capturing.

**Options** (gear icon) — include pointer in the PNG, capture delay (seconds), and
after-capture action (Copy, Save, Both, or Open in the default image viewer).

Configure defaults in `~/.config/metis/screenshot.json` (`default_mode`,
`draw_cursor`, `delay_seconds`, `after_capture`, `save_dir`). PNGs save under
`~/Pictures/Metis` by default; copies go to the compositor clipboard when
`after_capture` is `copy` or `copy_and_save`.

From a script: `metis-cmd screenshot` (same as PrtSc).

**Third-party apps** (Flameshot, browser pickers, etc.) still use the freedesktop
**Screenshot** portal (`org.freedesktop.impl.portal.Screenshot`):

- The **first** capture from an app may show a permission dialog; grant it once
  and later captures proceed silently.
- Portal screenshots are saved as PNGs under `$XDG_RUNTIME_DIR/metis-screenshot-*.png`
  and returned to the requesting app as a `file://` URI.

If screenshots fail after an upgrade, log out and back into Metis so the updated
compositor and portal binaries are running (`./run-metis.sh --install-session`
installs both). To verify portal capture directly:

```bash
metis-portal --capture-test /tmp/test.png
ls -la /tmp/test.png
```

### Flatpak apps and games

Flatpak apps run as ordinary Wayland clients in the same session and use the same
**xdg-desktop-portal** stack as native apps. Metis does not ship a Flatpak-specific
runner — installed Flatpaks launch like any other app.

**Host prerequisites** (Debian/Ubuntu shown; use your distro's packages otherwise):

```bash
# Flatpak + the portal stack Metis relies on for file dialogs, notifications,
# screenshots, and screencast.
sudo apt install flatpak xdg-desktop-portal xdg-desktop-portal-gtk
flatpak remote-add --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo

# DRM / evdev access for the standalone session and for games that read input
# devices directly. Log out and back in after changing groups.
sudo usermod -aG input,video,render "$USER"
```

**Apps appear in the launcher automatically.** Flatpak installs export their
`.desktop` entries and icons under `exports/share` trees
(`~/.local/share/flatpak/exports/share` for `--user` installs,
`/var/lib/flatpak/exports/share` for system installs) rather than the normal
applications dir. Metis adds both to `XDG_DATA_DIRS` at session start
(`metis-session`, and `run-metis.sh --session` for dev), so Flatpak apps show up
in the app launcher and running-apps dock — with their proper names and icons —
right alongside native apps, and new installs appear live without a restart.

> If you installed a Metis login session before 2026-07-03, re-run
> `./run-metis.sh --install-session` and log out/in so the updated
> `metis-session` (with the Flatpak export dirs) takes effect.

**Permissions** come from three places:

1. **Flatpak manifest / overrides** — e.g. `socket=wayland`, `device=dri`, and
   often `--device=all` for gamepads (`flatpak override --user --device=all …`).
2. **Portal prompts** — screenshot/screencast/file access; stored by system
   `xdg-permission-store` (the first-time Flameshot dialog).
3. **Metis portal backends** — Settings, Screenshot, ScreenCast, Background, and
   PowerProfileMonitor via `metis-portal`; idle-inhibit via legacy ScreenSaver
   D-Bus names; file dialogs and notifications via the GTK portal backend.

#### Portal permission management

First-time portal prompts (screenshot, screencast, etc.) are stored by the system
**permission store**, not by Metis:

```bash
# List persisted portal permissions for an app
flatpak permission-show com.example.App

# Reset all portal permissions for an app
flatpak permission-reset com.example.App

# Show Flatpak sandbox overrides (devices, filesystems, sockets)
flatpak override --show com.example.App
flatpak info --show-permissions com.example.App
```

On-disk state lives under `~/.local/share/xdg-desktop-portal/` (portal runtime
data) and the system `xdg-permission-store` service. If a sandboxed app keeps
failing capture or file access after you denied a prompt once, reset its
permissions and try again.

#### Flatpak override cookbook

| Goal | Command |
|------|---------|
| Gamepads / all input devices | `flatpak override --user --device=all com.example.Game` |
| GPU / render node | `flatpak override --user --device=dri com.example.Game` |
| Extra game library on another disk | `flatpak override --user --filesystem=/mnt/games com.example.Game` |
| Wayland socket (usually in manifest) | `flatpak override --user --socket=wayland com.example.Game` |
| Steam (Flatpak) — typical gaming setup | `flatpak override --user --device=all com.valvesoftware.Steam` |

**Controllers:** games read `/dev/input/event*` directly (SDL, Proton), not through
the compositor. Metis opens libinput devices in **shared** mode and does **not**
EVIOCGRAB gamepad nodes — native and Proton titles keep full evdev access while
Metis runs. Touchscreens are forwarded to Wayland clients via `wl_touch`. Check
*Settings → Gaming* for a live list of detected gamepads and touchscreens. If a
Flatpak game has no gamepad, try:

```bash
flatpak override --user --device=all com.example.Game
```

Your user should also be in the `input`, `video`, and `render` groups for DRM
and evdev access.

### Steam, Proton & SteamOS-class gaming

Metis is intended to work as a **gaming desktop** with the same stack SteamOS
Desktop Mode uses (Steam + Proton), without requiring KDE or GNOME.

**Install Steam (pick one):**

```bash
# Native .deb (Valve repo — Ubuntu/Debian)
sudo dpkg --add-architecture i386
sudo apt update
sudo apt install -y steam-installer mesa-vulkan-drivers mesa-vulkan-drivers:i386

# Or Flatpak
flatpak install flathub com.valvesoftware.Steam
```

For **native** Steam, also install the controller udev rules so gamepads and the
Steam Controller are accessible without root, and make sure a PipeWire/Pulse
sound server is running (Steam and most games expect one):

```bash
sudo apt install -y steam-devices        # /usr/lib/udev/rules.d for controllers
# PipeWire is standard on modern Ubuntu; Pulse works too. No compositor config needed.
```

For **Flatpak** Steam, the runtime is sandboxed (pressure-vessel). Games and the
Proton prefix live under `~/.var/app/com.valvesoftware.Steam/` (not `~/.steam`).
If controllers, extra drives, or specific devices are missing, widen its device
access and confirm portal permissions:

```bash
flatpak override --user --device=all com.valvesoftware.Steam
# Extra library on another disk:
flatpak override --user --filesystem=/mnt/games com.valvesoftware.Steam
```

**Launch:** open Steam from the app launcher or run `steam`. When Steam is
detected (native on `PATH` or the Flatpak package), Metis also shows a
controller-friendly **Big Picture** button in the app-menu rail, which runs
`steam -gamepadui` (or `flatpak run com.valvesoftware.Steam -gamepadui`). The
button is hidden entirely on machines without Steam.

**Proton** runs Windows games as child processes of Steam over Wayland/XWayland.
Enable it in *Steam → Settings → Compatibility → Run other titles with…* and pick
**Proton Experimental** or a **GE-Proton** build (install GE-Proton via
[ProtonUp-Qt](https://github.com/DavidoTek/ProtonUp-Qt) or by dropping it in
`compatibilitytools.d`). Common failure modes:

- **Black screen / no Vulkan** — missing 32-bit Vulkan. Install `i386` +
  `mesa-vulkan-drivers:i386` (native) or update the Flatpak runtime.
- **Wrong GPU picked** — see hybrid-GPU below.
- **Anti-cheat** — enable *Steam Play* for the title and check
  [ProtonDB](https://www.protondb.com) for per-game tweaks.

**Hybrid GPU (laptops).** Metis exports the compositor's own render GPU to
every spawned client, so Steam, Proton, XWayland, and Vulkan apps default to the
**same** card the session renders on instead of silently picking the wrong one.
The card is chosen by the compositor (override with `METIS_DRM_DEVICE`, see dev
docs) and forwarded as `DRI_PRIME` (Mesa GL) and `MESA_VK_DEVICE_SELECT` (Mesa
Vulkan). On hybrid laptops where the panel is driven by the integrated GPU but a
discrete GPU is present, Metis also **auto-offloads game and Steam launches** onto
the dGPU (NVIDIA PRIME offload or Mesa `DRI_PRIME` for the dGPU render node).
Lightweight desktop apps stay on the power-efficient iGPU. Override session-wide
with `METIS_GAME_GPU=igpu|dgpu|off`. To run a *specific* title on the discrete GPU
instead, set a per-game launch option in Steam (*Properties → Launch Options*) —
these still win because Metis only sets the vars when they are unset:

```text
DRI_PRIME=1 %command%
prime-run %command%
__NV_PRIME_RENDER_OFFLOAD=1 __GLX_VENDOR_LIBRARY_NAME=nvidia %command%   # NVIDIA
```

Set `METIS_NO_CLIENT_GPU=1` in the session environment to disable the automatic
forwarding entirely.

**Gaming Platform 2.0 (Settings → Gaming).** Metis productizes GPU routing and
Flatpak setup in `~/.config/metis/gaming.json` instead of env-var recipes:

- **Graphics mode** — `auto`, desktop iGPU / games dGPU, always dGPU/iGPU, or off.
- **On battery** — prefer iGPU unless you override per session.
- **Auto performance profile** — `metis-gamingd` switches to Performance while a
  game session is active and restores on exit.
- **Auto GameMode** — registers detected game PIDs with `gamemoded` when installed.
- **Flatpak GPU env** — applies NVIDIA/Mesa offload vars to Steam, Lutris, and Heroic
  via idempotent `flatpak override` (state in `gaming-flatpak.json`).
- **Optimize for gaming** — one-click health check + Flatpak fixes.
- **First-run wizard** — optional Gaming step in onboarding; re-run from Settings.

Flatpak Steam launches through `~/.local/share/metis/bin/launch-steam` when the
Flatpak is installed (Big Picture and menu entries use it automatically). Runtime
reload: `metis-cmd reload-gaming`; optimize: `metis-cmd optimize-gaming`.

**What Metis does not install automatically:** Steam, GPU drivers, 32-bit Vulkan
(`mesa-vulkan-drivers:i386`), GameMode, or PipeWire. Settings → Gaming detects
these gaps and shows install commands. Run **Optimize now** or **Run gaming setup**
for Flatpak overrides and GPU routing only. If you use **native** Steam (not
Flatpak), **Run gaming setup** may report that no Flatpak gaming apps were found —
that is expected; the launcher wrapper and compositor GPU routing still apply.

**Controllers & Steam Input.** Games read `/dev/input/event*` directly (SDL,
Proton, Steam Input) — Metis does **not** grab evdev devices, so gamepads,
the Steam Controller, DualSense, and Switch Pro controllers work as they do
under any desktop. Configure mappings in *Steam → Settings → Controller*; there
is no compositor-side gamepad driver. Flatpak Steam may still need
`--device=all` (above). Ensure your user is in the `input` group.

**Steam overlay (Shift+Tab).** Works on Metis: focus follows clicks (no
focus-follows-mouse), so the overlay keeps input while it is up. It is most
reliable on XWayland/Proton titles; some native-Wayland games render their own
overlay differently. If it seems unresponsive, click the game window first so it
holds focus.

**Remote Play / Steam Link.** In-home streaming captures the game via the
PipeWire **ScreenCast** portal that Metis ships (`metis-portal`), so host
streaming works without extra setup. Encoding performance is hardware-dependent.

**Power while gaming.** Steam and games hold an idle inhibitor through the
Wayland idle-inhibit protocol and the `org.freedesktop.ScreenSaver` /
`PowerManagement` D-Bus interfaces, both wired end-to-end in Metis, so the
screen will not blank and the machine will not auto-suspend mid-game. For
sustained performance, pick a performance profile in *Settings → Power*.

**Gaming polish (optional).** Add these as Steam launch-option prefixes per game:

```text
gamemoderun %command%                         # sudo apt install gamemode
mangohud %command%                            # FPS/frametime overlay
MANGOHUD=1 %command%                           # Flatpak Steam
```

[GameMode](https://github.com/FeralInteractive/gamemode) is a standalone D-Bus
service (`com.feralinteractive.GameMode`); games talk to it directly via
`gamemoderun`, so nothing needs configuring in Metis beyond installing the
package. MangoHud/vkBasalt likewise attach per game.

**Mouse-look & in-game menus (pointer lock).** Metis implements the standard
Wayland pointer-constraints and relative-pointer protocols. Games that lock the
pointer for camera control receive relative motion deltas while the system cursor
stays put. Proton titles that draw their own cursor in menus send a
`set_cursor_position_hint`; Metis remaps clicks through that hint so menu items
activate where you point, not at the frozen lock anchor. During pointer lock the
compositor does not repaint on mouse motion — only the game's commits drive
frames. Fullscreen games skip wallpaper, night light, and the compositor cursor
so the display path can promote the game buffer to direct scanout when formats
match. Enable **Adaptive sync** in Settings → Display for VRR on supported panels.
Keyboard navigation in menus always works as a fallback.

**Gamescope (optional):** SteamOS Gaming Mode uses [Gamescope](https://github.com/ValveSoftware/gamescope)
as its compositor. On Metis, Gamescope is optional — add to a game's Steam launch
options to wrap only that title:

```text
gamescope -W 1920 -H 1080 -f -- %command%
```

Metis stays the session compositor; Gamescope nests inside it for that game
(frame limit, scaling, FSR). Install: `sudo apt install gamescope` (where available).

**SteamOS / handheld (experimental).** Valve's SteamOS 3.x uses Gamescope for
handheld Gaming Mode and KDE for Desktop Mode. Running Metis *on* SteamOS
(replacing Desktop Mode) is experimental and unsupported: SteamOS mounts its
root filesystem read-only (use `steamos-readonly disable` at your own risk to
install packages), and Gamescope Gaming Mode and Metis are alternative session
compositors — you run one *or* the other, not as the outer session. The
supported target is **Steam + Proton working on a Metis session** on Ubuntu and
similar distros.

On Deck-class hardware running Metis on a normal distro, SD-card readers, volume
buttons, and gyro (where exposed as evdev) should pass through to Steam Input
like on any other desktop — this has **not** been verified on Metis hardware yet.
Use *Settings → Gaming* to confirm controllers and touchscreens are visible to the
session.

---

## 5. Window management

Metis draws **server-side decorations**, so every window (Wayland or XWayland)
gets a consistent titlebar and border that follow your theme. Electron/Chromium
apps (Cursor, Claude Desktop, …) are steered onto native Wayland when launched
from Metis, which is more stable than their default XWayland path and still gets
the Metis titlebar.

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
- `Super`+`Alt`+`←` / `→` — cycle to the previous / next workspace (wraps at
  1..=count). Always uses **Super**+**Alt** — not remapped by `METIS_MOD` (see
  nested sessions below).
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

Defaults are listed below. Change them anytime in **Settings → Keyboard →
Shortcuts** (saved to `~/.config/metis/keybinds.json`, live-reloaded). Ctrl+Alt+F1–F12
and Ctrl+Alt+Backspace are system-only and cannot be rebound.

| Shortcut | Action |
|----------|--------|
| `Super`+`1`..`9` | Switch to workspace 1–9 (monitor under the pointer) |
| `Super`+`Shift`+`1`..`9` | Move the focused window to workspace 1–9 |
| `Super`+`Alt`+`←` / `→` | Cycle to previous / next workspace (wraps at 1..=count) |
| `Super`+`Shift`+`←` / `→` | (grid) Move the focused window to the adjacent monitor; (scroll) move column/window |
| `Super`+`Ctrl`+`Shift`+`←` / `→` | Move the active workspace to the adjacent monitor (independent mode) |
| `Super`+`F` | Toggle maximize for the focused window (fills the area below the edge bar) |
| `Super`+`Shift`+`F` | Toggle true fullscreen |
| `Super`+`Q` | Close the focused window |
| `Super`+`M` | Minimize the focused window |
| `Super`+`L` | Lock the session |
| `Super`+`Esc` | Exit fullscreen / maximize / tile |
| `Super`+`/` | Enable grid tiling |
| `Super`+`\` | Disable tiling (free desktop) |
| `Print` / `Shift`+`Print` / `Ctrl`+`Print` | Screenshot interactive / full / window |
| `Super`+`←` `→` `↑` `↓` | (scrolling) Move focus between/within columns |
| `Super`+`,` / `Super`+`.` | (scrolling) Consume into / expel from a column |
| `Super`+`-` / `Super`+`=` | (scrolling) Snap the focused column to full / half width |

**Nested in GNOME?** `./run-metis.sh --session` may set `METIS_MOD=alt` for first-run
defaults — read **Super** as **Alt** in the table above **except** workspace cycle
(`Super`+`Alt`+`←`/`→`), which always uses the logo key plus **Alt**. Prefer changing
the Metis modifier and individual chords in Settings → Keyboard. Click the Metis
window first so it has keyboard focus. On a real Metis session, **Super** is the
logo / Windows key by default.

---

## 9. The Settings app

Launch Settings from the app launcher's quick-launch rail, or from a terminal:

```bash
metis-cmd settings            # open Settings
metis-cmd settings appearance # open a specific page
```

Pages are grouped in the sidebar (Displays, Desktop, Connectivity, Input,
System). Use the search field at the top of the sidebar to filter pages.
Launch a specific page with `metis-cmd settings <page>` (e.g. `display`,
`network`, `power`).

- **Display** — per-output scale, enable/disable, resolution & refresh (DRM mode
  list on real hardware), **Duplicate displays** (mirror clone with scale-to-fit
  letterboxing on DRM hardware), and multi-monitor arrangement (drag preview when
  two or more outputs are connected; hidden while duplicating; **Save display
  settings** with a keep/revert confirmation). Scale and **Active** apply live;
  duplicate mode, arrangement, and resolution changes are batched behind save.
  Night-light preferences apply live in the compositor (warm overlay; schedule
  not wired yet). Rotation is still upcoming.
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
  365) used by the Notification Center calendar.
- **Input** — mouse, touchpad, and keyboard layout/repeat settings (`input.json`),
  plus **Keyboard → Shortcuts** for desktop keybinds (`keybinds.json`, live
  reload). System VT/quit chords are listed but not editable.
- **Bluetooth** — adapter on/off, scan for devices (toggle stop, auto-stops after
  30s), pair / connect / trust / remove. Battery percentage and charging state
  appear when the device or driver reports them.
- **Printers** — list CUPS queues; open the system printer config when needed.
- **Gaming** — graphics mode (auto / iGPU / dGPU), battery and performance
  toggles, health checklist with Fix buttons, **Optimize now**, and **Run gaming
  setup** wizard; writes `gaming.json` and applies Flatpak overrides when installed.
- **Control Center** — enable/disable the pull-down system monitor, max panel
  height %, refresh interval, confirm-before-kill, overview widgets, and which
  process monitor **Open monitor** launches (auto-detect / installed / custom);
  writes `dashboard.json` for live shell reload.
- **Power** — power profile (power-saver / balanced / performance via
  `power-profiles-daemon`), laptop battery details, idle blank/suspend timeouts,
  lid-close action, and a **Connected devices** list for Bluetooth peripherals
  with battery status. See [Power profiles](#power-profiles-settings--power) below.
- **Remote access** — GNOME-style desktop sharing toggle: enable RDP to your
  **live** Metis session via `gnome-remote-desktop` (headless). Set credentials,
  copy the connection address, and connect from Microsoft Remote Desktop, Remmina,
  or FreeRDP. Requires a real (DRM) session — not nested dev. Open with
  `metis-cmd settings remote`.
- **Sound** — default output and input device selection (bar volume widget
  unchanged).

**Bluetooth battery notes.** Many devices only expose a coarse percentage over
plain Bluetooth (often updating on reconnect). Charging state requires a driver
that reports it — kernel HID batteries, UPower, or **Solaar** for Logitech
peripherals (optional; Metis ignores Solaar silently when it is not installed).
For the most accurate Logitech battery and charging info, use a Unifying/Bolt USB
receiver or install Solaar.

Most appearance and bar changes apply live; some device-backed settings only take
full effect under a real (DRM) session.

### Power profiles (Settings → Power)

| Control | Applies via | Notes |
|---------|-------------|-------|
| **Power saver / Balanced / Performance** | `powerprofilesctl` → `power-profiles-daemon` | Requires the daemon package (`power-profiles-daemon` on Ubuntu). Changes CPU/platform power behaviour on supported laptops. On desktops without platform profiles the effect may be minimal. Verify with `powerprofilesctl get`. |
| **Blank screen after** | Metis compositor (DPMS) | Live-reloaded via `ReloadPower` IPC; independent of logind. |
| **Suspend after idle** | systemd-logind (`busctl`) | Best-effort; needs logind and appropriate permissions. |
| **When lid is closed** | systemd-logind | Laptop only; suspend / ignore / hibernate / power off. |
| **Dim on battery** | *(not wired yet)* | Saved to `power.json` only — compositor dim hook is planned. |

While gaming, **Settings → Gaming → Auto performance profile** (via `metis-gamingd`)
can temporarily switch to **Performance** and restore your previous profile when the
game session ends.

### Remote desktop (RDP)

**Settings → System → Remote access** turns on headless RDP sharing for the
session you are logged into — the same idea as GNOME Settings → Sharing → Remote
Desktop. Metis orchestrates `gnome-remote-desktop` via the `metis-remote` helper;
credentials live in the headless store (`grdctl --headless`), not in Metis config
files.

1. Install the backend on the host (Ubuntu):
   `sudo apt install gnome-remote-desktop`
2. Open **Settings → Remote access** (or `metis-cmd settings remote`).
3. Click **Set password…** and choose the RDP username and password clients will
   use.
4. Turn on **Allow remote desktop connections**.
5. Copy the connection address (hostname or LAN IP plus port **3389**) and connect
   from another machine on your network.

**Clients.** Windows: *Remote Desktop Connection* (`mstsc`). macOS: *Microsoft
Remote Desktop* from the App Store. Linux:

```bash
xfreerdp /v:HOST:3389 /u:USERNAME /p:PASSWORD /dynamic-resolution
```

**Security.** `remote.json` defaults to LAN-only guidance (`lan_only: true`). Keep
port 3389 on your local network — do not expose RDP to the internet without a VPN
or strong perimeter controls. Example firewall (adjust interface/subnet):

```bash
sudo ufw allow from 192.168.0.0/16 to any port 3389 proto tcp
```

**Session lock.** While the session is locked (`Super+L`), the compositor blocks
screen capture — remote viewers see a frozen or black screen until you unlock.
This matches Metis’s local lock posture.

**Auto-start.** When `remote.json` has `"enabled": true` and `"auto_start": true`
(the defaults after you turn sharing on), `metis-session` runs `metis-remote
autostart` at login so you do not need to reopen Settings each time.

**Clipboard.** Text copy/paste between the Metis session and an RDP client is
synced via the portal's Mutter clipboard bridge (same path as GNOME). Image
clipboard is best-effort; text (`text/plain`, UTF-8) is the supported v1 path.

**Troubleshooting.** If the page shows an install hint, install
`gnome-remote-desktop` and re-login. If enable fails with “Set RDP credentials”,
set a password first. PipeWire and the Metis ScreenCast portal must be running in
the DRM session — re-run `./run-metis.sh --install-session` if portal capture is
broken. Check status from a terminal: `metis-remote status` (JSON).

RustDesk, VNC (`wayvnc`), and classic `xrdp` login sessions are planned follow-ups
— see [`TODO.md`](../metis-os-workspace/TODO.md) Phase 7.

### System dashboard (Control Center)

Open the monitor by **pressing on the edge bar** and **dragging toward the desktop**,
or click the **grid icon** to the right of the workspace dots. The panel is
embedded directly under the bar with no gap.

| Bar position | Drag direction |
|--------------|----------------|
| Top | Down |
| Bottom | Up |
| Left | Right |
| Right | Left |

The panel tracks your drag (rubber-band) and snaps open if you pull far enough.
The dashboard **loads on demand** — no background polling until you open it; it
tears down again when dismissed.

**Overview** tab:

| Row | Cards |
|-----|--------|
| 1 | **Processor** (per-core lines + Σ total, gradient charts) · **Memory** (RAM + swap) |
| 2 | **Network** (ethernet/wifi rates + chart) · **Disk I/O** (read/write chart) |
| 3 | **Session** (load, uptime) · **Storage** (mount tiles) |
| 4 | **CPU temp** gauge · **GPU temp** gauge(s) for discrete GPUs only · **System** (hostname, CPU, kernel) |

CPU and discrete-GPU temperature gauges use a 0–150 °C semicircle. On hybrid
laptops (Intel + NVIDIA), the Intel iGPU is not shown; NVIDIA temps are read from
sysfs when available, otherwise from `nvidia-smi`.

**Processes** tab — searchable, sortable PPID tree (Name, PID, User, Type, CPU,
Memory). Expand a parent to see child processes. Search keeps ancestor paths so
matches stay visible under their tree. Right-click a killable row for **End task**,
**Force quit** (SIGKILL), **End process tree** / **Force quit tree** (when it has
children), or **Copy PID**. End-task confirmation is optional
(`dashboard.json` → `confirm_before_kill`). Use **Open monitor** to launch your
configured process monitor (Settings → Control Center: auto-detect prefers
btop/htop in a terminal, then GUI monitors). The process list pauses refresh while
a context menu is open so actions stay usable.

Dismiss with **Esc**, the close button, **drag back toward the bar** on the
header, or by clicking the desktop. Configure in Settings → **Control Center** or
edit `~/.config/metis/dashboard.json` directly.

---

## 10. Configuration reference

All configuration lives in `~/.config/metis/` as JSON. You can edit files by
hand — `bar.json` and `themes/*.json` reload while Metis runs.

### Nested dev sessions (GNOME / host compositor)

When Metis runs inside another desktop (the default `./run-metis.sh --session`
winit window), the **host grabs Super** for its own shortcuts. Metis shortcuts
won't fire with Super unless you reconfigure the host.

**Default:** nested sessions may set `METIS_MOD=alt` so first-run defaults use Alt
instead of Super — e.g. **Alt+1** switches workspace. Prefer **Settings → Keyboard
→ Shortcuts** to set the Metis modifier and individual chords (`keybinds.json`).
**`Super`+`Alt`+←/→`** (workspace cycle) defaults to the logo/Windows key plus
**Alt**. **Click the Metis session window first** so it has keyboard focus.

Override with `METIS_MOD=super` or `METIS_MOD=ctrl` only when no `keybinds.json`
mod preference is set yet. On a real Metis session, the default modifier is Super.

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
| `input.json` | Mouse, touchpad, and keyboard layout/repeat (compositor live-reload) |
| `keybinds.json` | Desktop shortcuts (chords → actions); Settings → Keyboard → Shortcuts |
| `power.json` | Power profile, idle blank/suspend timeouts, lid-close action, dim-on-battery preference (dim not wired yet) |
| `remote.json` | Desktop sharing: enabled, backend (`gnome_rdp`), auto-start, LAN-only hint |
| `dashboard.json` | Control Center: enabled, widgets, height %, refresh, confirm-before-kill, process monitor |
| `gaming.json` | Graphics mode, on-battery iGPU preference, auto performance/GameMode, Flatpak GPU env |
| `gaming-flatpak.json` | Record of applied Flatpak gaming overrides (managed by `metis-gaming`) |
| `outputs.json` | Per-output scale, resolution/refresh, arrangement (`layout_x`/`layout_y`), `display_mode` / `mirror_source`, night-light prefs |

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
| Electron app (e.g. Claude Desktop) opens then immediately closes | Metis launches Electron/Chromium apps on native Wayland by default (`ELECTRON_OZONE_PLATFORM_HINT=auto`, and `CLAUDE_USE_WAYLAND=1` for Claude), which is stable; their XWayland path can quit on launch. Re-login after `./run-metis.sh --install-session` so the session env applies. To force XWayland for one app, launch it with `ELECTRON_OZONE_PLATFORM_HINT=x11` (or `CLAUDE_USE_WAYLAND=0`) |
| Apps slow to open / black screen on login | Ensure portal files are installed (`./run-metis.sh --install-session` or rebuild with `--session`); see [`CHANGELOG.md`](../CHANGELOG.md) 2026-06-28 |
| Screenshot / Flameshot fails | Re-login after `./run-metis.sh --install-session`; run `metis-portal --capture-test /tmp/test.png` to isolate portal vs app issues; grant the first-time portal permission |
| Flatpak app won't start / no Wayland | Install `flatpak` + portal packages; ensure app has `socket=wayland` (`flatpak info --show-permissions …`) |
| Flatpak app missing from the launcher | Metis adds the Flatpak `exports/share` dirs to `XDG_DATA_DIRS` at session start — re-run `./run-metis.sh --install-session` and log out/in if you installed the session before 2026-07-03. Verify with `echo $XDG_DATA_DIRS \| tr ':' '\n' \| grep flatpak` inside the session |
| Flatpak game: no controller | `flatpak override --user --device=all <app-id>`; confirm user is in `input` group |
| Steam / Proton game black screen or wrong GPU | Install 32-bit Vulkan (`i386` + `mesa-vulkan-drivers:i386`). Metis auto-forwards its render GPU to clients and auto-offloads game/Steam launches to a discrete GPU when present (`METIS_GAME_GPU` = igpu, dgpu, or off). Per-game, override with `DRI_PRIME=1 %command%` / `prime-run %command%` (or NVIDIA offload vars). Session-wide, set `METIS_DRM_DEVICE=/dev/dri/cardN`; disable fullscreen optimizations per-game |
| Proton game: keys dead but mouse works | Re-login after `./run-metis.sh --install-session` (2026-07-04 XWayland keyboard-focus fix). Click the game window so it holds focus; confirm Steam is not popping over the game (focus-stealing prevention is in place) |
| Proton game: menu clicks open wrong item / only Settings | Fixed 2026-07-04 (cursor-position-hint click remapping). Rebuild and reinstall the session; filter logs with `rg 'game-pointer' ~/.local/state/metis/logs/session-*.log` |
| Steam tray Quit / Exit does nothing | Fixed 2026-07-04 (dbusmenu label re-resolve). Rebuild shell and reinstall session |
| Steam overlay (Shift+Tab) missing | Click the game window so it holds focus (Metis is click-to-focus, no focus-follows-mouse). Most reliable on XWayland/Proton titles; some native-Wayland games draw the overlay differently |
| Big Picture button missing from menu | The rail shows it only when Steam is installed — native `steam` on `PATH` or the `com.valvesoftware.Steam` Flatpak. Install Steam and reopen the menu |
| Session sleeps during game | The idle-inhibit portal is implemented — video players, games, and browsers that request `org.freedesktop.ScreenSaver`/`PowerManagement.Inhibit` (or the Wayland idle-inhibit protocol) suspend blanking automatically. If something still sleeps, that app isn't requesting an inhibit; extend the timeout in Settings → Power, or confirm the inhibit reached the compositor |
| gdbus request path "does not exist" | Portal request objects are ephemeral — trigger a fresh `Screenshot` call; use `gdbus monitor --session --dest org.freedesktop.portal.Desktop` *before* the call to see the `Response` signal |
| Bluetooth shows stale battery | Many devices only refresh over BT on reconnect; install **Solaar** for Logitech charging state, or use a Unifying/Bolt receiver |
| Session won't start / behaves oddly | `./run-metis.sh --stop` then `./run-metis.sh --build --session` |
| Theme looks wrong | Delete `~/.config/metis/themes/*.json` and restart to regenerate |
| Verify the shell is reachable | `./run-metis.sh --verify` |
| Compare compositor vs shell grid | `./run-metis.sh --verify-grid` |
| Remote desktop toggle greyed out | Install `gnome-remote-desktop`; set a password on **Settings → Remote access** before enabling |
| RDP connects but screen is black | Confirm you are on a DRM session (not nested dev); unlock if the session is locked; check `metis-remote status` and PipeWire/portal stack |
| `metis-remote` not found | Rebuild with `./run-metis.sh --install-session` (installs `metis-remote` to `/usr/local/bin`) |

Logs are written to `~/.local/state/metis/logs/` (`latest.log` points at the most
recent run).

---

Questions, roadmap, and recent changes: see
[`../metis-os-workspace/TODO.md`](../metis-os-workspace/TODO.md) and
[`../CHANGELOG.md`](../CHANGELOG.md).
