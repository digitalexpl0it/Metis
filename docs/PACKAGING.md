# Packaging Metis (`.deb`)

Metis ships as a single Ubuntu **amd64** `.deb` for now. There is no PPA yet —
download from [GitHub Releases](https://github.com/digitalexpl0it/Metis/releases)
or build locally.

## Install from a release

```bash
# Example for Ubuntu 24.04 (from the directory that contains the .deb)
sudo apt install ./metis_0.1.0-1_amd64.ubuntu24.04.deb
```

You can also open the `.deb` in your distro’s package installer (Ubuntu Software,
GDebi, Discover, etc.) or run `sudo dpkg -i metis_*.deb` then
`sudo apt-get install -f` if dependencies need filling in.

If `apt` prints `N: … couldn't be accessed by user '_apt' … Permission denied`
when installing from a home folder, ignore it — the package still installed.
That notice is apt’s sandbox, not a Metis packaging bug.

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
| `/usr/bin/metis-{compositor,shell,settings,portal,remote,viewer,gamingd}` | Binaries |
| `/usr/bin/metis-session` | Greeter session launcher |
| `/usr/share/wayland-sessions/metis.desktop` | Session entry |
| `/usr/share/xdg-desktop-portal/…` | Portal backend registration |
| `/usr/share/applications/metis-settings.desktop` + hicolor icons | Settings launcher |
| `/usr/share/applications/metis-viewer.desktop` | Metis Viewer (RDP client UI) |
| `/usr/share/icons/hicolor/*/apps/metis-viewer.png` | Metis Viewer icon (48 + 256) |
| `/usr/share/metis/wallpapers/` | Bundled wallpapers (onboarding / Appearance) |
| `/usr/share/metis/locale/` | i18n catalogs (gettext `.mo` + Fluent `.ftl`) |
| `/etc/pam.d/metis` | Lock-screen PAM service |

### Dependency policy

| Field | Packages | Why |
|-------|----------|-----|
| **Depends** | GTK4, Adwaita, libseat, libinput, GBM/DRM, PipeWire, PulseAudio (`libpulse0`), portal, **kitty**, `liblcms2-2`, … | Required to start a Metis session; kitty is the default terminal; lcms2 for Stage 2 colour LUTs |
| **Bundled** | `libgtk4-layer-shell.so.0` | Not packaged on Ubuntu 24.04 — built in CI / copied from the build host into the `.deb` |
| **Recommends** | `gnome-keyring`, `xdg-desktop-portal-gtk`, **udisks2**, **gvfs**, **gvfs-fuse** | Keyring, portal helpers, and removable-volume mount/eject/LUKS (apt installs by default) |
| **Suggests** | `gnome-remote-desktop`, `freerdp3-wayland` \| `freerdp2-x11`, `nftables`, `policykit-1-gnome` (or another Polkit agent), `gamemode`, `flatpak`, `bluez`, `bluetooth`, `cups`, `system-config-printer`, `fprintd`, `libpam-fprintd`, `libpam-u2f` | Optional features (RDP host needs `gnome-remote-desktop`; Metis Viewer needs FreeRDP; LAN firewall needs nftables/Polkit; lock biometrics need fprintd/u2f + PAM lines) |

Optional Suggests are also offered in the first-run **Optional software** onboarding
step (detect → grey out if present → toggles → **Install selected** via
`pkexec apt-get install`).

## Build a `.deb` locally

Prerequisites: Ubuntu 24.04 build deps from [`UBUNTU_DEV.md`](UBUNTU_DEV.md), plus
`dpkg-dev`, `fakeroot`, and `gettext` (for `msgfmt` when compiling locale catalogs).

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
2. CI installs Rust, runs **`cargo audit`** in `metis-os-workspace/` (fails on
   unignored advisories), then builds on `ubuntu-24.04`, runs `package-deb.sh`,
   and uploads the `.deb` to the GitHub Release for that tag.
3. **workflow_dispatch** builds a prerelease tagged `test-<sha>` for smoke tests.

PRs that touch `Cargo.lock` / `Cargo.toml` also run
[`.github/workflows/audit.yml`](../.github/workflows/audit.yml).

### `cargo audit` ignore policy

Config: [`metis-os-workspace/audit.toml`](../metis-os-workspace/audit.toml).
Only ignore a RUSTSEC advisory when there is no usable patched crate yet **and**
you document why the issue is unreachable (or accepted) plus a tracking note.
Remove ignores as soon as an upgrade lands. Do not merge silent `ignore = []`
placeholders for active CVEs.

### Residual C ABI risk

Metis’s Rust crates sit on top of C libraries (GTK4, lcms2, OpenSSL / rustls
backends, libinput, libseat, PipeWire, PAM, etc.). Memory-safety guarantees stop
at those FFI boundaries — keep the host packages updated via distro security
updates. Release builds also enable `overflow-checks` and `panic = "abort"`.

## Developer install (not packaging)

For day-to-day development, nested sessions and `/usr/local` installs remain:

```bash
cd metis-os-workspace/metis-shell
./run-metis.sh --session              # nested winit
./run-metis.sh --install-session      # release → /usr/local + greeter entry
```

Prefer the `.deb` for end-user machines; prefer `run-metis.sh` while hacking on
the tree.
