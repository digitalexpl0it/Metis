//! ext-image-copy-capture / ext-image-capture-source support for portal clients.
//!
//! Capture frames are queued from the Wayland protocol handler and fulfilled on
//! the next GL render pass when a [`GlesRenderer`] is available.

use std::time::Instant;

use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::damage::OutputDamageTracker;
use smithay::backend::renderer::gles::{GlesRenderer, GlesTexture};
use smithay::backend::renderer::{Bind, ExportMem, Offscreen, Texture};
use smithay::output::{Output, WeakOutput};
use smithay::reexports::wayland_server::protocol::{wl_buffer::WlBuffer, wl_shm};
use smithay::utils::{Buffer, Physical, Point, Rectangle, Scale, Size, Transform};
use smithay::wayland::image_capture_source::{
    ImageCaptureSource, ImageCaptureSourceHandler, ImageCaptureSourceState,
    OutputCaptureSourceHandler, OutputCaptureSourceState,
};
use smithay::wayland::image_copy_capture::{
    BufferConstraints, CaptureFailureReason, Frame, ImageCopyCaptureHandler,
    ImageCopyCaptureState, Session, SessionRef,
};
use smithay::wayland::shm::{with_buffer_contents, with_buffer_contents_mut, BufferAccessError};

use crate::render::CLEAR_COLOR;
use crate::state::MetisState;

/// A capture frame waiting for the compositor renderer.
pub(crate) struct PendingCapture {
    pub output: Output,
    pub draw_cursor: bool,
    pub frame: Frame,
}

pub struct ImageCaptureRuntime {
    pub image_capture_source: ImageCaptureSourceState,
    pub output_capture_source: OutputCaptureSourceState,
    pub image_copy_capture: ImageCopyCaptureState,
    /// Owned sessions — dropping a [`Session`] stops capture for the client.
    active_sessions: Vec<Session>,
    pub(crate) pending: Vec<PendingCapture>,
}

impl ImageCaptureRuntime {
    pub fn new(display: &smithay::reexports::wayland_server::DisplayHandle) -> Self {
        Self {
            image_capture_source: ImageCaptureSourceState::new(),
            output_capture_source: OutputCaptureSourceState::new::<MetisState>(display),
            image_copy_capture: ImageCopyCaptureState::new::<MetisState>(display),
            active_sessions: Vec::new(),
            pending: Vec::new(),
        }
    }

    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    pub(crate) fn take_pending(&mut self) -> Vec<PendingCapture> {
        std::mem::take(&mut self.pending)
    }

    pub fn cleanup(&mut self) {
        self.image_copy_capture.cleanup();
    }
}

impl ImageCaptureSourceHandler for MetisState {
    fn source_destroyed(&mut self, _source: ImageCaptureSource) {}
}

impl OutputCaptureSourceHandler for MetisState {
    fn output_capture_source_state(&mut self) -> &mut OutputCaptureSourceState {
        &mut self.image_capture.output_capture_source
    }

    fn output_source_created(&mut self, source: ImageCaptureSource, output: &Output) {
        source.user_data().insert_if_missing(|| output.downgrade());
    }
}

impl ImageCopyCaptureHandler for MetisState {
    fn image_copy_capture_state(&mut self) -> &mut ImageCopyCaptureState {
        &mut self.image_capture.image_copy_capture
    }

    fn capture_constraints(&mut self, source: &ImageCaptureSource) -> Option<BufferConstraints> {
        output_constraints(source)
    }

    fn new_session(&mut self, session: Session) {
        let source = session.source();
        if let Some(constraints) = self.capture_constraints(&source).filter(|_| source.alive()) {
            session.update_constraints(constraints);
        }
        self.image_capture.active_sessions.push(session);
    }

    fn session_destroyed(&mut self, session: SessionRef) {
        self.image_capture
            .active_sessions
            .retain(|active| *active != session);
    }

    fn frame(&mut self, session: &SessionRef, frame: Frame) {
        let Some(output) = output_for_source(&session.source()) else {
            frame.fail(CaptureFailureReason::Stopped);
            return;
        };
        self.image_capture.pending.push(PendingCapture {
            output,
            draw_cursor: session.draw_cursor(),
            frame,
        });
        self.damaged = true;
        self.request_redraw();
    }
}

fn output_for_source(source: &ImageCaptureSource) -> Option<Output> {
    source
        .user_data()
        .get::<WeakOutput>()
        .and_then(|weak| weak.upgrade())
}

fn output_constraints(source: &ImageCaptureSource) -> Option<BufferConstraints> {
    let output = output_for_source(source)?;
    let mode = output.current_mode()?;
    Some(BufferConstraints {
        size: mode
            .size
            .to_logical(1)
            .to_buffer(1, Transform::Normal),
        shm: vec![
            wl_shm::Format::Argb8888,
            wl_shm::Format::Xrgb8888,
            wl_shm::Format::Abgr8888,
            wl_shm::Format::Xbgr8888,
        ],
        dma: None,
    })
}

fn shm_to_fourcc(format: wl_shm::Format) -> Option<Fourcc> {
    match format {
        wl_shm::Format::Argb8888 => Some(Fourcc::Abgr8888),
        wl_shm::Format::Xrgb8888 => Some(Fourcc::Xbgr8888),
        wl_shm::Format::Abgr8888 => Some(Fourcc::Argb8888),
        wl_shm::Format::Xbgr8888 => Some(Fourcc::Xbgr8888),
        _ => None,
    }
}

pub(crate) fn render_output_to_buffer(
    state: &mut MetisState,
    renderer: &mut GlesRenderer,
    output: &Output,
    draw_cursor: bool,
    buffer: &WlBuffer,
) -> Result<Vec<Rectangle<i32, Buffer>>, CaptureFailureReason> {
    let (width, height, _stride, shm_format) = with_buffer_contents(buffer, |_, _, data| {
        (data.width, data.height, data.stride, data.format)
    })
    .map_err(map_buffer_error)?;

    if width <= 0 || height <= 0 {
        return Err(CaptureFailureReason::BufferConstraints);
    }

    let copy_format = shm_to_fourcc(shm_format).ok_or(CaptureFailureReason::BufferConstraints)?;
    let size_phys: Size<i32, Physical> = Size::from((width, height));
    let size_buf: Size<i32, Buffer> = Size::from((width, height));

    let output_scale = Scale::from(output.current_scale().fractional_scale());
    let render_origin: Point<i32, Physical> = state
        .space
        .output_geometry(output)
        .map(|g| g.loc.to_physical_precise_round(output_scale))
        .unwrap_or_default();

    let mut elements = state.build_render_elements(
        renderer,
        render_origin,
        output_scale,
        crate::night_light::RenderTargetInfo {
            size: size_phys,
            output_name: Some(output.name().as_str()),
        },
    );
    if draw_cursor {
        let mut cursor = state.build_cursor_elements(renderer, output, output_scale);
        if !cursor.is_empty() {
            cursor.append(&mut elements);
            elements = cursor;
        }
    }

    let mut offscreen =
        Offscreen::<GlesTexture>::create_buffer(renderer, copy_format, size_buf).map_err(|err| {
            tracing::warn!(?err, "capture offscreen buffer creation failed");
            CaptureFailureReason::Unknown
        })?;
    let mut framebuffer = renderer
        .bind(&mut offscreen)
        .map_err(|_| CaptureFailureReason::Unknown)?;

    let mut damage_tracker =
        OutputDamageTracker::new(size_phys, output_scale, Transform::Normal);
    if let Err(err) = damage_tracker.render_output(
        renderer,
        &mut framebuffer,
        0,
        &elements,
        CLEAR_COLOR,
    ) {
        tracing::warn!(?err, "capture render_output failed");
        return Err(CaptureFailureReason::Unknown);
    }

    let region = Rectangle::from_size(size_buf);
    let mapping = renderer
        .copy_framebuffer(&framebuffer, region, copy_format)
        .map_err(|_| CaptureFailureReason::Unknown)?;
    let map_size = mapping.size();
    let pixels = renderer
        .map_texture(&mapping)
        .map_err(|_| CaptureFailureReason::Unknown)?;

    let _ = with_buffer_contents_mut(buffer, |dst, dst_len, data| {
        if data.width != width || data.height != height {
            return Err(CaptureFailureReason::BufferConstraints);
        }
        copy_pixels_to_shm(
            pixels,
            map_size.w as usize,
            map_size.h as usize,
            dst,
            dst_len,
            data.stride as usize,
            data.width as usize,
            data.height as usize,
        )
    })
    .map_err(map_buffer_error)?;

    Ok(vec![region])
}

fn copy_pixels_to_shm(
    src: &[u8],
    src_stride_px: usize,
    height: usize,
    dst: *mut u8,
    dst_len: usize,
    dst_stride: usize,
    width: usize,
    buf_height: usize,
) -> Result<(), CaptureFailureReason> {
    let src_stride = src_stride_px * 4;
    let row_bytes = width * 4;
    if row_bytes > dst_stride || row_bytes > src_stride {
        return Err(CaptureFailureReason::BufferConstraints);
    }
    let needed = dst_stride.saturating_mul(buf_height);
    if needed > dst_len || src_stride.saturating_mul(height) > src.len() {
        return Err(CaptureFailureReason::BufferConstraints);
    }

    // SAFETY: bounds checked above; capture completes before the client mutates the buffer.
    unsafe {
        let dst_base = dst;
        for row in 0..height.min(buf_height) {
            let src_off = row * src_stride;
            let dst_off = row * dst_stride;
            if src_off + row_bytes > src.len() || dst_off + row_bytes > dst_len {
                return Err(CaptureFailureReason::BufferConstraints);
            }
            std::ptr::copy_nonoverlapping(
                src.as_ptr().add(src_off),
                dst_base.add(dst_off),
                row_bytes,
            );
        }
    }
    Ok(())
}

fn map_buffer_error(err: BufferAccessError) -> CaptureFailureReason {
    match err {
        BufferAccessError::NotManaged | BufferAccessError::BadMap => {
            CaptureFailureReason::BufferConstraints
        }
        BufferAccessError::NotReadable | BufferAccessError::NotWritable => {
            CaptureFailureReason::Unknown
        }
    }
}

pub(crate) fn finish_pending_captures(
    state: &mut MetisState,
    renderer: &mut GlesRenderer,
    start_time: Instant,
) {
    let pending = state.image_capture.take_pending();
    for job in pending {
        let buffer = job.frame.buffer();
        match render_output_to_buffer(
            state,
            renderer,
            &job.output,
            job.draw_cursor,
            &buffer,
        ) {
            Ok(damage) => {
                job.frame
                    .success(Transform::Normal, Some(damage), start_time.elapsed());
            }
            Err(reason) => job.frame.fail(reason),
        }
    }
    state.image_capture.cleanup();
}
