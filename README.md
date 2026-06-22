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
│   ├── metis-settings/         # GTK4 settings app (appearance, weather, network, calendars)
│   ├── metis-config/           # Shared config + theme-token types (serde, no GTK)
│   ├── metis-secrets/          # Shared freedesktop Secret Service (oo7) wrapper
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

Configuration lives in `~/.config/metis/`. On first run the shell writes these
defaults:

| File | Purpose |
|------|---------|
| `bar.json` | Edge bar position, size, margins, opacity, backdrop blur, and widget order |
| `clock.json` | World clocks and alarms |
| `calendars.json` | Calendar accounts (local / CalDAV / Thunderbird / Microsoft 365) |
| `themes/dark.json`, `themes/light.json` | Design tokens — accent + secondary accent, semantic status colors, `text_on_accent`, shadows/glows |

Other files are created on demand:

| File | Created when | Purpose |
|------|--------------|---------|
| `config.json` | You change a preference (theme, onboarding) | Active theme, onboarding state, briefing-on-login |
| `dismissed.json` | You dismiss a calendar reminder | Dismissed reminder IDs |
| `desk.json` | The compositor persists its layout | Compositor window-grid layout |
| `briefing.json` | You create it yourself (optional) | Login-briefing weather coordinates and RSS feed URL |
| `weather.json` | You create it yourself (optional) | Bar weather: unit, auto-detect / IP-geolocation toggles, pinned locations |

Edit `bar.json` or `themes/*.json` while the shell runs — bar changes apply
within ~1s and theme edits re-apply the active theme live. Set `opacity` < 1 for
a see-through bar and `blur: true` (with an optional `blur_radius`, default 18)
for a compositor Gaussian backdrop blur behind it.

## Status

Phase 1 — a configurable edge bar on the Metis compositor — is complete. The bar
ships an **ArcMenu-style app launcher** (quick launchers + power actions, a
Frequent/alphabetical app list with search, and a pinnable apps grid), a tabbed
clock popover (calendar, world clocks, stopwatch, movable timer HUD, alarms), a
grouped notification popup fed by a freedesktop (`org.freedesktop.Notifications`)
D-Bus daemon, an interactive Wi-Fi popover, and a weather widget with a forecast
popover (IP-geolocation auto-detect via Open-Meteo). Theming is fully token-driven
with live reload, and the bar (and launcher panel) support transparency plus a
compositor backdrop blur.

Phase 2 is landing: a standalone **`metis-settings`** app (Appearance, Weather,
Network, Calendars pages) backed by the shared `metis-config`/`metis-secrets`
crates, plus **server-side window decorations** — the compositor forces SSD and
draws a titlebar (with title text), a border, and close/minimize/maximize
buttons around each tiled window. Titlebars follow the active light/dark theme,
can be made translucent, and auto-hide (revealing on top-strip hover) for
maximized and edge-snapped windows; **XWayland** lets X11-only apps run in the
session too. The Appearance page includes a Light/Dark style chooser and a
**background picker** — a picture (bundled + imported images, "Add Picture…"), a
solid colour, or a two-stop gradient with selectable direction — that switches the
desktop background live and remembers it across restarts. See
[`metis-os-workspace/TODO.md`](metis-os-workspace/TODO.md) for the current roadmap and
[`CHANGELOG.md`](CHANGELOG.md) for recent changes.

## License

Licensed under the [MIT License](LICENSE).

> Note: bundled panel icons under `metis-os-workspace/metis-shell/icons/papirus/`
> are from the [Papirus icon theme](https://github.com/PapirusDevelopmentTeam/papirus-icon-theme)
> and remain under their original license.
