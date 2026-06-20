# Metis

> **Metis** is a next-generation Wayland desktop environment built in Rust. The
> **Metis compositor** owns the Wayland session, window grid, and wallpaper; the
> **Metis shell** is a GTK4 layer-shell edge bar and command overlay, mapped on
> demand and torn down cleanly when dismissed.

## Philosophy

- **Performance first** — idiomatic, low-overhead Rust with `tokio` async and damage-driven rendering.
- **Compositor-first** — a Smithay compositor owns the session; the shell is spawned by it.
- **On-demand shell** — `wlr-layer-shell` overlays (edge bar, command overlay) summoned when needed.

## Workspace layout

```
.
├── metis-os-workspace/          # Cargo workspace
│   ├── metis-compositor/        # Smithay Wayland compositor (winit nested backend for dev)
│   ├── metis-shell/      # Metis shell — GTK4 layer-shell edge bar + command overlay
│   ├── metis-grid/              # Window grid / tiling reflow engine
│   ├── metis-protocol/          # Shared JSON IPC contracts between compositor and shell
│   └── assets/wallpapers/     # Bundled default wallpaper
└── docs/                      # Development setup and notes
```

## Technology stack

- **Backend:** Rust (stable), `tokio`, `zbus` (D-Bus), `serde`/`serde_json` for JSON contracts.
- **Compositor:** [Smithay](https://github.com/Smithay/smithay) with a `winit` nested backend for development.
- **Shell / UI:** GTK4 with [`gtk4-layer-shell`](https://github.com/wmww/gtk4-layer-shell).
- **Configuration:** JSON under `~/.config/metis/` (`bar.json`, `themes/*.json`, `briefing.json`).

## Quick start

See [`docs/UBUNTU_DEV.md`](docs/UBUNTU_DEV.md) for full system-package setup (Ubuntu 24.04+).

```bash
# Install GTK4 + layer-shell dev packages (Ubuntu example)
sudo apt install -y build-essential pkg-config libssl-dev \
  libgtk-4-dev libadwaita-1-dev libgtk-4-layer-shell-dev

# Build and launch a nested dev session
cd metis-os-workspace/metis-shell
./run-metis.sh --build --session
```

The nested session runs inside your existing Wayland session via the winit backend.
Session mode disables wallpaper and briefing by default; re-enable with:

```bash
METIS_NO_WALLPAPER= METIS_NO_BRIEFING= ./run-metis.sh --session
```

## Configuration

On first run, defaults are written to `~/.config/metis/` (falling back to `~/.config/metis/`):

| File | Purpose |
|------|---------|
| `config.json` | App preferences — active theme, onboarding state, briefing-on-login |
| `bar.json` | Edge bar position, size, margins, opacity, and widget order |
| `desk.json` | Compositor window-grid layout |
| `themes/dark.json`, `themes/light.json` | Design tokens |
| `briefing.json` | Weather coordinates and RSS feed URL |

Edit `bar.json` while the shell runs — changes apply within ~1s.

## Status

Phase 1 — a configurable edge bar on the Metis compositor. The bar ships a tabbed
clock popover (calendar, world clocks, stopwatch, movable timer HUD, alarms) and a
grouped notification popup with per-kind icons. See
[`metis-os-workspace/TODO.md`](metis-os-workspace/TODO.md) for the current roadmap and
[`CHANGELOG.md`](CHANGELOG.md) for recent changes.

## License

Licensed under the [MIT License](LICENSE).

> Note: bundled panel icons under `metis-os-workspace/metis-shell/icons/papirus/`
> are from the [Papirus icon theme](https://github.com/PapirusDevelopmentTeam/papirus-icon-theme)
> and remain under their original license.
