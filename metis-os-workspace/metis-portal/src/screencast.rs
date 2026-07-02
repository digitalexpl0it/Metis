use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use ashpd::{
    MaybeAppID, PortalError, WindowIdentifierType,
    backend::{
        request::RequestImpl,
        screencast::{ScreencastImpl, SelectSourcesResponse},
        session::{CreateSessionResponse, SessionImpl},
    },
    desktop::{
        CreateSessionOptions, HandleToken,
        screencast::{
            CursorMode, SelectSourcesOptions, SourceType, StartCastOptions,
            StreamBuilder, Streams, StreamsBuilder,
        },
    },
};
use async_trait::async_trait;
use enumflags2::BitFlags;

use crate::capture::{spawn_screencast_pump, CaptureHub};
use crate::compositor_ipc;
use crate::pipewire::PipeWireHub;

struct CastSession {
    streams: Vec<u32>,
    cancel: Arc<AtomicBool>,
    pump: Option<JoinHandle<()>>,
}

impl CastSession {
    fn stop(&mut self, pipewire: &PipeWireHub) {
        self.cancel.store(true, Ordering::Relaxed);
        if let Some(handle) = self.pump.take() {
            let _ = handle.join();
        }
        for node in self.streams.drain(..) {
            pipewire.destroy_stream(node);
        }
    }
}

pub struct MetisScreencast {
    capture: Arc<CaptureHub>,
    pipewire: Arc<PipeWireHub>,
    sessions: Mutex<HashMap<String, CastSession>>,
}

impl MetisScreencast {
    pub fn new(capture: Arc<CaptureHub>, pipewire: Arc<PipeWireHub>) -> Self {
        Self {
            capture,
            pipewire,
            sessions: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl RequestImpl for MetisScreencast {
    async fn close(&self, _token: HandleToken) {}
}

#[async_trait]
impl SessionImpl for MetisScreencast {
    async fn session_closed(&self, session_token: HandleToken) -> ashpd::backend::Result<()> {
        let key = session_token.to_string();
        if let Ok(mut sessions) = self.sessions.lock() {
            if let Some(mut session) = sessions.remove(&key) {
                session.stop(&self.pipewire);
            }
        }
        compositor_ipc::end_capture_overlay(None);
        Ok(())
    }
}

#[async_trait]
impl ScreencastImpl for MetisScreencast {
    fn available_source_types(&self) -> BitFlags<SourceType> {
        SourceType::Monitor.into()
    }

    fn available_cursor_mode(&self) -> BitFlags<CursorMode> {
        CursorMode::Hidden | CursorMode::Embedded | CursorMode::Metadata
    }

    async fn create_session(
        &self,
        _token: HandleToken,
        session_token: HandleToken,
        _app_id: Option<MaybeAppID>,
        _options: CreateSessionOptions,
    ) -> ashpd::backend::Result<CreateSessionResponse> {
        self.sessions
            .lock()
            .map_err(|err| PortalError::Failed(format!("session map lock: {err}")))?
            .insert(
                session_token.to_string(),
                CastSession {
                    streams: Vec::new(),
                    cancel: Arc::new(AtomicBool::new(false)),
                    pump: None,
                },
            );
        Ok(CreateSessionResponse::new(session_token))
    }

    async fn select_sources(
        &self,
        _session_token: HandleToken,
        _app_id: Option<MaybeAppID>,
        _options: SelectSourcesOptions,
    ) -> ashpd::backend::Result<SelectSourcesResponse> {
        Ok(SelectSourcesResponse {})
    }

    async fn start_cast(
        &self,
        session_token: HandleToken,
        app_id: Option<MaybeAppID>,
        _window_identifier: Option<WindowIdentifierType>,
        _options: StartCastOptions,
    ) -> ashpd::backend::Result<Streams> {
        tracing::info!(?app_id, "portal screencast start");
        compositor_ipc::begin_capture_overlay(compositor_ipc::portal_app_id(app_id));
        let (width, height) = self.capture.output_size().await;
        let stream = self
            .pipewire
            .create_stream(width, height)
            .map_err(|err| PortalError::Failed(format!("create PipeWire stream: {err}")))?;

        let paint_cursors = true;
        let cancel = Arc::new(AtomicBool::new(false));
        let pump = spawn_screencast_pump(
            Arc::clone(&self.pipewire),
            stream.node_id,
            paint_cursors,
            Arc::clone(&cancel),
        );

        if let Ok(mut sessions) = self.sessions.lock() {
            if let Some(session) = sessions.get_mut(&session_token.to_string()) {
                session.streams.push(stream.node_id);
                session.cancel = cancel;
                session.pump = Some(pump);
            }
        }

        let pw_stream = StreamBuilder::new(stream.node_id)
            .source_type(SourceType::Monitor)
            .size((width as i32, height as i32))
            .position((0, 0))
            .mapping_id("metis:0".to_string())
            .build();

        Ok(StreamsBuilder::new(vec![pw_stream]).build())
    }
}
