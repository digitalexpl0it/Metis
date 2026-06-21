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
                Id, Kind,
            },
            gles::{GlesRenderer, GlesTexture},
            utils::CommitCounter,
            Color32F,
        },
        winit::{self, WinitEvent},
    },
    desktop::{
        space::space_render_elements,
        Window,
    },
    output::{Mode, Output, PhysicalProperties, Subpixel},
    reexports::calloop::{
        EventLoop,
        timer::{TimeoutAction, Timer},
    },
    utils::{Logical, Physical, Point, Rectangle, Size, Transform},
};

use crate::ipc;
use crate::state::MetisState;

render_elements! {
    OutputStack<=GlesRenderer>;
    Wallpaper=TextureRenderElement<GlesTexture>,
    Cursor=WaylandSurfaceRenderElement<GlesRenderer>,
    Space=smithay::desktop::space::SpaceRenderElements<GlesRenderer, WaylandSurfaceRenderElement<GlesRenderer>>,
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

/// On-screen rectangle of the Metis bar layer surface, used to position the
/// backdrop blur. `None` when the bar is not (yet) mapped.
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

    let mode = Mode {
        size: backend.borrow().window_size(),
        refresh: 60_000,
    };

    let output = Output::new(
        "metis-0".to_string(),
        PhysicalProperties {
            size: (0, 0).into(),
            subpixel: Subpixel::Unknown,
            make: "Metis".into(),
            model: "Compositor".into(),
            serial_number: "0".into(),
        },
    );
    let _global = output.create_global::<MetisState>(&state.display_handle);
    output.change_current_state(
        Some(mode),
        Some(Transform::Flipped180),
        None,
        Some((0, 0).into()),
    );
    output.set_preferred(mode);

    state.space.map_output(&output, (0, 0));
    if let Some(geo) = state.space.output_geometry(&output) {
        state.monitor.width = geo.size.w;
        state.monitor.height = geo.size.h;
        state
            .wallpaper
            .resize(Size::from((geo.size.w, geo.size.h)));
    }

    if state.wallpaper.enabled() {
        state.wallpaper.start_async_decode();
    }

    let mut damage_tracker = OutputDamageTracker::from_output(&output);
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

            // Advance the debounced wallpaper decode off the render path and
            // repaint while it settles, so a re-decode (e.g. after maximize)
            // shows up without waiting for an unrelated damage event.
            if state.wallpaper.tick_decode() {
                state.damaged = true;
            }

            // Pick up live blur on/off + radius changes written to bar.json
            // (e.g. by a future Settings app), throttled to ~1s.
            if state.blur.maybe_refresh() {
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
                output.change_current_state(
                    Some(Mode {
                        size,
                        refresh: 60_000,
                    }),
                    Some(Transform::Flipped180),
                    None,
                    None,
                );
                state.monitor.width = size.w;
                state.monitor.height = size.h;
                state.wallpaper.schedule_redecode(Size::from((size.w, size.h)));
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

                            // Build the bar backdrop-blur element (samples the
                            // wallpaper under the bar through a Gaussian shader).
                            // Drawn below the bar surface, above wallpaper/windows.
                            let blur_element = {
                                state.blur.ensure_program(renderer);
                                let rect = bar_layer_rect(&output)
                                    .map(|r| state.blur.confine_to_pill(r));
                                match (rect, state.wallpaper.texture()) {
                                    (Some(rect), Some((tex, tex_size))) => {
                                        state.blur.element(rect, tex, tex_size)
                                    }
                                    _ => None,
                                }
                            };

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

                            let space_render_elements = match space_render_elements::<
                                GlesRenderer,
                                Window,
                                _,
                            >(
                                renderer, [&state.space], &output, 1.0,
                            ) {
                                Ok(elements) => elements,
                                Err(err) => {
                                    tracing::warn!(?err, "space_render_elements failed");
                                    Vec::new()
                                }
                            };

                            // smithay's damage renderer draws elements front-to-back:
                            // the FIRST element in the slice ends up on top, so the
                            // wallpaper goes last (behind everything).
                            let mut render_elements = Vec::with_capacity(
                                space_render_elements.len()
                                    + usize::from(wallpaper_owned.is_some()),
                            );
                            // Snap preview sits on top of all windows so the drop
                            // destination reads as a ghost over the dragged window.
                            if let Some(snap) = snap_element {
                                render_elements.push(OutputStack::Snap(snap));
                            }
                            render_elements.extend(
                                space_render_elements
                                    .into_iter()
                                    .map(OutputStack::Space),
                            );
                            // Decorations sit just below the client surfaces (they
                            // only fill the titlebar/border gap, never overlapping
                            // the client buffer) and above the wallpaper/blur.
                            render_elements.extend(
                                deco_elements.into_iter().map(OutputStack::Deco),
                            );
                            // Below the bar (and windows), above the wallpaper.
                            if let Some(blur) = blur_element {
                                render_elements.push(OutputStack::Blur(blur));
                            }
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
                    state.space.elements().for_each(|window| {
                        window.send_frame(&output, now, Some(Duration::ZERO), |_, _| {
                            Some(output.clone())
                        });
                    });
                    state.send_layer_frames(&output, now);
                    state.damaged = false;
                    state.defer_client_flush = true;
                }

                state.space.refresh();
                state.cleanup_destroyed_windows();
                state.popups.cleanup();
                if let Some(out) = state.space.outputs().next() {
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
