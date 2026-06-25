use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use smithay::{
    backend::{
        renderer::{
            damage::OutputDamageTracker,
            element::{
                render_elements,
                solid::SolidColorRenderElement,
                surface::WaylandSurfaceRenderElement,
                texture::TextureRenderElement,
                AsRenderElements, Id, Kind,
            },
            gles::{GlesRenderer, GlesTexture},
            utils::CommitCounter,
            Color32F,
        },
        winit::{self, WinitEvent},
    },
    desktop::{layer_map_for_output, Window},
    output::{Mode, Output, PhysicalProperties, Subpixel},
    reexports::calloop::{
        EventLoop,
        timer::{TimeoutAction, Timer},
    },
    utils::{Logical, Physical, Point, Rectangle, Scale, Size, Transform},
    wayland::shell::wlr_layer::Layer,
};

use crate::ipc;
use crate::state::MetisState;

render_elements! {
    OutputStack<=GlesRenderer>;
    Wallpaper=TextureRenderElement<GlesTexture>,
    Surface=WaylandSurfaceRenderElement<GlesRenderer>,
    Deco=crate::decoration::DecorationElement,
    Blur=crate::blur::BlurElement,
    Snap=SolidColorRenderElement,
}

/// Translucent fill for the snap-zone drop preview (accent blue @ ~30% alpha).
const SNAP_OVERLAY_COLOR: [f32; 4] = [0.36, 0.56, 0.96, 0.30];

/// Map the hovered resize edge to the matching host (winit) cursor shape. This
/// nested backend always draws the host cursor and ignores client cursor
/// surfaces, so directional resize feedback has to be applied here.
fn resize_cursor(edge: Option<crate::grabs::ResizeEdge>) -> smithay::reexports::winit::cursor::Cursor {
    use crate::grabs::ResizeEdge;
    use smithay::reexports::winit::cursor::CursorIcon;

    let icon = match edge {
        Some(e) if e == ResizeEdge::TOP_LEFT || e == ResizeEdge::BOTTOM_RIGHT => {
            CursorIcon::NwseResize
        }
        Some(e) if e == ResizeEdge::TOP_RIGHT || e == ResizeEdge::BOTTOM_LEFT => {
            CursorIcon::NeswResize
        }
        Some(e) if e.intersects(ResizeEdge::LEFT | ResizeEdge::RIGHT) => CursorIcon::EwResize,
        Some(e) if e.intersects(ResizeEdge::TOP | ResizeEdge::BOTTOM) => CursorIcon::NsResize,
        _ => CursorIcon::Default,
    };
    icon.into()
}

/// Number of virtual outputs to simulate in the nested dev session, from
/// `METIS_VIRTUAL_OUTPUTS` (default 1, clamped 1..=2). `2` tiles the winit window
/// into two side-by-side logical monitors so multi-output behavior (per-output
/// bars, placement, workspaces) can be exercised before a real DRM backend.
fn virtual_output_count() -> usize {
    std::env::var("METIS_VIRTUAL_OUTPUTS")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(1)
        .clamp(1, 2)
}

/// Tile the winit framebuffer into `count` side-by-side logical outputs, each as
/// `(mode size, global position)`. The outputs tile the global coordinate space
/// contiguously, so rendering everything in global coords fills the window.
fn output_layout(
    window: Size<i32, Physical>,
    count: usize,
) -> Vec<(Size<i32, Physical>, Point<i32, Logical>)> {
    let w = window.w.max(1);
    let h = window.h.max(1);
    if count >= 2 {
        let left = w / 2;
        vec![
            (Size::from((left, h)), Point::from((0, 0))),
            (Size::from((w - left, h)), Point::from((left, 0))),
        ]
    } else {
        vec![(Size::from((w, h)), Point::from((0, 0)))]
    }
}

/// On-screen rectangle of the Metis bar layer surface, used to position the
/// backdrop blur. Returned in the output's local physical coordinates; the caller
/// offsets by the output's global origin. `None` when the bar is not (yet) mapped.
fn bar_layer_rect(output: &Output) -> Option<Rectangle<i32, Physical>> {
    let map = smithay::desktop::layer_map_for_output(output);
    for layer in map.layers() {
        if layer.namespace() == "metis-bar" {
            if let Some(geo) = map.layer_geometry(layer) {
                return Some(geo.to_physical(1));
            }
        }
    }
    None
}

pub fn init_winit(
    event_loop: &mut EventLoop<'_, MetisState>,
    state: &mut MetisState,
) -> Result<(), Box<dyn std::error::Error>> {
    let (backend, winit) = winit::init::<GlesRenderer>()?;
    let backend = Rc::new(RefCell::new(backend));

    let window_size = backend.borrow().window_size();
    let count = virtual_output_count();
    let layout = output_layout(window_size, count);
    if count > 1 {
        tracing::info!(count, "METIS_VIRTUAL_OUTPUTS: simulating multiple outputs");
    }

    // Client-visible logical outputs — one per virtual monitor, tiling the window.
    let mut logical_outputs: Vec<Output> = Vec::new();
    for (i, (size, pos)) in layout.iter().enumerate() {
        let output = Output::new(
            format!("metis-{i}"),
            PhysicalProperties {
                size: (0, 0).into(),
                subpixel: Subpixel::Unknown,
                make: "Metis".into(),
                model: "Compositor".into(),
                serial_number: i.to_string(),
            },
        );
        let _global = output.create_global::<MetisState>(&state.display_handle);
        let mode = Mode { size: *size, refresh: 60_000 };
        output.change_current_state(Some(mode), Some(Transform::Flipped180), None, Some(*pos));
        output.set_preferred(mode);
        state.space.map_output(&output, *pos);
        logical_outputs.push(output);
    }

    // Dedicated full-window render output: NOT client-visible and NOT in the
    // Space. It only drives the damage tracker, render scale, wallpaper size, and
    // frame timing so the winit framebuffer is always fully covered no matter how
    // many logical outputs tile it. With one output it matches `metis-0` exactly.
    let render_output = Output::new(
        "metis-render".to_string(),
        PhysicalProperties {
            size: (0, 0).into(),
            subpixel: Subpixel::Unknown,
            make: "Metis".into(),
            model: "Compositor".into(),
            serial_number: "render".into(),
        },
    );
    let render_mode = Mode { size: window_size, refresh: 60_000 };
    render_output.change_current_state(
        Some(render_mode),
        Some(Transform::Flipped180),
        None,
        Some((0, 0).into()),
    );
    render_output.set_preferred(render_mode);

    if let Some(geo) = logical_outputs
        .first()
        .and_then(|o| state.space.output_geometry(o))
    {
        state.monitor.width = geo.size.w;
        state.monitor.height = geo.size.h;
    }
    // Compose the wallpaper per output: each logical monitor gets its own
    // cover-cropped image blitted into the shared framebuffer texture.
    let (wp_full, wp_regions) = state.wallpaper_layout();
    state.wallpaper.set_layout(wp_full, wp_regions);

    if state.wallpaper.enabled() {
        state.wallpaper.start_async_decode();
    }

    let mut damage_tracker = OutputDamageTracker::from_output(&render_output);
    let mut frame_age = 0usize;

    let backend_winit = backend.clone();
    state.set_redraw_trigger(Rc::new(move || {
        backend_winit.borrow().window().request_redraw();
    }));
    state.damaged = true;
    state.request_redraw();

    // Self-paced ~60fps heartbeat. The nested host does NOT throttle
    // RedrawRequested to vsync, so driving redraws from the Redraw handler
    // (re-request at end of frame) becomes an unbounded busy loop. Instead we
    // re-arm here only while damage is pending, capping us at ~60fps while
    // staying near-zero CPU when idle.
    event_loop
        .handle()
        .insert_source(Timer::from_duration(Duration::from_millis(16)), move |_, _, state| {
            // Drive the startup state machine from the heartbeat (not from
            // rendering) so going idle can never starve shell/client spawn.
            state.run_pending_startup();

            // Service shell IPC every tick, not just on render. Redraws are
            // damage-gated, so when the compositor is idle the `Redraw` handler
            // never fires — which previously left shell commands (taskbar
            // minimize/activate, etc.) unread until the 400ms client timeout
            // expired with `EAGAIN`. handle_ipc flags damage as needed below.
            ipc::drain_ipc(state);

            // Advance the debounced wallpaper decode off the render path and
            // repaint while it settles, so a re-decode (e.g. after maximize)
            // shows up without waiting for an unrelated damage event.
            if state.wallpaper.tick_decode() {
                state.damaged = true;
            }

            // Pick up live blur on/off + radius / bar-position changes from bar.json
            // (throttled to ~1s). A position change alters whether the bar reserves
            // screen space (top/bottom) or overlays it (left/right).
            let (blur_changed, bar_position_changed) = state.blur.maybe_refresh();
            if blur_changed {
                state.damaged = true;
            }
            if bar_position_changed {
                state.last_bar_position = state.blur.position;
                state.reflow_for_bar_geometry_change();
            }

            // Same for the configurable titlebar opacity + window border. A border
            // thickness change also resizes clients (the body inset changed), so
            // re-apply every window's rect before redrawing.
            let deco = state.decorations.maybe_refresh();
            if deco.damage {
                state.damaged = true;
            }
            if deco.relayout {
                let ids: Vec<u32> = state.windows.ids();
                for id in ids {
                    state.apply_window_rect(id);
                }
                state.sync_all_app_windows();
                state.damaged = true;
            }

            // Frame callbacks are delivered after each render (see Redraw), not
            // on a fixed clock — that keeps GTK's frame clock from spinning when
            // nothing changed. Every client commit schedules a redraw, so a
            // client waiting on its first frame callback is unblocked promptly.
            if state.damaged {
                state.request_redraw();
            }
            TimeoutAction::ToDuration(Duration::from_millis(16))
        })?;

    // Persistent identity for the snap-zone overlay so the damage tracker treats
    // it as one stable element; the commit only bumps when the target rect moves.
    let snap_overlay_id = Id::new();
    let mut snap_overlay_commit = CommitCounter::default();
    let mut last_snap_rect: Option<metis_grid::PixelRect> = None;

    let backend_winit = backend.clone();
    event_loop.handle().insert_source(winit, move |event, _, state| {
        match event {
            WinitEvent::Resized { size, .. } => {
                state.run_pending_startup();
                // Re-tile the logical outputs across the new framebuffer size.
                let layout = output_layout(size, logical_outputs.len().max(1));
                for (out, (out_size, pos)) in logical_outputs.iter().zip(layout.iter()) {
                    out.change_current_state(
                        Some(Mode { size: *out_size, refresh: 60_000 }),
                        Some(Transform::Flipped180),
                        None,
                        Some(*pos),
                    );
                    state.space.map_output(out, *pos);
                }
                render_output.change_current_state(
                    Some(Mode { size, refresh: 60_000 }),
                    Some(Transform::Flipped180),
                    None,
                    None,
                );
                if let Some(geo) = logical_outputs
                    .first()
                    .and_then(|o| state.space.output_geometry(o))
                {
                    state.monitor.width = geo.size.w;
                    state.monitor.height = geo.size.h;
                }
                let (wp_full, wp_regions) = state.wallpaper_layout();
                state.wallpaper.set_layout(wp_full, wp_regions);
                state.emit_monitor_changed();
                frame_age = 0;
                let ids: Vec<u32> = state.windows.ids();
                for id in ids {
                    state.apply_window_rect(id);
                }
                state.sync_all_app_windows();
                state.arrange_layers();
            }
            WinitEvent::Input(event) => {
                state.run_pending_startup();
                state.process_input_event(event);
            }
            WinitEvent::Redraw => {
                state.run_pending_startup();
                ipc::drain_ipc(state);

                // Damage-gated render keeps idle CPU near zero. Input handlers
                // flag damage on pointer motion/clicks, so cursor and UI feedback
                // still update during interaction; the heartbeat caps us at 60fps.
                let render = state.damaged;

                frame_age = 0;

                if render {
                    let mut backend = backend_winit.borrow_mut();
                    let size = backend.window_size();
                    let damage = Rectangle::from_size(size);

                    // Single cursor source: always show the host (winit) cursor and
                    // never render client cursor surfaces. In this nested compositor,
                    // rendering the client's own cursor produced a second cursor with a
                    // mismatched size over GTK surfaces; the host cursor stays uniform.
                    backend.window().set_cursor_visible(true);
                    backend.window().set_cursor(resize_cursor(state.hover_cursor));

                    match backend.bind() {
                        Ok((renderer, mut framebuffer)) => {
                            state.wallpaper.poll_decode();
                            state.wallpaper.ensure(renderer);

                            let wallpaper_owned = state.wallpaper.render_element();

                            // Build the bar backdrop-blur element per output (each
                            // output may carry its own bar). Sampled from the
                            // wallpaper under the bar through a Gaussian shader and
                            // drawn below the bar surface, above wallpaper/windows.
                            let bar_rects: Vec<Rectangle<i32, Physical>> = state
                                .space
                                .outputs()
                                .filter_map(|out| {
                                    let origin =
                                        state.space.output_geometry(out)?.loc.to_physical(1);
                                    let local = bar_layer_rect(out)?;
                                    Some(Rectangle::new(local.loc + origin, local.size))
                                })
                                .collect();
                            state.blur.ensure_program(renderer);
                            let blur_elements: Vec<crate::blur::BlurElement> = bar_rects
                                .into_iter()
                                .filter_map(|r| {
                                    let rect = state.blur.confine_to_pill(r);
                                    let (tex, tex_size) = state.wallpaper.texture()?;
                                    state.blur.element(rect, tex, tex_size)
                                })
                                .collect();

                            // Snap-zone drop preview (translucent fill at the
                            // destination), drawn on top of everything during a
                            // titlebar drag. Bump the commit only when it moves.
                            let snap_element = match state.snap_preview {
                                Some((rect, _label)) => {
                                    if last_snap_rect != Some(rect) {
                                        last_snap_rect = Some(rect);
                                        snap_overlay_commit.increment();
                                    }
                                    let geo = Rectangle::<i32, Logical>::new(
                                        Point::from((rect.x, rect.y)),
                                        Size::from((rect.width.max(1), rect.height.max(1))),
                                    )
                                    .to_physical(1);
                                    Some(SolidColorRenderElement::new(
                                        snap_overlay_id.clone(),
                                        geo,
                                        snap_overlay_commit,
                                        Color32F::from(SNAP_OVERLAY_COLOR),
                                        Kind::Unspecified,
                                    ))
                                }
                                None => {
                                    last_snap_rect = None;
                                    None
                                }
                            };

                            // Server-side window decorations (titlebar + border +
                            // controls). Built here so we have the GL renderer for
                            // title-text texture uploads.
                            let deco_elements = {
                                let specs = state.decoration_specs();
                                state.decorations.elements(renderer, &specs)
                            };
                            let crate::decoration::DecoElements {
                                below: mut deco_below,
                                overlay: deco_overlay,
                            } = deco_elements;

                            let output_scale =
                                Scale::from(render_output.current_scale().fractional_scale());

                            // Layer-shell surfaces, gathered from every output's layer
                            // map: background/bottom render beneath windows, top/overlay
                            // above them (the Metis bar is a Top layer). Each surface is
                            // offset by its output's global origin so multi-output bars
                            // land on the right monitor.
                            let mut upper_layer_elems: Vec<OutputStack> = Vec::new();
                            let mut lower_layer_elems: Vec<OutputStack> = Vec::new();
                            let layer_outputs: Vec<Output> =
                                state.space.outputs().cloned().collect();
                            for out in &layer_outputs {
                                let origin = state
                                    .space
                                    .output_geometry(out)
                                    .map(|g| g.loc)
                                    .unwrap_or_default();
                                let map = layer_map_for_output(out);
                                for surface in map.layers().rev() {
                                    let Some(geo) = map.layer_geometry(surface) else {
                                        continue;
                                    };
                                    let loc = (geo.loc + origin)
                                        .to_physical_precise_round(output_scale);
                                    let elems =
                                        AsRenderElements::<GlesRenderer>::render_elements::<
                                            WaylandSurfaceRenderElement<GlesRenderer>,
                                        >(
                                            surface, renderer, loc, output_scale, 1.0
                                        );
                                    let target = if matches!(
                                        surface.layer(),
                                        Layer::Background | Layer::Bottom
                                    ) {
                                        &mut lower_layer_elems
                                    } else {
                                        &mut upper_layer_elems
                                    };
                                    target.extend(elems.into_iter().map(OutputStack::Surface));
                                }
                            }

                            // smithay's damage renderer draws elements front-to-back:
                            // the FIRST element in the slice ends up on top, so the
                            // wallpaper goes last (behind everything).
                            let mut render_elements: Vec<OutputStack> = Vec::new();

                            // Snap preview sits on top of all windows so the drop
                            // destination reads as a ghost over the dragged window.
                            if let Some(snap) = snap_element {
                                render_elements.push(OutputStack::Snap(snap));
                            }
                            // Auto-hide titlebar reveal: drawn ABOVE the client so it
                            // overlays the top of a maximized/snapped window.
                            render_elements.extend(
                                deco_overlay.into_iter().map(OutputStack::Deco),
                            );
                            // Top/overlay layer surfaces (the bar) above all windows.
                            render_elements.extend(upper_layer_elems);
                            // Defensive: chrome whose window can't be matched in the
                            // space stacking below would otherwise render behind every
                            // window (titlebar tucked behind an overlapping app). Draw
                            // any such orphans on top instead — a visible glitch beats
                            // an invisible one — and flag it so we can chase the cause.
                            {
                                let stack_ids: std::collections::HashSet<u32> = state
                                    .space
                                    .elements()
                                    .filter_map(|w| state.windows.id_for_window(w))
                                    .collect();
                                let orphans: Vec<u32> = deco_below
                                    .keys()
                                    .copied()
                                    .filter(|id| !stack_ids.contains(id))
                                    .collect();
                                for id in orphans {
                                    tracing::warn!(id, "deco: window chrome not matched to a stacked window — drawing on top");
                                    if let Some(decos) = deco_below.remove(&id) {
                                        render_elements
                                            .extend(decos.into_iter().map(OutputStack::Deco));
                                    }
                                }
                            }
                            // Windows top-to-bottom, each immediately followed by its
                            // own server-side chrome so an overlapping window can
                            // never hide a lower window's titlebar/border.
                            // `space.elements()` yields bottom-to-top, so reverse it.
                            let stacking: Vec<Window> =
                                state.space.elements().cloned().collect();
                            for window in stacking.iter().rev() {
                                // Match smithay's `render_location`: the surface
                                // origin sits at the mapped location minus the
                                // window-geometry offset (CSD shadow margin). GTK
                                // changes that offset between floating and tiled, so
                                // using the raw map location shifts snapped windows.
                                let loc = (state
                                    .space
                                    .element_location(window)
                                    .unwrap_or_default()
                                    - window.geometry().loc)
                                    .to_physical_precise_round(output_scale);
                                let elems = AsRenderElements::<GlesRenderer>::render_elements::<
                                    WaylandSurfaceRenderElement<GlesRenderer>,
                                >(
                                    window, renderer, loc, output_scale, 1.0
                                );
                                render_elements
                                    .extend(elems.into_iter().map(OutputStack::Surface));
                                if let Some(id) = state.windows.id_for_window(window) {
                                    if let Some(decos) = deco_below.remove(&id) {
                                        render_elements
                                            .extend(decos.into_iter().map(OutputStack::Deco));
                                    }
                                }
                            }
                            // Any remaining chrome belongs to mapped windows that were
                            // handled inline above; nothing should be left here.
                            debug_assert!(deco_below.is_empty());
                            // Background/bottom layer surfaces beneath the windows.
                            render_elements.extend(lower_layer_elems);
                            // Below the windows, above the wallpaper.
                            render_elements
                                .extend(blur_elements.into_iter().map(OutputStack::Blur));
                            if let Some(wallpaper) = wallpaper_owned {
                                render_elements.push(OutputStack::Wallpaper(wallpaper));
                            }

                            if let Err(err) = damage_tracker.render_output(
                                renderer,
                                &mut framebuffer,
                                frame_age,
                                &render_elements,
                                [0.08, 0.09, 0.11, 1.0],
                            ) {
                                tracing::warn!(?err, "render_output failed");
                            }
                        }
                        Err(err) => {
                            tracing::warn!(?err, "winit GL bind failed — skipping frame");
                        }
                    }
                    frame_age = frame_age.saturating_add(1);
                    if let Err(err) = backend.submit(Some(&[damage])) {
                        tracing::warn!(?err, "winit frame submit failed");
                    }
                }

                if render {
                    // Deliver frame callbacks for the frame we just presented so
                    // clients paint their next frame; then clear damage + flush.
                    let now = state.start_time.elapsed();
                    let frame_outputs: Vec<Output> = state.space.outputs().cloned().collect();
                    if let Some(primary) = frame_outputs.first() {
                        let primary = primary.clone();
                        state.space.elements().for_each(|window| {
                            window.send_frame(&primary, now, Some(Duration::ZERO), |_, _| {
                                Some(primary.clone())
                            });
                        });
                    }
                    for out in &frame_outputs {
                        state.send_layer_frames(out, now);
                    }
                    state.damaged = false;
                    state.defer_client_flush = true;
                }

                state.space.refresh();
                state.cleanup_destroyed_windows();
                state.popups.cleanup();
                for out in state.space.outputs() {
                    smithay::desktop::layer_map_for_output(out).cleanup();
                }

            }
            WinitEvent::CloseRequested => {
                tracing::info!("compositor winit window close requested — shutting down");
                state.kill_spawned_clients();
                state.loop_signal.stop();
            }
            _ => (),
        }
    })?;

    Ok(())
}
