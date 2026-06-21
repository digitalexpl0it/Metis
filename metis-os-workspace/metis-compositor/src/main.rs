#![allow(irrefutable_let_patterns)]

mod blur;
mod decoration;
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
mod window_state;
mod windows;
mod xwayland;

use smithay::reexports::{calloop::EventLoop, wayland_server::Display};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer;

use crate::state::MetisState;

/// Drops one specific benign Smithay log line.
///
/// GTK creates short-lived sub-popups (text-entry selection menus, etc.) and
/// tears them down before their xdg-popup grab is processed, which makes
/// Smithay's pre-commit hook log `surface missing from known popups` at ERROR.
/// The popover still works; this filter swallows only that exact message while
/// keeping every other xdg-shell diagnostic intact.
struct DropKnownPopupNoise;

impl<S> tracing_subscriber::layer::Filter<S> for DropKnownPopupNoise {
    fn enabled(
        &self,
        _meta: &tracing::Metadata<'_>,
        _cx: &tracing_subscriber::layer::Context<'_, S>,
    ) -> bool {
        true
    }

    fn event_enabled(
        &self,
        event: &tracing::Event<'_>,
        _cx: &tracing_subscriber::layer::Context<'_, S>,
    ) -> bool {
        if event.metadata().target() != "smithay::wayland::shell::xdg" {
            return true;
        }
        let mut probe = PopupNoiseProbe::default();
        event.record(&mut probe);
        !probe.matched
    }
}

#[derive(Default)]
struct PopupNoiseProbe {
    matched: bool,
}

impl tracing::field::Visit for PopupNoiseProbe {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message"
            && format!("{value:?}").contains("surface missing from known popups")
        {
            self.matched = true;
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
                )
                .with_filter(DropKnownPopupNoise),
        )
        .init();

    let mut event_loop: EventLoop<'_, MetisState> = EventLoop::try_new()?;
    let display: Display<MetisState> = Display::new()?;
    let mut state = MetisState::new(&mut event_loop, display);

    winit::init_winit(&mut event_loop, &mut state)?;
    ipc::init_ipc(&mut state)?;
    state.start_xwayland(event_loop.handle());

    // Capture the host's WAYLAND_DISPLAY before we overwrite it with our nested
    // socket, so the activation-env import (below) can be undone on exit.
    let host_wayland_display = std::env::var_os("WAYLAND_DISPLAY");

    unsafe {
        std::env::set_var("WAYLAND_DISPLAY", &state.socket_name);
    }

    // Opt-in dev convenience (set by `run-metis.sh --import-env`): point the user
    // D-Bus session bus and `systemd --user` activation environment at this
    // nested compositor, so D-Bus-activated and single-instance apps open inside
    // Metis instead of the host session. It is opt-in because, in a nested dev
    // session, this temporarily redirects activation for the whole logged-in
    // user; we restore the host value when the event loop exits.
    let import_activation_env = std::env::var_os("METIS_IMPORT_ACTIVATION_ENV").is_some();
    if import_activation_env {
        update_activation_environment(&state.socket_name.to_string_lossy());
        tracing::info!(
            wayland_display = ?state.socket_name,
            "imported nested WAYLAND_DISPLAY into D-Bus/systemd activation environment"
        );
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

    // Hand the user's D-Bus/systemd activation environment back to the host
    // compositor so later app launches don't keep targeting our dead socket.
    if import_activation_env {
        match host_wayland_display.as_deref().map(|v| v.to_string_lossy()) {
            Some(host) => {
                update_activation_environment(&host);
                tracing::info!(
                    wayland_display = %host,
                    "restored host WAYLAND_DISPLAY in activation environment"
                );
            }
            None => tracing::warn!(
                "no prior WAYLAND_DISPLAY to restore — activation env still points at the closed Metis socket"
            ),
        }
    }

    tracing::info!("Metis compositor event loop exited");
    Ok(())
}

/// Push `WAYLAND_DISPLAY=<display>` into the user D-Bus session bus and the
/// `systemd --user` manager. `dbus-update-activation-environment --systemd`
/// updates both in a single call; failures are non-fatal (logged) since this is
/// a best-effort dev convenience.
fn update_activation_environment(display: &str) {
    let result = std::process::Command::new("dbus-update-activation-environment")
        .arg("--systemd")
        .arg(format!("WAYLAND_DISPLAY={display}"))
        .status();
    match result {
        Ok(status) if status.success() => {}
        Ok(status) => tracing::warn!(
            code = ?status.code(),
            "dbus-update-activation-environment exited non-zero"
        ),
        Err(err) => tracing::warn!(
            %err,
            "could not run dbus-update-activation-environment (is dbus installed?)"
        ),
    }
}

fn parse_client_command() -> Option<String> {
    let mut args = std::env::args().skip(1);
    match (args.next().as_deref(), args.next()) {
        (Some("-c" | "--command"), Some(command)) => Some(command),
        _ => None,
    }
}
