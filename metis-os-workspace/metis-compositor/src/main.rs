#![allow(irrefutable_let_patterns)]

mod blur;
mod cursor;
mod decoration;
mod desk_input;
mod events;
mod focus;
mod grabs;
mod handlers;
mod input;
mod ipc;
mod keybinds;
mod render;
mod state;
mod udev;
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

/// Which rendering/session backend Metis runs under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Backend {
    /// Nested winit window inside an existing Wayland/X11 session (dev path).
    Winit,
    /// Standalone DRM/KMS + libseat + libinput session on a bare TTY/GPU.
    Drm,
}

/// Pick the backend. `METIS_BACKEND=winit|drm` forces a choice; otherwise we
/// autodetect: a parent Wayland/X11 session (`WAYLAND_DISPLAY`/`DISPLAY` set)
/// means we are nested (winit), and a bare TTY means the standalone DRM session.
fn select_backend() -> Backend {
    match std::env::var("METIS_BACKEND").ok().as_deref() {
        Some("winit") => return Backend::Winit,
        Some("drm") | Some("udev") | Some("tty") => return Backend::Drm,
        Some(other) if !other.is_empty() => {
            tracing::warn!(%other, "unknown METIS_BACKEND, autodetecting");
        }
        _ => {}
    }
    let nested = std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var_os("DISPLAY").is_some();
    if nested {
        Backend::Winit
    } else {
        Backend::Drm
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

    let backend = select_backend();
    tracing::info!(?backend, "selected compositor backend");

    let mut event_loop: EventLoop<'static, MetisState> = EventLoop::try_new()?;
    let display: Display<MetisState> = Display::new()?;
    let mut state = MetisState::new(&mut event_loop, display);

    match backend {
        Backend::Winit => {
            winit::init_winit(&mut event_loop, &mut state)?;
            // Nested winit dev session — shell skips dbus notification takeover and
            // other host-session side effects that fight GNOME during startup.
            unsafe {
                std::env::set_var("METIS_NESTED", "1");
            }
        }
        Backend::Drm => {
            udev::init_udev(&mut event_loop, &mut state)?;
        }
    }

    ipc::init_ipc(&mut state)?;
    state.start_xwayland(event_loop.handle());

    // Capture the host's WAYLAND_DISPLAY before we overwrite it with our own
    // socket, so the activation-env import (below) can be undone on exit. Only
    // meaningful in the nested winit session.
    let host_wayland_display = std::env::var_os("WAYLAND_DISPLAY");

    unsafe {
        std::env::set_var("WAYLAND_DISPLAY", &state.socket_name);
    }

    if backend == Backend::Drm {
        // GtkApplication-based apps (Cheese, many GTK4 clients) do a *synchronous*
        // portal Settings proxy during startup. If xdg-desktop-portal is cold, that
        // blocks for up to ~25s per launch. Pre-start it once our socket exists.
        prewarm_desktop_portal();
        update_standalone_activation_env(&state.socket_name.to_string_lossy());
    }

    // Opt-in dev convenience (set by `run-metis.sh --import-env`): point the user
    // D-Bus session bus and `systemd --user` activation environment at this
    // nested compositor, so D-Bus-activated and single-instance apps open inside
    // Metis instead of the host session. It is opt-in because, in a nested dev
    // session, this temporarily redirects activation for the whole logged-in
    // user; we restore the host value when the event loop exits. The standalone
    // DRM session owns the seat outright, so it never touches the host env here.
    let import_activation_env =
        backend == Backend::Winit && std::env::var_os("METIS_IMPORT_ACTIVATION_ENV").is_some();
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
        keybind_mod = crate::keybinds::keybind_mod_label(),
        "Metis compositor running — apps, layer-shell overlays, and notifications supported"
    );

    event_loop.run(Some(std::time::Duration::from_millis(1)), &mut state, |state| {
        state.flush_clients_if_pending();
    })?;

    // Standalone DRM session: release the GPU deterministically before the rest
    // of teardown so DRM master / KMS state is handed back cleanly (otherwise the
    // next session can fail to become master and the TTY is left on a black
    // framebuffer). Dropping `UdevState` drops the DrmDevice + libseat session.
    if state.is_drm_backend() {
        tracing::info!("releasing DRM devices and seat session");
        state.udev = None;
    }

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

/// Push session variables into the user D-Bus / systemd activation environment.
/// Failures are non-fatal (logged).
fn push_activation_environment(vars: &[&str]) {
    let mut cmd = std::process::Command::new("dbus-update-activation-environment");
    cmd.arg("--systemd");
    for var in vars {
        cmd.arg(var);
    }
    match cmd.status() {
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

/// Nested dev convenience: point activation at our Wayland socket.
fn update_activation_environment(display: &str) {
    push_activation_environment(&[&format!("WAYLAND_DISPLAY={display}")]);
}

/// Standalone DRM session: publish Wayland + GTK hardening vars so D-Bus-activated
/// apps (and later spawns) land in this session without portal/a11y stalls.
fn update_standalone_activation_env(display: &str) {
    let gdk_debug = std::env::var("GDK_DEBUG").unwrap_or_else(|_| "no-portals".into());
    let gdk_debug = if gdk_debug.is_empty() {
        "no-portals".to_string()
    } else if !gdk_debug.split(',').any(|p| p == "no-portals" || p == "portals") {
        format!("{gdk_debug},no-portals")
    } else {
        gdk_debug
    };
    unsafe {
        std::env::set_var("GDK_DEBUG", &gdk_debug);
        std::env::set_var("GTK_A11Y", "none");
        std::env::set_var("NO_AT_BRIDGE", "1");
    }
    let gsk_renderer = std::env::var("GSK_RENDERER").unwrap_or_else(|_| "cairo".into());
    push_activation_environment(&[
        &format!("WAYLAND_DISPLAY={display}"),
        &format!("GDK_DEBUG={gdk_debug}"),
        "GTK_A11Y=none",
        "NO_AT_BRIDGE=1",
        "GDK_BACKEND=wayland",
        &format!("GSK_RENDERER={gsk_renderer}"),
    ]);
}

/// Best-effort: start xdg-desktop-portal once our Wayland socket exists so the
/// first GtkApplication launch does not cold-start the whole portal stack.
fn prewarm_desktop_portal() {
    if std::env::var_os("METIS_NO_PORTAL_PREWARM").is_some() {
        return;
    }
    match std::process::Command::new("xdg-desktop-portal")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(_) => tracing::info!("pre-started xdg-desktop-portal"),
        Err(err) => tracing::debug!(%err, "xdg-desktop-portal unavailable"),
    }
}

fn parse_client_command() -> Option<String> {
    let mut args = std::env::args().skip(1);
    match (args.next().as_deref(), args.next()) {
        (Some("-c" | "--command"), Some(command)) => Some(command),
        _ => None,
    }
}
