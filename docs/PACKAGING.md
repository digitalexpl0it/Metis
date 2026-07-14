# Packaging Metis (`.deb`)

Metis ships as a single Ubuntu **amd64** `.deb` for now. There is no PPA yet —
download from [GitHub Releases](https://github.com/digitalexpl0it/Metis/releases)
or build locally.

## Install from a release

```bash
# Example for Ubuntu 24.04
sudo apt install ./metis_0.1.0-1_amd64.ubuntu24.04.deb
```

Then **log out** and pick **Metis** from your display manager’s session menu
(GDM on Ubuntu, SDDM on Kubuntu, and other Wayland-capable greeters). The package
does not reconfigure the greeter — it only installs
`/usr/share/wayland-sessions/metis.desktop`.

Use the `.deb` whose filename matches your Ubuntu series (`ubuntu24.04` today;
`ubuntu26.04` will be added when that LTS is supported). Do **not** mix a
`/usr` package install with `./run-metis.sh --install-session` (which writes
`/usr/local`) without cleaning one of them first.

## What the package installs

| Path | Role |
|------|------|
| `/usr/bin/metis-{compositor,shell,settings,portal,remote,gamingd}` | Binaries |
| `/usr/bin/metis-session` | Greeter session launcher |
| `/usr/share/wayland-sessions/metis.desktop` | Session entry |
| `/usr/share/xdg-desktop-portal/…` | Portal backend registration |
| `/usr/share/applications/metis-settings.desktop` + hicolor icons | Settings launcher |
| `/usr/share/metis/wallpapers/` | Bundled wallpapers (onboarding / Appearance) |
| `/etc/pam.d/metis` | Lock-screen PAM service |

### Dependency policy

| Field | Packages | Why |
|-------|----------|-----|
| **Depends** | GTK4, Adwaita, libseat, libinput, GBM/DRM, PipeWire, portal, … | Required to start a Metis session |
| **Bundled** | `libgtk4-layer-shell.so.0` | Not packaged on Ubuntu 24.04 — built in CI / copied from the build host into the `.deb` |
| **Recommends** | `gnome-keyring`, `xdg-desktop-portal-gtk` | Keyring + portal helpers (apt installs by default) |
| **Suggests** | `gnome-remote-desktop`, `gamemode`, `flatpak`, `bluez`, `bluetooth`, `cups`, `system-config-printer` | Optional features |

Optional Suggests are also offered in the first-run **Optional software** onboarding
step (detect → grey out if present → toggles → **Install selected** via
`pkexec apt-get install`).

## Build a `.deb` locally

Prerequisites: Ubuntu 24.04 build deps from [`UBUNTU_DEV.md`](UBUNTU_DEV.md), plus
`dpkg-dev` and `fakeroot`.

```bash
cd metis-os-workspace
VERSION=0.1.0 ./scripts/package-deb.sh
# → dist/metis_0.1.0-1_amd64.ubuntu24.04.deb

# Or reuse an existing release build:
VERSION=0.1.0 SKIP_BUILD=1 ./scripts/package-deb.sh
```

Environment:

| Variable | Default | Meaning |
|----------|---------|---------|
| `VERSION` | *(required)* | Package version (`0.1.0` or `v0.1.0`) |
| `UBUNTU_SUITE` | `24.04` | Filename / suite label |
| `DEB_REVISION` | `1` | Debian revision |
| `SKIP_BUILD` | `0` | `1` = only stage + pack existing `target/release` binaries |
| `METIS_CARGO_TARGET_DIR` | auto | Override Cargo target directory |

## GitHub Actions release

Workflow: [`.github/workflows/release-deb.yml`](../.github/workflows/release-deb.yml)

1. Push a version tag: `git tag v0.1.0 && git push origin v0.1.0`
2. CI builds on `ubuntu-24.04`, runs `package-deb.sh`, uploads the `.deb` to the
   GitHub Release for that tag.
3. **workflow_dispatch** builds a prerelease tagged `test-<sha>` for smoke tests.

## Developer install (not packaging)

For day-to-day development, nested sessions and `/usr/local` installs remain:

```bash
cd metis-os-workspace/metis-shell
./run-metis.sh --session              # nested winit
./run-metis.sh --install-session      # release → /usr/local + greeter entry
```

Prefer the `.deb` for end-user machines; prefer `run-metis.sh` while hacking on
the tree.
