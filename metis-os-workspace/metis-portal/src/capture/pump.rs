//! ScreenCast frame pump — Wayland capture → PipeWire.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use ashpd::PortalError;

use crate::capture::session::CaptureSession;
use crate::pipewire::PipeWireHub;

/// Convert compositor BGRA (ARGB8888 SHM) pixels to PipeWire BGRx.
fn bgra_to_bgrx(data: &[u8], width: u32, height: u32, stride: u32) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    let src_stride = stride as usize;
    let dst_stride = w * 4;
    let mut out = vec![0u8; dst_stride * h];
    for y in 0..h {
        let src_row = y * src_stride;
        let dst_row = y * dst_stride;
        for x in 0..w {
            let si = src_row + x * 4;
            let di = dst_row + x * 4;
            if si + 3 >= data.len() || di + 3 >= out.len() {
                continue;
            }
            out[di] = data[si];
            out[di + 1] = data[si + 1];
            out[di + 2] = data[si + 2];
            out[di + 3] = 255;
        }
    }
    out
}

pub fn spawn_screencast_pump(
    pipewire: Arc<PipeWireHub>,
    node_id: u32,
    paint_cursors: bool,
    cancel: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("metis-screencast".into())
        .spawn(move || {
            let session = match CaptureSession::open(paint_cursors) {
                Ok(s) => s,
                Err(err) => {
                    tracing::error!(%err, "screencast capture session failed");
                    pipewire.destroy_stream(node_id);
                    return;
                }
            };
            let mut session = session;
            let frame_interval = Duration::from_millis(33);
            while !cancel.load(Ordering::Relaxed) {
                let start = std::time::Instant::now();
                match session.capture_next_frame() {
                    Ok(frame) => {
                        let pixels =
                            bgra_to_bgrx(&frame.data, frame.width, frame.height, frame.stride);
                        if let Err(err) = pipewire.push_frame(node_id, pixels) {
                            tracing::warn!(%err, "pipewire push_frame failed");
                        }
                    }
                    Err(err) => tracing::warn!(%err, "screencast frame capture failed"),
                }
                let elapsed = start.elapsed();
                if elapsed < frame_interval {
                    thread::sleep(frame_interval - elapsed);
                }
            }
            pipewire.destroy_stream(node_id);
        })
        .expect("spawn screencast pump thread")
}

pub fn portal_err(msg: impl Into<String>) -> PortalError {
    PortalError::Failed(msg.into())
}
