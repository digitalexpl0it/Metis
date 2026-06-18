use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use smithay::{
    backend::{
        renderer::{
            damage::OutputDamageTracker,
            element::{
                render_elements,
                surface::WaylandSurfaceRenderElement,
                texture::TextureRenderElement,
            },
            gles::{GlesRenderer, GlesTexture},
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
    utils::{Rectangle, Size, Transform},
};

use crate::ipc;
use crate::state::MetisState;

render_elements! {
    OutputStack<=GlesRenderer>;
    Wallpaper=TextureRenderElement<GlesTexture>,
    Cursor=WaylandSurfaceRenderElement<GlesRenderer>,
    Space=smithay::desktop::space::SpaceRenderElements<GlesRenderer, WaylandSurfaceRenderElement<GlesRenderer>>,
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
        event_loop
            .handle()
            .insert_source(Timer::from_duration(Duration::from_millis(200)), move |_, _, state| {
                if state.wallpaper.decode_in_flight() {
                    state.request_redraw();
                    TimeoutAction::ToDuration(Duration::from_millis(200))
                } else {
                    TimeoutAction::Drop
                }
            })?;
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

            // Frame callbacks are delivered after each render (see Redraw), not
            // on a fixed clock — that keeps GTK's frame clock from spinning when
            // nothing changed. Every client commit schedules a redraw, so a
            // client waiting on its first frame callback is unblocked promptly.
            if state.damaged {
                state.request_redraw();
            }
            TimeoutAction::ToDuration(Duration::from_millis(16))
        })?;

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
                state.wallpaper.resize(Size::from((size.w, size.h)));
                if state.wallpaper.enabled() {
                    state.wallpaper.start_async_decode();
                }
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

                    match backend.bind() {
                        Ok((renderer, mut framebuffer)) => {
                            state.wallpaper.poll_decode();
                            state.wallpaper.ensure(renderer);

                            let wallpaper_owned = state.wallpaper.render_element();

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
                            render_elements.extend(
                                space_render_elements
                                    .into_iter()
                                    .map(OutputStack::Space),
                            );
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
