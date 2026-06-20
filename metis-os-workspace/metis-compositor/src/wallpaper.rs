use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use image::imageops::Triangle;
use smithay::backend::{
    allocator::Fourcc,
    renderer::{
        element::{
            Kind,
            texture::{TextureBuffer, TextureRenderElement},
        },
        gles::{GlesRenderer, GlesTexture},
    },
};
use smithay::utils::{Physical, Point, Size, Transform};

pub struct Wallpaper {
    path: PathBuf,
    buffer: Option<TextureBuffer<GlesTexture>>,
    output_size: Size<i32, Physical>,
    /// Decoded RGBA pixels (CPU) ready for a fast GPU upload during render.
    cpu_pixels: Option<Vec<u8>>,
    decode_result: Option<Arc<Mutex<Option<Vec<u8>>>>>,
    decode_thread: Option<JoinHandle<()>>,
}

/// True only when `METIS_NO_WALLPAPER` is set to a non-empty value. An explicit
/// empty value (`METIS_NO_WALLPAPER=`) means "enable wallpaper".
fn wallpaper_disabled() -> bool {
    std::env::var("METIS_NO_WALLPAPER")
        .map(|v| !v.is_empty())
        .unwrap_or(false)
}

impl Wallpaper {
    pub fn new() -> Self {
        let path = if wallpaper_disabled() {
            PathBuf::new()
        } else {
            resolve_path().unwrap_or_default()
        };
        if path.is_file() {
            tracing::info!(path = %path.display(), "wallpaper configured");
        }
        Self {
            path,
            buffer: None,
            output_size: Size::from((0, 0)),
            cpu_pixels: None,
            decode_result: None,
            decode_thread: None,
        }
    }

    pub fn enabled(&self) -> bool {
        if wallpaper_disabled() {
            return false;
        }
        self.path.is_file()
    }

    pub fn invalidate(&mut self) {
        self.buffer = None;
        self.cpu_pixels = None;
        if let Some(handle) = self.decode_thread.take() {
            let _ = handle.join();
        }
        self.decode_result = None;
    }

    pub fn resize(&mut self, size: Size<i32, Physical>) {
        if self.output_size != size {
            self.output_size = size;
            self.invalidate();
        }
    }

    /// Start JPEG decode on a background thread so compositor init stays responsive.
    pub fn start_async_decode(&mut self) {
        if !self.enabled() || self.cpu_pixels.is_some() || self.decode_thread.is_some() {
            return;
        }
        if self.output_size.w <= 0 || self.output_size.h <= 0 {
            return;
        }

        let w = self.output_size.w as u32;
        let h = self.output_size.h as u32;
        let path = self.path.clone();
        let slot = Arc::new(Mutex::new(None));
        let slot_worker = Arc::clone(&slot);

        tracing::debug!(width = w, height = h, "starting wallpaper decode thread");
        let handle = std::thread::Builder::new()
            .name("metis-wallpaper-decode".into())
            .spawn(move || {
                let Ok(img) = image::open(&path) else {
                    tracing::warn!(path = %path.display(), "failed to open wallpaper");
                    return;
                };
                let pixels = cover_crop_rgba(img, w, h);
                if let Ok(mut guard) = slot_worker.lock() {
                    *guard = Some(pixels);
                }
            })
            .ok();

        if let Some(handle) = handle {
            self.decode_result = Some(slot);
            self.decode_thread = Some(handle);
        }
    }

    /// Pull decoded pixels from the worker thread when ready.
    pub fn poll_decode(&mut self) {
        if self.cpu_pixels.is_some() {
            return;
        }
        if let Some(slot) = &self.decode_result {
            if let Ok(mut guard) = slot.lock() {
                if let Some(pixels) = guard.take() {
                    tracing::info!(
                        path = %self.path.display(),
                        width = self.output_size.w,
                        height = self.output_size.h,
                        "wallpaper decoded"
                    );
                    self.cpu_pixels = Some(pixels);
                }
            }
        }
        if let Some(handle) = self.decode_thread.take() {
            if handle.is_finished() {
                let _ = handle.join();
            } else {
                self.decode_thread = Some(handle);
            }
        }
    }

    pub fn ensure(&mut self, renderer: &mut GlesRenderer) {
        if !self.enabled() || self.buffer.is_some() {
            return;
        }
        if self.output_size.w <= 0 || self.output_size.h <= 0 {
            return;
        }

        self.poll_decode();

        let Some(rgba) = self.cpu_pixels.as_ref() else {
            return;
        };

        let w = self.output_size.w;
        let h = self.output_size.h;

        match TextureBuffer::from_memory(
            renderer,
            rgba,
            Fourcc::Abgr8888,
            (w, h),
            false,
            1,
            Transform::Normal,
            None,
        ) {
            Ok(buf) => {
                tracing::info!(path = %self.path.display(), width = w, height = h, "wallpaper ready");
                self.buffer = Some(buf);
            }
            Err(err) => tracing::warn!(?err, "failed to upload wallpaper texture"),
        }
    }

    pub fn render_element(&self) -> Option<TextureRenderElement<GlesTexture>> {
        let buffer = self.buffer.as_ref()?;
        Some(TextureRenderElement::from_texture_buffer(
            Point::from((0.0, 0.0)),
            buffer,
            None,
            None,
            None,
            Kind::Unspecified,
        ))
    }

    /// True while a background decode is running and the texture is not uploaded yet.
    pub fn decode_in_flight(&self) -> bool {
        self.enabled()
            && self.buffer.is_none()
            && (self.decode_thread.is_some() || self.decode_result.is_some())
    }
}

fn cover_crop_rgba(img: image::DynamicImage, out_w: u32, out_h: u32) -> Vec<u8> {
    let rgba = img.to_rgba8();
    let (iw, ih) = (rgba.width(), rgba.height());
    if iw == 0 || ih == 0 {
        return vec![0; (out_w as usize) * (out_h as usize) * 4];
    }

    let scale = (out_w as f32 / iw as f32).max(out_h as f32 / ih as f32);
    let rw = ((iw as f32 * scale).ceil() as u32).max(1);
    let rh = ((ih as f32 * scale).ceil() as u32).max(1);
    let resized = image::imageops::resize(&rgba, rw, rh, Triangle);
    let x = rw.saturating_sub(out_w) / 2;
    let y = rh.saturating_sub(out_h) / 2;
    image::imageops::crop_imm(&resized, x, y, out_w, out_h)
        .to_image()
        .into_raw()
}

pub fn resolve_path() -> Option<PathBuf> {
    if let Ok(raw) = std::env::var("METIS_WALLPAPER") {
        let path = PathBuf::from(raw);
        if path.is_file() {
            return Some(path);
        }
        tracing::warn!(path = %path.display(), "METIS_WALLPAPER is not a file");
    }

    if let Some(dirs) = directories::ProjectDirs::from("com", "metis", "metis") {
        for name in ["wallpaper.jpg", "wallpaper.png", "wallpaper.webp"] {
            let path = dirs.config_dir().join(name);
            if path.is_file() {
                return Some(path);
            }
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for rel in [
                "assets/wallpapers/default.jpg",
                "../assets/wallpapers/default.jpg",
                "../../assets/wallpapers/default.jpg",
            ] {
                let path = dir.join(rel);
                if path.is_file() {
                    return path.canonicalize().ok();
                }
            }
        }
    }

    let bundled = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../assets/wallpapers/default.jpg");
    if bundled.is_file() {
        return bundled.canonicalize().ok();
    }

    None
}
