# Changelog

All notable changes to Metis are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project aims to follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

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
