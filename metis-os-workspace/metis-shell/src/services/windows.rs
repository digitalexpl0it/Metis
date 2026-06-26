//! Shell-side cache of the compositor's open windows, driven by the compositor
//! event stream (`WindowOpened`/`WindowClosed`/`WindowMetadata`/`WindowFocused`/
//! `WindowMinimized`) with a periodic `ListWindows` reconcile as a safety net.
//! The taskbar widget reads this cache to render running apps.

use std::cell::RefCell;
use std::rc::Rc;

use metis_protocol::{CompositorEvent, WindowInfo};

#[derive(Debug, Clone, Default)]
pub struct WindowsSnapshot {
    pub windows: Vec<WindowInfo>,
    pub focused: Option<u32>,
}

thread_local! {
    static STORE: RefCell<WindowsSnapshot> = RefCell::new(WindowsSnapshot::default());
    /// Repaint hooks installed by each bar's tasks widget (one per output in a
    /// multi-monitor session). Weak so a torn-down bar's hook drops itself.
    static REFRESH: RefCell<Vec<std::rc::Weak<dyn Fn()>>> = const { RefCell::new(Vec::new()) };
}

/// Register a callback invoked whenever the window cache changes so every bar's
/// taskbar can repaint. Each bar registers its own hook; dead hooks (from
/// rebuilt/removed bars) are pruned on the next register/fire.
pub fn register_refresh(cb: Rc<dyn Fn()>) {
    REFRESH.with(|r| {
        let mut list = r.borrow_mut();
        list.retain(|w| w.strong_count() > 0);
        list.push(Rc::downgrade(&cb));
    });
}

fn fire_refresh() {
    // Collect live callbacks first so we don't hold the REFRESH borrow while a
    // callback runs (a callback may re-enter via register_refresh).
    let callbacks: Vec<Rc<dyn Fn()>> = REFRESH.with(|r| {
        let mut list = r.borrow_mut();
        list.retain(|w| w.strong_count() > 0);
        list.iter().filter_map(std::rc::Weak::upgrade).collect()
    });
    for cb in callbacks {
        cb();
    }
}

/// Current snapshot of known windows.
pub fn snapshot() -> WindowsSnapshot {
    STORE.with(|s| s.borrow().clone())
}

/// Replace the cache from an authoritative `ListWindows` response (initial seed
/// and slow reconcile). Best-effort: a failed IPC leaves the cache untouched.
pub fn reconcile_now() {
    match crate::compositor::list_windows() {
        Ok(windows) => {
            let list_focus = windows.iter().find(|w| w.focused).map(|w| w.id);
            STORE.with(|s| {
                let mut store = s.borrow_mut();
                // Focus is authoritative from the event stream (`WindowFocused`),
                // not from this snapshot. `list_windows` derives focus from live
                // keyboard focus, which is `None` whenever the pointer is in the
                // shell UI (bar, start menu, dock) — so recomputing it here would
                // clear the dock highlight every reconcile. Keep our event-driven
                // focus, only falling back to the list when we have none or the
                // tracked window has gone away.
                let focused = match store.focused {
                    Some(fid) if windows.iter().any(|w| w.id == fid) => Some(fid),
                    _ => list_focus,
                };
                store.windows = windows;
                store.focused = focused;
                for w in &mut store.windows {
                    w.focused = Some(w.id) == focused;
                }
            });
            fire_refresh();
        }
        Err(err) => tracing::debug!(%err, "list_windows reconcile failed"),
    }
}

/// Fold a compositor event into the window cache, repainting on any change.
/// Non-window events are ignored.
pub fn apply_event(evt: &CompositorEvent) {
    match evt {
        CompositorEvent::WindowOpened { id, app_id, .. }
        | CompositorEvent::WindowMetadata { id, app_id, .. } => {
            tracing::info!(id = *id, ?app_id, "windows: applying window event");
        }
        CompositorEvent::WindowClosed { id }
        | CompositorEvent::WindowFocused { id }
        | CompositorEvent::WindowMinimized { id, .. } => {
            tracing::info!(id = *id, "windows: applying window event");
        }
        _ => {}
    }
    let changed = STORE.with(|s| {
        let mut store = s.borrow_mut();
        match evt {
            CompositorEvent::WindowOpened {
                id,
                title,
                app_id,
                suggested_rect,
            } => {
                if let Some(w) = store.windows.iter_mut().find(|w| w.id == *id) {
                    w.title = title.clone();
                    w.app_id = app_id.clone();
                } else {
                    store.windows.push(WindowInfo {
                        id: *id,
                        title: title.clone(),
                        app_id: app_id.clone(),
                        rect: *suggested_rect,
                        fullscreen: false,
                        minimized: false,
                        focused: false,
                        output: String::new(),
                        workspace: 0,
                    });
                }
                true
            }
            CompositorEvent::WindowClosed { id } => {
                let before = store.windows.len();
                store.windows.retain(|w| w.id != *id);
                if store.focused == Some(*id) {
                    store.focused = None;
                }
                store.windows.len() != before
            }
            CompositorEvent::WindowMetadata { id, title, app_id } => {
                if let Some(w) = store.windows.iter_mut().find(|w| w.id == *id) {
                    w.title = title.clone();
                    w.app_id = app_id.clone();
                    true
                } else {
                    false
                }
            }
            CompositorEvent::WindowFocused { id } => {
                // The compositor re-emits focus on every click into a window,
                // even when it was already focused. Ignore no-op focus changes
                // so the dock doesn't rebuild (and re-enumerate every installed
                // app) on each click.
                if store.focused == Some(*id) {
                    false
                } else {
                    store.focused = Some(*id);
                    for w in &mut store.windows {
                        w.focused = w.id == *id;
                    }
                    true
                }
            }
            CompositorEvent::WindowMinimized { id, minimized } => {
                if let Some(w) = store.windows.iter_mut().find(|w| w.id == *id) {
                    w.minimized = *minimized;
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    });
    if changed {
        fire_refresh();
    }
}
