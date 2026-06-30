//! PipeWire video sources for ScreenCast sessions.

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use ashpd::PortalError;
use pipewire as pw;
use pw::{properties::properties, spa};
use spa::pod::Pod;

#[derive(Debug, Clone, Copy)]
pub struct StreamHandle {
    pub node_id: u32,
}

enum PwCommand {
    CreateStream {
        width: u32,
        height: u32,
        reply: Sender<Result<StreamHandle, String>>,
    },
    PushFrame {
        node_id: u32,
        pixels: Vec<u8>,
    },
    DestroyStream {
        node_id: u32,
    },
    Shutdown,
}

struct StreamUserData {
    frame: Arc<Mutex<Vec<u8>>>,
    stride: u32,
    height: u32,
}

struct StreamSlot {
    _stream: pw::stream::Stream,
    _listener: pw::stream::StreamListener<StreamUserData>,
    frame: Arc<Mutex<Vec<u8>>>,
}

pub struct PipeWireHub {
    cmd_tx: Sender<PwCommand>,
    _thread: JoinHandle<()>,
}

impl PipeWireHub {
    pub fn start() -> Result<Self, PortalError> {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("metis-pipewire".into())
            .spawn(move || pipewire_thread_main(cmd_rx))
            .map_err(|err| PortalError::Failed(format!("spawn pipewire thread: {err}")))?;

        Ok(Self {
            cmd_tx,
            _thread: thread,
        })
    }

    pub fn create_stream(&self, width: u32, height: u32) -> Result<StreamHandle, PortalError> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.cmd_tx
            .send(PwCommand::CreateStream {
                width,
                height,
                reply: reply_tx,
            })
            .map_err(|err| PortalError::Failed(format!("pipewire command send: {err}")))?;
        reply_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|err| PortalError::Failed(format!("pipewire create_stream timeout: {err}")))?
            .map_err(PortalError::Failed)
    }

    pub fn push_frame(&self, node_id: u32, pixels: Vec<u8>) -> Result<(), PortalError> {
        self.cmd_tx
            .send(PwCommand::PushFrame { node_id, pixels })
            .map_err(|err| PortalError::Failed(format!("pipewire push send: {err}")))
    }

    pub fn destroy_stream(&self, node_id: u32) {
        let _ = self.cmd_tx.send(PwCommand::DestroyStream { node_id });
    }
}

impl Drop for PipeWireHub {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(PwCommand::Shutdown);
    }
}

fn pipewire_thread_main(cmd_rx: Receiver<PwCommand>) {
    pw::init();

    let thread_loop = match unsafe { pw::thread_loop::ThreadLoop::new(Some("metis-pw"), None) } {
        Ok(t) => t,
        Err(err) => {
            tracing::error!(?err, "pipewire thread loop creation failed");
            return;
        }
    };
    thread_loop.start();

    let core = {
        let _lock = thread_loop.lock();
        let context = match pw::context::Context::new(&thread_loop) {
            Ok(c) => c,
            Err(err) => {
                tracing::error!(?err, "pipewire context creation failed");
                return;
            }
        };
        match context.connect(None) {
            Ok(c) => c,
            Err(err) => {
                tracing::error!(?err, "pipewire core connect failed");
                return;
            }
        }
    };

    let mut streams: HashMap<u32, StreamSlot> = HashMap::new();

    loop {
        let _lock = thread_loop.lock();
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                PwCommand::CreateStream {
                    width,
                    height,
                    reply,
                } => {
                    let _ = reply.send(
                        create_video_stream(&core, width, height).map(|(handle, slot)| {
                            streams.insert(handle.node_id, slot);
                            handle
                        }),
                    );
                }
                PwCommand::PushFrame { node_id, pixels } => {
                    if let Some(slot) = streams.get(&node_id) {
                        if let Ok(mut frame) = slot.frame.lock() {
                            *frame = pixels;
                        }
                    }
                }
                PwCommand::DestroyStream { node_id } => {
                    streams.remove(&node_id);
                }
                PwCommand::Shutdown => {
                    return;
                }
            }
        }
        thread_loop.loop_().iterate(Duration::from_millis(16));
    }
}

fn create_video_stream(
    core: &pw::core::Core,
    width: u32,
    height: u32,
) -> Result<(StreamHandle, StreamSlot), String> {
    let stride = width * 4;
    let frame = Arc::new(Mutex::new(vec![0u8; (stride * height) as usize]));
    let user_data = StreamUserData {
        frame: Arc::clone(&frame),
        stride,
        height,
    };

    let stream = pw::stream::Stream::new(
        core,
        "metis-screencast",
        properties! {
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Screen",
            *pw::keys::NODE_NAME => "metis-screencast",
        },
    )
    .map_err(|err| format!("create pipewire stream: {err}"))?;

    let frame_cb = Arc::clone(&frame);
    let listener = stream
        .add_local_listener_with_user_data(user_data)
        .process(move |stream, user_data| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let datas = buffer.datas_mut();
            if datas.is_empty() {
                return;
            }
            let data = &mut datas[0];
            let Some(slice) = data.data() else {
                return;
            };
            let needed = (user_data.stride * user_data.height) as usize;
            if slice.len() < needed {
                return;
            }
            if let Ok(frame) = frame_cb.lock() {
                let copy_len = needed.min(frame.len()).min(slice.len());
                slice[..copy_len].copy_from_slice(&frame[..copy_len]);
            }
            let chunk = data.chunk_mut();
            *chunk.offset_mut() = 0;
            *chunk.stride_mut() = user_data.stride as i32;
            *chunk.size_mut() = needed as u32;
        })
        .register()
        .map_err(|err| format!("register pipewire listener: {err}"))?;

    let pod_bytes = build_video_params(width, height)?;
    let pod = Pod::from_bytes(&pod_bytes).ok_or_else(|| "invalid pipewire format pod".to_string())?;
    let mut params = [pod];
    stream
        .connect(
            spa::utils::Direction::Output,
            None,
            pw::stream::StreamFlags::AUTOCONNECT
                | pw::stream::StreamFlags::MAP_BUFFERS
                | pw::stream::StreamFlags::RT_PROCESS,
            &mut params,
        )
        .map_err(|err| format!("connect pipewire stream: {err}"))?;

    let node_id = stream.node_id();
    tracing::info!(node_id, width, height, "registered PipeWire screencast stream");

    Ok((
        StreamHandle { node_id },
        StreamSlot {
            _stream: stream,
            _listener: listener,
            frame,
        },
    ))
}

fn build_video_params(width: u32, height: u32) -> Result<Vec<u8>, String> {
    let obj = pw::spa::pod::object!(
        pw::spa::utils::SpaTypes::ObjectParamFormat,
        pw::spa::param::ParamType::EnumFormat,
        pw::spa::pod::property!(
            pw::spa::param::format::FormatProperties::MediaType,
            Id,
            pw::spa::param::format::MediaType::Video
        ),
        pw::spa::pod::property!(
            pw::spa::param::format::FormatProperties::MediaSubtype,
            Id,
            pw::spa::param::format::MediaSubtype::Raw
        ),
        pw::spa::pod::property!(
            pw::spa::param::format::FormatProperties::VideoFormat,
            Id,
            pw::spa::param::video::VideoFormat::BGRx
        ),
        pw::spa::pod::property!(
            pw::spa::param::format::FormatProperties::VideoSize,
            Rectangle,
            pw::spa::utils::Rectangle { width, height }
        ),
        pw::spa::pod::property!(
            pw::spa::param::format::FormatProperties::VideoFramerate,
            Fraction,
            pw::spa::utils::Fraction { num: 30, denom: 1 }
        ),
    );
    Ok(pw::spa::pod::serialize::PodSerializer::serialize(
        Cursor::new(Vec::new()),
        &pw::spa::pod::Value::Object(obj),
    )
    .map_err(|err| format!("serialize video params: {err}"))?
    .0
    .into_inner())
}
