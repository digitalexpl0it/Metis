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
| Layer surfaces invisible | Confirm Wayland session + `echo $WAYLAND_DISPLAY` |
| Missing layer-shell | Install `libgtk-4-layer-shell-dev` |
| Shell hangs on startup | Rebuild compositor + shell (`./run-metis.sh --build --session`) |
| Theme not applied | Delete `~/.config/metis/themes/*.json` and restart to regenerate |
