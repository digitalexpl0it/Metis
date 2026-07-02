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
