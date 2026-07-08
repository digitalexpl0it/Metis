//! Event-driven game session detection via compositor events.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use metis_config::{load_gaming_config, PowerProfile};
use metis_protocol::CompositorEvent;

use crate::power::{
    apply_session_power, read_current_power_profile, register_gamemode, unregister_gamemode,
    SessionPowerAction,
};

pub struct GamingDaemon {
    game_active: bool,
    saved_profile: Option<PowerProfile>,
    game_pid: Option<u32>,
    window_app_ids: HashMap<u32, String>,
}

impl GamingDaemon {
    pub fn new() -> Self {
        Self {
            game_active: false,
            saved_profile: None,
            game_pid: None,
            window_app_ids: HashMap::new(),
        }
    }

    pub fn on_game_session(&mut self, active: bool, label: Option<String>, pid: Option<u32>) {
        let cfg = load_gaming_config();
        if active && !self.game_active {
            tracing::info!(label = ?label, pid, "game session started");
            if cfg.auto_performance_profile {
                self.saved_profile = read_current_power_profile();
                apply_session_power(SessionPowerAction::EnterPerformance, self.saved_profile);
            }
            if cfg.auto_gamemode {
                if let Some(pid) = pid {
                    register_gamemode(pid);
                    self.game_pid = Some(pid);
                }
            }
            self.game_active = true;
        } else if !active && self.game_active {
            tracing::info!("game session ended");
            if cfg.auto_performance_profile {
                apply_session_power(
                    SessionPowerAction::RestoreBalanced,
                    self.saved_profile,
                );
            }
            if let Some(pid) = self.game_pid.take() {
                unregister_gamemode(pid);
            }
            self.game_active = false;
            self.saved_profile = None;
        }
    }

    pub fn handle_event(&mut self, evt: CompositorEvent) {
        match evt {
            CompositorEvent::GameSession { active, label, pid } => {
                self.on_game_session(active, label, pid);
            }
            CompositorEvent::WindowMetadata { id, app_id, .. } => {
                if let Some(app_id) = app_id {
                    self.window_app_ids.insert(id, app_id);
                }
            }
            CompositorEvent::WindowClosed { id } => {
                self.window_app_ids.remove(&id);
            }
            CompositorEvent::WindowFullscreen { id, fullscreen, .. } => {
                if fullscreen {
                    let app_id = self.window_app_ids.get(&id).cloned();
                    let game_like = app_id.as_deref().is_some_and(|id| {
                        let l = id.to_ascii_lowercase();
                        l.contains("steam_app_")
                            || l.contains("lutris")
                            || l.contains("wine")
                            || l.contains(".exe")
                    });
                    if game_like {
                        self.on_game_session(true, app_id, None);
                    }
                }
            }
            _ => {}
        }
    }
}

pub fn spawn_event_listener(tx: mpsc::Sender<CompositorEvent>) {
    thread::Builder::new()
        .name("metis-gaming-events".into())
        .spawn(move || event_listen_loop(tx))
        .ok();
}

fn event_listen_loop(tx: mpsc::Sender<CompositorEvent>) {
    loop {
        if connect_and_subscribe().is_ok() {
            if let Ok(stream) = connect_events_socket() {
                read_events(stream, &tx);
            }
        }
        thread::sleep(Duration::from_millis(800));
    }
}

fn connect_events_socket() -> std::io::Result<UnixStream> {
    let stream = UnixStream::connect(metis_protocol::events_socket_path())?;
    stream.set_read_timeout(None)?;
    Ok(stream)
}

fn connect_and_subscribe() -> std::io::Result<()> {
    let _ = metis_protocol::send_compositor_command(
        &metis_protocol::CompositorCommand::SubscribeEvents,
    )?;
    Ok(())
}

fn read_events(stream: UnixStream, tx: &mpsc::Sender<CompositorEvent>) {
    let reader = BufReader::new(stream);
    for line in reader.lines() {
        let Ok(line) = line else {
            tracing::warn!("gamingd: compositor event stream disconnected");
            break;
        };
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(evt) = serde_json::from_str::<CompositorEvent>(&line) {
            let _ = tx.send(evt);
        }
    }
}

pub fn request_reload() {
    let path = metis_protocol::runtime_command_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&path)
    {
        let _ = writeln!(f, "reload-gaming");
    }
}

pub fn request_optimize() {
    let path = metis_protocol::runtime_command_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&path)
    {
        let _ = writeln!(f, "optimize-gaming");
    }
}
