mod background;
mod capture;
mod compositor_ipc;
mod pipewire;
mod power_profile;
mod screensaver;
mod screenshot;
mod screencast;
mod settings;

use std::sync::Arc;

use futures_util::future::pending;
use background::MetisBackground;
use screenshot::MetisScreenshot;
use screencast::MetisScreencast;
use settings::MetisSettings;

const DBUS_NAME: &str = "org.freedesktop.impl.portal.desktop.metis";

#[tokio::main]
async fn main() -> ashpd::Result<()> {
    tracing_subscriber::fmt::init();

    let mut args = std::env::args().skip(1);
    if args.next().as_deref() == Some("--capture-test") {
        let path = args
            .next()
            .unwrap_or_else(|| "/tmp/metis-capture-test.png".into());
        tracing::info!(%path, "capture test");
        let captured = capture::capture_fullscreen_png()
            .await
            .map_err(|err| ashpd::PortalError::Failed(err))?;
        std::fs::copy(&captured.path, &path).map_err(|err| {
            ashpd::PortalError::Failed(format!("copy screenshot to {path}: {err}"))
        })?;
        tracing::info!(%path, "capture test ok");
        return Ok(());
    }

    tracing::info!(
        "starting Metis portal backend (Settings, Screenshot, ScreenCast, Background, PowerProfile)"
    );

    let pipewire = Arc::new(pipewire::PipeWireHub::start()?);
    let capture = Arc::new(capture::CaptureHub::new(Arc::clone(&pipewire)));

    let connection = zbus::Connection::session().await.map_err(|err| {
        ashpd::PortalError::Failed(format!("session bus connection: {err}"))
    })?;

    ashpd::backend::Builder::new(DBUS_NAME)?
        .settings(MetisSettings)
        .screenshot(MetisScreenshot::new(Arc::clone(&capture)))
        .screencast(MetisScreencast::new(capture, pipewire))
        .background(MetisBackground)
        .build_with_connection(connection.clone())
        .await?;

    if let Err(err) = power_profile::serve(&connection).await {
        tracing::warn!(%err, "PowerProfileMonitor portal unavailable");
    }

    // Own the legacy idle-inhibit D-Bus names (games/media keep the screen awake
    // through these). Kept alive for the whole session; a startup failure is
    // non-fatal (Wayland `zwp_idle_inhibit` still works).
    let _screensaver = match screensaver::serve().await {
        Ok(conn) => Some(conn),
        Err(err) => {
            tracing::warn!(%err, "screensaver: idle-inhibit service unavailable");
            None
        }
    };

    pending::<()>().await;
    Ok(())
}
