//! Standalone DRM/KMS + libseat + libinput backend.
//!
//! Runs Metis directly on the GPU/TTY as its own session. Stage C establishes
//! the render path on the primary GPU's [`GlesRenderer`]: open the DRM device via
//! libseat, create a [`DrmOutputManager`], bring up one [`DrmOutput`] per
//! connected output, and drive damage-gated frames off the vblank. Input
//! (Stage D), the hardware cursor (Stage E), hotplug (Stage F) and full
//! multi-GPU (Stage G) build on this.

use std::collections::HashMap;
use std::time::Duration;

use input::DeviceCapability;
use smithay::{
    backend::{
        allocator::{
            format::FormatSet,
            gbm::{GbmAllocator, GbmBufferFlags, GbmDevice},
            Fourcc,
        },
        drm::{
            compositor::FrameFlags,
            exporter::gbm::{GbmFramebufferExporter, NodeFilter},
            output::{DrmOutput, DrmOutputManager, DrmOutputRenderElements},
            DrmDevice, DrmDeviceFd, DrmEvent, DrmEventMetadata, DrmEventTime, DrmNode, DrmSurface,
            NodeType,
        },
        egl::{EGLContext, EGLDevice, EGLDisplay},
        input::InputEvent,
        libinput::{LibinputInputBackend, LibinputSessionInterface},
        renderer::{
            element::{
                default_primary_scanout_output_compare,
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
                utils::select_dmabuf_feedback,
                Kind, RenderElementStates,
            },
            gles::GlesRenderer,
            ImportDma, ImportEgl, ImportMemWl,
        },
        session::{libseat::LibSeatSession, Event as SessionEvent, Session},
        udev::{all_gpus, primary_gpu, UdevBackend, UdevEvent},
    },
    desktop::utils::{
        surface_presentation_feedback_flags_from_states, surface_primary_scanout_output,
        update_surface_primary_scanout_output, OutputPresentationFeedback,
    },
    input::pointer::CursorImageStatus,
    output::{Mode as WlMode, Output, PhysicalProperties, Subpixel},
    reexports::{
        calloop::{
            timer::{TimeoutAction, Timer},
            EventLoop, LoopHandle, RegistrationToken,
        },
        drm::control::{connector, crtc, Device as DrmControlDevice, Mode},
        input::Libinput,
        rustix::fs::OFlags,
        wayland_protocols::wp::{
            linux_dmabuf::zv1::server::zwp_linux_dmabuf_feedback_v1::TrancheFlags,
            presentation_time::server::wp_presentation_feedback,
        },
        wayland_server::backend::GlobalId,
    },
    utils::{Clock, DeviceFd, Monotonic, Physical, Point, Scale, Time, Transform},
    wayland::{
        dmabuf::{
            DmabufFeedback, DmabufFeedbackBuilder, DmabufGlobal, DmabufHandler, DmabufState,
            ImportNotifier,
        },
        drm_syncobj::{supports_syncobj_eventfd, DrmSyncobjState},
        presentation::{PresentationState, Refresh},
    },
};
use xcursor::parser::Image as XCursorImage;
use smithay_drm_extras::{
    display_info,
    drm_scanner::{DrmScanEvent, DrmScanner},
};

use crate::night_light::RenderTargetInfo;
use crate::render::{OutputStack, CLEAR_COLOR};
use crate::state::MetisState;

/// Color formats we ask the DRM compositor to consider, in preference order:
/// 10-bit first when available, falling back to plain 8-bit.
const SUPPORTED_FORMATS: &[Fourcc] = &[
    Fourcc::Abgr2101010,
    Fourcc::Argb2101010,
    Fourcc::Abgr8888,
    Fourcc::Argb8888,
];

/// Each queued frame carries the presentation feedback collected at render time
/// (`wp_presentation`). On vblank we hand this to the client with the real
/// scan-out timestamp so games can pace their frames accurately. `None` when a
/// frame carries no surfaces that requested feedback.
type FrameFeedback = Option<OutputPresentationFeedback>;
type MetisDrmOutput = DrmOutput<
    GbmAllocator<DrmDeviceFd>,
    GbmFramebufferExporter<DrmDeviceFd>,
    FrameFeedback,
    DrmDeviceFd,
>;
type MetisDrmOutputManager = DrmOutputManager<
    GbmAllocator<DrmDeviceFd>,
    GbmFramebufferExporter<DrmDeviceFd>,
    FrameFeedback,
    DrmDeviceFd,
>;

/// Two dmabuf feedbacks per scan-out surface: the default render feedback and a
/// scanout-preferring feedback whose top tranche advertises the display's
/// directly-scannable plane formats. Sent per-surface so a fullscreen game that
/// is a primary-plane candidate is told to allocate directly-scannable buffers
/// (zero-copy direct scanout), while everything else keeps the render feedback.
pub struct SurfaceDmabufFeedback {
    pub render_feedback: DmabufFeedback,
    pub scanout_feedback: DmabufFeedback,
}

/// Per-connector scan-out surface (one CRTC → one `Output`).
pub struct SurfaceData {
    pub output: Output,
    pub global: Option<GlobalId>,
    pub drm_output: MetisDrmOutput,
    pub connector: connector::Handle,
    /// Modes advertised by the connector when this output was connected.
    pub modes: Vec<Mode>,
    /// User turned this output off in Settings while the connector stays connected.
    pub user_disabled: bool,
    /// A frame is committed and awaiting its vblank; do not render again until
    /// `frame_submitted` clears this.
    pub queued: bool,
    /// Damage arrived (possibly while a frame was queued) and this surface needs
    /// to repaint at the next opportunity.
    pub pending: bool,
    /// Render/scanout dmabuf feedback for this display, `None` if the feedback
    /// could not be built (falls back to the default global feedback).
    pub dmabuf_feedback: Option<SurfaceDmabufFeedback>,
}

/// All DRM/udev backend state, stored in `MetisState::udev`.
pub struct UdevState {
    pub session: LibSeatSession,
    pub loop_handle: LoopHandle<'static, MetisState>,
    /// The primary (card) DRM node we opened for KMS.
    pub node: DrmNode,
    /// Render node used to build the [`GlesRenderer`].
    pub render_node: DrmNode,
    /// Single primary-GPU renderer. Taken out (and restored) around each frame so
    /// `build_render_elements` can borrow the rest of `MetisState`.
    pub renderer: Option<GlesRenderer>,
    pub drm_output_manager: MetisDrmOutputManager,
    pub drm_scanner: DrmScanner,
    pub surfaces: HashMap<crtc::Handle, SurfaceData>,
    pub dmabuf_state: Option<(DmabufState, DmabufGlobal)>,
    /// calloop token for the DRM vblank/event notifier (for teardown).
    pub drm_token: Option<RegistrationToken>,
    /// libinput context, retained so the session can suspend/resume it on VT
    /// switch.
    pub libinput: Option<Libinput>,
    /// Named-theme pointer cursor (DRM backend paints its own cursor).
    pub cursor: crate::cursor::XCursor,
    /// Theme name used to load [`Self::cursor`] (for on-demand resize cursors).
    pub cursor_theme: String,
    /// Cache of uploaded cursor frames, keyed by the source xcursor image.
    pub pointer_buffers: Vec<(XCursorImage, MemoryRenderBuffer)>,
    /// Cached mirror-source frame for the current damage-dispatch batch.
    pub mirror_batch: Option<crate::mirror::MirrorBatchCache>,
    /// `wp_presentation` global; hands out per-surface feedback objects and lets
    /// us report real scan-out timestamps on vblank.
    pub presentation: PresentationState,
    /// Monotonic clock used for presentation timestamps when the DRM event does
    /// not carry a hardware timestamp.
    pub clock: Clock<Monotonic>,
}

#[derive(Debug, thiserror::Error)]
enum BackendError {
    #[error("no GPU found for seat")]
    NoGpu,
    #[error("failed to initialize libseat session: {0}")]
    Session(#[from] smithay::backend::session::libseat::Error),
    #[error("no device path for primary GPU node")]
    NoDevicePath,
    #[error("failed to open DRM device: {0}")]
    Open(String),
    #[error("EGL init failed: {0}")]
    Egl(#[from] smithay::backend::egl::Error),
    #[error("GLES renderer init failed: {0}")]
    Gles(#[from] smithay::backend::renderer::gles::GlesError),
}

/// Components produced by opening the primary GPU.
struct OpenedDevice {
    render_node: DrmNode,
    renderer: GlesRenderer,
    manager: MetisDrmOutputManager,
    drm_token: RegistrationToken,
    /// The DRM device fd, retained so we can set up `linux-drm-syncobj-v1`
    /// explicit sync against the GPU that imports client buffers.
    device_fd: DrmDeviceFd,
}

pub fn init_udev(
    event_loop: &mut EventLoop<'static, MetisState>,
    state: &mut MetisState,
) -> Result<(), Box<dyn std::error::Error>> {
    let loop_handle = event_loop.handle();

    // 1. Session: take control of the seat (DRM master + input) via libseat.
    let (session, session_notifier) = LibSeatSession::new().map_err(BackendError::Session)?;
    let seat_name = session.seat();
    tracing::info!(seat = %seat_name, "libseat session acquired");

    // 2. Pick the primary GPU (normalized to its card node for KMS).
    let node = pick_primary_gpu(&seat_name).ok_or(BackendError::NoGpu)?;
    tracing::info!(?node, "primary GPU");

    // 3. Open the device: GBM allocator + GLES renderer + DRM output manager,
    //    and register the vblank notifier.
    let opened = open_primary_device(&loop_handle, &session, node)?;

    let mut udev = UdevState {
        session,
        loop_handle: loop_handle.clone(),
        node,
        render_node: opened.render_node,
        renderer: Some(opened.renderer),
        drm_output_manager: opened.manager,
        drm_scanner: DrmScanner::new(),
        surfaces: HashMap::new(),
        dmabuf_state: None,
        drm_token: Some(opened.drm_token),
        libinput: None,
        cursor: {
            let (theme, size) = state.xcursor_config();
            crate::cursor::XCursor::load(theme, size)
        },
        cursor_theme: state.xcursor_config().0.to_string(),
        pointer_buffers: Vec::new(),
        mirror_batch: None,
        presentation: {
            let clock = Clock::<Monotonic>::new();
            PresentationState::new::<MetisState>(&state.display_handle, clock.id() as u32)
        },
        clock: Clock::<Monotonic>::new(),
    };

    // 4. dmabuf global from the primary renderer's formats so EGL/GPU clients
    //    (GTK) can submit hardware buffers; also bind wl_drm for legacy EGL.
    if let Some(renderer) = udev.renderer.as_mut() {
        if let Err(err) = renderer.bind_wl_display(&state.display_handle) {
            tracing::info!(?err, "wl_drm (EGL) bind unavailable");
        }
        let dmabuf_formats = renderer.dmabuf_formats();
        if let Ok(default_feedback) =
            DmabufFeedbackBuilder::new(udev.render_node.dev_id(), dmabuf_formats).build()
        {
            let mut dmabuf_state = DmabufState::new();
            let global = dmabuf_state.create_global_with_default_feedback::<MetisState>(
                &state.display_handle,
                &default_feedback,
            );
            udev.dmabuf_state = Some((dmabuf_state, global));
            tracing::info!("dmabuf global created");
        }
        let shm_formats = renderer.shm_formats();
        state.shm_state.update_formats(shm_formats);
    }

    // 5. libinput: feed real input devices into the shared, backend-agnostic
    //    `process_input_event`.
    let mut libinput_context = Libinput::new_with_udev::<LibinputSessionInterface<LibSeatSession>>(
        udev.session.clone().into(),
    );
    if libinput_context.udev_assign_seat(&seat_name).is_err() {
        tracing::warn!("failed to assign udev seat to libinput");
    }
    udev.libinput = Some(libinput_context.clone());
    let libinput_backend = LibinputInputBackend::new(libinput_context);
    loop_handle
        .insert_source(libinput_backend, move |mut event, _, state| {
            if let InputEvent::DeviceAdded { device } = &mut event {
                if device.has_capability(DeviceCapability::Keyboard) {
                    if let Some(led_state) =
                        state.seat.get_keyboard().map(|keyboard| keyboard.led_state())
                    {
                        let _ = device.led_update(led_state.into());
                    }
                }
                if device.has_capability(DeviceCapability::Touch) {
                    state.ensure_touch_device();
                }
                state.input_runtime.on_device_added(device.clone());
            } else             if let InputEvent::DeviceRemoved { device } = &event {
                state.input_runtime.on_device_removed(device);
            }
            if let Some(device) = crate::device_input::libinput_device_from_event(&event) {
                state.input_runtime.note_pointer_device(&device);
            }
            state.process_input_event(event);
        })
        .map_err(|e| BackendError::Open(format!("libinput source: {e}")))?;

    // 6. Register session (VT switch / suspend) events.
    loop_handle
        .insert_source(session_notifier, move |event, _, state| {
            state.on_session_event(event);
        })
        .map_err(|e| BackendError::Open(format!("session source: {e}")))?;

    // 6. Register udev hotplug source (GPU add/remove + connector changes).
    let udev_backend = UdevBackend::new(&seat_name)
        .map_err(|e| BackendError::Open(format!("udev backend: {e}")))?;
    loop_handle
        .insert_source(udev_backend, move |event, _, state| {
            state.on_udev_event(event);
        })
        .map_err(|e| BackendError::Open(format!("udev source: {e}")))?;

    // The DRM backend is driven by the housekeeping heartbeat + vblank, not by a
    // host redraw request, so the redraw trigger is a no-op (damage is coalesced
    // by the 16ms tick below).
    state.set_redraw_trigger(std::rc::Rc::new(|| {}));

    // Steer spawned clients (games, XWayland, Vulkan apps) onto the same GPU the
    // compositor renders on. Resolved from the render node's PCI identity; None
    // for exotic nodes, in which case no GPU env is exported.
    state.client_gpu = crate::state::ClientGpuHint::from_render_node(&udev.render_node);
    if let Some(hint) = &state.client_gpu {
        tracing::info!(?hint, "client GPU steering env resolved");
    }
    // On a hybrid (Optimus) laptop the compositor renders on the iGPU that owns
    // the panel; detect a discrete GPU so games / Steam Big Picture can be
    // PRIME-offloaded onto it instead of being pinned to the weak iGPU.
    state.dgpu_offload = crate::state::DgpuOffload::detect(&udev.render_node);
    match &state.dgpu_offload {
        Some(offload) => tracing::info!(
            ?offload,
            "discrete GPU detected — game/launcher processes will be offloaded to it"
        ),
        None => tracing::info!("no discrete GPU detected; all clients use the display GPU"),
    }

    state.udev = Some(udev);

    // 6b. Explicit sync (`linux-drm-syncobj-v1`). Advertise it only when the
    //     primary GPU supports syncobj eventfd (otherwise we can't build the
    //     acquire-fence blocker). With it, NVIDIA + DXVK/VKD3D and modern
    //     XWayland negotiate explicit fences instead of implicit sync, which
    //     removes the tell-tale Proton stutter/glitching on this hardware.
    if supports_syncobj_eventfd(&opened.device_fd) {
        state.drm_syncobj_state = Some(DrmSyncobjState::new::<MetisState>(
            &state.display_handle,
            opened.device_fd.clone(),
        ));
        tracing::info!("linux-drm-syncobj-v1 explicit sync enabled");
    } else {
        tracing::info!("GPU lacks syncobj eventfd; explicit sync disabled");
    }

    // 7. Bring up every currently-connected output.
    state.scan_connectors();

    // 8. Housekeeping heartbeat. This is NO LONGER the frame pacer — high-refresh
    //    rendering is vblank-driven (a damaged surface repaints on its next
    //    vblank; see `schedule_redraw` + `on_drm_vblank`). This tick only:
    //      * runs shared housekeeping (space refresh, client flush, cleanup), and
    //      * kicks the *first* frame out of an idle state (nothing queued, no
    //        vblank pending), bounding idle→first-paint latency to one tick.
    //    Because the fast path bypasses it, this interval no longer caps FPS.
    loop_handle.insert_source(
        Timer::from_duration(Duration::from_millis(16)),
        move |_, _, state| {
            state.tick_housekeeping();
            state.drm_dispatch_damage();
            TimeoutAction::ToDuration(Duration::from_millis(16))
        },
    )?;

    state.damaged = true;
    tracing::info!("DRM/udev backend initialized");
    Ok(())
}

/// Find the GPU to drive the session, normalized to its card (Primary) node so
/// it can be opened as DRM master.
///
/// `METIS_DRM_DEVICE` forces a choice. Otherwise we **rank all GPUs by whether
/// they actually have a connected output** (and prefer the `boot_vga` device on a
/// tie), rather than trusting udev's `primary_gpu()`. This is essential on hybrid
/// laptops: smithay's `primary_gpu()` often returns the discrete NVIDIA GPU,
/// whose KMS is flaky and which usually has *no* connected panel — the eDP is
/// wired to the Intel iGPU. Driving the GPU that owns the connected display gives
/// a stable session.
fn pick_primary_gpu(seat: &str) -> Option<DrmNode> {
    if let Ok(var) = std::env::var("METIS_DRM_DEVICE") {
        if let Ok(node) = DrmNode::from_path(&var) {
            tracing::info!(%var, "using METIS_DRM_DEVICE");
            return Some(to_primary_node(node));
        }
        tracing::warn!(%var, "METIS_DRM_DEVICE invalid — autodetecting");
    }

    let mut candidates: Vec<DrmNode> = all_gpus(seat)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|p| DrmNode::from_path(p).ok())
        .map(to_primary_node)
        .collect();

    // Ensure udev's notion of the primary GPU is at least in the running.
    if let Some(p) = primary_gpu(seat).ok().flatten().and_then(|p| DrmNode::from_path(p).ok()) {
        let p = to_primary_node(p);
        if !candidates.contains(&p) {
            candidates.push(p);
        }
    }

    // Higher score wins: a connected output dominates; boot_vga breaks ties.
    let best = candidates.into_iter().max_by_key(|node| gpu_rank(node));
    if let Some(node) = best {
        tracing::info!(?node, has_output = gpu_has_connected_output(node), "selected primary GPU");
        return Some(node);
    }
    None
}

/// Normalize a DRM node to its card (Primary) node for KMS/DRM-master use.
fn to_primary_node(node: DrmNode) -> DrmNode {
    node.node_with_type(NodeType::Primary)
        .and_then(|r| r.ok())
        .unwrap_or(node)
}

/// Rank a GPU for session use: connected output is worth far more than being the
/// boot VGA device, so an iGPU driving the panel beats an idle dGPU.
fn gpu_rank(node: &DrmNode) -> i32 {
    let mut score = 0;
    if gpu_has_connected_output(*node) {
        score += 100;
    }
    if gpu_is_boot_vga(*node) {
        score += 10;
    }
    score
}

/// The sysfs DRM card directory for a node (e.g. `/sys/class/drm/card2`), derived
/// from its device path (`/dev/dri/card2`).
fn gpu_sysfs_dir(node: DrmNode) -> Option<std::path::PathBuf> {
    let path = node.dev_path()?;
    let name = path.file_name()?.to_str()?;
    Some(std::path::PathBuf::from("/sys/class/drm").join(name))
}

/// True if any connector on this GPU reports `connected` (i.e. it owns a live
/// display). Reads `…/cardN-*/status` from sysfs.
fn gpu_has_connected_output(node: DrmNode) -> bool {
    let Some(dir) = gpu_sysfs_dir(node) else {
        return false;
    };
    let card = match dir.file_name().and_then(|n| n.to_str()) {
        Some(c) => c.to_string(),
        None => return false,
    };
    let Ok(entries) = std::fs::read_dir(&dir.parent().unwrap_or(std::path::Path::new("/sys/class/drm"))) else {
        return false;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        // Connectors are named like "card2-eDP-1"; skip the bare "card2" dir.
        if !name.starts_with(&format!("{card}-")) {
            continue;
        }
        if let Ok(status) = std::fs::read_to_string(entry.path().join("status")) {
            if status.trim() == "connected" {
                return true;
            }
        }
    }
    false
}

/// True if this GPU is the firmware boot VGA device (sysfs `boot_vga`).
fn gpu_is_boot_vga(node: DrmNode) -> bool {
    gpu_sysfs_dir(node)
        .and_then(|dir| std::fs::read_to_string(dir.join("device/boot_vga")).ok())
        .map(|s| s.trim() == "1")
        .unwrap_or(false)
}

fn open_primary_device(
    loop_handle: &LoopHandle<'static, MetisState>,
    session: &LibSeatSession,
    node: DrmNode,
) -> Result<OpenedDevice, BackendError> {
    let path = node.dev_path().ok_or(BackendError::NoDevicePath)?;

    let mut session = session.clone();
    let fd = session
        .open(
            &path,
            OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOCTTY | OFlags::NONBLOCK,
        )
        .map_err(|e| BackendError::Open(format!("libseat open {path:?}: {e}")))?;
    let fd = DrmDeviceFd::new(DeviceFd::from(fd));

    let (drm, drm_notifier) =
        DrmDevice::new(fd.clone(), true).map_err(|e| BackendError::Open(format!("drm: {e}")))?;
    let device_fd = fd.clone();
    let gbm = GbmDevice::new(fd).map_err(|e| BackendError::Open(format!("gbm: {e}")))?;

    // EGL + GLES renderer on this GPU.
    let egl_display = unsafe { EGLDisplay::new(gbm.clone())? };
    let render_node = EGLDevice::device_for_display(&egl_display)
        .ok()
        .and_then(|d| d.try_get_render_node().ok().flatten())
        .unwrap_or(node);
    let egl_context = EGLContext::new(&egl_display)?;
    let renderer = unsafe { GlesRenderer::new(egl_context)? };

    let render_formats = renderer
        .egl_context()
        .dmabuf_render_formats()
        .iter()
        .copied()
        .collect::<FormatSet>();

    let allocator = GbmAllocator::new(
        gbm.clone(),
        GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT,
    );
    let exporter = GbmFramebufferExporter::new(gbm.clone(), NodeFilter::from(Some(render_node)));

    let manager = DrmOutputManager::new(
        drm,
        allocator,
        exporter,
        Some(gbm),
        SUPPORTED_FORMATS.iter().copied(),
        render_formats,
    );

    // VBlank / DRM error notifier.
    let drm_token = loop_handle
        .insert_source(drm_notifier, move |event, meta, state| match event {
            DrmEvent::VBlank(crtc) => state.on_drm_vblank(crtc, meta.take()),
            DrmEvent::Error(err) => tracing::warn!(?err, "DRM error"),
        })
        .map_err(|e| BackendError::Open(format!("drm notifier source: {e}")))?;

    Ok(OpenedDevice {
        render_node,
        renderer,
        manager,
        drm_token,
        device_fd,
    })
}

/// Build the render + scanout dmabuf feedbacks for a display surface. The
/// scanout tranche advertises the plane formats the display can scan out
/// directly, limited to formats we can also render from so there is always a
/// fallback path when a client buffer can't be promoted to a plane.
fn build_surface_dmabuf_feedback(
    render_node: DrmNode,
    render_formats: FormatSet,
    surface: &DrmSurface,
) -> Option<SurfaceDmabufFeedback> {
    let planes = surface.planes().clone();
    let planes_formats = surface
        .plane_info()
        .formats
        .iter()
        .copied()
        .chain(planes.overlay.into_iter().flat_map(|p| p.formats))
        .collect::<FormatSet>()
        .intersection(&render_formats)
        .copied()
        .collect::<FormatSet>();

    let builder = DmabufFeedbackBuilder::new(render_node.dev_id(), render_formats);
    let render_feedback = builder.clone().build().ok()?;
    let scanout_feedback = builder
        .add_preference_tranche(
            surface.device_fd().dev_id().ok()?,
            Some(TrancheFlags::Scanout),
            planes_formats,
        )
        .build()
        .ok()?;

    Some(SurfaceDmabufFeedback {
        render_feedback,
        scanout_feedback,
    })
}

impl MetisState {
    /// Scan the device's connectors and bring up / tear down outputs.
    pub(crate) fn scan_connectors(&mut self) {
        let scan = {
            let Some(udev) = self.udev.as_mut() else {
                return;
            };
            match udev
                .drm_scanner
                .scan_connectors(udev.drm_output_manager.device())
            {
                Ok(scan) => scan,
                Err(err) => {
                    tracing::warn!(?err, "connector scan failed");
                    return;
                }
            }
        };
        for event in scan {
            match event {
                DrmScanEvent::Connected {
                    connector,
                    crtc: Some(crtc),
                } => self.connector_connected(connector, crtc),
                DrmScanEvent::Disconnected {
                    connector: _,
                    crtc: Some(crtc),
                } => self.connector_disconnected(crtc),
                _ => {}
            }
        }
        // Re-tile windows/layers after the output set changed.
        self.retile_outputs();
    }

    fn connector_connected(&mut self, connector: connector::Info, crtc: crtc::Handle) {
        if self
            .udev
            .as_ref()
            .map(|u| u.surfaces.contains_key(&crtc))
            .unwrap_or(true)
        {
            return;
        }

        let name = format!(
            "{}-{}",
            connector.interface().as_str(),
            connector.interface_id()
        );

        let cfg = self.output_runtime.cached().clone();
        let modes: Vec<Mode> = connector.modes().to_vec();
        let prefs = metis_config::output_prefs(&cfg, &name);
        let mode_id = crate::output_modes::pick_drm_mode_index(&modes, &prefs);
        let Some(drm_mode) = modes.get(mode_id).copied() else {
            tracing::warn!(%name, "connector has no modes");
            return;
        };
        let wl_mode = WlMode::from(drm_mode);

        let (make, model) = {
            let udev = self.udev.as_ref().unwrap();
            let drm_device = udev.drm_output_manager.device();
            let info = display_info::for_connector(drm_device, connector.handle());
            (
                info.as_ref()
                    .and_then(|i| i.make())
                    .unwrap_or_else(|| "Unknown".into()),
                info.as_ref()
                    .and_then(|i| i.model())
                    .unwrap_or_else(|| "Unknown".into()),
            )
        };
        let (phys_w, phys_h) = connector.size().unwrap_or((0, 0));

        let output = Output::new(
            name.clone(),
            PhysicalProperties {
                size: (phys_w as i32, phys_h as i32).into(),
                subpixel: Subpixel::Unknown,
                make,
                model,
                serial_number: name.clone(),
            },
        );
        let global = output.create_global::<MetisState>(&self.display_handle);

        // Place the output using saved layout or auto-pack left-to-right.
        let position = crate::output_prefs::output_position_for_connect(self, &cfg, &name);
        output.set_preferred(wl_mode);
        output.change_current_state(Some(wl_mode), Some(Transform::Normal), None, Some(position));
        self.space.map_output(&output, position);

        let planes = {
            let udev = self.udev.as_ref().unwrap();
            udev.drm_output_manager.device().planes(&crtc).ok()
        };
        let mut renderer = self.udev.as_mut().unwrap().renderer.take();
        let drm_output = {
            let udev = self.udev.as_mut().unwrap();
            let Some(renderer) = renderer.as_mut() else {
                tracing::error!("no renderer for connector setup");
                self.space.unmap_output(&output);
                return;
            };
            udev.drm_output_manager
                .lock()
                .initialize_output::<GlesRenderer, crate::render::OutputStack>(
                    crtc,
                    drm_mode,
                    &[connector.handle()],
                    &output,
                    planes,
                    renderer,
                    &DrmOutputRenderElements::default(),
                )
        };
        self.udev.as_mut().unwrap().renderer = renderer;

        let drm_output = match drm_output {
            Ok(o) => o,
            Err(err) => {
                tracing::warn!(?err, %name, "failed to initialize DRM output");
                self.space.unmap_output(&output);
                return;
            }
        };

        // Per-surface dmabuf feedback (render + scanout tranche) so fullscreen
        // clients can allocate directly-scannable buffers on this display.
        let dmabuf_feedback = {
            let udev = self.udev.as_ref().unwrap();
            let render_node = udev.render_node;
            let render_formats = udev.renderer.as_ref().map(|r| r.dmabuf_formats());
            render_formats.and_then(|formats| {
                drm_output.with_compositor(|compositor| {
                    build_surface_dmabuf_feedback(render_node, formats, compositor.surface())
                })
            })
        };

        self.udev.as_mut().unwrap().surfaces.insert(
            crtc,
            SurfaceData {
                output: output.clone(),
                global: Some(global),
                drm_output,
                connector: connector.handle(),
                modes,
                user_disabled: false,
                queued: false,
                pending: true,
                dmabuf_feedback,
            },
        );
        tracing::info!(%name, ?position, "output connected");

        self.ensure_desk_for_output(&output);
        let cfg = self.output_runtime.cached().clone();
        crate::output_prefs::apply_outputs(self, &cfg);
        self.damaged = true;
    }

    fn connector_disconnected(&mut self, crtc: crtc::Handle) {
        let removed = self.udev.as_mut().and_then(|u| u.surfaces.remove(&crtc));
        if let Some(mut surface) = removed {
            let output = surface.output.clone();
            if let Some(global) = surface.global.take() {
                self.display_handle.remove_global::<MetisState>(global);
            }
            self.space.unmap_output(&output);
            tracing::info!(output = %output.name(), "output disconnected");
        }
    }

    /// Damage-gated render dispatch from the heartbeat. Propagates the global
    /// `damaged` flag onto every surface, then renders each surface that needs a
    /// frame and is not already waiting on a vblank.
    pub(crate) fn drm_dispatch_damage(&mut self) {
        if self.udev.is_none() {
            return;
        }
        // While the screen is blanked (DPMS off) do no scan-out: page-flipping to
        // a powered-down connector can withhold the vblank and wedge the surface
        // as permanently `queued`. Still run housekeeping so client bookkeeping
        // keeps ticking; the wake path sets `damaged` to force a fresh repaint.
        if self.idle.is_blanked() {
            self.space.refresh();
            self.cleanup_destroyed_windows();
            self.popups.cleanup();
            let outputs: Vec<Output> = self.space.outputs().cloned().collect();
            for out in &outputs {
                smithay::desktop::layer_map_for_output(out).cleanup();
            }
            self.defer_client_flush = true;
            return;
        }
        if self.damaged {
            self.damaged = false;
            if let Some(udev) = self.udev.as_mut() {
                udev.mirror_batch = None;
                for surface in udev.surfaces.values_mut() {
                    if !surface.user_disabled {
                        surface.pending = true;
                    }
                }
            }
        }
        // While a portal screencast holds an image-copy session, keep repainting
        // so capture frames are produced even when the desktop is visually static.
        if self.image_capture.screencast_active() || self.image_capture.has_pending() {
            if let Some(udev) = self.udev.as_mut() {
                for surface in udev.surfaces.values_mut() {
                    if !surface.user_disabled {
                        surface.pending = true;
                    }
                }
            }
        }
        let crtcs: Vec<crtc::Handle> = self
            .udev
            .as_ref()
            .map(|u| {
                u.surfaces
                    .iter()
                    .filter(|(_, s)| !s.user_disabled && s.pending && !s.queued)
                    .map(|(c, _)| *c)
                    .collect()
            })
            .unwrap_or_default();
        for crtc in crtcs {
            self.render_surface(crtc);
        }

        // Housekeeping that the winit backend does in its Redraw handler.
        self.space.refresh();
        self.cleanup_destroyed_windows();
        self.popups.cleanup();
        let outputs: Vec<Output> = self.space.outputs().cloned().collect();
        for out in &outputs {
            smithay::desktop::layer_map_for_output(out).cleanup();
        }
        self.defer_client_flush = true;
    }

    /// VBlank: the queued frame scanned out. Recycle buffers, report the real
    /// scan-out time to `wp_presentation` clients, and repaint if more damage
    /// accumulated while the frame was in flight.
    pub(crate) fn on_drm_vblank(
        &mut self,
        crtc: crtc::Handle,
        metadata: Option<DrmEventMetadata>,
    ) {
        let (still_pending, feedback, output, now, vrr_active) = {
            let Some(udev) = self.udev.as_mut() else {
                return;
            };
            let Some(surface) = udev.surfaces.get_mut(&crtc) else {
                return;
            };
            surface.queued = false;
            // `frame_submitted` recycles the just-scanned-out buffer and returns
            // the presentation feedback we attached at queue time.
            let feedback = match surface.drm_output.frame_submitted() {
                Ok(user_data) => user_data.flatten(),
                Err(err) => {
                    tracing::warn!(?err, "frame_submitted failed");
                    None
                }
            };
            let vrr_active = surface.drm_output.with_compositor(|c| c.vrr_enabled());
            let output = surface.output.clone();
            let now = udev.clock.now();
            (surface.pending, feedback, output, now, vrr_active)
        };

        // Report the frame to clients that requested presentation feedback. Prefer
        // the hardware vblank timestamp/sequence from the DRM event; fall back to
        // the monotonic clock when the driver did not supply one.
        if let Some(mut feedback) = feedback {
            let tp = metadata.as_ref().and_then(|m| match m.time {
                DrmEventTime::Monotonic(tp) if !tp.is_zero() => Some(tp),
                _ => None,
            });
            let seq = metadata.as_ref().map(|m| m.sequence).unwrap_or(0);
            let (time, flags): (Time<Monotonic>, wp_presentation_feedback::Kind) = match tp {
                Some(tp) => (
                    tp.into(),
                    wp_presentation_feedback::Kind::Vsync
                        | wp_presentation_feedback::Kind::HwClock
                        | wp_presentation_feedback::Kind::HwCompletion,
                ),
                None => (now, wp_presentation_feedback::Kind::Vsync),
            };
            // With VRR the panel refreshes on flip, so the mode refresh is only
            // the *fastest* (minimum interval) — report it as Variable so clients
            // pace against the floor rather than assuming a fixed cadence.
            let refresh = output
                .current_mode()
                .filter(|m| m.refresh > 0)
                .map(|m| {
                    let interval = Duration::from_secs_f64(1_000.0 / m.refresh as f64);
                    if vrr_active {
                        Refresh::variable(interval)
                    } else {
                        Refresh::fixed(interval)
                    }
                })
                .unwrap_or(Refresh::Unknown);
            feedback.presented(time, refresh, seq as u64, flags);
        }

        if still_pending {
            self.render_surface(crtc);
        }
    }

    /// Record the primary scan-out output for every surface shown on `output`
    /// (so frame callbacks and feedback target the right output) and collect the
    /// `wp_presentation` feedback callbacks for this frame.
    fn build_presentation_feedback(
        &self,
        output: &Output,
        states: &RenderElementStates,
    ) -> OutputPresentationFeedback {
        for window in self.space.elements() {
            window.with_surfaces(|surface, surface_data| {
                update_surface_primary_scanout_output(
                    surface,
                    output,
                    surface_data,
                    None,
                    states,
                    default_primary_scanout_output_compare,
                );
            });
        }
        let mut feedback = OutputPresentationFeedback::new(output);
        for window in self.space.elements() {
            if self.space.outputs_for_element(window).contains(output) {
                window.take_presentation_feedback(
                    &mut feedback,
                    surface_primary_scanout_output,
                    |surface, _| {
                        surface_presentation_feedback_flags_from_states(surface, None, states)
                    },
                );
            }
        }
        let map = smithay::desktop::layer_map_for_output(output);
        for layer_surface in map.layers() {
            layer_surface.with_surfaces(|surface, surface_data| {
                update_surface_primary_scanout_output(
                    surface,
                    output,
                    surface_data,
                    None,
                    states,
                    default_primary_scanout_output_compare,
                );
            });
            layer_surface.take_presentation_feedback(
                &mut feedback,
                surface_primary_scanout_output,
                |surface, _| {
                    surface_presentation_feedback_flags_from_states(surface, None, states)
                },
            );
        }
        feedback
    }

    fn render_surface(&mut self, crtc: crtc::Handle) {
        // Pull the renderer out so `build_render_elements` can borrow the rest of
        // `self`; it is restored before we return.
        let mut renderer = match self.udev.as_mut().and_then(|u| u.renderer.take()) {
            Some(r) => r,
            None => return,
        };

        let user_disabled = self
            .udev
            .as_ref()
            .and_then(|u| u.surfaces.get(&crtc))
            .is_some_and(|s| s.user_disabled);
        if user_disabled {
            self.udev.as_mut().unwrap().renderer = Some(renderer);
            return;
        }

        let output = match self
            .udev
            .as_ref()
            .and_then(|u| u.surfaces.get(&crtc))
            .map(|s| s.output.clone())
        {
            Some(o) => o,
            None => {
                self.process_pending_captures(&mut renderer);
                self.udev.as_mut().unwrap().renderer = Some(renderer);
                return;
            }
        };
        if let Some(s) = self.udev.as_mut().and_then(|u| u.surfaces.get_mut(&crtc)) {
            s.pending = false;
        }

        // Captured from a successful non-mirror render so we can build the
        // presentation feedback after the renderer is restored.
        let mut frame_states: Option<RenderElementStates> = None;
        let outcome: Result<bool, String> = if self.mirror_mode_active() {
            crate::mirror::render_mirror_surface(self, &mut renderer, crtc)
        } else {
            let scale = Scale::from(output.current_scale().fractional_scale());
            let origin: Point<i32, Physical> = self
                .space
                .output_geometry(&output)
                .map(|g| g.loc.to_physical_precise_round(scale))
                .unwrap_or_default();

            let mut elements = self.build_render_elements(
                &mut renderer,
                origin,
                scale,
                RenderTargetInfo {
                    size: output
                        .current_mode()
                        .map(|m| m.size)
                        .unwrap_or_default(),
                    output_name: Some(output.name().as_str()),
                    skip_night_light: false,
                },
            );

            // Pointer goes on top of everything; only on the output under the cursor.
            let cursor = self.build_cursor_elements(&mut renderer, &output, scale);
            if !cursor.is_empty() {
                let mut stacked = cursor;
                stacked.append(&mut elements);
                elements = stacked;
            }

            crate::output_vrr::prepare_vrr_for_render(self, crtc);
            let udev = self.udev.as_mut().unwrap();
            let surface = udev.surfaces.get_mut(&crtc).unwrap();
            match surface.drm_output.render_frame(
                &mut renderer,
                &elements,
                CLEAR_COLOR,
                FrameFlags::DEFAULT,
            ) {
                Ok(res) => {
                    let empty = res.is_empty;
                    frame_states = Some(res.states);
                    Ok(!empty)
                }
                Err(err) => Err(format!("{err:?}")),
            }
        };

        self.process_pending_captures(&mut renderer);

        // Restore the renderer before any early return below.
        self.udev.as_mut().unwrap().renderer = Some(renderer);

        match outcome {
            Ok(rendered) => {
                if rendered {
                    // Collect presentation feedback (and record primary scan-out
                    // outputs) from the states captured during render, then attach
                    // it to the frame so `on_drm_vblank` can report the real
                    // scan-out time to clients.
                    let feedback = frame_states
                        .as_ref()
                        .map(|states| self.build_presentation_feedback(&output, states));
                    let udev = self.udev.as_mut().unwrap();
                    let surface = udev.surfaces.get_mut(&crtc).unwrap();
                    match surface.drm_output.queue_frame(feedback) {
                        Ok(()) => surface.queued = true,
                        Err(err) => tracing::warn!(?err, "queue_frame failed"),
                    }
                    // Deliver frame callbacks so clients paint their next frame.
                    let now = self.start_time.elapsed();
                    let out = output.clone();
                    self.space.elements().for_each(|window| {
                        window.send_frame(&out, now, Some(Duration::ZERO), |_, _| Some(out.clone()));
                    });
                    self.send_layer_frames(&out, now);

                    // Per-surface dmabuf feedback: tell a surface that was scanned
                    // out directly (a fullscreen game on the primary plane) to keep
                    // allocating scannable buffers; everyone else gets the render
                    // feedback. Requires the render states captured this frame.
                    if let Some(states) = frame_states.as_ref() {
                        if let Some(udev) = self.udev.as_ref() {
                            if let Some(feedback) = udev
                                .surfaces
                                .get(&crtc)
                                .and_then(|s| s.dmabuf_feedback.as_ref())
                            {
                                for window in self.space.elements() {
                                    window.send_dmabuf_feedback(
                                        &out,
                                        surface_primary_scanout_output,
                                        |surface, _| {
                                            select_dmabuf_feedback(
                                                surface,
                                                states,
                                                &feedback.render_feedback,
                                                &feedback.scanout_feedback,
                                            )
                                        },
                                    );
                                }
                            }
                        }
                    }
                }
            }
            Err(err) => tracing::warn!(%err, "render_frame failed"),
        }
    }

    /// Build the pointer render element(s) for `output`, in output-local physical
    /// coordinates. Honors a client-supplied cursor surface (`set_cursor`), hides
    /// the pointer when the client requested it, and otherwise paints the named
    /// theme cursor. Returns empty when the pointer is not over this output.
    pub(crate) fn build_cursor_elements(
        &mut self,
        renderer: &mut GlesRenderer,
        output: &Output,
        scale: Scale<f64>,
    ) -> Vec<OutputStack> {
        let mut out = Vec::new();
        let Some(geo) = self.space.output_geometry(output) else {
            return out;
        };
        let Some(pointer) = self.seat.get_pointer() else {
            return out;
        };
        let loc = pointer.current_location();
        if !geo.to_f64().contains(loc) {
            return out;
        }
        let over_bar = self.metis_bar_ui_hit(loc);
        // Output-local logical pointer position.
        let local = loc - geo.loc.to_f64();

        if matches!(self.cursor_status, CursorImageStatus::Hidden) {
            return out;
        }
        if self.active_pointer_lock_suppresses_cursor() {
            return out;
        }

        let millis = self.start_time.elapsed().as_millis() as u32;
        let udev = match self.udev.as_mut() {
            Some(u) => u,
            None => return out,
        };

        // Always paint a compositor-owned pointer on DRM. Client wl_pointer surfaces
        // were composited onto the primary plane; switching to the resize cursor on
        // the hardware cursor plane left those pixels behind, so the arrow never
        // appeared to change. The nested winit session already ignores client cursors.
        let image = if !over_bar {
            if let Some(edge) = self.hover_cursor {
                udev.cursor.frame_resize(&udev.cursor_theme, edge, millis)
            } else {
                udev.cursor.frame(millis).clone()
            }
        } else {
            udev.cursor.frame(millis).clone()
        };
        let buffer = match udev.pointer_buffers.iter().find(|(i, _)| *i == image) {
            Some((_, buf)) => buf.clone(),
            None => {
                let buf = MemoryRenderBuffer::from_slice(
                    &image.pixels_rgba,
                    Fourcc::Argb8888,
                    (image.width as i32, image.height as i32),
                    1,
                    Transform::Normal,
                    None,
                );
                udev.pointer_buffers.push((image.clone(), buf.clone()));
                buf
            }
        };
        let hotspot: Point<f64, Physical> =
            Point::from((image.xhot as f64, image.yhot as f64));
        let pos = local.to_physical(scale) - hotspot;
        if let Ok(elem) = MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            pos,
            &buffer,
            None,
            None,
            None,
            Kind::Cursor,
        ) {
            out.push(OutputStack::CursorMemory(elem));
        }
        out
    }

    /// Re-pack mapped outputs left-to-right in global space so a disconnect never
    /// leaves a hole (and a reconnect never overlaps). Order is kept stable by
    /// current x then name, so the surviving outputs don't shuffle unexpectedly.
    fn repack_outputs(&mut self) {
        if self.mirror_mode_active() {
            let cfg = self.output_runtime.cached().clone();
            crate::mirror::apply_mirror_layout(self, &cfg);
            return;
        }
        let mut outputs: Vec<Output> = self
            .udev
            .as_ref()
            .map(|u| {
                u.surfaces
                    .values()
                    .filter(|s| !s.user_disabled)
                    .map(|s| s.output.clone())
                    .collect()
            })
            .unwrap_or_else(|| self.space.outputs().cloned().collect());
        outputs.sort_by(|a, b| {
            let ax = self
                .space
                .output_geometry(a)
                .map(|g| g.loc.x)
                .unwrap_or(0);
            let bx = self
                .space
                .output_geometry(b)
                .map(|g| g.loc.x)
                .unwrap_or(0);
            ax.cmp(&bx).then_with(|| a.name().cmp(&b.name()))
        });
        let mut x = 0;
        for output in outputs {
            let width = self
                .space
                .output_geometry(&output)
                .map(|g| g.size.w)
                .or_else(|| output.current_mode().map(|m| m.size.w))
                .unwrap_or(0);
            let position = Point::from((x, 0));
            output.change_current_state(None, None, None, Some(position));
            self.space.map_output(&output, position);
            x += width;
        }
    }

    /// Re-apply window/layer geometry after the output set changed (connect /
    /// disconnect / hotplug). Mirrors the winit resize path.
    pub(crate) fn retile_outputs(&mut self) {
        self.repack_outputs();
        if let Some(first) = self
            .space
            .outputs()
            .next()
            .and_then(|o| self.space.output_geometry(o))
        {
            self.monitor.width = first.size.w;
            self.monitor.height = first.size.h;
        }
        let (wp_full, wp_regions) = self.wallpaper_layout();
        self.wallpaper.set_layout(wp_full, wp_regions);
        if self.wallpaper.enabled() {
            self.wallpaper.start_async_decode();
        }
        let ids: Vec<u32> = self.windows.ids();
        for id in ids {
            self.apply_window_rect(id);
        }
        self.sync_all_app_windows();
        self.refresh_all_scroll_offsets();
        self.arrange_layers();
        self.emit_monitor_changed();
        self.damaged = true;
    }

    /// libseat session pause/resume (VT switch, suspend).
    pub(crate) fn on_session_event(&mut self, event: SessionEvent) {
        match event {
            SessionEvent::PauseSession => {
                tracing::info!("session paused (VT switch away / suspend)");
                if let Some(udev) = self.udev.as_mut() {
                    if let Some(li) = udev.libinput.as_mut() {
                        li.suspend();
                    }
                    udev.drm_output_manager.pause();
                }
            }
            SessionEvent::ActivateSession => {
                tracing::info!("session resumed");
                if let Some(udev) = self.udev.as_mut() {
                    if let Some(li) = udev.libinput.as_mut() {
                        if let Err(err) = li.resume() {
                            tracing::warn!(?err, "failed to resume libinput");
                        }
                    }
                    if let Err(err) = udev.drm_output_manager.lock().activate(false) {
                        tracing::warn!(?err, "failed to reactivate DRM after resume");
                    }
                    for surface in udev.surfaces.values_mut() {
                        surface.queued = false;
                        surface.pending = true;
                    }
                }
                // A VT switch resets the CRTC gamma ramp; re-upload calibration.
                crate::output_gamma::apply_output_gamma(self);
                self.damaged = true;
                self.drm_dispatch_damage();
            }
        }
    }

    /// Switch to virtual terminal `vt` (Ctrl+Alt+F<n>). Only meaningful under the
    /// DRM backend; a no-op (logged) when nested.
    pub(crate) fn drm_change_vt(&mut self, vt: i32) {
        if let Some(udev) = self.udev.as_mut() {
            if let Err(err) = udev.session.change_vt(vt) {
                tracing::warn!(?err, vt, "failed to change VT");
            }
        }
    }

    /// True when running under the standalone DRM backend.
    pub(crate) fn is_drm_backend(&self) -> bool {
        self.udev.is_some()
    }

    /// Safe quit for the standalone session (Ctrl+Alt+Backspace): tear down
    /// clients and stop the event loop, returning to the greeter.
    pub(crate) fn drm_quit(&mut self) {
        tracing::info!("safe-quit keybind — shutting down DRM session");
        self.end_compositor_session();
    }

    /// udev device add/remove (GPU hotplug). Stage F/G expand this; Stage C only
    /// reacts to connector changes on the already-open primary device.
    pub(crate) fn on_udev_event(&mut self, event: UdevEvent) {
        let primary = self.udev.as_ref().map(|u| u.node);
        match event {
            UdevEvent::Changed { device_id } => {
                if DrmNode::from_dev_id(device_id).ok() == primary {
                    self.scan_connectors();
                }
            }
            UdevEvent::Removed { device_id } => {
                // Losing the primary GPU mid-session (e.g. eGPU unplug) can't be
                // recovered on a single-renderer build; quit cleanly to the greeter
                // rather than spin on a dead device. Secondary GPUs are Stage G.
                if DrmNode::from_dev_id(device_id).ok() == primary {
                    tracing::error!("primary GPU removed — shutting down DRM session");
                    self.drm_quit();
                }
            }
            UdevEvent::Added { .. } => {
                // Secondary-GPU hotplug handled in Stage G (multi-renderer).
            }
        }
    }

    /// Turn off a connected DRM output without disconnecting the connector.
    pub(crate) fn udev_disable_output(&mut self, name: &str) -> bool {
        let Some(udev) = self.udev.as_mut() else {
            return false;
        };
        let crtc = udev
            .surfaces
            .iter()
            .find(|(_, s)| s.output.name() == name && !s.user_disabled)
            .map(|(c, _)| *c);
        let Some(crtc) = crtc else {
            return false;
        };
        let surface = udev.surfaces.get_mut(&crtc).unwrap();
        let output = surface.output.clone();
        if let Some(global) = surface.global.take() {
            self.display_handle.remove_global::<MetisState>(global);
        }
        self.space.unmap_output(&output);
        surface.user_disabled = true;
        surface.pending = false;
        surface.queued = false;
        tracing::info!(output = %name, "output disabled by user");
        true
    }

    /// Re-enable a user-disabled DRM output.
    pub(crate) fn udev_enable_output(&mut self, name: &str) -> bool {
        let Some(udev) = self.udev.as_mut() else {
            return false;
        };
        let crtc = udev
            .surfaces
            .iter()
            .find(|(_, s)| s.output.name() == name && s.user_disabled)
            .map(|(c, _)| *c);
        let Some(crtc) = crtc else {
            return false;
        };
        let surface = udev.surfaces.get_mut(&crtc).unwrap();
        let output = surface.output.clone();
        let global = output.create_global::<MetisState>(&self.display_handle);
        surface.global = Some(global);
        surface.user_disabled = false;
        surface.pending = true;
        tracing::info!(output = %name, "output re-enabled by user");
        let _ = output;
        true
    }

    /// Power every active connector on or off via its DRM `DPMS` property. Used
    /// by the idle blanker: `on == false` powers the panels down (backlight off)
    /// after the idle timeout, `on == true` wakes them and requests a repaint.
    ///
    /// No-op under the nested winit backend (there is no DRM device); the panel
    /// there is owned by the host compositor. User-disabled outputs are left
    /// alone — they are already off. This deliberately does **not** touch the
    /// `wl_output` globals or the CRTC mode, so clients never see the monitor
    /// "disconnect" and nothing reflows across a blank/wake cycle.
    pub(crate) fn set_outputs_dpms(&mut self, on: bool) {
        let Some(udev) = self.udev.as_ref() else {
            return;
        };
        let device = udev.drm_output_manager.device();
        for surface in udev.surfaces.values() {
            if surface.user_disabled {
                continue;
            }
            set_connector_dpms(device, surface.connector, on, &surface.output.name());
        }
        // On wake, mark surfaces dirty so the heartbeat repaints once powered.
        if on {
            if let Some(udev) = self.udev.as_mut() {
                for surface in udev.surfaces.values_mut() {
                    if !surface.user_disabled {
                        surface.pending = true;
                    }
                }
            }
            self.damaged = true;
        }
    }
}

/// DRM `DPMS` connector-property value for the "on" state.
const DRM_MODE_DPMS_ON: u64 = 0;
/// DRM `DPMS` connector-property value for the "off" (powered down) state.
const DRM_MODE_DPMS_OFF: u64 = 3;

/// Set one connector's `DPMS` property. Mirrors Smithay's internal
/// `set_connector_state`; failures are logged and swallowed so a stubborn
/// connector can never take the session down (the worst case is a panel that
/// stays lit, and any input still wakes the rest of the pipeline).
fn set_connector_dpms<D: DrmControlDevice>(
    device: &D,
    conn: connector::Handle,
    on: bool,
    name: &str,
) {
    let props = match device.get_properties(conn) {
        Ok(props) => props,
        Err(err) => {
            tracing::warn!(output = %name, ?err, "dpms: get_properties failed");
            return;
        }
    };
    let (handles, _) = props.as_props_and_values();
    for handle in handles {
        let Ok(info) = device.get_property(*handle) else {
            continue;
        };
        if info.name().to_str().map(|n| n == "DPMS").unwrap_or(false) {
            let value = if on { DRM_MODE_DPMS_ON } else { DRM_MODE_DPMS_OFF };
            if let Err(err) = device.set_property(conn, *handle, value) {
                tracing::warn!(output = %name, on, ?err, "dpms: set_property failed");
            }
            return;
        }
    }
    tracing::debug!(output = %name, "dpms: connector has no DPMS property");
}

impl DmabufHandler for MetisState {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        &mut self
            .udev
            .as_mut()
            .expect("dmabuf only active under DRM backend")
            .dmabuf_state
            .as_mut()
            .expect("dmabuf global initialized")
            .0
    }

    fn dmabuf_imported(
        &mut self,
        _global: &DmabufGlobal,
        dmabuf: smithay::backend::allocator::dmabuf::Dmabuf,
        notifier: ImportNotifier,
    ) {
        let ok = self
            .udev
            .as_mut()
            .and_then(|u| u.renderer.as_mut())
            .map(|r| r.import_dmabuf(&dmabuf, None).is_ok())
            .unwrap_or(false);
        if ok {
            let _ = notifier.successful::<MetisState>();
        } else {
            notifier.failed();
        }
    }
}
