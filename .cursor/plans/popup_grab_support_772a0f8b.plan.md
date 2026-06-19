---
name: Popup grab support
overview: Implement real xdg_popup grabs in the DMI compositor so GTK popovers can hold keyboard/pointer focus, then switch the clock/calendar popover to the grab-based model so its text fields, dropdowns, and dismissal work. Volume/notification popovers stay as-is to avoid regressing working code.
todos:
  - id: compositor-grab
    content: Implement grab() in handlers/xdg_shell.rs using smithay PopupGrab + PopupKeyboardGrab/PopupPointerGrab; add helper to resolve root KeyboardFocusTarget (Window or LayerSurface).
    status: completed
  - id: input-guard
    content: In input.rs, skip the close-popovers runtime command write when pointer.is_grabbed() to avoid double-dismiss.
    status: completed
  - id: clock-menubutton
    content: Convert clock trigger to gtk::MenuButton with an autohide(true) popover via set_popover; wire refresh-on-open, single-open close_all, and active CSS; update root type/return.
    status: completed
  - id: dropdown-register
    content: Add dropdown::register() helper so the grab-based clock popover is still in the POPOVERS list for the close-popovers safety net.
    status: completed
  - id: css-menubutton
    content: Add menubutton.metis-bar-widget CSS to mirror the button styling and hide the default arrow.
    status: completed
  - id: test-session
    content: Build and run nested session; verify clock text input + dismissal works, volume/notifications unregressed, and no MAP->UNMAP/grab errors in logs. Apply KeyboardMode::OnDemand contingency if needed.
    status: completed
isProject: false
---

# Fix the clock popover: implement xdg_popup grabs

## Root cause (proven via instrumented run)
The clock popover `MAP`s then `UNMAP`s ~15ms later. GTK requests an `xdg_popup.grab` to route keyboard to the popover's `Entry`/`SpinButton` widgets. The compositor's `grab()` is an intentional no-op, and because the popover is `autohide(false)` GTK grabs *after* the popup is mapped, which smithay rejects (`InvalidGrab` -> `surface missing from known popups`), destroying the popup. Volume/notifications work only because they contain no text inputs and never grab.

## Strategy
- Make the compositor honor popup grabs (standard smithay `PopupGrab`). The infra is already present: `KeyboardFocusTarget::Popup` exists in [focus.rs](metis-os-workspace/metis-compositor/src/focus.rs), seat has a keyboard ([state.rs:110](metis-os-workspace/metis-compositor/src/state.rs)), and `PointerFocus = WlSurface: From<KeyboardFocusTarget>` ([handlers/mod.rs:29](metis-os-workspace/metis-compositor/src/handlers/mod.rs)).
- Switch ONLY the clock popover to the grab model (`autohide(true)`), so GTK grabs *before* map (correct protocol order). Leave volume/notifications untouched (`autohide(false)`, no grab) to avoid regressions.

## 1. Compositor: implement `grab()` 
In [metis-compositor/src/handlers/xdg_shell.rs](metis-os-workspace/metis-compositor/src/handlers/xdg_shell.rs) replace the no-op `grab()` (lines 124-127) with the smithay pattern:
- Resolve the popup root surface via `find_popup_root_surface(&PopupKind::Xdg(surface))`.
- Build the root `KeyboardFocusTarget`: if root is a space `Window` -> `Window(..)`; else look it up as a layer surface (reuse the `layer_map_for_output`/`layer_for_surface` logic already in `layer_geometry_for_surface`, lines 289-299, factored to also return the `LayerSurface`) -> `LayerSurface(..)`.
- `let grab = self.popups.grab_popup(root_target, kind, &seat, serial)?;`
- If `Ok`, set keyboard + pointer grabs with the alive/serial guards (anvil pattern):
  - `keyboard.set_focus(self, grab.current_grab(), serial)` then `keyboard.set_grab(self, PopupKeyboardGrab::new(&grab), serial)`
  - `pointer.set_grab(self, PopupPointerGrab::new(&grab), serial, Focus::Keep)`
  - On `is_grabbed` serial mismatch, `grab.ungrab(PopupUngrabStrategy::All)` and return.
- Add imports: `PopupKeyboardGrab`, `PopupPointerGrab`, `PopupUngrabStrategy` (from `smithay::desktop`), `KeyboardFocusTarget` (crate `focus`).

## 2. Compositor: avoid double-dismiss
In [metis-compositor/src/input.rs](metis-os-workspace/metis-compositor/src/input.rs) (lines 148-155), skip the `close-popovers` runtime-command write when `pointer.is_grabbed()` — when a popup grab is active, smithay's `PopupPointerGrab` already sends `popup_done` on outside clicks, so the manual signal is redundant (and could fight the grab).

## 3. Shell: clock popover uses the grab model
In [metis-shell/src/ui/bar/widgets/clock/mod.rs](metis-os-workspace/metis-shell/src/ui/bar/widgets/clock/mod.rs):
- Make the clock trigger a `gtk::MenuButton` (instead of `gtk::Button` + `wire_toggle_prepare`). `MenuButton` is built for autohide/grab popovers and handles toggle + outside-click dismissal natively (avoids the "click-to-close then reopen" race a plain Button hits with grabs).
  - `menu.set_child(Some(&row))` (time/date labels), `menu.set_always_show_arrow(false)`, reuse `metis-bar-widget`/`metis-bar-clock` css classes.
  - Build the popover with `autohide(true)` (default), `has_arrow(true)`, `position(Bottom)`, `child(panel)`, then `menu.set_popover(Some(&popover))`.
- Refresh-on-open + single-open: connect `menu.connect_active_notify`: when active, call `dropdown::close_all()` (close volume/notif) and send `CalCommand::Refresh`; toggle the `metis-bar-dropdown-active` class via the popover's `connect_map`/`connect_unmap` (kept).
- Register the popover in `dropdown`'s `POPOVERS` list (add a small `dropdown::register(&popover)` helper) so the existing `close-popovers` path can still pop it down as a safety net.
- `ClockWidget.root` type changes `gtk::Button` -> `gtk::MenuButton`; update `root()` return type. Verify the bar append in [widgets/mod.rs](metis-os-workspace/metis-shell/src/ui/bar/widgets/mod.rs) accepts it (it appends `&impl IsA<Widget>`).

## 4. CSS
In [metis-shell/src/ui/theme/css.rs](metis-os-workspace/metis-shell/src/ui/theme/css.rs): add `menubutton.metis-bar-widget > button { ... }` rules mirroring the existing `button.metis-bar-widget` styling (background, padding, hover gradient, active state) and hide the default `menubutton > button > .arrow`. Keep existing `popover.metis-bar-popover` rules (they apply to the MenuButton's popover too).

## 5. Contingency
If text fields receive the popup but still can't get keyboard input (because the parent bar layer is `KeyboardMode::None`), set the bar to `KeyboardMode::OnDemand` in [bar/mod.rs:233](metis-os-workspace/metis-shell/src/ui/bar/mod.rs). Will confirm during testing.

## 6. Test matrix (nested session via `./run-metis.sh --session`)
- Clock popover: opens and STAYS open; type in "Add event" title + times; Clocks tab timezone search/autocomplete; Calendars tab dropdown + password entry; stopwatch/timer.
- Dismissal: click bare desktop / another window / another bar icon closes it; clicking the clock again toggles it.
- Regression: volume + notification popovers still open/close and work.
- Logs: no `surface missing from known popups`, no `MAP`->immediate-`UNMAP`.