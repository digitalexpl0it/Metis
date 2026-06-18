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

On first run, Metis writes defaults to `~/.config/metis/` (or `~/.config/metis/`):

- `config.json` — active theme, onboarding state, briefing-on-login
- `bar.json` — edge bar layout and widgets
- `themes/dark.json`, `themes/light.json` — design tokens
- `briefing.json` — weather coordinates and RSS feed URL

The compositor reads `desk.json` (same directory) for its window grid layout, separate from the edge bar.

## Troubleshooting

| Issue | Fix |
|-------|-----|
| Layer surfaces invisible | Confirm Wayland session + `echo $WAYLAND_DISPLAY` |
| Missing layer-shell | Install `libgtk-4-layer-shell-dev` |
| Shell hangs on startup | Rebuild compositor + shell (`./run-metis.sh --build --session`) |
| Theme not applied | Delete `~/.config/metis/themes/*.json` and restart to regenerate |
