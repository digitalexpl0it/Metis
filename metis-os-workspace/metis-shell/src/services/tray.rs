//! System tray host (`org.kde.StatusNotifierWatcher`) for the edge bar.

use std::cell::RefCell;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use tokio::sync::mpsc as async_mpsc;

use super::tray_menu::TrayMenu;

/// ARGB32 icon payload from StatusNotifierItem.
#[derive(Debug, Clone)]
pub struct IconPixmap {
    pub width: i32,
    pub height: i32,
    pub pixels: Vec<u8>,
}

/// One StatusNotifierItem tracked for the bar UI.
#[derive(Debug, Clone)]
pub struct TrayItem {
    pub bus_name: String,
    pub object_path: String,
    pub id: String,
    pub title: String,
    pub icon_name: Option<String>,
    pub icon_theme_path: Option<String>,
    pub icon_pixmap: Option<IconPixmap>,
    pub menu_path: Option<String>,
    pub menu: Option<TrayMenu>,
    /// When true, left click should prefer `ContextMenu` over `Activate`.
    pub item_is_menu: bool,
}

#[derive(Debug, Clone)]
pub enum TrayEvent {
    Update(TrayItem),
    Remove { bus_name: String },
    /// Fresh DBusMenu layout fetched on demand (e.g. before opening a context menu).
    ContextMenuReady(TrayItem),
}

#[derive(Debug, Clone)]
pub enum TrayCommand {
    MenuClicked {
        bus_name: String,
        menu_path: String,
        submenu_id: i32,
        /// The clicked row's label. Steam (and other ayatana→dbusmenu bridges)
        /// *renumber* their menu item ids whenever the tree is rebuilt, so the
        /// `submenu_id` captured when the menu was shown is frequently dead by
        /// the time of the click. The dispatcher re-fetches the live layout and
        /// re-resolves the id by this (stable) label before delivering the event.
        label: String,
    },
    Activate {
        bus_name: String,
        object_path: String,
        x: i32,
        y: i32,
    },
    SecondaryActivate {
        bus_name: String,
        object_path: String,
        x: i32,
        y: i32,
    },
    /// Re-query the watcher for all registered items (e.g. before opening the popover).
    SyncRegistered,
    /// Fetch a fresh DBusMenu layout and emit [`TrayEvent::ContextMenuReady`].
    OpenContextMenu {
        bus_name: String,
        object_path: String,
    },
}

pub struct TrayChannels {
    pub events: Receiver<TrayEvent>,
    pub commands: async_mpsc::Sender<TrayCommand>,
}

thread_local! {
    static REFRESH: RefCell<Vec<std::rc::Weak<dyn Fn()>>> =
        const { RefCell::new(Vec::new()) };
    static CONTEXT_MENU_READY: RefCell<Option<std::rc::Rc<dyn Fn(&TrayItem) -> bool>>> =
        const { RefCell::new(None) };
}

/// GTK hook: return `true` when the pending context menu was shown (skip bar rebuild).
pub fn register_context_menu_ready(cb: std::rc::Rc<dyn Fn(&TrayItem) -> bool>) {
    CONTEXT_MENU_READY.with(|c| *c.borrow_mut() = Some(cb));
}

pub fn register_refresh(cb: std::rc::Rc<dyn Fn()>) {
    REFRESH.with(|r| {
        let mut list = r.borrow_mut();
        list.retain(|w| w.strong_count() > 0);
        list.push(std::rc::Rc::downgrade(&cb));
    });
}

fn fire_refresh() {
    let callbacks: Vec<std::rc::Rc<dyn Fn()>> = REFRESH.with(|r| {
        let mut list = r.borrow_mut();
        list.retain(|w| w.strong_count() > 0);
        list.iter().filter_map(std::rc::Weak::upgrade).collect()
    });
    for cb in callbacks {
        cb();
    }
}

pub fn spawn_tray_service() -> TrayChannels {
    let (event_tx, event_rx) = mpsc::channel();
    let (cmd_tx, cmd_rx) = async_mpsc::channel(32);

    let disabled = std::env::var("METIS_NO_TRAY")
        .map(|v| !v.is_empty())
        .unwrap_or(false);

    if disabled {
        tracing::info!("tray: skipped (METIS_NO_TRAY)");
        drop(event_tx);
        return TrayChannels {
            events: event_rx,
            commands: cmd_tx,
        };
    }

    if let Err(err) = thread::Builder::new()
        .name("metis-tray-dbus".into())
        .spawn(move || run_tray_thread(event_tx, cmd_rx))
    {
        tracing::error!(%err, "tray: failed to spawn dbus thread");
    }

    TrayChannels {
        events: event_rx,
        commands: cmd_tx,
    }
}

fn run_tray_thread(
    event_tx: Sender<TrayEvent>,
    cmd_rx: async_mpsc::Receiver<TrayCommand>,
) {
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(err) => {
            tracing::error!(%err, "tray: failed to build runtime");
            return;
        }
    };

    rt.block_on(async {
        if let Err(err) = super::tray_watcher::run(event_tx, cmd_rx).await {
            tracing::error!(%err, "tray: dbus service stopped");
        }
    });
}

#[derive(Debug, Clone, Default)]
pub struct TraySnapshot {
    pub items: Vec<TrayItem>,
}

thread_local! {
    static STORE: RefCell<TraySnapshot> = RefCell::new(TraySnapshot {
        items: Vec::new(),
    });
}

pub fn snapshot() -> TraySnapshot {
    STORE.with(|s| s.borrow().clone())
}

fn upsert_item(snap: &mut TraySnapshot, item: TrayItem) {
    if let Some(pos) = snap
        .items
        .iter()
        .position(|i| i.bus_name == item.bus_name && i.object_path == item.object_path)
    {
        snap.items[pos] = item;
    } else {
        snap.items.push(item);
    }
    snap.items.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
}

pub fn apply_event(event: TrayEvent) {
    match event {
        TrayEvent::ContextMenuReady(item) => {
            STORE.with(|s| upsert_item(&mut s.borrow_mut(), item.clone()));
            let consumed = CONTEXT_MENU_READY.with(|c| {
                c.borrow()
                    .as_ref()
                    .map(|handler| handler(&item))
                    .unwrap_or(false)
            });
            if !consumed {
                fire_refresh();
            }
        }
        other => {
            STORE.with(|s| {
                let mut snap = s.borrow_mut();
                match other {
                    TrayEvent::Update(item) => upsert_item(&mut snap, item),
                    TrayEvent::Remove { bus_name } => {
                        snap.items.retain(|i| {
                            i.bus_name != bus_name
                                && i.object_path != bus_name
                                && !i.id.contains(&bus_name)
                        });
                    }
                    TrayEvent::ContextMenuReady(_) => unreachable!(),
                }
            });
            fire_refresh();
        }
    }
}

pub fn sync_tray() {
    send_command(TrayCommand::SyncRegistered);
}

pub fn send_command(cmd: TrayCommand) {
    TRAY_CMD.with(|cell| {
        if let Some(tx) = cell.borrow().as_ref() {
            let _ = tx.try_send(cmd);
        }
    });
}

thread_local! {
    static TRAY_CMD: RefCell<Option<async_mpsc::Sender<TrayCommand>>> =
        const { RefCell::new(None) };
}

pub fn set_command_sender(tx: async_mpsc::Sender<TrayCommand>) {
    TRAY_CMD.with(|cell| *cell.borrow_mut() = Some(tx));
}
