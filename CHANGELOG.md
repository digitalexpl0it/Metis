# Changelog

All notable changes to Metis are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project aims to follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [2026-07-02]

### Added

- **Compositor-rendered session lock** — Metis now has a first-party lock
  screen. `Super+L`, the shell menu's **Lock** button, and the `LockSession` IPC
  command all put the compositor into a locked mode where it renders the lock UI
  itself (background + optional blur/dim + clock + password field) and no client
  is composited or sent input; keyboard focus is cleared, and focus/reveal/launch
  and screen-capture IPC are refused while locked. Authentication runs real
  **PAM** on a worker thread (service `/etc/pam.d/metis`, installed by
  `run-metis.sh`, with a graceful fallback to the system `login` stack), and the
  result is marshaled back to the event loop via a `calloop` channel; the typed
  password is never logged, is zeroized after every attempt, and failures are
  throttled. The lock background is configurable on **Settings → Appearance →
  Background → Lock screen**: reuse the desktop wallpaper (default), a dedicated
  picture, a solid colour, or a gradient — with blur and a ~35% dim on by
  default, a show-clock toggle, and a **Lock when the screen blanks** option that
  closes the idle → lock loop (`lock.json`, live-reloaded via `ReloadLock`). The
  lock UI reuses the existing `fontdue` text and wallpaper/blur GL pipelines, so
  it renders identically on the nested winit dev session and the DRM backend.
  The lock screen now also: offers a **12-/24-hour clock** toggle (Settings →
  Background); draws a rounded, translucent **password field** (brighter border
  once you start typing, with an "Enter Password" placeholder) so it's obvious
  where input goes; greets you by your account **display name** (GECOS), falling
  back to the login name; and shows **suspend / restart / shut down** controls in
  the bottom-right that highlight on hover and are clickable straight from the
  lock screen (each drawn as a small anti-aliased glyph).
- **Idle screen blanking with inhibitors** — the compositor now powers the
  display down after the Settings → Power **screen blank** timeout
  (`power.json` `blank_after_minutes`, `0` = never) and wakes instantly on any
  input. A new `idle` module tracks the last input activity, arms a self-
  correcting calloop timer, and on timeout drives every connected DRM connector's
  `DPMS` property off (`udev` backend; a no-op under the nested winit dev
  session). Blanking keeps the `wl_output` globals and CRTC mode intact, so
  clients never see a monitor "disconnect" and nothing reflows across a
  blank/wake cycle; scan-out is suspended while blanked so a page-flip can't wedge
  a powered-down connector. The blank timeout live-reloads — saving the Power
  page sends the new `ReloadPower` IPC command and the compositor re-arms without
  a restart.
- **Idle inhibitors (keep the screen awake)** — three inhibitor sources feed a
  single count that suspends the blanker (and wakes a blanked screen):
  - **Wayland `zwp_idle_inhibit_manager_v1`** — native apps (video players,
    presentations) that mark a surface as "keep awake".
  - **`ext_idle_notify_v1`** — idle-notification clients (swayidle-style) are kept
    in sync via Smithay's `IdleNotifierState` (activity + inhibit state).
  - **D-Bus `org.freedesktop.ScreenSaver`** (and
    `org.freedesktop.PowerManagement.Inhibit`) — the interfaces Chromium/Electron,
    Firefox, SDL games, and VLC/mpv actually use. `metis-portal` now owns both
    names, allocates per-caller cookies, forwards `Inhibit`/`UnInhibit` to the
    compositor over the new `InhibitIdle`/`UninhibitIdle` IPC commands, and
    reclaims a crashed client's inhibitors when its D-Bus peer drops off the bus
    (`NameOwnerChanged` watch), so a dead browser can never pin the screen awake.
  - While any inhibitor is held the compositor also takes a **logind `idle`
    inhibitor lock** (`systemd-inhibit --what=idle --mode=block`), so a running
    game/video blocks automatic idle-suspend as well as blank — without blocking
    a manual suspend. Released as soon as the last inhibitor clears.
- **Settings inputs commit cleanly** — the Power page persists on a debounced
  background thread (no more UI stall from `powerprofilesctl`/`busctl` on every
  keystroke), stops periodic refreshes from stomping a value mid-edit, and every
  settings page now drops focus from an entry when you click empty space.
- **Per-output ICC colour calibration (hardware gamma)** — a display's ICC
  profile assigned on the Settings → Display page now actually affects the
  screen. A new dependency-free parser (`color_management/vcgt.rs`) reads the
  profile's `vcgt` calibration curves (both table and formula encodings, fully
  bounds-checked with unit tests), and `output_gamma.rs` resamples them to each
  CRTC's gamma length and uploads them via DRM `set_gamma`. Calibration is
  applied on every outputs reload (after `apply_color_profiles`), on connector
  bring-up, after mode-sets, and re-applied on VT-switch/session resume (which
  reset the ramp). Outputs with no profile — or a profile without a `vcgt` tag —
  get an identity ramp, so toggling a profile off restores neutral output.
  Full 3D gamut mapping / HDR (a GLES LUT post-pass) remains a documented
  Stage 2 follow-up.
- **`wp_color_management_v1` handler hardened** (still opt-in, `METIS_COLOR_MGMT=1`)
  — every request now initialises the objects it creates, validates client
  input, and reclaims its description records on destroy (previously the
  `descriptions`/`description_objects` maps grew without bound as Chromium
  churned image-description objects). The global is **not** advertised by
  default: advertising it still destabilises the session — Chromium (Cursor)
  engaged the colour pipeline and the compositor died with heap corruption
  (`malloc_consolidate(): unaligned fastbin chunk`), blanking the display. The
  per-output ICC hardware-gamma calibration above is independent of this global
  and stays on. See below for the root-cause investigation.

### Fixed

- **`wp_color_management_v1` description records no longer leak** — destroying a
  `wp_image_description_v1` now runs a `destroyed` handler that drops its record
  from the `descriptions`/`description_objects` maps (and inert/error-path
  descriptions register for the same cleanup). Chromium/Ozone create and destroy
  image descriptions constantly — reusing the freed protocol ids for other
  interfaces — so without this the maps grew without bound and pinned each
  destroyed object's `alive` `Arc` for the life of the session.

### Investigated (not yet fixed)

- **`wp_color_management_v1` heap corruption root-caused to the wayland-rs object
  lifecycle** — the crash that blanks the display when the global is advertised
  was reproduced deterministically (~4 s) in a nested `--session` under gdb with
  `METIS_COLOR_MGMT=1` and a Chromium client, matching the hardware signature
  exactly (`malloc_consolidate(): unaligned fastbin chunk`). The abort backtrace
  is a use-after-free while dropping a wayland `ObjectData` `Arc` inside
  `wayland-backend`'s `resource_dispatcher` — **not** in Metis code. It fires
  when Chromium destroys a `wp_image_description_v1` and immediately reuses the
  freed protocol id for a `wp_image_description_info_v1` in the same dispatch
  batch. None of the compositor's `unsafe` (the ICC memfd path) executes in the
  crashing trace — only safe `get_information` → parametric-info event sends — and
  adding the description-cleanup handler above did not change it, confirming the
  fault is in the wayland-rs/Smithay-fork lifecycle rather than Metis. A
  dependency bump was tried and ruled out: `wayland-backend 0.3.15` /
  `wayland-server 0.31.13` are already the newest published, and bumping
  `wayland-protocols` to 0.32.13 (which only adds the `windows_bt2100` v3
  feature) reproduced the identical crash. Since the generic destroy+id-reuse
  pattern works for every other client, this is a colour-management-path bug in
  the sys backend, not a version lag. **Decision:** the global stays **opt-in**
  and we wait for an upstream fix; revisiting means an ASAN build to pin the
  exact allocation, or bisecting which generated info event triggers it.

- **`wp_color_management_v1` can no longer panic and abort the compositor** —
  several request handlers returned early on error (unresolved output, a surface
  that already had a colour object, `create` before the required ICC/parametric
  data was set, `get_information` on a stale/opaque description, and the
  unadvertised `create_windows_scrgb`) while leaving the request's newly created
  protocol object uninitialised. wayland-rs treats an uninitialised `New<>` as a
  fatal error and panics, which under the DRM backend takes down the whole
  session. Every such path now initialises the object first and then raises the
  spec-mandated protocol error (`unsupported_feature`, `incomplete_set`,
  `surface_exists`, `no_information`), faulting only the offending client. This
  is what made the protocol safe to advertise by default.

- **First-class XWayland window management** — X11 windows are now full Metis
  citizens instead of bare centered surfaces. They enter the shared window
  registry, receive a Metis server-side titlebar (drag-move, close/maximize/
  minimize controls, edge-resize), are placed as bar-aware floating windows
  (restoring saved per-app geometry), participate in focus/stacking, and appear
  in the dock with open/close/focus IPC events. This is what actually fixes
  **Claude Desktop**: its launcher defaults to `--ozone-platform=x11`
  (`use_x11_on_wayland=true` unless `CLAUDE_USE_WAYLAND=1`), so Claude runs as an
  XWayland client and never used the native-Wayland `xdg-decoration` path the
  previous fix targeted — which is why it kept loading under the edge bar with no
  titlebar and no way to move or resize it.
  - `WindowRecord` now holds a `WindowSurface` enum (`Wayland(ToplevelSurface)` /
    `X11(X11Surface)`); `WindowRegistry::register_x11` keys X11 windows by their
    stable X11 window id (their `wl_surface` may associate later).
  - A single `send_window_configure` seam pushes geometry to either client type
    (Wayland pending-size + configure, X11 absolute `configure`), so
    `apply_window_rect`, maximize, snap-restore, and resize all work for both.
  - X11 windows are floating-only (kept out of the tiling grid); maximize and
    fullscreen route through the X11 `set_maximized` / dedicated fullscreen path.
  - Move/resize grabs and titlebar hit-testing were switched from surface-keyed
    to `id_for_window` lookups so they no longer panic on X11 windows.
  - Override-redirect surfaces (menus/tooltips) stay undecorated and untracked.

### Fixed

- **Electron apps launched from the Metis menu prefer native Wayland (fixes Claude
  Desktop "opens then closes")** — Claude Desktop launched from the menu opened for a
  moment and immediately quit. Its own launcher log showed `Electron exited with
  code: 0` (a clean `window-all-closed` quit, not a crash): Claude defaults to the
  **XWayland** backend (`--ozone-platform=x11`), and Electron's XWayland map/unmap
  window juggling on launch is unstable under Metis, so it tore its own windows down
  and quit. Launching it as a native Wayland client (`CLAUDE_USE_WAYLAND=1`) is
  stable. Metis now injects native-Wayland preference into every app it spawns —
  `ELECTRON_OZONE_PLATFORM_HINT=auto` (standard Electron opt-in) and
  `CLAUDE_USE_WAYLAND=1` (Claude's launcher ignores the hint and force-passes
  `--ozone-platform=x11` otherwise) — from the compositor's client-spawn env, the
  installed `metis-session`, and the dev `run-metis.sh --session`. Both defer to any
  value the surrounding session already set, so XWayland can still be forced per app.
- **Restoring an XWayland app from its system tray no longer flashes open and
  closes** — a client-driven remap (e.g. left-clicking Claude Desktop's tray icon)
  went straight through `map_x11_toplevel` without adopting the active workspace,
  unlike the dock-activate path. `apply_window_rect`'s visibility guard then unmapped
  the window whose stale workspace no longer matched the active one, so it opened for
  a frame and vanished. The remap path now re-homes the window to the current
  workspace before applying geometry.
- **XWayland snap zones now apply correctly** — dragging an X11 window into an edge
  snap zone tiled it to the wrong size: the move-grab release ran an unconditional
  X11 position-sync `configure` *after* `apply_snap`, clobbering the snapped size
  with the client's pre-snap size. The sync is now scoped to plain drags only
  (snap/tile paths already push geometry to the client themselves).
- **Electron "close to tray" no longer leaves a hollow dock window, and tray restore
  no longer flashes open-and-closed** — when an X11 app (Claude Desktop) withdraws
  its window to the system tray, Metis drops it from the dock/tasklist (matching
  GNOME/KDE) instead of keeping a stale entry that restored to an empty frame. The
  withdraw is now **debounced** (600 ms): Electron unmaps/remaps its X11 window
  constantly during normal operation and while restoring from the tray, so reacting
  to every unmap thrashed the dock and made the window flash open then vanish. Metis
  now only treats a *sustained* unmap as a real withdraw; a re-map within the grace
  period cancels it. The registry record is retained keyed by X11 window id, so a
  genuine tray restore re-announces (WindowOpened) and re-shows the window with focus
  on the current workspace.
- **XWayland windows no longer snap to the top-left corner under the edge bar** —
  `configure_notify` was mapping the compositor-side element to whatever position
  the X11 client reported. Chromium/Electron apps (Claude Desktop) map at `(0,0)`
  and re-assert their own position, so right after Metis placed the window below
  the bar the client's next configure dragged it back into the corner — both on
  first map and immediately after an interactive move. Metis now owns placement
  for managed X11 toplevels and ignores client-driven position changes (only
  override-redirect surfaces track their own geometry); the move grab syncs the X
  server to the final on-screen position on release so client popups/menus still
  land correctly.
- **Terminals get Metis server-side titlebars again** — `SSD_APP_IDS` now lists the
  bare runtime `app_id`s terminals actually report (`kitty`, `Alacritty`, `foot`,
  `ghostty`, `wezterm`, `xterm`, …) instead of only reverse-DNS forms. Terminals
  bind `xdg-decoration` early, so an unlisted id fell through to the CSD fallback
  and lost Metis chrome. Also removed the garbled `org.wezfwez.foot` entry.
- **Frameless Electron apps (e.g. Claude Desktop) get a Metis titlebar** — these
  apps report the generic `chromium` Wayland `app_id`, so Metis treated them like
  the Chromium browser (client-side decorations) — but they launch Ozone with
  `WaylandWindowDecorations` + no custom titlebar, i.e. they *ask the compositor*
  to decorate and draw no controls themselves. That left them with no titlebar,
  no resize borders, unmovable, and mapped under the edge bar. `resolve_uses_ssd`
  now **honors an explicit `xdg-decoration` server-side request over the
  `chromium ⇒ CSD` heuristic**, so such apps get full Metis server-side chrome
  (titlebar, drag-move, resize, correct placement below the bar). Real browsers
  request client-side, so they're unaffected. As a secondary signal, a
  chromium-class window whose client process (`/proc/<pid>/comm`) isn't a known
  browser is also treated as an Electron shell.
- **Capture-overlay detection no longer grabs large app windows** — the non-portal
  geometry heuristic now requires a floating window to span the *entire* desktop
  (including the edge-bar strip at y≈0), rather than merely covering ≥85% of it. A
  normal or maximized app always sits below the bar, so it can no longer be mistaken
  for a screenshot overlay (Flameshot) and yanked under the bar or have its stacking
  hijacked.
- **System tray icons clear when an app exits** — the `StatusNotifierWatcher` now
  watches `org.freedesktop.DBus.NameOwnerChanged` and drops any tray item whose
  owning connection vanishes. Apps usually exit without calling
  `UnregisterStatusNotifierItem`, so their tray icon previously lingered until a
  later interaction failed with `ServiceUnknown`. Matches both items registered
  under a unique connection name (Claude: `:1.55`) and those advertising a
  well-known name (Flameshot / most Qt/KDE apps: `org.kde.StatusNotifierItem-…`),
  and reports the item's destination bus name so the bar UI actually removes it.
- **`outputs.json` reload feedback loop** — the night-light schedule time picker
  wrote its entry programmatically (`refresh` / reformat), which re-fired GTK's
  `changed` signal, rescheduled the 450 ms debounce, and called `save_and_apply`
  → `ReloadOutputs` again — an endless ~2×/sec loop that started with no user
  interaction while the Settings **Display** page was open. The picker now
  suppresses `changed` during its own writes and skips no-op applies. As
  defense-in-depth the compositor now treats a `ReloadOutputs` whose on-disk
  config is unchanged as a no-op (no wallpaper re-decode / reflow / log spam).
- **Settings: broken (missing) icons on several rows** — six symbolic icon names
  used across the settings pages don't exist in the Adwaita icon theme and were
  rendering as the "missing image" placeholder: `view-columns` (Edge bar → New
  workspace layout), `preferences-desktop-effects` (Windows → Window animations),
  `gesture-single-tap` / `gesture-drag` (Touchpad → Tap to click / Tap and drag),
  and `preferences-system-mouse` / `speedometer` (Mouse + Touchpad → Pointer speed
  / Acceleration). All now use on-theme names that actually ship (`view-dual`,
  `preferences-desktop-screensaver`, `input-touchpad`, `object-select`,
  `input-mouse`, `power-profile-performance`).
- **Settings: mouse wheel over a slider no longer drags it (and spams IPC)** — the
  Edge bar and Windows sliders (opacity, blur radius, distance, and the border
  thickness controls) now forward wheel events to the page scroller instead of
  changing their own value, matching the Display page. Scrolling past a slider
  scrolls the page rather than nudging the setting (which also fired a `reload-bar`
  on every notch).
- **Settings: toggle switches no longer lag when flipped** — the Edge bar "Backdrop
  blur" and Windows "Window animations" switches ran their `bar.json` write + IPC
  synchronously inside the toggle handler, so the switch visibly stalled before
  moving. They now paint the new state first and defer the save to the next idle
  turn (via `defer_switch_active_notify`).

### Changed

- **Settings: split the "Appearance" page into four focused pages** — the single
  page had grown to cover theme, colours, font, wallpaper, the entire edge bar,
  and every window-decoration control. It is now four sidebar entries under
  **Desktop**: **Appearance** (theme mode, accent/semantic colours, font),
  **Background** (wallpaper picture/solid/gradient plus per-display overrides),
  **Edge bar** (position, displays, workspaces, layout, tray mode, distance,
  opacity, blur, and the bar border), and **Windows** (animations, titlebar
  opacity, and the title-pill/window-frame borders). Shared colour/wallpaper
  plumbing moved to a new `appearance_common` module, and Edge bar/Windows now
  persist only the `bar.json` fields they each own (via `update_bar`) so the two
  pages can't clobber each other's settings.

## [2026-07-01]

### Added

- **Generic capture-overlay session** — Screenshot/screencast portal requests notify the
  compositor via `BeginCaptureOverlay` / `EndCaptureOverlay` IPC. Floating windows that
  span the desktop are elevated automatically (no per-app whitelist). While active,
  pointer routing and focus stacking ignore apps beneath the overlay so hover cannot
  steal stacking from tools like Flameshot; region-drag clicks stay on the overlay
  instead of raising apps beneath transparent areas.
- **Night light compositor** — Settings → Display night light toggle and colour
  temperature now apply live via a warm fullscreen overlay (`outputs.json`
  `night_light_enabled` / `night_light_temperature`); works in nested winit and
  DRM sessions.
- **Display — duplicate (mirror) mode** — Settings → Display **Duplicate displays**
  toggle with a **Show on** source picker; persisted in `outputs.json`
  (`display_mode`, `mirror_source`). On a DRM session with two or more monitors,
  the compositor maps all outputs to the origin, renders the source once per
  frame, and scale-to-fits onto each CRTC with letterboxing when resolutions
  differ. Uses the existing **Save display settings** keep/revert flow.
- **VRR / adaptive sync** — Settings → Display **Adaptive sync** toggle per
  output (Applies live); persisted as `vrr_enabled` in `outputs.json` and
  applied via DRM `VRR_ENABLED` on real hardware when supported.
- **Night light schedule** — Settings → Display **Use schedule** with local
  **From** / **To** times (overnight ranges supported); compositor applies the
  warm overlay only inside the window while night light is enabled.
- **Colour profile paths** — per-output ICC file picker in Settings → Display;
  paths saved to `outputs.json` and loaded by the compositor on apply (GPU
  colour transforms still pending).
- **`wp_color_management_v1` compositor handler** — Metis can advertise the colour
  management protocol when `METIS_COLOR_MGMT=1` (off by default). Per-output ICC
  profiles from `outputs.json` are loaded on apply/reload. Disabled by default
  because Chromium/Ozone currently destabilises the DRM session when the global
  is present.

### Fixed

- **Chromium / Cursor session crash** — `wp_color_management_v1` is now **off by
  default** (`METIS_COLOR_MGMT=1` to enable). Chromium/Ozone was crashing the
  DRM session once colour info was delivered; disabling the global restores the
  pre-protocol launch path while ICC profile loading in `outputs.json` remains
  for follow-up GPU transforms.
- **CSD browser chrome + resize** — grant `ClientSide` xdg-decoration as soon as
  app_id classifies Chromium/Cursor; defer decoration pushes until app_id is
  known; include `decoration_mode` on the first configure (Chromium latched
  server-side / close-only when an early bare configure raced ahead); restore
  compositor edge resize bands on native CSD footprints.
- **Chromium / video fullscreen** — client `xdg_toplevel` fullscreen requests
  (e.g. YouTube in Chromium) now enter true fullscreen on the correct output
  instead of being ignored; exiting video fullscreen from a maximized window
  restores maximize below the edge bar instead of leaving the window under the
  bar or losing maximized state (re-seat maximized CSD clients on commit and
  when the edge bar layer reclaims its exclusive zone).
- **Capture overlay false positive** — maximized Chromium was mistaken for a
  screenshot overlay (session lockup, window under the edge bar). Overlay
  detection now requires a floating near-fullscreen window and excludes browsers;
  input blocking only applies while an overlay window is actually registered.
- **X11 / XWayland fullscreen** — `_NET_WM_STATE_FULLSCREEN` from X11 clients
  (Steam, legacy players, etc.) now fills the monitor and restores the prior
  window geometry on exit.
- **Settings → Detect displays crash** — clicking **Detect displays** no longer
  panics with `RefCell already borrowed`; the display arrangement canvas now
  releases the selection borrow before updating the selected monitor.
- **Settings sidebar width on maximize** — left nav stays fixed at 248 px; only
  the page content expands when the window is maximized.

## [2026-06-30]

### Added

- **Settings — macOS-style control center** — grouped sidebar (Displays, Desktop,
  Connectivity, Input, System), coloured icon badges per page, headers with
  subtitles, inset grouped cards, sidebar search, and **Display** as the default
  landing page.
- **Display — resolution & refresh** — per-output DRM mode dropdown in
  Settings → Display; modes saved to `outputs.json` (`mode_width` /
  `mode_height` / `mode_refresh_millihz`) and applied via `DrmOutput::use_mode`
  on `ReloadOutputs` (real DRM session).
- **Display — arrangement canvas** — multi-monitor drag-to-arrange preview with
  **Save display settings** and a 15-second keep/revert confirmation;
  single-monitor preview only (no drag).
- **`ListOutputModes` IPC** — compositor exposes the DRM mode list per output for
  the settings UI (`OutputModeInfo`, `OutputModes` event).

### Changed

- **Settings navigation** — Display first; Weather and Calendars grouped under
  Desktop; Input and System sections aligned with GNOME / macOS conventions.
- **Display save model** — arrangement and resolution changes batch behind
  **Save display settings**; per-output scale and **Active** still apply live.

### Fixed

- **Settings scroll lag** — capture-phase wheel handler on page scrollers,
  non-overlay scrollbars, kinetic scrolling disabled; page content scrolls
  reliably over rows and controls.
- **Settings search lag and lockups** — plain search field (no GtkSearchEntry
  delay), 16 ms debounced filter while typing, immediate flush on Backspace/Delete
  release, selection guard during filter applies, and backspace key-repeat
  swallowed when the field is already empty.
- **Display arrangement on one monitor** — drag preview no longer commits layout
  live (compositor keeps a single output at the origin).
- **Settings row padding** — printers status, power battery line, and display
  per-monitor cards use consistent inset rows.
- **Arrangement save on single output** — no longer displaced the desktop away
  from the origin (edge bar, title bar, and regions stayed usable).

## [2026-06-29]

### Added

- **ScreenCast portal (Phase 3)** — `metis-portal` now runs a persistent
  `ext-image-copy-capture-v1` session and a ~30 Hz frame pump that pushes BGRx
  frames into a real PipeWire output stream node (dedicated PipeWire thread).
  `start_cast` spawns the pump; session close tears down the stream.
- **Clipboard history edge-bar widget** — new `clipboard` bar widget with a
  popover listing recent clipboard entries (text previews and image thumbnails).
  Click a row to recall via `SetClipboard` compositor IPC. History is persisted
  to `~/.local/state/metis/clipboard.json` (50-entry ring buffer; 10 MB image cap).
- **Clipboard IPC** — `CompositorEvent::ClipboardChanged` and
  `CompositorCommand::SetClipboard` in `metis-protocol`; compositor reads client
  clipboard offers on selection change and can set the Wayland clipboard from the
  shell.

### Changed

- **Live `bar.json` reload** — cosmetic edits (opacity, tray mode, margins, blur,
  borders, taskbar pins) now update the bar in place without destroying widgets or
  closing open popovers. A full rebuild runs only when the widget list, bar position,
  display set, clock format, or workspace count changes.
- **Bar symbolic icons** — bluetooth, clipboard, and notification bar icons use GTK
  symbolic rendering with theme text color so they match the rest of the bar in both
  light and dark modes. Removed the unused bundled Papirus icon set from
  `metis-shell`.

### Fixed

- **Terminal right-click and middle-click paste** — right/middle button press now
  syncs data-device and primary-selection focus from the resolved pointer target
  *before* titlebar/resize chrome handlers, so context menus and primary-selection
  paste work reliably in Wayland terminals (kitty, foot) across tiled, floating,
  maximized, and auto-hide-titlebar layouts.
- **Settings changes closed bar popovers** — changing tray mode, opacity, or other
  edge-bar settings used to run a full bar rebuild that called `close_all()` on
  every popover (bluetooth, clock, etc.) and recreated widgets. Cosmetic settings
  now take the live-apply path so open popovers stay up.
- **Weather flashed empty on bar reload** — the weather label briefly cleared
  during structural bar rebuilds. The shell now caches the last weather snapshot
  and re-applies it immediately after rebuild; the status label stays hidden until
  data arrives.
- **Tray pinned mode did nothing** — switching from collapsed to pinned in Settings
  left the tray empty until the next D-Bus update. The tray widget now rebuilds its
  icon row immediately when the mode changes.
- **Tray tooltips showed internal ids** — SNI items like Cursor could display
  `chrome_status_icon_1` instead of the app name. Tooltips now prefer the SNI
  `ToolTip` title and fall back to a readable name from the D-Bus bus name.
- **Tray icons washed out on a light bar** — pixmap tray icons and light symbolic
  assets are inverted or filtered in light mode so they remain visible against the
  pale bar surface.
- **Light-mode popover controls stayed dark** — entries, buttons, switches, and icon
  actions inside bar dropdown panels (clock calendar add/refresh/dismiss, clipboard,
  network, notifications, etc.) now derive colors from Metis theme tokens instead of
  GTK Adwaita defaults.
- **Bundled wallpapers missing in Settings** — the background picker now discovers
  `default.png` … `default9.png` from the workspace assets tree (and install
  prefixes), not a hard-coded `default.jpg` path relative to the binary.

## [2026-06-28]

### Added

- **Release build profiles** — workspace `release` uses thin LTO, `codegen-units=1`,
  and `strip=symbols` (~34% smaller installed binaries). Optional `release-small`
  profile (`opt-level=s`, fat LTO, compositor stays at `opt-level=3`) for ~56%
  smaller installs; `./run-metis.sh --release-small --install-session`.
- **Performance audit** — [`docs/PERF_AUDIT.md`](docs/PERF_AUDIT.md) documents
  compositor hot paths, hotspots (capture, scanout, `state.rs` size), and sizing
  measurements.
- **Bluetooth device battery in the edge bar** — the Bluetooth popover now lists
  every connected device with a battery icon and percentage when the device reports
  one. Low levels (≤20%) use an amber warning style; charging devices show a
  charging icon and `(charging)` label. A **Bluetooth settings** shortcut remains
  at the bottom of the popover.
- **Multi-source peripheral battery reads** — connected-device battery level and
  charging state are assembled from several sources, in priority order:
  1. Kernel HID battery (`/sys/class/power_supply/hid-<mac>-battery`) — capacity
     plus a `status` field mapped to charging/discharging.
  2. **UPower** — peripheral devices enumerated once per poll; percentage and
     `state` (charging / discharging / fully-charged) when the driver reports them.
  3. **Solaar** (optional) — when installed, `solaar show` is parsed for Logitech
     HID++ charging state (and percentage when BlueZ/UPower lack one). Results are
     cached (~20s) on a background thread in the shell so the ~2s CLI call never
     blocks the bar poller; if Solaar is absent the path is a silent no-op.
  4. BlueZ `Battery1` via `bluetoothctl info` — percentage only (the standard BT
     Battery Service has no charging characteristic).
- **Bluetooth low-battery alerts** — when a connected device's battery drops to
  ≤20%, Metis fires a one-shot in-bar notification (with sound, unless Do Not
  Disturb is on). Alerts use per-device hysteresis and are suppressed while the
  device is charging; the latch clears at ≥25% or on disconnect.
- **Power settings — connected devices** — Settings → Power now has a
  **Connected devices** section listing paired Bluetooth peripherals with battery
  percentage, charging icon, and low-battery styling (same source stack as the
  bar).
- **Bluetooth scan toggle** — Settings → Bluetooth **Scan for devices** toggles
  to **Stop scanning** while discovery is active and auto-stops after 30 seconds
  if left running.
- **Window animation effects** — optional maximize **wobble** (whole-window position
  ripple) and **genie** minimize (shrink/fade toward the edge bar before unmap).
  Toggle in **Settings → Appearance → Windows → Window animations**
  (`bar.json` → `window_animations`, default on).
- **Auto-hide titlebar slide-down** — maximized and edge-snapped SSD windows hide
  the Metis titlebar so the client fills the tile; hovering the top edge slides the
  chrome down as an animated overlay (~200 ms) with a sticky zone while shown.
- **Compact titlebar overlay for tabbed browsers** — Chromium-family apps keep SSD
  but reveal only a top-right control strip (~96 px) so the tab bar stays clickable;
  other SSD apps (kitty, Settings, …) use the full-width overlay.
- **Double-click titlebar to maximize** — a double-click on the titlebar (not the
  traffic-light buttons) toggles maximize/unmaximize; single-click no longer
  demotes a maximized window until you drag (5 px movement threshold).
- **Screenshot portal (`org.freedesktop.impl.portal.Screenshot`)** — `metis-portal`
  captures the desktop via a native Wayland client using
  `ext-image-copy-capture-v1` / `ext-image-capture-source-v1` (no `grim` or
  `wlr-screencopy`). PNGs are written under `$XDG_RUNTIME_DIR/metis-screenshot-*.png`
  and returned to apps through `xdg-desktop-portal`. Verified with Flameshot.
- **Compositor image capture** — the Smithay compositor registers
  `ext_output_image_capture_source_manager_v1` and
  `ext_image_copy_capture_manager_v1`, queues capture frames from the protocol
  handler, and renders them on the next GL pass into client SHM buffers.

### Changed

- **metis-shell tokio features** — trimmed from `full` to `rt`, `rt-multi-thread`,
  `macros`, `time`, `sync` (smaller shell binary, same runtime surface).
- **Client-side vs server-side decoration policy** — Chromium and similar tabbed
  browsers are forced to Metis server-side chrome; Firefox and GNOME apps keep
  native client-side decorations where appropriate.
- **Bluetooth polling** — `bluetoothctl` reads in the bar poller run every ~6s
  (was ~1.6s) and all external commands (`bluetoothctl`, `nmcli`, `upower`,
  `solaar`) go through bounded timeouts so a stuck daemon cannot stall the UI.
- **Wi-Fi bar icon stability** — during an `nmcli` rescan the active network can
  briefly disappear or report zero signal; the poller now holds the last known
  connection through a short grace window so the icon does not flash to "no bars"
  and back.
- **Portal stack startup** — `xdg-desktop-portal` and `xdg-desktop-portal-gtk`
  are started on a detached background thread so portal cold-start no longer
  blocks the compositor event loop (which caused a 10+ second black screen on
  login).
- **Client rendering defaults** — `GSK_RENDERER=cairo` is forced only for
  `metis-shell` (via `METIS_SHELL_GSK_RENDERER`); other spawned apps no longer
  inherit a global software-rendering override, restoring hardware-accelerated GTK
  for Chromium and other clients.
- **Portal backend selection** — `metis-portals.conf` sets `default=gtk` and
  routes Settings to Metis; `xdg-desktop-portal` is spawned with
  `XDG_CURRENT_DESKTOP=Metis` (GNOME stripped) so unimplemented GNOME portal
  backends are not probed (each miss used to cost ~25s). Portal files install to
  `/usr/share/xdg-desktop-portal/`; `XDG_DATA_DIRS` / `XDG_CONFIG_DIRS` propagate
  through the session activation environment.

### Fixed

- **Slow app launch (Chromium and others)** — root causes were forced Cairo
  rendering for all clients, synchronous portal startup blocking login, and
  `xdg-desktop-portal` timing out on missing GNOME backends; all three are
  addressed above.
- **Bluetooth battery parsing in Settings** — `Battery Percentage: 0x40 (64)` from
  BlueZ is now decoded via the decimal in parentheses (or hex fallback), matching
  the shell poller.
- **Settings build warnings** — removed unused imports/variables in the Display and
  Sound pages; wired up the previously unused `stop_scan()` backend.
- **Maximized auto-hide titlebar would not reveal** — the edge bar's input block
  (bar + margin + shadow pad) overlaps the top of maximized clients; moving the
  pointer there was treated as "over the bar," which cleared the reveal and never
  ran the slide-down logic, leaving apps like kitty stuck maximized with no way to
  reach the unmaximize control. Reveal now continues while over the bar strip,
  treats horizontal pointer-over-window in that strip as a trigger, and uses the
  full titlebar height on the client top edge.
- **Chromium tab bar blocked by full-width overlay** — the auto-hide titlebar
  spanned the whole window width; the compact top-right control strip fixes this.
- **Maximize wobble reset every frame** — post-snap `reclamp_auto_hide` ran on
  every commit during the wobble FX and snapped the window back; wobble is now
  skipped until the effect finishes.
- **Auto-hide titlebar clicks missed during slide** — decoration presses were
  hit-tested against the client frame instead of the sliding chrome rect.
- **Single-click unmaximize on maximized windows** — titlebar press now arms a
  pending drag instead of immediately demoting, so double-click and button clicks
  work reliably.
- **Screenshot capture sessions stopped immediately** — the compositor's
  `new_session` handler did not retain the owned `Session`, so the capture source
  was torn down before the client could attach a buffer. Sessions are now stored
  in `ImageCaptureRuntime` until the client destroys them.
- **Portal screenshot on Metis failed with Ubuntu `grim`** — Debian/Ubuntu grim
  only supports `wlr-screencopy-unstable-v1`, which Metis does not implement;
  replaced with the native `ext-image-copy-capture` client in `metis-portal`.
- **Resize edge bleed through a front auto-hide window** — when the top window's
  Metis titlebar was hidden, `resize_edge_at` skipped it (`ssd_frame_for_mapped_window`
  returned `None`) and the compositor hit-tested the window below, showing resize
  cursors and starting resizes through the foreground app. Resize bands now use
  client geometry plus a 12 px occlusion halo (`resize_frame_for_mapped_window` /
  `resize_occlusion_rect`).
- **Resize cursor bleed through maximized windows** — maximized and fullscreen
  windows were omitted from `resize_edge_at`, so a lower window's edge bands were
  still active under a maximized neighbor (resize cursor and click-to-resize on the
  background window). Maximized/fullscreen windows now occlude lower windows without
  exposing their own resize edges.
- **Pointer hover and click through the auto-hide titlebar overlay** — Smithay's
  `element_under` only considers mapped client geometry; the sliding Metis titlebar
  sits above that rect, so motion and button events could reach windows stacked
  below (background buttons highlighting and receiving clicks through the
  foreground titlebar). `topmost_window_at_pointer` now owns revealed overlay
  chrome and permanent SSD border strips before falling through to client geometry.
- **Log out left a blank screen on the DRM session** — `EndSession` only stopped
  the compositor event loop without tearing down clients or handing the VT back to
  the display manager. `end_compositor_session()` now kills spawned clients, switches
  to VT 1 on the DRM backend, then stops the loop; the shell also falls back to
  `loginctl terminate-user` when IPC does not reply.

## [2026-06-27]

### Added

- **Standalone DRM/KMS session — Metis runs as a real desktop on its own GPU.**
  A new DRM/udev + libseat + libinput backend lets the compositor own a TTY/GPU
  directly, alongside the existing nested winit dev backend:
  - **Backend selection** (`main.rs`) — autodetects nested (winit) vs. standalone
    (DRM) from `WAYLAND_DISPLAY`/`DISPLAY`; override with `METIS_BACKEND=winit|drm`.
    Nested-only side effects (host activation-env import) are confined to winit.
  - **Shared render path** — `render.rs::build_render_elements` and
    `state.tick_housekeeping()` are now shared by both backends; the DRM backend
    renders one framebuffer per output in output-local coordinates.
  - **DRM render** (`udev.rs`) — libseat session, primary-GPU selection (render
    node detection, `METIS_DRM_DEVICE` override), GBM allocator + `GlesRenderer`,
    a `DrmOutput` per connector, damage-gated page-flips driven by a 16 ms
    heartbeat + vblank (zero GPU work when idle), and a dmabuf global so EGL/GPU
    clients (GTK) submit hardware buffers.
  - **Input + session control** — real devices via libinput feed the shared
    `process_input_event`; relative pointer motion is clamped to the desktop.
    **Ctrl+Alt+F<n>** switches VT, **Ctrl+Alt+Backspace** safe-quits, and
    session pause/resume (VT switch / suspend) re-arms input and DRM.
  - **Pointer** — the DRM session paints its own XCursor-themed cursor (with a
    generated fallback) on the cursor plane and honors client `set_cursor`
    surfaces; `XCURSOR_THEME`/`XCURSOR_SIZE` apply.
  - **Hotplug / robustness** — live connector connect/disconnect with output
    re-packing (no gaps/overlap), and a clean shutdown if the primary GPU is
    removed.
  - **Login-manager entry (GDM/SDDM/greetd)** — `assets/metis-session` launcher +
    `assets/metis.desktop` wayland-session, installed via
    `run-metis.sh --install-session`; pick **Metis** from the greeter like
    Hyprland. `run-metis.sh --session --drm` runs it directly from a TTY.
- **Settings portal (`metis-portal`)** — a new `metis-portal` binary serves
  `org.freedesktop.impl.portal.Settings` for the standalone Metis session:
  color-scheme and gtk-theme from `metis-config`, plus empty decoration/button
  layouts so GTK clients defer to Metis server-side chrome. Registered via
  `metis.portal` + `metis-portals.conf`; the compositor starts it before
  `xdg-desktop-portal` on DRM boot (replacing the old `GDK_DEBUG=no-portals`
  workaround).

### Changed

- **Scrolling layout reworked (niri / paneru style)** — the strip is now an
  infinite row of full-height columns with continuous, mouse-resizable widths:
  - Drag a window's **right** border to set its width; columns to the right slide
    over to make room. Dragging the **left** border resizes the previous window.
    Columns are full-height, so there is no vertical resize. New
    `ScrollResizeGrab` drives the resize and reflows live.
  - Column width is stored as a fraction of the viewport (continuous), replacing
    the fixed ⅓/½/⅔/full presets. `Super`+`-` / `Super`+`=` now snaps the focused
    column to full width, then back to half.
  - Opening a new window never resizes the windows already on the strip — it just
    appends a column (new windows open at half-width).
- **Scroll viewport easing** — focus changes in a scroll workspace now animate
  the strip toward the focused column. Client surfaces stay mapped at the target
  offset; the compositor applies a render-time X nudge
  (`scroll_x_target - scroll_x`) so resize-averse apps are not reconfigured every
  frame.

### Fixed

- **New windows join the scroll strip** — in a scrolling workspace, opening a
  window used to drop it as a centered floating window on top of the strip
  (`place_new_window` floated scroll-mode windows). They are now strip-managed
  like grid tiles, so they form their own column.
- **Scroll strip reflows on open/close** — adding or removing a window now slides
  the existing columns into their new positions instead of leaving the newcomer
  painted on top of its neighbour (the scroll path had no equivalent of the grid
  auto-reflow).
- **Scroll columns clipped to their display** — a column scrolled past the screen
  edge is now clipped to its own output (via `CropRenderElement`) instead of
  bleeding onto the adjacent monitor; fully off-screen columns stay unmapped.
- **Copy/paste across XWayland and Wayland clients** — the compositor now
  bridges clipboard and primary selection between native Wayland apps and X11
  clients (e.g. Chrome → terminal). Previously only the data-device globals were
  advertised; selections were never forwarded in either direction.

## [2026-06-26]

### Added

- **Cross-output window moves** — dragging a window onto another monitor (or
  snapping it there) re-homes its desk tile and scroll membership to that output
  automatically. On grid workspaces, `Super`+`Shift`+`←`/`→` moves the focused
  window to the adjacent output; scroll workspaces keep those keys for column
  navigation. New `MoveWindowToOutput` compositor IPC command.
- **Move workspace to another output** — `Super`+`Ctrl`+`Shift`+`←`/`→` moves
  every window on the active workspace (under the pointer) to the same workspace
  number on the adjacent monitor, including scroll state and layout mode.
  Independent per-output workspace mode only. `MoveWorkspaceToOutput` IPC.

### Fixed

- **Session lockup on pointer input** — fixed a self-deadlock where moving the
  pointer over a window could freeze the whole session. Input hit-testing
  (`surface_under` / `focus_target_at`) acquired the per-output layer map and then
  re-entered it via grid-zone resolution; Smithay's layer map is a non-reentrant
  mutex, so the compositor thread deadlocked. The desk hit is now classified before
  the layer map is locked.
- **Free-mode window geometry restore** — in the default desktop layout, moving or
  resizing a window and then closing and reopening it now restores the saved
  position and size instead of always reopening centered at the default size.
  Placement is no longer locked in before the app's `app_id` is known (GTK assigns
  it just after the first commit), so the saved geometry lookup is no longer
  skipped.
- **Nested session keybinds** — GNOME grabs Super globally; nested dev sessions now
  default to `METIS_MOD=alt` so shortcuts work as Alt+… (override with
  `METIS_MOD=super` when desired).
- **Edge snap top gap** — left/right half snaps on the outermost monitor no longer
  pick up a spurious titlebar inset after cross-output adoption.
- **Mod+F maximize** — the shortcut now toggles maximize (usable area below the
  edge bar), matching the titlebar maximize button and top-edge snap, instead of
  true XDG fullscreen that drew under the bar.
- **Automatic grid tiling** — on grid workspaces, opening or closing an app window
  (or switching back to a workspace) re-splits the area below desk widgets among
  visible tiled windows; the focused window takes the primary slot when three or
  more share the workspace.
- **Scrolling layout polish** — column focus pans with a smooth viewport animation;
  off-screen scroll columns are unmapped so they no longer bleed onto adjacent
  outputs; vertical stacks distribute height evenly; scroll offset is clamped to
  the strip width. `ListOutputs` now marks the primary monitor and sorts outputs
  left-to-right.
- **Scroll layout lockup fix** — changing the default layout to Scrolling now
  actually seeds scroll strips (the early-return path used to skip that when
  `bar.json` already matched), `Mod+\` is debounced against key-repeat, and
  scroll animation only reconfigures scroll-layout windows instead of the whole
  session each frame.

## [2026-06-25]

### Added

- **Scrolling workspace layout (niri / PaperWM style)** — a second per-workspace
  layout mode alongside the grid. App windows form a horizontal strip of columns,
  each holding a vertical stack of windows; the viewport scrolls so the focused
  column stays visible.
  - **`metis-grid` scroll engine** — new `scroll.rs` (`ScrollState` / `ScrollColumn`
    / `ColumnWidth`) with pure insert/remove/focus/move/consume/expel/width-cycle
    ops and pixel-frame layout + scroll-into-view math. A `LayoutKind { Grid, Scroll }`
    enum selects the mode.
  - **Per-workspace mode** — each (output, workspace) tracks its own `layout_kind`
    and scroll strip. App tiles remain the membership/stash source of truth, so
    open/close, workspace switch/move, and dock filtering are unchanged; scroll mode
    only overrides pixel placement and hit-testing.
  - **Keybinds** (active scroll workspace only) — `Super`+arrows move focus across
    columns / within a column; `Super`+`Shift`+arrows move the column / window;
    `Super`+`,` consumes a window into the previous column, `Super`+`.` expels it to
    a new column; `Super`+`-`/`=` cycles the focused column width. `Super`+`\`
    toggles the active workspace between grid and scroll on any workspace.
  - **Settings + IPC** — `bar.json#default_layout` (Settings → Appearance → Edge bar
    → New workspace layout) acts as a live global on/off: changing it applies the
    mode to every workspace on every output at once (`SetDefaultLayout` IPC). A
    `SetWorkspaceLayout` command sets the mode for a single output/workspace.

- **Per-output workspaces (Phase 3)** — each output now owns an independent set
  of virtual workspaces, Hyprland-style. Every monitor has its own active
  workspace and its own grid of app windows; switching one output never disturbs
  the others.
  - **Per-output desk state** — the compositor's single global grid /
    `active_workspace` / stash was replaced by an `OutputDesk` per output
    (`desks: HashMap<output, OutputDesk>`), seeded lazily as outputs map. The
    primary output keeps the `desk.json` widget tiles; secondary outputs get an
    app-only grid. Grid metrics, tiling, hit-testing, and window placement are
    now computed against the window's (or cursor's) own output.
  - **Window → output binding** — windows are tagged with the output they open
    on (the one under the cursor) and tile within it; a window is mapped only
    while its output's active workspace matches.
  - **Keybinds** — `Super`+`1`…`9` switch the workspace on the output *under the
    pointer*; `Super`+`Shift`+`1`…`9` move the focused window between its own
    output's workspaces.
  - **Per-output bar dots** — each monitor's edge bar drives and reflects its own
    output's workspaces. The shell tracks active workspace per output and matches
    each bar to its compositor output via the GDK monitor connector name.
  - **Protocol** — `SwitchWorkspace` and `WorkspaceChanged` now carry an `output`
    name (back-compatible default); `WorkspaceChanged` is emitted per output.
  - **Workspace mode toggle** — Settings → Appearance → Edge bar → Workspaces lets
    you pick `separate` (independent per output, default) or `linked` (every monitor
    switches to the same workspace at once). Stored as `bar.json#workspace_mode`; the
    compositor routes workspace switches (`Super`+`n` and `SwitchWorkspace`) through
    `switch_workspace_routed`, fanning out to all outputs in linked mode.
- **Taskbar follows the output + workspace** — each monitor's dock now shows only
  the windows on that output's currently-visible workspace (pinned launchers still
  appear everywhere). `WindowInfo.output` is populated with the window's monitor
  name; the dock filters by `(output, active workspace)`, repaints on workspace
  switch, and dedups per bar (so multiple monitors no longer thrash one shared
  signature). The shell reconciles its window cache on workspace change / window
  open so a window lands on the right dock immediately.

## [2026-06-24]

### Added

- **Multi-output groundwork (Phase 3)** — first slices of the output-agnostic
  refactor:
  - **Output-geometry foundation** — a centralized helper layer
    (`primary_output` / `output_rect` / `desktop_bounds`) that
    `grid_metrics`, `usable_zone`, `placement_zone`, `set_fullscreen`, and
    `arrange_layers` now route through instead of scattered
    `space.outputs().next()` / cached-`monitor` reads. Behavior is identical with
    one output; per-output work now only changes "which output" at these
    chokepoints.
  - **Virtual-output dev rig** — `METIS_VIRTUAL_OUTPUTS=2` tiles the nested winit
    window into two side-by-side logical monitors so multi-output behavior
    (per-output bars, placement, layer-shell) can be exercised before a real
    DRM/udev backend. A dedicated full-window render output drives the damage
    tracker / scale / wallpaper / frame timing, and the render loop now gathers
    layer-shell surfaces + bar blur from every output (offset by each output's
    global origin). Default (unset / `1`) is byte-for-byte the previous
    single-output path.
- **Virtual workspaces (single output)** — the workspace dots in the bar are now
  live. Each workspace is its own independently-tiled set of app windows; the desk
  widgets (clock/weather/rss) are shared across all of them. Switching stashes the
  current workspace's app tiles, hides its windows, then restores and remaps the
  target's, and focuses the topmost window there.
  - **Keybinds** — `Super`+`1`…`9` switch workspace; `Super`+`Shift`+`1`…`9` move
    the focused window to a workspace (digit detection is shift-independent).
  - **Bar dots** — clicking a dot switches via the compositor; the compositor's
    new `WorkspaceChanged` event keeps the active dot in sync (single source of
    truth), with an optimistic local update for instant feedback.
  - **Protocol** — new `SwitchWorkspace` / `MoveWindowToWorkspace` commands and a
    `WorkspaceChanged` event; `WindowInfo.workspace` is now populated (1-based).
    Workspace count comes from `bar.json` `workspace_count` (1–12, default 4).
  - **Multi-output input/drag fixes** — absolute-pointer motion now maps across
    the whole virtual desktop (union of all outputs) instead of the first output,
    so the cursor is no longer compressed into the primary monitor; and titlebar
    drags clamp to the full desktop bounds so a window can be moved between
    outputs (previously it was pinned to the primary output's zone).
  - **Per-output edge bar** — the shell now spawns one edge bar per connected
    output (bound via `gtk4-layer-shell` `set_monitor`), rebuilding on monitor
    hotplug. A new **Settings · Appearance · Edge bar → "Show bar on"** control
    (`bar.json` `displays`: `all` | `primary`, default `all`) switches between a
    bar on every display and a single bar on the primary output.
  - **Per-bar live updates on every output** — the notification and taskbar
    refresh registries held a single callback, so only the last-built bar updated
    instantly while others waited (5–10s) for an unrelated poll change. They now
    fan out to one (weak) hook per bar, so notifications and dock changes appear on
    every display at once. Volume/mic slider and mute actions now broadcast the
    new level to every bar instantly (optimistic, with poll suppression) so the
    other displays update immediately instead of waiting for the pactl read-back;
    audio actions also force an immediate poll read-back as a backstop.
  - **Popover positioning on secondary outputs** — `unconstrain_popup` now
    expresses the allowed area in the parent surface's local frame (subtracting the
    output's global origin as well as the layer offset), and toplevel popups
    constrain to the window's actual output. Without this, every bar popover
    (Metis Menu, calendar, Wi-Fi, notifications, weather) on a non-primary output
    was pushed off-screen and could neither be seen nor clicked.
  - **Per-output wallpaper** — the desktop background is now composed per output
    instead of one image stretched across the whole framebuffer. Each display is
    cover-cropped to its own resolution (so the same picture fills a 16:9 and a
    16:10 monitor correctly), and a display can carry its own picture via
    `wallpaper.json`'s new `per_output` map (output name → path). The compositor
    still uploads a single framebuffer-sized texture (each output's crop blitted at
    its global origin), so the bar backdrop-blur path is unchanged. A new
    **Settings · Appearance · Per-display background** card (shown only with 2+
    displays) lists each output and lets you set or clear a per-display picture;
    it discovers outputs through the new `ListOutputs` IPC command
    (`CompositorEvent::OutputList` / `OutputInfo`). Sources are cached by path, so
    two displays sharing an image only read it from disk once.
  - **Per-output window placement, snapping & maximize** — window management now
    follows the monitor the cursor (or window) is on instead of always the primary
    output. New floating windows open centered on the output **under the cursor**;
    dragging a window to a screen edge snaps/tiles it on the **hovered** output;
    the maximize button/zone fills the output the window currently sits on; and a
    floating window is clamped within **its own** monitor so it's no longer yanked
    back to the primary. Built on new output-resolution helpers (`output_at` /
    `output_under_pointer` / `output_for_window`) plus output-parameterized zone
    helpers (`usable_zone_for` / `placement_zone_for` / `window_placement_zone_for`
    / `centered_rect_in`); the overlay-bar strip is now reserved only on outputs
    that actually show a bar. Grid/tiling stays on the primary output until
    per-output workspaces land. Single-output behavior is unchanged.
- **Metis Menu settings page** — a new **Settings · Metis Menu** page gathers all
  launcher settings in one place, separate from the Edge bar card:
  - **Quick launchers** — choose which **terminal** and **file manager** the rail
    opens. Each picker auto-detects installed options
    (kgx/gnome-terminal/konsole/foot/… and nautilus/dolphin/nemo/thunar/…) and
    offers a **Custom** entry with a file chooser to point at any executable path.
    Choices persist to `menu.json` (`terminal` / `file_manager`); the shell reads
    them at launch time and falls back to `$TERMINAL`/`$FILE_MANAGER`, then the
    known candidates, then `xdg-open` — so an unset or missing program degrades
    gracefully without a restart.
  - **Appearance** — the menu **panel opacity** moved here from the Edge bar card
    (still stored in `bar.json` `menu_opacity`, applied live via `reload-bar`).

### Fixed

- **Snapping/maximizing on a secondary output pulled the window toward the
  primary** — the snap math was correct, but the follow-up auto-hide re-anchor
  (`reclamp_auto_hide`, run after every snap/maximize) still clamped against the
  *primary* output's zone, so a left-half snap on the second display was dragged
  back across the monitor boundary (half on each screen). It now re-anchors within
  the window's own output.
- **Shell crash when changing the bar's "Show bar on" display set (root cause)** —
  removing a per-output bar tore down its `zwlr_layer_surface_v1`, and the
  compositor killed the shell with `invalid_size` ("width 0 requested without
  setting left and right anchors"). This was a smithay + gtk4-layer-shell teardown
  interaction: destroying a layer surface resets its cached state to defaults
  (size 0×0, no anchors), and the trailing `attach(null); commit` that the toolkit
  sends then fails smithay's pre-commit size/anchor validation. The compositor now
  installs a pre-commit hook (ahead of smithay's, registered on the bare surface in
  `new_surface`) that repairs that degenerate teardown state so the unmap commits
  cleanly. Normal layer surfaces are untouched. This is the actual fix; the
  shell-side rebuild changes below are defensive hardening that remain in place.
- **Shell crash when changing the bar's "Show bar on" display set** — several
  issues around rebuilding the per-output bars at runtime:
  - Rebuilding destroyed every bar window *before* building the replacement, so the
    `GtkApplication` briefly owned zero windows and auto-quit, tearing down the
    Wayland connection ("Error flushing display: Broken pipe"). The rebuild now
    builds the new bars first and destroys the old ones afterward (window count
    never hits zero, no one-frame flash), and the shell holds the application alive
    across transient zero-window states as a backstop.
  - A fresh bar's layer-shell role/anchors/output binding are now established before
    the window is realized or any child widgets are built — runtime window creation
    realizes immediately (unlike the forgiving startup path), so out-of-order setup
    could commit an invalid surface and get the client dropped.
  - Rebuild triggers are coalesced: a single settings change writes `bar.json`
    *and* sends `reload-bar`, so multiple rebuilds were storming in within a few
    hundred ms and racing each other; they now collapse into one deferred pass.
  - The compositor now logs client disconnects, including the exact object/code/
    message of a Wayland protocol error, so client-kill bugs are diagnosable
    instead of silent.
- **Bottom edge-bar left too much gap below maximized/snapped windows** — overlay
  bars reserved an extra `SHADOW_PAD` (16px) of transparent pad above the pill, so
  windows stopped ~18px short of the visible bar regardless of the **Distance from
  edge** value (even 0–1). All overlay bars (bottom/left/right) now reserve only the
  visible strip (`margin + body`), matching the top bar, so windows tuck right up to
  the pill with the same small `BAR_GAP_PX` breathing gap.
- **Start-menu scroll over a window** — with the launcher open above an app window,
  the window behind it stole wheel/click focus, so the app list only scrolled when
  the window was moved away. The compositor now gives the bar strip and its popovers
  top pointer priority in `surface_under`, hit-testing them before the desk-grid
  app-body passthrough; transparent popover gutters route to the popup surface
  instead of falling through to whatever is underneath.
- **Start-menu dismissal** — clicking the desktop or another window's titlebar now
  reliably closes an open popover. Hit-testing uses the bar strip's `bbox()` plus each
  popup's client `geometry()` (instead of `bbox_with_popups()`, which could balloon to
  the whole output and swallow outside clicks), and the `close-popovers` command is
  issued before titlebar/resize handlers can consume the press and return early.
- **Compositor deadlock on pointer move** — `surface_under` no longer calls
  `metis_bar_ui_hit()` while already holding the output's layer map (a re-lock that
  froze the compositor thread); the region check now uses the held guard directly.
- **Dark-mode start-menu scrollbar artifacts** — the shell now syncs GTK's built-in
  Adwaita dark/light variant to the saved theme preference on live `reload-theme`, and
  the menu CSS flattens GtkScrolledWindow undershoot/overshoot/trough chrome so the
  scroll gutter stays flat and scrollable in dark themes. Invalid GTK4 CSS (GTK3-only
  scrollbar stepper props, `min-width: 100%`) was removed.
- **Settings app re-theming the shell on open** — building the Appearance page no
  longer fires spurious `reload-theme` commands; programmatic init of the theme/font
  controls is suppressed so opening Settings doesn't re-trigger the shell's theming.
- **Type-to-search restored** — the launcher again filters as you type without first
  clicking the search box, via `SearchEntry::set_key_capture_widget` (which only takes
  focus once typing begins) instead of grabbing focus on open, so wheel scrolling over
  the app list keeps working.

### Changed

- **Input-routing & menu cleanup** — collapsed redundant compositor hit-test helpers,
  pruned the stacked `wire_vertical_scroll` controllers down to one Capture-phase
  controller per `ScrolledWindow`, and removed dead shell/compositor code (unused
  theme/bar/dropdown helpers and imports).

## [2026-06-23]

### Added

- **Configurable edge-bar position** — the bar can now anchor to the **top, bottom,
  left, or right** of the screen via **Settings · Appearance · Edge bar** (`bar.json`
  → `position`, now incl. `bottom`). The reserved exclusive zone follows the chosen
  edge, the pill sits flush against it (drop-shadow pad on the inner side), the
  layout flips between horizontal and vertical correctly on a live switch, and bar
  popovers / the app launcher / task pickers open away from the bar (down for top,
  up for bottom, right for left, left for right).
- **Edge-bar distance slider** — a new **Distance from edge** control sets the gap
  between the bar and its anchored screen edge (`bar.json` → `margin_top`, applied to
  whichever edge the bar is on).
- **Configurable edge-bar border** — the bar pill's border is now styleable via
  `bar.json` → `bar_border` and **Settings · Appearance · Edge bar**: `mode`
  (`accent` follows the theme accent gradient, `solid` a single color, or a custom
  `gradient`), per-stop colors, and `width_px` (**0 disables the border**). The
  gradient follows the bar's long axis and hugs the pill's rounded corners (rendered
  via a layered `background-clip` stroke). Applied live (~1s) — no restart.

## [2026-06-22]

### Added

- **Configurable title-pill border** — the window title now sits on a flat, solid
  plate (dark on dark themes, light on light) ringed by a thin accent stroke on the
  focused window; unfocused windows use a muted slate. The stroke is configurable
  via `bar.json` → `titlebar_pill_border` and **Settings · Appearance · Windows**:
  `mode` (`accent` follows the theme accent gradient, `solid` a single color, or a
  custom `gradient`), per-stop colors, and `width_px`. The accent gradient flows
  left→right across the pill. Picked up live (~1s) — no restart.
- **Configurable window-frame border (independent of the pill)** — the whole window
  frame (titlebar ring + left/right/bottom edges) draws the same style options as a
  **top→bottom** gradient, configured separately via `bar.json` → `window_border`
  and its own controls in **Settings · Appearance · Windows**. The frame
  **thickness** is now adjustable (`width_px`, 0–16px): it both strokes the border
  and insets the client body to match, applied live via a runtime grid border width
  (`metis_grid::set_app_tile_border_px`) plus a relayout so existing windows resize.

### Fixed

- **Titlebar click-through raising the wrong window** — clicking a foreground
  window over a spot where a window *behind* it had its titlebar/border could raise
  the background window. Server-side decoration presses are now hit-tested in
  stacking order (topmost first), so the window in front owns the click and a
  covered window's chrome can no longer intercept it. Genuinely exposed background
  titlebars still raise as expected.

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
