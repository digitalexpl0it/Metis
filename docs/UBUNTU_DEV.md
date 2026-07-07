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

### Keyring (Secret Service) — runtime dependency

Metis is only a *client* of the freedesktop Secret Service (`org.freedesktop.secrets`, via `oo7`), and so are apps like Cursor, GitHub Desktop, and browsers. A Metis session must therefore have a **provider** running, or those apps fall back to plaintext credential storage ("encryption is low"). The session launcher (`metis-session` / `run-metis.sh --session --drm`) auto-detects and starts whichever of these is installed — install **one** (any desktop works; `gnome-keyring` is not GNOME-specific and is the lightest):

```bash
sudo apt install -y gnome-keyring   # recommended, desktop-independent
# alternatives that also implement the Secret Service API:
#   kwalletd6 / kwalletd5 (KWallet) · keepassxc · pass + pass-secret-service
```

Without PAM auto-unlock (`pam_gnome_keyring`), the login keyring starts locked and the first secret access prompts once per session via gcr's prompter (pulled in by `gnome-keyring`).

### Phase 4 runtime tools (standalone session)

Several settings pages shell out to system services (same pattern as `nmcli` for
Network). Install what you need on the host:

```bash
sudo apt install -y \
  bluez bluetooth \
  cups system-config-printer \
  power-profiles-daemon
```

PipeWire/PulseAudio (`pipewire-pulse` / `pulseaudio-utils` for `pactl`) is usually
already present on Ubuntu desktop installs.

### Remote desktop (Phase 7 — RDP)

Desktop sharing uses **gnome-remote-desktop** in **headless** mode (not the GNOME
Shell interactive sharing daemon). Metis ships `metis-remote` to start/stop RDP
and report JSON status for Settings.

```bash
sudo apt install -y gnome-remote-desktop
```

**Systemd user unit:** `gnome-remote-desktop-headless.service`  
**CLI:** `grdctl --headless` (status, `rdp enable`, `rdp set-credentials`, …)  
**Default port:** 3389

Manual spike on a DRM Metis session (RicePudding or `./run-metis.sh --session --drm`):

```bash
systemctl --user enable --now gnome-remote-desktop-headless.service
grdctl --headless rdp set-credentials "$USER" 'your-strong-password'
grdctl --headless rdp enable
grdctl --headless status
# From another machine on LAN:
xfreerdp /v:$(hostname -I | awk '{print $1}'):3389 /u:$USER /p:'your-strong-password' /dynamic-resolution
```

Metis integration: `~/.config/metis/remote.json` + `metis-remote {status|enable|disable|autostart|set-credentials}`; Settings page **Remote access**; `metis-session` calls `metis-remote autostart` when enabled.

**Compatibility matrix (remote desktop):**

| Tool | Status | Notes |
|------|--------|-------|
| **gnome-remote-desktop (RDP)** | Supported (v1) | Headless unit; portal/PipeWire capture; Settings toggle; text clipboard |
| **RustDesk** | Planned (M2) | Document + optional backend in `metis-remote` |
| **wayvnc / VNC** | Planned (M3) | Needs Smithay capture spike |
| **xrdp** | Out of toggle scope | Separate X11 login session; USER_GUIDE appendix only if needed |
| **AnyDesk / Chrome Remote Desktop** | Spot-check later | Not integrated |

Lock behaviour: compositor refuses capture while locked — remote view freezes until unlock (documented in USER_GUIDE).

### Portal stack (standalone session)

Screenshot and ScreenCast apps talk to **xdg-desktop-portal**, which routes
capture requests to **metis-portal**. Install the portal front-end and GTK
helper on the host:

```bash
sudo apt install -y xdg-desktop-portal xdg-desktop-portal-gtk
```

`./run-metis.sh --install-session` installs `metis-portal` plus
`metis.portal` / `metis-portals.conf` under `/usr/share/xdg-desktop-portal/`.
The compositor starts `metis-portal` before `xdg-desktop-portal` on DRM boot.

To verify screenshot capture without Flameshot:

```bash
metis-portal --capture-test /tmp/test.png
```

### Flatpak (optional)

For sandboxed apps and games from Flathub:

```bash
sudo apt install -y flatpak xdg-desktop-portal xdg-desktop-portal-gtk
flatpak remote-add --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
```

Flatpak apps use the same portal stack as native apps and show up in the Metis
launcher/dock automatically — `run-metis.sh --session` (and the installed
`metis-session`) add the Flatpak `exports/share` dirs to `XDG_DATA_DIRS`, which
is what makes GIO find their `.desktop` entries. Gamepads usually need a Flatpak
device override (`flatpak override --user --device=all <app-id>`) — see the
[User Guide](USER_GUIDE.md#flatpak-apps-and-games) and
[`TODO.md`](../metis-os-workspace/TODO.md) Phase 6.

### Steam / Proton (gaming)

For SteamOS-class desktop gaming on Metis:

```bash
sudo dpkg --add-architecture i386
sudo apt update
sudo apt install -y \
  steam-installer \
  mesa-vulkan-drivers mesa-vulkan-drivers:i386
# NVIDIA: also install 32-bit GL/Vulkan for your driver series, e.g.
# sudo apt install -y libnvidia-gl-XXX libnvidia-gl-XXX:i386
```

Optional: `gamescope` for per-game nested compositor (Steam launch options:
`gamescope -W 1920 -H 1080 -f -- %command%`).

Hybrid GPU laptops: see `METIS_DRM_DEVICE` and `METIS_GAME_GPU` in the
standalone session section below. To debug Proton pointer-lock / menu-click issues,
filter compositor logs with:

```bash
rg 'game-pointer' ~/.local/state/metis/logs/session-*.log
```

Full gaming checklist: [User Guide — Steam & Proton](USER_GUIDE.md#steam-proton--steamos-class-gaming).

### Release build profiles

Default **`release`** uses thin LTO, single codegen unit, and strips symbols —
smaller than stock Cargo release with minimal compositor perf impact. For the
smallest install footprint:

```bash
./run-metis.sh --build --release-small
./run-metis.sh --install-session --release-small
```

The compositor stays at `opt-level=3` in `release-small`; GTK/shell binaries use
size optimization. Details: [`PERF_AUDIT.md`](PERF_AUDIT.md).

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

- `/usr/local/bin/{metis-compositor,metis-shell,metis-settings,metis-portal}`
- `/usr/local/bin/metis-session` — the session launcher (sets
  `XDG_CURRENT_DESKTOP=Metis`, `METIS_BACKEND=drm`, exports the activation
  environment, then execs the compositor)
- `/usr/local/share/wayland-sessions/metis.desktop` — the greeter entry
- `/usr/share/xdg-desktop-portal/{metis-portals.conf,portals/metis.portal}` —
  routes Settings, Screenshot, ScreenCast, Background, and PowerProfileMonitor
  to the Metis portal backend

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

**Client GPU steering (hybrid laptops).** On the DRM backend, the compositor
resolves the PCI identity of the render node it actually draws on and forwards it
to every spawned client as `DRI_PRIME` (Mesa GL) and `MESA_VK_DEVICE_SELECT`
(Mesa Vulkan), so Steam/Proton/XWayland default to the *same* GPU the session
uses rather than silently picking the wrong card. It only sets vars that are
unset, so per-game Steam launch options (`DRI_PRIME=1 %command%`, `prime-run`,
NVIDIA offload) still win. Set `METIS_NO_CLIENT_GPU=1` to disable forwarding, or
`METIS_DRM_DEVICE` to change which card the whole session (and thus clients) use.
When a discrete GPU is present but the panel is driven by the integrated GPU,
Metis also auto-offloads **game and Steam launches** onto the dGPU
(`DgpuOffload::detect`). Override with `METIS_GAME_GPU=igpu|dgpu|off`. This is
inert under the nested winit backend, where the host compositor owns device
selection.

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
| Screenshot / Flameshot fails | `./run-metis.sh --install-session`, log out and back in, then `metis-portal --capture-test /tmp/test.png`; install `xdg-desktop-portal` + `xdg-desktop-portal-gtk` if missing |
