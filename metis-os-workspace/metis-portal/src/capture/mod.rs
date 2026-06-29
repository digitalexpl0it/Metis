//! Screen capture for the Metis portal backend.

mod shm;
mod wayland;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ashpd::PortalError;

use crate::pipewire::PipeWireHub;

#[derive(Debug, Clone)]
pub struct CapturedPng {
    pub path: PathBuf,
}

pub struct CaptureHub {
    _pipewire: Arc<PipeWireHub>,
}

impl CaptureHub {
    pub fn new(pipewire: Arc<PipeWireHub>) -> Self {
        Self {
            _pipewire: pipewire,
        }
    }

    pub async fn screenshot_png(&self) -> Result<PathBuf, PortalError> {
        capture_fullscreen_png()
            .await
            .map(|c| c.path)
            .map_err(PortalError::Failed)
    }

    pub async fn output_size(&self) -> (u32, u32) {
        match tokio::task::spawn_blocking(wayland::capture_output_frame).await {
            Ok(Ok(frame)) => (frame.width, frame.height),
            _ => (1920, 1080),
        }
    }
}

pub async fn capture_fullscreen_png() -> Result<CapturedPng, String> {
    let path = screenshot_path();
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|err| format!("create screenshot dir: {err}"))?;
    }
    let _ = tokio::fs::remove_file(&path).await;

    let frame = tokio::task::spawn_blocking(wayland::capture_output_frame)
        .await
        .map_err(|err| format!("capture task failed: {err}"))??;

    let rgba = bgra_to_rgba(&frame.data, frame.width, frame.height, frame.stride);
    write_png(&path, frame.width, frame.height, &rgba)?;

    Ok(CapturedPng { path })
}

fn bgra_to_rgba(data: &[u8], width: u32, height: u32, stride: u32) -> Vec<u8> {
    let mut out = vec![0u8; (width * height * 4) as usize];
    for y in 0..height {
        let src_row = (y * stride) as usize;
        let dst_row = (y * width * 4) as usize;
        for x in 0..width {
            let si = src_row + (x * 4) as usize;
            let di = dst_row + (x * 4) as usize;
            if si + 3 >= data.len() || di + 3 >= out.len() {
                continue;
            }
            out[di] = data[si + 2];
            out[di + 1] = data[si + 1];
            out[di + 2] = data[si];
            out[di + 3] = 255;
        }
    }
    out
}

fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|err| format!("create png: {err}"))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|err| format!("png header: {err}"))?;
    writer
        .write_image_data(rgba)
        .map_err(|err| format!("png write: {err}"))?;
    writer
        .finish()
        .map_err(|err| format!("png finish: {err}"))?;
    Ok(())
}

fn screenshot_path() -> PathBuf {
    let base = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    Path::new(&base).join(format!("metis-screenshot-{millis}.png"))
}
