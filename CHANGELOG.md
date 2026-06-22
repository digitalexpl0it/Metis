# Changelog

All notable changes to Metis are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project aims to follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [2026-06-21]

### Added

- **App menu launcher (ArcMenu-style)** — the bar's Metis brand icon now opens a
  popover app menu anchored to the icon (not a fullscreen overlay): a left rail of
  quick launchers (Files, Terminal, Settings) and power actions (Suspend, Log Out,
  Restart, Shut Down via `systemctl`), a center column with a **Frequent Apps** +
  alphabetical list and an apps-only **search**, and a **Pinned** grid you can add
  to / remove from. Launching an app (or opening Settings) dismisses the menu
  synchronously so it never lingers behind the new window, and icon tooltips render
  as an in-surface overlay label so they always paint on top of the panel.
- **Start menu & window-titlebar transparency** — both the launcher panel and the
  server-side window titlebar can be made translucent, with independent **Start
  menu opacity** and **Titlebar opacity** sliders in Settings · Appearance (and
  `menu_opacity` / `titlebar_opacity` in `bar.json`). Only the backgrounds go
  transparent — text, icons, and the control buttons stay fully opaque. The
  titlebar is a cached, anti-aliased texture with rounded top corners and a border
  that wraps continuously around the window and under the titlebar.
- **Theme-aware titlebars + auto-hide for maximized / edge-snapped windows** — the
  server-side titlebar follows the active light/dark theme palette (live, via the
  compositor's ~1s theme poll). Maximized windows and left/right/top-corner snaps
  (whose top meets the bar) hide their titlebar so the client uses the whole
  footprint; moving the pointer into the top strip reveals it as a translucent,
  borderless overlay with working minimize / maximize / close, hidden again when
  the pointer leaves.
- **XWayland support** — the compositor spawns and manages an XWayland server
  (`X11Wm` / `XwmHandler`), so X11-only apps run inside a nested Metis session and
  are placed in the window grid alongside Wayland clients.
- **`run-metis.sh --import-env`** — an opt-in flag for `--session` that pushes the
  nested `WAYLAND_DISPLAY` (and `DISPLAY`) into the user D-Bus / systemd activation
  environment so D-Bus-activated apps open inside the nested session, restoring the
  previous values on exit.
- **Decoration polish — rounded buttons + focus-aware dimming** — the server-side
  window controls are no longer flat squares: each is a cached, anti-aliased
  rounded button rendered at 3× supersampling. Focused windows show the
  traffic-light colors (red/green/yellow) with a dark glyph (× close, + maximize,
  − minimize); unfocused windows desaturate all three buttons to gray with no
  glyph, complementing the existing focus-aware titlebar/border/title dimming.
  Button textures are cached per window and only re-rasterized when focus flips.
- **Settings · Appearance Font section** — a new Font card lets you choose the
  DE-wide UI font family + size (via a native font picker) and the body text
  color. Font family/size are stored as theme tokens (`font_family`,
  `font_size_pt`) and applied through the shared stylesheet's base `window` rule;
  when unset (the default) rendering is unchanged.
- **Settings · Network tab overhaul** — the Network page is now split into three
  segmented pill tabs (**Wireless / Wired / Proxy**):
  - **Wireless** keeps the Wi-Fi scan/connect/radio controls and known-networks
    list, and adds a **DNS override** for the active connection (a manual DNS list
    applied with `ipv4.ignore-auto-dns` so it overrides the DHCP-provided servers;
    empty restores DHCP DNS).
  - **Wired** offers per-NIC IPv4 configuration: **Automatic (DHCP)** or **Manual
    (static)** with address/gateway, plus a DNS override that applies in either
    mode.
  - **Proxy** edits the system proxy (**None / Manual / Automatic (PAC)**) with
    per-protocol HTTP/HTTPS/SOCKS host:port, an ignore-hosts list, and a PAC URL.
    Values are read from and written to GNOME's `org.gnome.system.proxy`
    gsettings (honoured by GLib/GTK apps via the default proxy resolver); the tab
    degrades to a hint when the schema is unavailable.
- **Window snap zones (edge/corner drag-to-tile)** — dragging a window by its
  titlebar shows a live translucent preview when the pointer nears a screen edge
  and drops the window into that region: top edge → maximize, left/right →
  halves, the four corners → quarters, bottom → bottom half. The maximize zone
  routes through the same path as the titlebar maximize button, and half/quarter
  snaps mark the window *tiled* (so GTK squares its corners and drops its CSD
  shadow) with edge gaps matching the maximize look — uniform spacing between and
  around snapped windows. Pulling a snapped window off by drag or edge-resize
  restores its normal floating chrome. The top/maximize trigger band is tight
  (16 px) so a normal drag upward doesn't prematurely maximize; the other edges
  use a roomy 64 px band. Core hit-testing lives in
  `metis-grid::pixel_snap_target` (unit-tested); the compositor applies the gaps,
  wires it into `MoveSurfaceGrab`, and renders the overlay in the winit pass.
- **Session relaunch guards + launch audit (`run-metis.sh`)** — `--session` is now
  hardened against an automatic close→reopen loop:
  - **Single-instance lock** (`flock`): a relaunch that overlaps a live session
    can't stack a second nested compositor.
  - **Rapid-relaunch cooldown**: a session that respawns within `4s`
    (`METIS_SESSION_COOLDOWN`) of a clean exit is refused, breaking an instant
    auto-reopen. Override an intentional quick restart with `METIS_FORCE=1`.
  - **Launch audit**: every invocation appends its PID + full parent-process
    chain to `~/.local/state/metis/launch-audit.log`, so an unexpected reopen can
    be traced to the exact invoker (the script/compositor have no respawn logic).
- **Window edge/corner resizing** — any window can now be resized by grabbing its
  edges or corners (an 8px band straddling the border). Because the compositor
  forces server-side decorations, it hit-tests the border itself and starts the
  existing `ResizeSurfaceGrab` directly. Grabbing a tiled window's edge pops it
  out of the grid into a freely-resizable floating window; the new size is
  persisted so it survives later re-layouts. The host cursor shows the matching
  directional resize arrow on hover (↔, ↕, and the two diagonals).

### Changed

- **Window dragging may now run off the left/right/bottom screen edges** —
  floating windows can be dragged partially off those edges (Hyprland-style),
  keeping a grabbable `MIN_VISIBLE_PX` (64px) slice on-screen so they can always
  be pulled back. The top edge is still hard-blocked just under the edge bar.
  Windows that end up off *every* active output (e.g. an external monitor that
  disconnected) are still rescued onto the primary screen.

### Fixed

- **Windows could be dragged "through" the open app launcher** — the bar's
  popovers don't take a Wayland pointer grab, so a press over the open menu fell
  through to whatever window resize band / titlebar sat geometrically beneath it,
  letting you move or resize that window through the menu. Presses over the bar UI
  (strip + any open popover, popup region included) now skip the window-chrome
  hit-tests entirely.
- **Snapped / maximized windows lost their screen-edge gap** — an app that can't
  shrink to its snapped footprint (minimum size larger than the zone, common on a
  small nested screen) overflowed past the reserved edge gap. Oversized auto-hide
  windows are now re-anchored to the snapped edge once they commit, so the
  screen-edge gap survives (the overflow spills toward screen center instead).
- **App launcher stayed open after launching an app** — launching from the menu
  deferred the popdown to an idle callback, which the newly focused window
  swallowed. App launches now close the menu synchronously before spawning.
- **Duplicate / mis-stacked launcher tooltips** — the rail showed both a native
  GTK tooltip and the custom one, and they rendered behind the translucent panel.
  The native tooltip is gone and the custom tooltip is an in-surface overlay label
  (accessible name preserved), so a single tooltip always paints on top.
- **Light-mode launcher search box stayed dark** — the search entry inherited
  GTK's default dark styling; it now uses theme-aware background/border/text/caret
  colors so it matches both light and dark themes.
- **Settings · unreadable text on dark accents** — picking a dark accent (e.g.
  black) left the on-accent text dark and illegible. Changing the primary accent
  now auto-derives a readable on-accent text color (near-black or white) from the
  accent's perceived luminance.
- **Settings · "Colours" → "Colors"** — the Appearance section and related color
  labels now use the American spelling.
- **Settings · colour picker was transparent** — the shared bar stylesheet makes
  every `window` transparent for the layer-shell overlays, which leaked into the
  settings app's spawned dialogs (the colour chooser rendered see-through). The
  settings app now forces a solid themed background on its own windows/dialogs.
- **Settings · Network card padding + stretched Wi-Fi toggle** — list/card content
  (SSID rows, NIC editor text) was flush against the card edges; the
  `.metis-settings-list` container now has internal padding, and the Wi-Fi radio
  switch centres vertically instead of stretching to the row height.
- **Resize band swallowed edge-hugging scrollbars** — the window resize grab band
  reached 8 px *inside* each edge, so hovering a right-edge scrollbar triggered the
  resize cursor instead of the scrollbar. The band now reaches mostly *outside* the
  window (8 px into the gap) and only 3 px inside, so scrollbars stay clickable.
- **Metis brand icon was hard to see in light mode** — the gradient launcher icon
  washed out against the pale light-mode bar. It now gets a soft `-gtk-icon-shadow`
  in light themes (omitted in dark, where a dark shadow would be invisible).
- **Maximized windows could still be dragged** — a maximized (or fullscreen)
  window's GTK headerbar still issued `xdg_toplevel` move requests, letting it be
  dragged around the screen. The compositor now ignores client move requests for
  maximized/fullscreen windows; unmaximize first to move.
- **Session "closed and auto-reopened" (the real root cause)** — `run-metis.sh`'s
  `binary_needs_rebuild()` probed the binaries with `"$bin" --help`. But
  `metis-compositor` ignores `--help` (its parser only knows `-c`/`--command`),
  so the probe **booted a full nested compositor window**. The user saw that
  probe window, closed it (assuming it was the session), and the script then
  proceeded to launch the *actual* session — looking exactly like a
  close→auto-reopen. It was intermittent because the probe only runs when the
  binaries are already up to date (otherwise `find -newer` returns first). The
  binary is no longer executed to test it; the ELF interpreter is checked on disk
  instead.
- **Backdrop blur bled into the bar's drop shadow** — the blur was applied to the
  bar's entire layer surface, which includes the transparent shadow-padding margin
  around the pill, producing an ugly blurred rectangle below/around the bar. The
  compositor now confines the blur to the visible pill (excluding the shadow pad),
  using bar-geometry constants shared via `metis-config`.

### Added

- **Background picker (Appearance)** — the Settings app's Appearance page now has
  a GNOME-style **Style** chooser (two large Light/Dark preview tiles that show
  the current background with a mock window) plus a **Background** section with a
  **Type** selector offering three modes:
  - **Picture** — a thumbnail grid of bundled and user-imported wallpapers plus
    an **Add Picture…** button that copies a chosen image into
    `~/.config/metis/wallpapers/`.
  - **Solid colour** — a single colour picker.
  - **Gradient** — start/end colour pickers and a direction selector
    (top↓bottom, bottom↑top, left→right, right→left, and both diagonals).

  Changes apply **live** via a new `ApplyBackground` compositor IPC command and
  persist to `~/.config/metis/wallpaper.json` so they survive restarts.
- **Live background switching (compositor)** — `CompositorCommand::ApplyBackground`
  makes the compositor re-read `wallpaper.json` and rebuild the background without
  a restart. Solid/gradient backgrounds are generated procedurally at the output
  resolution (and feed the bar's backdrop blur just like a wallpaper image does).
  `resolve_path` honors `wallpaper.json`, and `run-metis.sh` defers to it instead
  of forcing `METIS_WALLPAPER` to a default when a selection exists.
- **Settings app (`metis-settings`)** — a standalone GTK4 application with a
  sidebar/stack layout and four pages: **Appearance** (theme mode, accent/
  secondary/semantic color pickers, bar opacity + blur + blur radius),
  **Weather** (units, auto-detect/IP-geolocation toggles, Open-Meteo location
  search, saved-location management), **Network** (Wi-Fi scan/connect/forget +
  radio toggle, and per-NIC Ethernet IPv4 DHCP/static editors via `nmcli`), and
  **Calendars** (CalDAV + Microsoft 365 account management). Pages write the
  shared `~/.config/metis/*.json` files; the running shell picks changes up via
  its file watchers and new `reload-theme`/`reload-weather`/`reload-calendars`
  runtime commands. Launch it from the bar's launcher icon, the wired-only
  network popover's "Network Settings…" button, or `metis-cmd settings [page]`
  (`--page {appearance|weather|network|calendars}` preselects a page).
- **Shared `metis-config` crate** — all pure (serde + filesystem, no GTK)
  configuration types and the theme token model / stylesheet builder moved into a
  new workspace crate so both the shell and the settings app consume one source
  of truth. Added `save_bar_config`/`save_weather_config`/`save_theme_tokens`
  helpers and exported the per-file path getters.
- **Shared `metis-secrets` crate** — a thin `oo7` (freedesktop Secret Service)
  wrapper so both apps read/write the same keyring items for CalDAV passwords and
  Microsoft 365 refresh tokens.
- **Server-side window decorations** — the compositor now advertises
  `zxdg_decoration_manager_v1` and forces server-side mode on every toplevel, so
  GTK omits its client-side headerbar. Each tiled app window gets a compositor-
  drawn titlebar (with the window title rendered via `fontdue`), a thin border,
  and macOS-style close / minimize / maximize buttons. Clicking the buttons maps
  to the existing close/minimize/maximize actions and dragging the titlebar moves
  the window; the old undrawn 44 px grid "tile header" inset is replaced by this
  real chrome.

## [2026-06-20]

### Added

- **Live theme reload** — editing `~/.config/metis/themes/dark.json` or
  `light.json` now re-applies the active theme within ~250 ms (a GFileMonitor on
  the themes directory re-runs `init_theme()`), mirroring the existing live
  `bar.json` reload. No restart needed to tweak colors.
- **Freedesktop notifications** — Metis now runs an `org.freedesktop.Notifications`
  D-Bus daemon (zbus, on a background thread) so any desktop app's notifications
  surface in the in-bar notification popup. The bus name is acquired with the
  replace flag, so Metis takes over from a previously running daemon (dunst/mako).
  Urgency hints map to notification kinds (low→info, normal→alert, critical→error).
- **Themeable palette** — the stylesheet is now token-driven instead of hardcoded.
  Themes gain a secondary accent (`accent[1]`), a `semantic` status palette
  (`error`/`warning`/`success`/`info`/`payment`), and `text_on_accent`. ~27 fixed
  cyan literals, the notification kind colors, on-accent text, and the floating
  card shadows now derive from theme tokens, so accent/secondary/semantic/shadow
  changes flow through the whole bar UI. New token fields use serde defaults, so
  existing theme files keep working.
- **Bar transparency + backdrop blur** — `bar.json` `opacity` makes the bar
  see-through, and a new compositor Gaussian **backdrop blur** frosts the
  wallpaper behind the bar. Controlled by `blur` (on/off) and a new `blur_radius`
  (pixels, 1–64). The compositor samples the wallpaper under the bar through a
  custom GLES shader and re-reads the blur fields from `bar.json` live (~1s), so
  a future Settings app only needs to write the file. (Blur targets the bar's
  exclusive-zone rectangle; rounded-corner masking is a future refinement.)

## [2026-06-19]

### Added

- **Weather widget** — a condition icon + temperature in the bar opens a popover
  with current conditions (temp, label, daily high/low), a short hourly forecast
  strip, any additional saved locations, and an Open-Meteo attribution footer.
  Data is fetched (keyless) from Open-Meteo on a background thread and refreshed
  every 15 minutes (and on popover open). Location auto-detects via IP
  geolocation (city-level, keyless ipwho.is) with an offline system-timezone
  fallback; temperature unit auto-resolves (US-style regions → °F, otherwise °C).
  A `weather.json` config (unit, auto-detect, pinned locations) is read when
  present (incl. an `ip_geolocation` toggle) — the upcoming Settings app will
  manage it. Auto-detection prefers IP geolocation and falls back to the offline
  `zoneinfo` tables, caches the result, applies a 12s timeout to all HTTP so a
  stalled host can't hang the widget, logs failures, and retries every 30s after
  a failed fetch instead of waiting the full refresh window.
- **Wi-Fi popover** — the bar network icon is now interactive: clicking it opens a
  popover with a read-only Ethernet status row, a scrollable list of nearby Wi-Fi
  networks (signal strength, lock for secured, check for the active one), a Wi-Fi
  radio toggle, and a refresh/rescan button. Secured networks reveal an inline
  password entry; a spinner shows on the row while connecting. Backed by `nmcli`
  through a background command queue mirroring the audio widget.
- **Startup splash** — a centered overlay shows the Metis logo on a translucent card
  with a loading progress bar at session start. The bar crawls while the shell comes
  up, ramps to 100% once it's ready (with a minimum on-screen time and a hard timeout
  fallback), then fades out. The logo is embedded in the binary. Like the timer HUD,
  the layer surface is kept mapped and parked off-screen rather than destroyed, so
  closing it never disconnects the shell.
- **Startup chime** — an embedded `startup.mp3` plays once alongside the splash via
  GTK's media backend (best-effort; degrades silently if no media backend is present).
- **Launcher icon** — the Metis brand icon is pinned to the far-leading edge of the
  bar as a button (the seed of the upcoming app-menu launcher). The icon asset is
  embedded in the binary, so it renders regardless of the working directory.
- **Clock popover suite** — the bar clock now opens a tabbed popover with pill-style
  tabs: Calendar, World Clocks, Stopwatch, Timer, and Alarms.
  - **World Clocks** — inline searchable timezone picker (entry + list, no nested
    dropdown), up to three additional zones listed under the calendar.
  - **Stopwatch** — full-size view with lap times in a scrollable list.
  - **Timer** — a movable, always-on-top layer-shell HUD with pause/close controls
    that can be dragged anywhere on screen but never over the edge bar.
  - **Alarms** — a segmented sound selector (replaces the dropdown) for choosing the
    alarm tone.
- **Notification popup** improvements:
  - "Clear all" button (bottom-right) that clears every notification.
  - Identical notifications are grouped into one card with a count badge.
  - Per-kind icons (error, alarm/notification, success, information, payment),
    tinted to match the notification's accent color.
  - Clearing animates each card sliding out to the left.
  - Vertical scrollbar when the list overflows.
  - Demo feed for testing, enabled with `METIS_DEMO_NOTIFICATIONS=1`.
- **In-bar notification routing** — timer/alarm/reminder alerts are pushed into the
  bar's notification popup via a runtime notification store instead of spawning an
  external `notify-send` process.

### Changed

- Existing `bar.json` layouts are migrated to include the new `weather` widget
  ahead of the system/clock cluster.
- Dropped two redundant `nmcli` calls per network poll (the legacy
  `network_label`/`network_connected` snapshot fields the Wi-Fi popover replaced).
- Timer-finished alerts no longer tear down the HUD's layer surface; the HUD is parked
  off-screen instead, keeping the surface mapped for the session.
- Notification cards are wider with proper internal padding so text no longer sits at
  the card edge.
- **Wallpaper decoding** is now fully off the compositor's main thread and debounced:
  - `invalidate()` detaches the in-flight decode instead of joining it on the main
    loop, fixing a multi-second freeze on every window resize.
  - Resizes (maximize/restore) are debounced into a single decode and driven from the
    compositor heartbeat, so a re-decoded wallpaper appears promptly instead of after
    the next unrelated damage event (previously a 10–20s delay on maximize).
  - The full-resolution source image is cached in memory, so resizing only re-scales
    instead of re-reading and re-decoding the JPEG from disk.
- **Dev builds compile dependencies optimized** (`[profile.dev.package."*"] opt-level = 3`).
  The `image` crate was running unoptimized, making wallpaper decode/resize take
  several seconds; this brings it down to tens of milliseconds while keeping our own
  crates fast to compile.

### Fixed

- Silenced the benign `surface missing from known popups` ERROR from Smithay
  (GTK tears down short-lived entry sub-popups before their grab resolves) with a
  targeted log filter that keeps all other xdg-shell diagnostics.
- **Blank screen on startup from the chime** — the GStreamer media backend aborts
  (`gtk_gst_media_file_open: code should not be reached`) when a `MediaFile` is built
  from an input stream, which killed the shell before the bar appeared. The embedded
  chime is now materialized to a temp file and opened via `MediaFile::for_filename`.
- **Square edge-bar shadow** — the rounded pill's drop shadow was clipped square at
  the layer surface's rectangular edge. The surface now reserves a small padding
  around the pill (`BAR_SHADOW_PAD`) with the pill inset inside it, and the shadow was
  tightened so it renders fully and follows the bar's rounded corners.
- **Edge bar crash on timer completion** — removing the HUD's tooltips and keeping its
  layer surface mapped eliminates the `Broken pipe` Wayland protocol error that
  disconnected the shell when a timer ended.
- **Clock popover freeze / `surface missing from known popups`** — keyboard/text input
  in the popover now works via proper `xdg_popup` grabs; dropdowns no longer render
  behind the popover.
- **Notification popup layout overflow** — bounded the wrapping title/message labels
  (`max_width_chars`) and pinned the per-kind icon to a fixed size, fixing the GTK
  `gtk_widget_measure`/`size_allocate` overflow (huge/`INT_MIN` widths) introduced with
  the notification icons.
- **Wallpaper/briefing could not be re-enabled in `--session`** — `METIS_NO_WALLPAPER=`
  / `METIS_NO_BRIEFING=` (explicit empty) now correctly enable wallpaper and briefing.
  `run-metis.sh` no longer collapses an empty value back to `1` (`${VAR:-1}`), and the
  compositor/shell now treat the flags as disabled only when set to a *non-empty* value
  (previously any set value, including empty, counted as disabled).
