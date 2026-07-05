//! Persistent ext-image-copy-capture session for live ScreenCast frames.

use std::time::{Duration, Instant};

use wayland_client::{
    globals::{registry_queue_init, GlobalListContents},
    protocol::{wl_output::WlOutput, wl_registry::WlRegistry, wl_shm::Format, wl_shm::WlShm},
    Connection, Dispatch, QueueHandle, WEnum,
};
use wayland_protocols::ext::{
    image_capture_source::v1::client::{
        ext_image_capture_source_v1::ExtImageCaptureSourceV1,
        ext_output_image_capture_source_manager_v1::ExtOutputImageCaptureSourceManagerV1,
    },
    image_copy_capture::v1::client::{
        ext_image_copy_capture_frame_v1::{self, ExtImageCopyCaptureFrameV1},
        ext_image_copy_capture_manager_v1::{self, ExtImageCopyCaptureManagerV1},
        ext_image_copy_capture_session_v1::{self, ExtImageCopyCaptureSessionV1},
    },
};

use super::shm::{BufferFormat, ShmBuffer};
use super::wayland::{prefer_shm_format, Frame};

enum CaptureMode {
    OneShot,
    Continuous,
}

struct SessionState {
    constraints: BufferFormat,
    needs_allocate: bool,
    frame_ready: bool,
    shm: Option<ShmBuffer>,
    _source: ExtImageCaptureSourceV1,
    session: ExtImageCopyCaptureSessionV1,
    frame_pending: bool,
}

struct AppState {
    shm: Option<WlShm>,
    copy_manager: Option<ExtImageCopyCaptureManagerV1>,
    source_manager: Option<ExtOutputImageCaptureSourceManagerV1>,
    output: Option<WlOutput>,
    session: Option<SessionState>,
    result: Option<Result<Frame, String>>,
    mode: CaptureMode,
    paint_cursors: bool,
}

impl AppState {
    fn fail(&mut self, msg: impl Into<String>) {
        if self.result.is_none() {
            self.result = Some(Err(msg.into()));
        }
        if let Some(session) = self.session.take() {
            session.session.destroy();
        }
    }

    fn start_capture(&mut self, qh: &QueueHandle<Self>) {
        let Some(output) = self.output.clone() else {
            self.fail("no wl_output");
            return;
        };
        let Some(source_manager) = self.source_manager.as_ref() else {
            self.fail("ext output capture source manager missing");
            return;
        };
        let Some(copy_manager) = self.copy_manager.as_ref() else {
            self.fail("ext image copy capture manager missing");
            return;
        };

        let source = source_manager.create_source(&output, qh, ());
        let options = if self.paint_cursors {
            ext_image_copy_capture_manager_v1::Options::PaintCursors
        } else {
            ext_image_copy_capture_manager_v1::Options::empty()
        };
        let session = copy_manager.create_session(&source, options, qh, ());

        self.session = Some(SessionState {
            constraints: BufferFormat {
                format: Format::Argb8888,
                width: 0,
                height: 0,
                stride: 0,
            },
            needs_allocate: false,
            frame_ready: false,
            shm: None,
            _source: source,
            session,
            frame_pending: false,
        });
    }

    fn on_session_done(&mut self, qh: &QueueHandle<Self>) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        if session.constraints.width == 0 || session.constraints.height == 0 {
            self.fail("invalid capture buffer size from compositor");
            return;
        }
        if session.constraints.stride == 0 {
            session.constraints.stride = session.constraints.width * 4;
        }

        let Some(shm_global) = self.shm.as_ref() else {
            self.fail("wl_shm missing");
            return;
        };

        if session.shm.is_none() {
            match ShmBuffer::new(shm_global, qh, session.constraints) {
                Ok(buf) => session.shm = Some(buf),
                Err(err) => {
                    self.fail(err);
                    return;
                }
            }
        }

        self.request_frame(qh);
    }

    fn request_frame(&mut self, qh: &QueueHandle<Self>) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        if session.frame_pending {
            return;
        }
        let Some(shm) = session.shm.as_ref() else {
            return;
        };
        let frame = session.session.create_frame(qh, ());
        frame.attach_buffer(&shm.buffer);
        frame.damage_buffer(
            0,
            0,
            session.constraints.width as i32,
            session.constraints.height as i32,
        );
        frame.capture();
        session.frame_pending = true;
    }

    fn on_frame_ready(&mut self) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        let Some(shm) = session.shm.as_ref() else {
            self.fail("missing shm buffer on frame ready");
            return;
        };
        let frame = Frame {
            width: shm.format.width,
            height: shm.format.height,
            stride: shm.format.stride,
            shm_format: shm.format.format,
            data: shm.pixels().to_vec(),
        };
        if matches!(self.mode, CaptureMode::OneShot) {
            let session = self.session.take().unwrap();
            session.session.destroy();
            self.result = Some(Ok(frame));
            return;
        }
        session.frame_pending = false;
        self.result = Some(Ok(frame));
    }

    fn tick(&mut self, qh: &QueueHandle<Self>) {
        if self.result.is_some() {
            return;
        }
        let needs_allocate = self
            .session
            .as_ref()
            .is_some_and(|session| session.needs_allocate);
        let frame_ready = self
            .session
            .as_ref()
            .is_some_and(|session| session.frame_ready);

        if needs_allocate {
            if let Some(session) = self.session.as_mut() {
                session.needs_allocate = false;
            }
            self.on_session_done(qh);
        }
        if frame_ready {
            if let Some(session) = self.session.as_mut() {
                session.frame_ready = false;
            }
            self.on_frame_ready();
        }
    }
}

/// Live capture handle — keeps an ext-image-copy session open across frames.
pub struct CaptureSession {
    conn: Connection,
    queue: wayland_client::EventQueue<AppState>,
    state: AppState,
}

impl CaptureSession {
    pub fn open(paint_cursors: bool) -> Result<Self, String> {
        let conn = Connection::connect_to_env()
            .map_err(|err| format!("connect to WAYLAND_DISPLAY: {err}"))?;
        let (globals, queue) =
            registry_queue_init::<AppState>(&conn).map_err(|err| format!("registry init: {err}"))?;
        let qh = queue.handle();

        let shm = globals.bind(&qh, 1..=1, ()).ok();
        let copy_manager = globals.bind(&qh, 1..=1, ()).ok();
        let source_manager = globals.bind(&qh, 1..=1, ()).ok();
        let output = globals.bind(&qh, 1..=4, ()).ok();

        let mut state = AppState {
            shm,
            copy_manager,
            source_manager,
            output,
            session: None,
            result: None,
            mode: CaptureMode::Continuous,
            paint_cursors,
        };

        if state.copy_manager.is_none() || state.source_manager.is_none() {
            return Err(
                "compositor does not expose ext-image-copy-capture (rebuild metis-compositor)"
                    .into(),
            );
        }

        state.start_capture(&qh);

        let mut session = Self { conn, queue, state };
        session.wait_until_ready(Duration::from_secs(8))?;
        Ok(session)
    }

    fn wait_until_ready(&mut self, timeout: Duration) -> Result<(), String> {
        let deadline = Instant::now() + timeout;
        while self.state.session.is_none() && Instant::now() < deadline {
            self.queue
                .blocking_dispatch(&mut self.state)
                .map_err(|err| format!("wayland dispatch: {err}"))?;
            self.state.tick(&self.queue.handle());
            if self.state.result.is_some() {
                break;
            }
        }
        if self.state.session.is_some() {
            Ok(())
        } else {
            self.state
                .result
                .take()
                .unwrap_or(Err("capture session setup timed out".into()))
                .map(|_| ())
        }
    }

    pub fn capture_next_frame(&mut self) -> Result<Frame, String> {
        self.state.result = None;
        let qh = self.queue.handle();
        self.state.request_frame(&qh);

        let deadline = Instant::now() + Duration::from_millis(500);
        while self.state.result.is_none() && Instant::now() < deadline {
            self.queue
                .blocking_dispatch(&mut self.state)
                .map_err(|err| format!("wayland dispatch: {err}"))?;
            self.state.tick(&qh);
        }

        match self.state.result.take() {
            Some(result) => result,
            None => Err("capture frame timed out".into()),
        }
    }
}

impl Drop for CaptureSession {
    fn drop(&mut self) {
        if let Some(session) = self.state.session.take() {
            session.session.destroy();
        }
    }
}

fn prefer_shm_format_local(current: Format, offered: Format) -> Format {
    prefer_shm_format(current, offered)
}

impl Dispatch<WlRegistry, GlobalListContents> for AppState {
    fn event(
        _state: &mut Self,
        _registry: &WlRegistry,
        _event: <WlRegistry as wayland_client::Proxy>::Event,
        _data: &GlobalListContents,
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
    }
}

wayland_client::delegate_noop!(AppState: ignore WlShm);
wayland_client::delegate_noop!(AppState: ignore WlOutput);
wayland_client::delegate_noop!(AppState: ignore ExtOutputImageCaptureSourceManagerV1);
wayland_client::delegate_noop!(AppState: ignore ExtImageCopyCaptureManagerV1);
wayland_client::delegate_noop!(AppState: ignore ExtImageCaptureSourceV1);
wayland_client::delegate_noop!(AppState: ignore wayland_client::protocol::wl_shm_pool::WlShmPool);
wayland_client::delegate_noop!(AppState: ignore wayland_client::protocol::wl_buffer::WlBuffer);

impl Dispatch<ExtImageCopyCaptureSessionV1, ()> for AppState {
    fn event(
        state: &mut Self,
        _proxy: &ExtImageCopyCaptureSessionV1,
        event: <ExtImageCopyCaptureSessionV1 as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        qhandle: &QueueHandle<Self>,
    ) {
        let Some(session) = state.session.as_mut() else {
            return;
        };
        match event {
            ext_image_copy_capture_session_v1::Event::BufferSize { width, height, .. } => {
                session.constraints.width = width;
                session.constraints.height = height;
            }
            ext_image_copy_capture_session_v1::Event::ShmFormat { format, .. } => {
                if let WEnum::Value(fmt) = format {
                    session.constraints.format =
                        prefer_shm_format_local(session.constraints.format, fmt);
                }
            }
            ext_image_copy_capture_session_v1::Event::Done { .. } => {
                session.needs_allocate = true;
            }
            ext_image_copy_capture_session_v1::Event::Stopped { .. } => {
                state.fail("capture session stopped");
            }
            _ => {}
        }
        if state.result.is_none()
            && state.session.as_ref().is_some_and(|session| session.needs_allocate)
        {
            if let Some(session) = state.session.as_mut() {
                session.needs_allocate = false;
            }
            state.on_session_done(qhandle);
        }
    }
}

impl Dispatch<ExtImageCopyCaptureFrameV1, ()> for AppState {
    fn event(
        state: &mut Self,
        _proxy: &ExtImageCopyCaptureFrameV1,
        event: <ExtImageCopyCaptureFrameV1 as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        match event {
            ext_image_copy_capture_frame_v1::Event::Ready { .. } => {
                if let Some(session) = state.session.as_mut() {
                    session.frame_pending = false;
                    session.frame_ready = true;
                }
            }
            ext_image_copy_capture_frame_v1::Event::Failed { .. } => {
                state.fail("capture frame failed");
            }
            _ => {}
        }
        if state.result.is_none()
            && state.session.as_ref().is_some_and(|session| session.frame_ready)
        {
            if let Some(session) = state.session.as_mut() {
                session.frame_ready = false;
            }
            state.on_frame_ready();
        }
    }
}
