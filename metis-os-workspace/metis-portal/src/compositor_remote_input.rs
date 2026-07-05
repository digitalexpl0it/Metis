//! Forward remote-desktop input events to the compositor over IPC.

use metis_protocol::{CompositorCommand, CompositorEvent};

pub fn inject_pointer_absolute(x: f64, y: f64) {
    send(&CompositorCommand::InjectRemotePointerAbsolute { x, y });
}

pub fn inject_pointer_relative(dx: f64, dy: f64) {
    send(&CompositorCommand::InjectRemotePointerRelative { dx, dy });
}

pub fn inject_pointer_button(button: u32, pressed: bool) {
    send(&CompositorCommand::InjectRemotePointerButton { button, pressed });
}

pub fn inject_pointer_scroll(dx: f64, dy: f64) {
    send(&CompositorCommand::InjectRemotePointerScroll { dx, dy });
}

pub fn inject_key(keycode: u32, pressed: bool) {
    send(&CompositorCommand::InjectRemoteKey { keycode, pressed });
}

fn send(cmd: &CompositorCommand) {
    match metis_protocol::send_compositor_command(cmd) {
        Ok(CompositorEvent::Pong) => {}
        Ok(CompositorEvent::Error { message }) => {
            tracing::warn!(%message, "remote input IPC rejected");
        }
        Ok(_) => {}
        Err(err) => tracing::debug!(%err, "remote input IPC failed"),
    }
}
