# Ubuntu development setup — Metis Shell

Target: **Ubuntu 24.04+** with the **Metis Smithay compositor** (nested session for dev).

## System packages

```bash
sudo apt update
sudo apt install -y \
  build-essential pkg-config libssl-dev \
  libgtk-4-dev libadwaita-1-dev libgtk-4-layer-shell-dev \
  curl git
```

To build/run the **standalone DRM session** (Metis on its own TTY/GPU, not nested),
also install the session, input, and GPU libraries:

```bash
sudo apt install -y \
  libudev-dev libinput-dev libseat-dev \
  libgbm-dev libdrm-dev libegl1-mesa-dev libgles2-mesa-dev
```

If `libgtk-4-layer-shell-dev` is unavailable on your release, build [gtk4-layer-shell](https://github.com/wmww/gtk4-layer-shell) from source and set `PKG_CONFIG_PATH` accordingly.

## Rust toolchain

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

## Build & run

```bash
cd metis-os-workspace/metis-shell
./run-metis.sh --build --session
```

To exercise multi-monitor behaviour in the nested session, split the window into
N side-by-side virtual outputs:

```bash
METIS_VIRTUAL_OUTPUTS=2 ./run-metis.sh --session
```

For day-to-day usage (keybinds, workspaces, scrolling layout, settings), see the
[User Guide](USER_GUIDE.md).

## Standalone session (run on a real TTY/GPU)

Metis autodetects its backend: with `WAYLAND_DISPLAY`/`DISPLAY` set it nests
(winit), otherwise it drives DRM/KMS directly. Force it with
`METIS_BACKEND=winit|drm`.

### Option A — log in from your display manager (recommended, Hyprland-style)

Install the session entry, then pick **Metis** from the GDM/SDDM/greetd session
menu, exactly like selecting Hyprland:

```bash
cd metis-os-workspace/metis-shell
./run-metis.sh --install-session    # builds release; prompts for sudo
```

This installs:

- `/usr/local/bin/{metis-compositor,metis-shell,metis-settings}`
- `/usr/local/bin/metis-session` — the session launcher (sets
  `XDG_CURRENT_DESKTOP=Metis`, `METIS_BACKEND=drm`, exports the activation
  environment, then execs the compositor)
- `/usr/local/share/wayland-sessions/metis.desktop` — the greeter entry

Log out and choose **Metis** at the login screen. The display manager hands the
session its own VT + seat, so libseat takes DRM master cleanly and exiting drops
back to the greeter. **Keep an SSH session open the first few times** in case the
greeter does not return.

### Option B — from a bare TTY (quick test)

Switch to a free VT (`Ctrl+Alt+F3`), log in, then:

```bash
cd metis-os-workspace/metis-shell
./run-metis.sh --session --drm
```

### Escape hatches (DRM session only)

- **Ctrl+Alt+Backspace** — quit Metis (returns to the greeter / shell)
- **Ctrl+Alt+F<n>** — switch virtual terminal

`METIS_DRM_DEVICE=/dev/dri/cardN` overrides primary-GPU autodetection. The DRM
session paints its own (XCursor-themed) pointer; set `XCURSOR_THEME` /
`XCURSOR_SIZE` to change it.

On first run, Metis writes defaults to `~/.config/metis/`:

- `bar.json` — edge bar layout and widgets
- `clock.json` — world clocks and alarms
- `calendars.json` — calendar accounts
- `themes/dark.json`, `themes/light.json` — design tokens

Created later, on demand:

- `config.json` — active theme, onboarding state, briefing-on-login (written when you change a preference)
- `menu.json` — app launcher terminal / file-manager defaults and pinned apps
- `wallpaper.json` — background picture / colour / gradient (and per-output overrides)
- `weather.json` — bar weather unit, auto-detect / IP-geolocation, saved locations
- `dismissed.json` — dismissed calendar reminders
- `desk.json` — compositor window-grid layout (written by the compositor, same directory)
- `briefing.json` — weather coordinates and RSS feed URL (optional; create it yourself)

## Troubleshooting

| Issue | Fix |
|-------|-----|
| Compositor shortcuts don't work (nested in GNOME) | GNOME grabs **Super** globally. Nested sessions default to **`METIS_MOD=alt`** — use **Alt+1**…**Alt+9**, **Alt+Shift+←/→**, etc. Click the Metis window first so it has keyboard focus. To force Super: `METIS_MOD=super ./run-metis.sh --session` after disabling conflicting GNOME shortcuts (Settings → Keyboard → Keyboard Shortcuts). |
| Layer surfaces invisible | Confirm Wayland session + `echo $WAYLAND_DISPLAY` |
| Missing layer-shell | Install `libgtk-4-layer-shell-dev` |
| Shell hangs on startup | Rebuild compositor + shell (`./run-metis.sh --build --session`) |
| Theme not applied | Delete `~/.config/metis/themes/*.json` and restart to regenerate |
| DRM session: black screen / no input | Run from a VT you own (or via the display-manager entry) so libseat can take DRM master; check the log and SSH in to `Ctrl+Alt+Backspace` is unavailable — `pkill metis-compositor`. |
| DRM session: "no GPU found for seat" | Ensure you are in the `video`/`render`/`input` groups and `seatd`/logind is running; try `METIS_DRM_DEVICE=/dev/dri/card0`. |
