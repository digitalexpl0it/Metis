//! StatusNotifierWatcher + host client (zbus 5), replacing the stray crate.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use futures_util::StreamExt;
use tokio::sync::mpsc;
use zbus::fdo::RequestNameFlags;
use zbus::interface;
use zbus::proxy;
use zbus::zvariant::{OwnedValue, Value};

use super::tray::{TrayCommand, TrayEvent, TrayItem};
use super::tray_dbus_types::{parse_item_props, service_parts, MenuLayout, ServiceParts};
use super::tray_menu::{parse_menu_layout, TrayMenu};

const WATCHER_PATH: &str = "/StatusNotifierWatcher";
const WATCHER_NAME: &str = "org.kde.StatusNotifierWatcher";

pub async fn run(
    event_tx: std::sync::mpsc::Sender<TrayEvent>,
    mut cmd_rx: tokio::sync::mpsc::Receiver<TrayCommand>,
) -> zbus::Result<()> {
    let (item_tx, mut item_rx) = mpsc::unbounded_channel();
    let state = Arc::new(Mutex::new(WatcherState::default()));
    let watcher = WatcherService {
        state: state.clone(),
        item_tx: item_tx.clone(),
    };

    let conn = zbus::connection::Builder::session()?
        .serve_at(WATCHER_PATH, watcher)?
        .build()
        .await?;

    let flags = RequestNameFlags::ReplaceExisting | RequestNameFlags::AllowReplacement;
    match conn
        .request_name_with_flags(WATCHER_NAME, flags)
        .await
    {
        Ok(_) => tracing::info!("tray: acquired {WATCHER_NAME}"),
        Err(err) => {
            tracing::warn!(
                %err,
                "tray: could not own {WATCHER_NAME}; using existing watcher as host"
            );
        }
    }

    register_host(&conn).await?;

    let conn = Arc::new(conn);
    let event_loop = {
        let conn = conn.clone();
        async move {
            while let Some(msg) = item_rx.recv().await {
                match msg {
                    ItemMsg::Update(parts) => {
                        match fetch_item(&conn, &parts).await {
                            Some(item) => {
                                tracing::info!(
                                    id = %item.id,
                                    title = %item.title,
                                    "tray: item updated"
                                );
                                let _ = event_tx.send(TrayEvent::Update(item));
                            }
                            None => {
                                tracing::warn!(
                                    bus = %parts.bus_name,
                                    path = %parts.object_path,
                                    "tray: failed to fetch item properties"
                                );
                            }
                        }
                    }
                    ItemMsg::Remove { bus_name } => {
                        tracing::info!(%bus_name, "tray: item removed");
                        let _ = event_tx.send(TrayEvent::Remove { bus_name });
                    }
                }
            }
        }
    };

    let item_tx_sync = item_tx.clone();
    let conn_sync = conn.clone();
    let cmd_loop = async move {
        while let Some(cmd) = cmd_rx.recv().await {
            match cmd {
                TrayCommand::SyncRegistered => {
                    sync_registered_items(&conn_sync, &item_tx_sync).await;
                }
                other => {
                    if let Err(err) = dispatch_command(&conn_sync, other).await {
                        tracing::warn!(%err, "tray: command failed");
                    }
                }
            }
        }
    };

    spawn_item_listener(conn.clone(), item_tx.clone());
    sync_registered_items(&conn, &item_tx).await;

    tokio::select! {
        _ = event_loop => {}
        _ = cmd_loop => {}
    }

    Ok(())
}

#[derive(Default)]
struct WatcherState {
    hosts: HashSet<String>,
    items: HashSet<String>,
    host_registered: bool,
}

struct WatcherService {
    state: Arc<Mutex<WatcherState>>,
    item_tx: mpsc::UnboundedSender<ItemMsg>,
}

#[derive(Clone, Debug)]
enum ItemMsg {
    Update(ServiceParts),
    Remove { bus_name: String },
}

#[interface(name = "org.kde.StatusNotifierWatcher")]
impl WatcherService {
    async fn register_status_notifier_host(&self, service: &str) {
        tracing::info!(host = service, "tray: host registered");
        let mut state = self.state.lock().expect("watcher state lock");
        state.hosts.insert(service.to_string());
        state.host_registered = true;
    }

    async fn register_status_notifier_item(
        &self,
        service: &str,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) {
        let Some(sender) = header.sender() else {
            return;
        };
        let unique = sender.to_string();
        let item_id = format!("{unique}{service}");
        tracing::info!(item = %item_id, "tray: item registered");
        {
            let mut state = self.state.lock().expect("watcher state lock");
            state.items.insert(item_id.clone());
        }
        if let Some(parts) = service_parts(&item_id) {
            let _ = self.item_tx.send(ItemMsg::Update(parts));
        }
    }

    async fn unregister_status_notifier_item(&self, service: &str) {
        tracing::info!(service, "tray: item unregistered");
        let mut state = self.state.lock().expect("watcher state lock");
        state.items.retain(|item| !item.contains(service));
        let _ = self.item_tx.send(ItemMsg::Remove {
            bus_name: service.to_string(),
        });
    }

    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.state
            .lock()
            .expect("watcher state lock")
            .items
            .iter()
            .cloned()
            .collect()
    }

    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        self.state
            .lock()
            .expect("watcher state lock")
            .host_registered
    }

    #[zbus(property)]
    fn protocol_version(&self) -> i32 {
        0
    }
}

async fn register_host(conn: &zbus::Connection) -> zbus::Result<()> {
    let pid = std::process::id();
    let host_name = format!("org.freedesktop.StatusNotifierHost-{pid}-metis-bar");
    conn.request_name_with_flags(
        host_name.as_str(),
        RequestNameFlags::AllowReplacement.into(),
    )
    .await?;
    let proxy = WatcherProxy::builder(conn)
        .destination(WATCHER_NAME)?
        .path(WATCHER_PATH)?
        .build()
        .await?;
    proxy
        .register_status_notifier_host(host_name.as_str())
        .await?;
    tracing::info!(host = host_name, "tray: registered StatusNotifierHost");
    Ok(())
}

async fn sync_registered_items(conn: &zbus::Connection, item_tx: &mpsc::UnboundedSender<ItemMsg>) {
    let proxy = match watcher_proxy(conn).await {
        Some(proxy) => proxy,
        None => return,
    };
    let Ok(items) = proxy.registered_status_notifier_items().await else {
        return;
    };
    tracing::info!(count = items.len(), "tray: syncing registered items");
    for item_id in items {
        if let Some(parts) = service_parts(&item_id) {
            let _ = item_tx.send(ItemMsg::Update(parts));
        } else {
            tracing::warn!(item_id, "tray: could not parse registered item id");
        }
    }
}

async fn watcher_proxy(conn: &zbus::Connection) -> Option<WatcherProxy<'_>> {
    WatcherProxy::builder(conn)
        .destination(WATCHER_NAME)
        .ok()?
        .path(WATCHER_PATH)
        .ok()?
        .build()
        .await
        .ok()
}

fn spawn_item_listener(conn: Arc<zbus::Connection>, item_tx: mpsc::UnboundedSender<ItemMsg>) {
    tokio::spawn(async move {
        let proxy = match watcher_proxy(&conn).await {
            Some(proxy) => proxy,
            None => return,
        };
        let Ok(mut stream) = proxy.receive_status_notifier_item_registered().await else {
            return;
        };
        while let Some(signal) = stream.next().await {
            let Ok(args) = signal.args() else {
                continue;
            };
            tracing::info!(service = args.service(), "tray: StatusNotifierItemRegistered");
            sync_registered_items(&conn, &item_tx).await;
        }
    });
}

async fn fetch_item(conn: &zbus::Connection, parts: &ServiceParts) -> Option<TrayItem> {
    let props = PropertiesProxy::builder(conn)
        .destination(parts.bus_name.as_str())
        .ok()?
        .path(parts.object_path.as_str())
        .ok()?
        .build()
        .await
        .ok()?
        .get_all("org.kde.StatusNotifierItem")
        .await
        .ok()?;

    let parsed = parse_item_props(&props);
    let menu_path = parsed.menu.clone();
    let menu = if let Some(path) = menu_path.clone() {
        fetch_menu(conn, &parts.bus_name, path.as_str()).await
    } else {
        None
    };

    Some(TrayItem {
        bus_name: parts.bus_name.clone(),
        object_path: parts.object_path.clone(),
        id: parsed.id,
        title: parsed.title,
        icon_name: parsed.icon_name,
        icon_theme_path: parsed.icon_theme_path,
        icon_pixmap: parsed.icon_pixmap.map(|p| super::tray::IconPixmap {
            width: p.width,
            height: p.height,
            pixels: p.pixels,
        }),
        menu_path,
        menu,
    })
}

async fn fetch_menu(
    conn: &zbus::Connection,
    bus_name: &str,
    menu_path: &str,
) -> Option<TrayMenu> {
    let proxy = DBusMenuProxy::builder(conn)
        .destination(bus_name)
        .ok()?
        .path(menu_path)
        .ok()?
        .build()
        .await
        .ok()?;
    let layout: MenuLayout = proxy.get_layout(0, -1, &[]).await.ok()?;
    Some(parse_menu_layout(layout))
}

async fn dispatch_command(conn: &zbus::Connection, cmd: TrayCommand) -> Result<(), String> {
    match cmd {
        TrayCommand::MenuClicked {
            bus_name,
            menu_path,
            submenu_id,
        } => {
            let proxy = DBusMenuProxy::builder(conn)
                .destination(bus_name.as_str())
                .map_err(|e| e.to_string())?
                .path(menu_path.as_str())
                .map_err(|e| e.to_string())?
                .build()
                .await
                .map_err(|e| e.to_string())?;
            proxy
                .event(
                    submenu_id,
                    "clicked",
                    &Value::I32(0),
                    chrono::Utc::now().timestamp_subsec_millis() as u32,
                )
                .await
                .map_err(|e| e.to_string())
        }
        TrayCommand::Activate {
            bus_name,
            object_path,
            x,
            y,
        } => {
            let proxy = StatusNotifierItemProxy::builder(conn)
                .destination(bus_name.as_str())
                .map_err(|e| e.to_string())?
                .path(object_path.as_str())
                .map_err(|e| e.to_string())?
                .build()
                .await
                .map_err(|e| e.to_string())?;
            proxy.activate(x, y).await.map_err(|e| e.to_string())
        }
        TrayCommand::SecondaryActivate {
            bus_name,
            object_path,
            x,
            y,
        } => {
            let proxy = StatusNotifierItemProxy::builder(conn)
                .destination(bus_name.as_str())
                .map_err(|e| e.to_string())?
                .path(object_path.as_str())
                .map_err(|e| e.to_string())?
                .build()
                .await
                .map_err(|e| e.to_string())?;
            proxy
                .secondary_activate(x, y)
                .await
                .map_err(|e| e.to_string())
        }
        TrayCommand::SyncRegistered => Ok(()),
    }
}

#[proxy(
    interface = "org.kde.StatusNotifierWatcher",
    default_path = "/StatusNotifierWatcher"
)]
trait Watcher {
    fn register_status_notifier_host(&self, service: &str) -> zbus::Result<()>;
    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> zbus::Result<Vec<String>>;
    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> zbus::Result<bool>;
    #[zbus(signal)]
    fn status_notifier_item_registered(&self, service: &str) -> zbus::Result<()>;
}

#[proxy(
    interface = "org.freedesktop.DBus.Properties",
    assume_defaults = true
)]
trait Properties {
    #[zbus(name = "GetAll")]
    fn get_all(&self, interface_name: &str) -> zbus::Result<HashMap<String, OwnedValue>>;
}

#[proxy(interface = "com.canonical.dbusmenu", assume_defaults = true)]
trait DBusMenu {
    fn get_layout(
        &self,
        parent_id: i32,
        recursion_depth: i32,
        property_names: &[&str],
    ) -> zbus::Result<MenuLayout>;
    fn event(
        &self,
        id: i32,
        event_id: &str,
        data: &Value<'_>,
        timestamp: u32,
    ) -> zbus::Result<()>;
}

#[proxy(
    interface = "org.kde.StatusNotifierItem",
    assume_defaults = true
)]
trait StatusNotifierItem {
    fn activate(&self, x: i32, y: i32) -> zbus::Result<()>;
    fn secondary_activate(&self, x: i32, y: i32) -> zbus::Result<()>;
}
