#![allow(irrefutable_let_patterns)]

mod desk_input;
mod events;
mod focus;
mod grabs;
mod handlers;
mod input;
mod ipc;
mod state;
mod winit;
mod wallpaper;
mod windows;

use smithay::reexports::{calloop::EventLoop, wayland_server::Display};

use crate::state::MetisState;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let mut event_loop: EventLoop<'_, MetisState> = EventLoop::try_new()?;
    let display: Display<MetisState> = Display::new()?;
    let mut state = MetisState::new(&mut event_loop, display);

    winit::init_winit(&mut event_loop, &mut state)?;
    ipc::init_ipc(&mut state)?;

    unsafe {
        std::env::set_var("WAYLAND_DISPLAY", &state.socket_name);
    }

    let shell = if std::env::var("METIS_NO_SHELL").is_ok() {
        None
    } else {
        Some(
            std::env::var("METIS_SHELL_BIN").unwrap_or_else(|_| {
                std::env::current_exe()
                    .ok()
                    .and_then(|p| {
                        p.parent()
                            .map(|d| d.join("metis-shell").display().to_string())
                    })
                    .unwrap_or_else(|| "metis-shell".into())
            }),
        )
    };

    let client = parse_client_command();
    state.queue_startup(shell, client);

    tracing::info!(
        socket = ?state.socket_name,
        "Metis compositor running — apps, layer-shell overlays, and notifications supported"
    );

    event_loop.run(Some(std::time::Duration::from_millis(1)), &mut state, |state| {
        state.flush_clients_if_pending();
    })?;

    tracing::info!("Metis compositor event loop exited");
    Ok(())
}

fn parse_client_command() -> Option<String> {
    let mut args = std::env::args().skip(1);
    match (args.next().as_deref(), args.next()) {
        (Some("-c" | "--command"), Some(command)) => Some(command),
        _ => None,
    }
}
