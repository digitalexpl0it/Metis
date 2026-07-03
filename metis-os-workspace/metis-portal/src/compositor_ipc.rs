use metis_protocol::{CompositorCommand, CompositorEvent};

pub fn portal_app_id(app_id: Option<ashpd::MaybeAppID>) -> Option<String> {
    app_id.map(|id| id.to_string())
}

pub fn begin_capture_overlay(app_id: Option<String>) {
    let cmd = CompositorCommand::BeginCaptureOverlay { app_id };
    match metis_protocol::send_compositor_command(&cmd) {
        Ok(CompositorEvent::Pong) => {}
        Ok(CompositorEvent::Error { message }) => {
            tracing::warn!(%message, "BeginCaptureOverlay rejected");
        }
        Ok(_) => {}
        Err(err) => tracing::debug!(%err, "BeginCaptureOverlay IPC failed"),
    }
}

pub fn end_capture_overlay(app_id: Option<String>) {
    let cmd = CompositorCommand::EndCaptureOverlay { app_id };
    match metis_protocol::send_compositor_command(&cmd) {
        Ok(CompositorEvent::Pong) => {}
        Ok(CompositorEvent::Error { message }) => {
            tracing::warn!(%message, "EndCaptureOverlay rejected");
        }
        Ok(_) => {}
        Err(err) => tracing::debug!(%err, "EndCaptureOverlay IPC failed"),
    }
}

/// Tell the compositor to hold an idle inhibitor for `cookie` (blocking IPC —
/// call from `spawn_blocking`). Failures are logged, not fatal: the worst case
/// is the screen still blanks under a running media app.
pub fn inhibit_idle(cookie: u32, app_name: Option<String>, reason: Option<String>) {
    let cmd = CompositorCommand::InhibitIdle {
        cookie,
        app_name,
        reason,
    };
    match metis_protocol::send_compositor_command(&cmd) {
        Ok(CompositorEvent::Pong) => {}
        Ok(CompositorEvent::Error { message }) => {
            tracing::warn!(%message, "InhibitIdle rejected");
        }
        Ok(_) => {}
        Err(err) => tracing::debug!(%err, "InhibitIdle IPC failed"),
    }
}

/// Release the idle inhibitor previously taken for `cookie`.
pub fn uninhibit_idle(cookie: u32) {
    let cmd = CompositorCommand::UninhibitIdle { cookie };
    match metis_protocol::send_compositor_command(&cmd) {
        Ok(CompositorEvent::Pong) => {}
        Ok(CompositorEvent::Error { message }) => {
            tracing::warn!(%message, "UninhibitIdle rejected");
        }
        Ok(_) => {}
        Err(err) => tracing::debug!(%err, "UninhibitIdle IPC failed"),
    }
}
