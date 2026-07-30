# Distro dependency profiles for `./install.sh`

| File | Target |
|------|--------|
| `ubuntu-24.04.sh` | Ubuntu 24.04 (noble) — builds gtk4-layer-shell from source |
| `ubuntu-26.04.sh` | Ubuntu 26.04 — `libgtk4-layer-shell-dev` |
| `debian-13.sh` | Debian 13 (trixie) — `libgtk4-layer-shell-dev` |
| `arch.sh` | Arch Linux (`pacman`) |

Each file sets package arrays and `METIS_LAYER_SHELL_FROM_SOURCE`.
