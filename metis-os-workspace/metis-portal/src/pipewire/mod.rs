//! PipeWire video sources for ScreenCast sessions.

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use ashpd::PortalError;
use pipewire as pw;
use pw::{properties::properties, spa};
use spa::param::video::VideoInfoRaw;
use spa::pod::Pod;

#[derive(Debug, Clone, Copy)]
pub struct StreamHandle {
    pub node_id: u32,
}

enum PwCommand {
    CreateStream {
        width: u32,
        height: u32,
        mapping_id: String,
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

/// Pixel buffer + negotiated geometry shared between the stream listener and
/// the command loop (PushFrame flushes memfd buffers on the PipeWire thread).
struct StreamSharedState {
    frame: Mutex<Vec<u8>>,
    width: Mutex<u32>,
    height: Mutex<u32>,
    stride: Mutex<i32>,
    seq: Mutex<u32>,
    frames_queued: Mutex<u64>,
    process_calls: AtomicU64,
}

struct StreamUserData {
    shared: Arc<StreamSharedState>,
    format: VideoInfoRaw,
}

struct StreamSlot {
    stream: pw::stream::Stream,
    _listener: pw::stream::StreamListener<StreamUserData>,
    shared: Arc<StreamSharedState>,
}

struct PendingStream {
    slot: StreamSlot,
    reply: Sender<Result<StreamHandle, String>>,
    started: std::time::Instant,
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
        self.create_stream_with_mapping(width, height, "metis:0")
    }

    pub fn create_stream_with_mapping(
        &self,
        width: u32,
        height: u32,
        mapping_id: &str,
    ) -> Result<StreamHandle, PortalError> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.cmd_tx
            .send(PwCommand::CreateStream {
                width,
                height,
                mapping_id: mapping_id.to_string(),
                reply: reply_tx,
            })
            .map_err(|err| PortalError::Failed(format!("pipewire command send: {err}")))?;
        reply_rx
            .recv_timeout(Duration::from_secs(10))
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

    let mainloop = match pw::main_loop::MainLoop::new(None) {
        Ok(m) => m,
        Err(err) => {
            tracing::error!(?err, "pipewire main loop creation failed");
            return;
        }
    };

    let core = {
        let context = match pw::context::Context::new(&mainloop) {
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
    let mut pending: Vec<PendingStream> = Vec::new();
    let mut running = true;

    while running {
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                PwCommand::CreateStream {
                    width,
                    height,
                    mapping_id,
                    reply,
                } => match create_video_stream(&core, width, height, &mapping_id) {
                    Ok(slot) => pending.push(PendingStream {
                        slot,
                        reply,
                        started: std::time::Instant::now(),
                    }),
                    Err(err) => {
                        let _ = reply.send(Err(err));
                    }
                },
                PwCommand::PushFrame { node_id, pixels } => {
                    if let Some(slot) = streams.get(&node_id) {
                        if let Ok(mut frame) = slot.shared.frame.lock() {
                            *frame = pixels;
                        }
                        let _ = slot.stream.set_active(true);
                        if let Err(err) = slot.stream.trigger_process() {
                            tracing::warn!(%err, node_id, "pipewire trigger_process failed");
                        }
                    }
                }
                PwCommand::DestroyStream { node_id } => {
                    streams.remove(&node_id);
                }
                PwCommand::Shutdown => {
                    running = false;
                }
            }
        }

        let mut i = 0;
        while i < pending.len() {
            let node_id = pending[i].slot.stream.node_id();
            if node_id != pw::constants::ID_ANY {
                let pending_stream = pending.remove(i);
                tracing::info!(node_id, "pipewire screencast node ready");
                let _ = pending_stream
                    .reply
                    .send(Ok(StreamHandle { node_id }));
                streams.insert(node_id, pending_stream.slot);
                continue;
            }
            if pending[i].started.elapsed() > Duration::from_secs(5) {
                let pending_stream = pending.remove(i);
                let _ = pending_stream.reply.send(Err(
                    "pipewire stream did not receive a node id".into(),
                ));
                continue;
            }
            i += 1;
        }

        // Drive PipeWire on this thread so process/param callbacks run here too.
        mainloop.loop_().iterate(Duration::from_millis(16));
    }

    mainloop.quit();
}

fn create_video_stream(
    core: &pw::core::Core,
    width: u32,
    height: u32,
    mapping_id: &str,
) -> Result<StreamSlot, String> {
    let stride = (width * 4) as i32;
    let shared = Arc::new(StreamSharedState {
        frame: Mutex::new(vec![0u8; (width * height * 4) as usize]),
        width: Mutex::new(width),
        height: Mutex::new(height),
        stride: Mutex::new(stride),
        seq: Mutex::new(0),
        frames_queued: Mutex::new(0),
        process_calls: AtomicU64::new(0),
    });
    let user_data = StreamUserData {
        shared: Arc::clone(&shared),
        format: VideoInfoRaw::default(),
    };

    let stream = pw::stream::Stream::new(
        core,
        "metis-screencast",
        properties! {
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Screen",
            *pw::keys::NODE_NAME => "metis-screencast",
            "pipewire.tag.mapping-id" => mapping_id,
        },
    )
    .map_err(|err| format!("create pipewire stream: {err}"))?;

    let listener = stream
        .add_local_listener_with_user_data(user_data)
        .param_changed(|stream, user_data, id, param| {
            let Some(param) = param else {
                return;
            };
            if id != pw::spa::param::ParamType::Format.as_raw() {
                return;
            }

            let Ok((media_type, media_subtype)) =
                pw::spa::param::format_utils::parse_format(param)
            else {
                return;
            };
            if media_type != pw::spa::param::format::MediaType::Video
                || media_subtype != pw::spa::param::format::MediaSubtype::Raw
            {
                return;
            }

            let mut peer = VideoInfoRaw::default();
            if peer.parse(param).is_err() {
                return;
            }

            let width = if peer.size().width > 0 {
                peer.size().width
            } else {
                *user_data.shared.width.lock().unwrap_or_else(|e| e.into_inner())
            };
            let height = if peer.size().height > 0 {
                peer.size().height
            } else {
                *user_data.shared.height.lock().unwrap_or_else(|e| e.into_inner())
            };
            let stride = (width * 4) as i32;

            user_data.format = negotiated_format(width, height, peer.format());
            *user_data.shared.width.lock().unwrap_or_else(|e| e.into_inner()) = width;
            *user_data.shared.height.lock().unwrap_or_else(|e| e.into_inner()) = height;
            *user_data.shared.stride.lock().unwrap_or_else(|e| e.into_inner()) = stride;

            let Ok(param_bytes) = build_buffer_params(stride, height) else {
                tracing::warn!("pipewire failed to build buffer params");
                return;
            };
            let mut pod_refs: Vec<&Pod> = param_bytes
                .iter()
                .filter_map(|bytes| Pod::from_bytes(bytes))
                .collect();
            if pod_refs.is_empty() {
                tracing::warn!("pipewire buffer param pods empty");
                return;
            }
            match stream.update_params(&mut pod_refs) {
                Ok(()) => {
                    tracing::info!(width, height, stride, "pipewire screencast buffers negotiated");
                }
                Err(err) => tracing::warn!(%err, "pipewire update_params failed"),
            }

            if let Ok(mut buf) = user_data.shared.frame.lock() {
                buf.resize((width * height * 4) as usize, 0);
            }
        })
        .add_buffer(|_stream, user_data, pw_buf| {
            let stride = *user_data
                .shared
                .stride
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let height = *user_data
                .shared
                .height
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if let Err(err) = unsafe { alloc_memfd_buffer(pw_buf, stride, height) } {
                tracing::error!(%err, "pipewire add_buffer failed");
            }
        })
        .remove_buffer(|_stream, _user_data, pw_buf| {
            unsafe {
                free_memfd_buffer(pw_buf);
            }
        })
        .process(|stream, user_data| {
            process_output_buffer(stream, &user_data.shared);
        })
        .state_changed(|stream, _user_data, _old, new| {
            tracing::info!(?new, "pipewire screencast stream state changed");
            if matches!(new, pw::stream::StreamState::Streaming) {
                if let Err(err) = stream.set_active(true) {
                    tracing::warn!(%err, "pipewire set_active(true) on Streaming failed");
                }
            }
        })
        .register()
        .map_err(|err| format!("register pipewire listener: {err}"))?;

    let mut format_pods = build_enum_formats(width, height)?;
    let mut params: Vec<&Pod> = Vec::with_capacity(format_pods.len());
    for bytes in &format_pods {
        let pod = Pod::from_bytes(bytes)
            .ok_or_else(|| "invalid pipewire format pod".to_string())?;
        params.push(pod);
    }
    stream
        .connect(
            spa::utils::Direction::Output,
            None,
            pw::stream::StreamFlags::DRIVER | pw::stream::StreamFlags::ALLOC_BUFFERS,
            &mut params,
        )
        .map_err(|err| format!("connect pipewire stream: {err}"))?;

    Ok(StreamSlot {
        stream,
        _listener: listener,
        shared,
    })
}

/// Allocate a memfd-backed plane for each PipeWire buffer (required by gnome-remote-desktop).
unsafe fn alloc_memfd_buffer(
    pw_buf: *mut pw::sys::pw_buffer,
    stride: i32,
    height: u32,
) -> Result<(), String> {
    let spa_buf = (*pw_buf).buffer;
    if spa_buf.is_null() {
        return Err("null spa buffer".into());
    }
    let spa = &mut *spa_buf;
    if spa.n_datas == 0 || spa.datas.is_null() {
        return Err("spa buffer has no datas".into());
    }
    let d = &mut *spa.datas;

    let memfd_bit = 1u32 << spa::sys::SPA_DATA_MemFd;
    if d.type_ & memfd_bit == 0 {
        return Err(format!(
            "peer rejected MemFd buffers (type mask 0x{:x})",
            d.type_
        ));
    }

    let maxsize = (stride as u32).saturating_mul(height);
    if maxsize == 0 {
        return Err("zero screencast buffer size".into());
    }

    let fd = libc::memfd_create(
        b"metis-screencast\0".as_ptr() as *const libc::c_char,
        (libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING) as u32,
    );
    if fd < 0 {
        return Err(format!(
            "memfd_create failed: {}",
            std::io::Error::last_os_error()
        ));
    }

    if libc::ftruncate(fd, maxsize as libc::off_t) < 0 {
        libc::close(fd);
        return Err(format!(
            "ftruncate failed: {}",
            std::io::Error::last_os_error()
        ));
    }

    let ptr = libc::mmap(
        std::ptr::null_mut(),
        maxsize as usize,
        libc::PROT_READ | libc::PROT_WRITE,
        libc::MAP_SHARED,
        fd,
        0,
    );
    if ptr == libc::MAP_FAILED {
        libc::close(fd);
        return Err(format!("mmap failed: {}", std::io::Error::last_os_error()));
    }

    d.type_ = spa::sys::SPA_DATA_MemFd as u32;
    d.flags = spa::sys::SPA_DATA_FLAG_READWRITE | spa::sys::SPA_DATA_FLAG_MAPPABLE;
    d.fd = fd as i64;
    d.mapoffset = 0;
    d.maxsize = maxsize;
    d.data = ptr as *mut _;

    if !d.chunk.is_null() {
        (*d.chunk).offset = 0;
        (*d.chunk).size = 0;
        (*d.chunk).stride = stride;
        (*d.chunk).flags = 0;
    }

    Ok(())
}

unsafe fn free_memfd_buffer(pw_buf: *mut pw::sys::pw_buffer) {
    let spa_buf = (*pw_buf).buffer;
    if spa_buf.is_null() {
        return;
    }
    let spa = &*spa_buf;
    if spa.n_datas == 0 || spa.datas.is_null() {
        return;
    }
    let d = &*spa.datas;
    if !d.data.is_null() && d.maxsize > 0 {
        libc::munmap(d.data as *mut _, d.maxsize as usize);
    }
    if d.fd >= 0 {
        libc::close(d.fd as libc::c_int);
    }
}

unsafe fn fill_screencast_frame(
    spa_buf: *mut spa::sys::spa_buffer,
    state: &StreamSharedState,
) -> bool {
    let spa = &mut *spa_buf;
    if spa.n_datas == 0 || spa.datas.is_null() {
        return false;
    }
    let d = &mut *spa.datas;
    if d.data.is_null() || d.maxsize == 0 {
        return false;
    }

    let height = *state.height.lock().unwrap_or_else(|e| e.into_inner());
    let stride = *state.stride.lock().unwrap_or_else(|e| e.into_inner());
    let needed = (stride as u32).saturating_mul(height) as usize;
    let slice = std::slice::from_raw_parts_mut(d.data as *mut u8, needed.min(d.maxsize as usize));

    let frame = state.frame.lock().unwrap_or_else(|e| e.into_inner());
    if frame.is_empty() || frame.iter().all(|&b| b == 0) {
        if !d.chunk.is_null() {
            (*d.chunk).offset = 0;
            (*d.chunk).stride = stride;
            (*d.chunk).size = 0;
            (*d.chunk).flags = 0;
        }
        return false;
    }

    let copy_len = needed.min(frame.len()).min(slice.len());
    slice[..copy_len].copy_from_slice(&frame[..copy_len]);

    if !d.chunk.is_null() {
        (*d.chunk).offset = 0;
        (*d.chunk).stride = stride;
        // Match Mutter: report the full plane size once pixels are ready.
        (*d.chunk).size = d.maxsize;
        (*d.chunk).flags = 0;
    }
    true
}

#[repr(C)]
struct SpaMetaHeader {
    flags: u32,
    offset: i64,
    pts: i64,
    seq: u32,
    dts_offset: i32,
}

unsafe fn set_header_meta(spa_buf: *mut spa::sys::spa_buffer, state: &StreamSharedState) {
    let buf = &*spa_buf;
    for i in 0..buf.n_metas {
        let meta = &*buf.metas.add(i as usize);
        if meta.type_ == spa::sys::SPA_META_Header as u32 && !meta.data.is_null() {
            let header = &mut *(meta.data as *mut SpaMetaHeader);
            header.flags = 0;
            header.offset = 0;
            let mut seq = state.seq.lock().unwrap_or_else(|e| e.into_inner());
            // Mutter uses capture timestamps in microseconds × SPA_NSEC_PER_USEC.
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default();
            header.pts = now.as_micros() as i64 * 1000;
            header.dts_offset = 0;
            header.seq = *seq;
            *seq = seq.saturating_add(1);
            return;
        }
    }
}

/// Fill one dequeued output buffer and return it to PipeWire (process callback only).
fn process_output_buffer(stream: &pw::stream::StreamRef, state: &StreamSharedState) {
    let call = state.process_calls.fetch_add(1, Ordering::Relaxed) + 1;
    if call == 1 {
        tracing::info!("pipewire screencast process callback invoked");
    }

    let pw_buf = unsafe { stream.dequeue_raw_buffer() };
    if pw_buf.is_null() {
        if call <= 3 {
            tracing::warn!(call, "pipewire screencast process: no buffer available to dequeue");
        }
        return;
    }
    let spa_buf = unsafe { (*pw_buf).buffer };
    if !spa_buf.is_null() {
        let filled = unsafe { fill_screencast_frame(spa_buf, state) };
        if filled {
            unsafe { set_header_meta(spa_buf, state) };
            let mut total = state.frames_queued.lock().unwrap_or_else(|e| e.into_inner());
            let first = *total == 0;
            *total += 1;
            if first {
                let width = *state.width.lock().unwrap_or_else(|e| e.into_inner());
                let height = *state.height.lock().unwrap_or_else(|e| e.into_inner());
                tracing::info!(
                    width,
                    height,
                    "pipewire screencast first buffer queued to GRD"
                );
            }
        } else if call <= 5 {
            tracing::warn!(call, "pipewire screencast process: frame not ready yet");
        }
    }
    unsafe { stream.queue_raw_buffer(pw_buf) };
}

fn negotiated_format(
    width: u32,
    height: u32,
    format: pw::spa::param::video::VideoFormat,
) -> VideoInfoRaw {
    let mut info = VideoInfoRaw::default();
    info.set_format(format);
    info.set_modifier(0);
    info.set_size(pw::spa::utils::Rectangle { width, height });
    info.set_framerate(pw::spa::utils::Fraction { num: 0, denom: 1 });
    info.set_max_framerate(pw::spa::utils::Fraction { num: 30, denom: 1 });
    info
}

/// Buffer allocation + metadata params sent after the peer fixates video format.
fn build_buffer_params(stride: i32, height: u32) -> Result<Vec<Vec<u8>>, String> {
    use spa::pod::{ChoiceValue, Property, Value};
    use spa::utils::{Choice, ChoiceEnum, ChoiceFlags, Id};

    let buffer_size = (stride as u32).saturating_mul(height);
    let memfd_mask = 1i32 << spa::sys::SPA_DATA_MemFd;

    let buffers = spa::pod::Object {
        type_: spa::utils::SpaTypes::ObjectParamBuffers.as_raw(),
        id: spa::param::ParamType::Buffers.as_raw(),
        properties: vec![
            Property::new(
                spa::sys::SPA_PARAM_BUFFERS_buffers,
                Value::Choice(ChoiceValue::Int(Choice(
                    ChoiceFlags::empty(),
                    ChoiceEnum::Range {
                        default: 8,
                        min: 2,
                        max: 16,
                    },
                ))),
            ),
            Property::new(spa::sys::SPA_PARAM_BUFFERS_blocks, Value::Int(1)),
            Property::new(
                spa::sys::SPA_PARAM_BUFFERS_size,
                Value::Int(buffer_size as i32),
            ),
            Property::new(spa::sys::SPA_PARAM_BUFFERS_stride, Value::Int(stride)),
            Property::new(
                spa::sys::SPA_PARAM_BUFFERS_dataType,
                Value::Choice(ChoiceValue::Int(Choice(
                    ChoiceFlags::empty(),
                    ChoiceEnum::Flags {
                        default: memfd_mask,
                        flags: vec![memfd_mask],
                    },
                ))),
            ),
        ],
    };

    let header_meta = spa::pod::Object {
        type_: spa::utils::SpaTypes::ObjectParamMeta.as_raw(),
        id: spa::param::ParamType::Meta.as_raw(),
        properties: vec![
            Property::new(
                spa::sys::SPA_PARAM_META_type,
                Value::Id(Id(spa::sys::SPA_META_Header as u32)),
            ),
            Property::new(
                spa::sys::SPA_PARAM_META_size,
                Value::Int(std::mem::size_of::<SpaMetaHeader>() as i32),
            ),
        ],
    };

    let cursor_meta_bytes = cursor_meta_size(CURSOR_BITMAP_MAX, CURSOR_BITMAP_MAX);
    let cursor_meta = spa::pod::Object {
        type_: spa::utils::SpaTypes::ObjectParamMeta.as_raw(),
        id: spa::param::ParamType::Meta.as_raw(),
        properties: vec![
            Property::new(
                spa::sys::SPA_PARAM_META_type,
                Value::Id(Id(spa::sys::SPA_META_Cursor as u32)),
            ),
            Property::new(
                spa::sys::SPA_PARAM_META_size,
                Value::Choice(ChoiceValue::Int(Choice(
                    ChoiceFlags::empty(),
                    ChoiceEnum::Range {
                        default: cursor_meta_bytes,
                        min: cursor_meta_size(1, 1),
                        max: cursor_meta_bytes,
                    },
                ))),
            ),
        ],
    };

    Ok(vec![
        serialize_pod(buffers)?,
        serialize_pod(header_meta)?,
        serialize_pod(cursor_meta)?,
    ])
}

const CURSOR_BITMAP_MAX: u32 = 384;

fn cursor_meta_size(width: u32, height: u32) -> i32 {
    std::mem::size_of::<spa::sys::spa_meta_cursor>() as i32
        + std::mem::size_of::<spa::sys::spa_meta_bitmap>() as i32
        + (width * height * 4) as i32
}

/// Match gnome-remote-desktop / Mutter: advertise raw BGRx (+ BGRA fallback)
/// at the fixed monitor size so PipeWire can fixate during link setup.
fn build_enum_formats(width: u32, height: u32) -> Result<Vec<Vec<u8>>, String> {
    let size = pw::spa::utils::Rectangle { width, height };
    let framerate = pw::spa::utils::Fraction { num: 0, denom: 1 };
    let max_fps = pw::spa::utils::Fraction { num: 30, denom: 1 };
    let min_fps = pw::spa::utils::Fraction { num: 1, denom: 1 };

    let mut out = Vec::new();
    for format in [
        pw::spa::param::video::VideoFormat::BGRx,
        pw::spa::param::video::VideoFormat::BGRA,
    ] {
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
                format
            ),
            pw::spa::pod::property!(
                pw::spa::param::format::FormatProperties::VideoSize,
                Choice,
                Range,
                Rectangle,
                size,
                size,
                size
            ),
            pw::spa::pod::property!(
                pw::spa::param::format::FormatProperties::VideoFramerate,
                Fraction,
                framerate
            ),
            pw::spa::pod::property!(
                pw::spa::param::format::FormatProperties::VideoMaxFramerate,
                Choice,
                Range,
                Fraction,
                max_fps,
                min_fps,
                max_fps
            ),
        );
        out.push(serialize_pod(obj)?);
    }
    Ok(out)
}

fn serialize_pod(obj: pw::spa::pod::Object) -> Result<Vec<u8>, String> {
    Ok(
        pw::spa::pod::serialize::PodSerializer::serialize(
            Cursor::new(Vec::new()),
            &pw::spa::pod::Value::Object(obj),
        )
        .map_err(|err| format!("serialize pod: {err}"))?
        .0
        .into_inner(),
    )
}
