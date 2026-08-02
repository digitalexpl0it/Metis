# Security

Metis is a same-user desktop environment. A process running as your UID can
drive the session by design; the control plane is not a sandbox against
same-UID malware. Cross-user and capability boundaries are enforced where they
matter.

For the full product description see the [User Guide](docs/USER_GUIDE.md). This
file is the short map for auditors and contributors.

## Trust model (IPC)

| Boundary | Behaviour |
|----------|-----------|
| Runtime dir | `$XDG_RUNTIME_DIR/metis/` — mode `0700`; fails closed if `XDG_RUNTIME_DIR` is unset (no `/tmp/metis`) |
| Sockets / command files | Mode `0600` |
| Accept path | Linux `SO_PEERCRED` — peer UID must match the compositor euid (`metis_protocol::accept_same_euid`) |
| Widgets process | Short-lived `METIS_IPC_TOKEN` → **widgets** capability only (no EndSession, input inject, capture overlays) |
| Command files | Verb allowlist + 512-byte cap (`parse_runtime_command`); same-UID poke channel, weaker than socket+token |
| Session lock | Rejects focus / launch / clipboard / capture / workspace / session-control and remote-input inject until unlock |

Details: [User Guide — Session IPC trust model](docs/USER_GUIDE.md#session-ipc-trust-model),
[Ubuntu/dev notes](docs/UBUNTU_DEV.md#ipc-trust-model).

## X11 / XWayland

- Native Wayland clients are not keylogged or buffer-scraped by X11 clients.
- Default: one shared XWayland (`config.json` → `"xwayland_mode": "shared"`). Classic X11↔X11 risks remain among X11 apps.
- Opt-in: `"xwayland_mode": "isolated"` — second gaming/Proton XWayland bucket (experimental).
- Abstract X11 socket default-off (`"xwayland_abstract_socket": false`).
- Metis does **not** claim XSECURITY or true per-app XWayland sandboxes yet.

Details: [User Guide — Window management](docs/USER_GUIDE.md#5-window-management).

## Gaming / Flatpak overrides

- `metis-gamingd` / Flatpak optimize uses `Command::new("flatpak").args(...)` with a fixed app/flag allowlist — no shell interpolation of untrusted strings.
- Optimize requires explicit consent (`optimize-gaming yes` / Settings dialog).
- `gaming-flatpak.json` is a ledger of applied overrides, not a command script source.

Residual polish: stricter validation of user-editable paths such as
`extra_steam_paths` and generated launcher env lines.

Details: [User Guide — Steam & Proton](docs/USER_GUIDE.md#steam-proton--steamos-class-gaming),
[Ubuntu/dev — Gaming](docs/UBUNTU_DEV.md#steam--proton-gaming).

## Colour management (upstream)

`wp_color_management_v1` stays **default-off**. Enable only with
`METIS_COLOR_MGMT=1` for testing. Advertising the global to Chromium/Ozone can
abort the session due to an upstream wayland-rs **server/sys** `ObjectData` UAF.
Hardware ICC / LUT / HDR do **not** need that env var.

Tracking: [docs/upstream/README.md](docs/upstream/README.md),
[wayland-rs ObjectData UAF](docs/upstream/wayland-rs-server-objectdata-uaf.md).

## Native libraries

GTK, lcms2, OpenSSL/system TLS, libinput, PipeWire, and similar C dependencies
are outside Rust’s safety model — rely on distro security updates
([PACKAGING.md](docs/PACKAGING.md)).

## Reporting

Prefer a private report to the maintainers (GitHub Security Advisories on this
repository when available) for issues that could affect session integrity or
cross-user isolation. Please include Metis version / commit, distro, and
whether the session is nested (winit) or DRM.

## Residual hardening (post Phase 16)

Phase 15 (product security) and Phase 16 (engineering hardening: CI, deny,
trust-boundary tests, panic triage, command-file allowlist) are shipped — see
[`TODO.md`](metis-os-workspace/TODO.md) and [`CHANGELOG.md`](CHANGELOG.md).

Still open by design or upstream:

1. Sanitize gaming config path/env edges (`extra_steam_paths`, launcher exports).
2. True per-app / per-sandbox rootless XWayland (beyond the two-bucket prototype).
3. Default-on colour management after a wayland-rs **server/sys** fix.
