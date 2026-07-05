//! Apply `input.json` to libinput devices and the seat keyboard (xkb + repeat).
//!
//! Only pointer-class devices (mouse, touchpad) receive scroll/accel settings.
//! Gamepads, joysticks, and other non-pointer libinput nodes are tracked for
//! logging but intentionally not configured or forwarded — games read
//! `/dev/input/event*` directly via evdev/SDL/Proton, and libinput does not
//! EVIOCGRAB those nodes, so compositor seat membership does not block them.

use std::time::{Duration, Instant};

use input::{AccelProfile as LiAccelProfile, Device, DeviceCapability, DeviceConfigResult};
use metis_config::{AccelProfile, InputConfig};
use smithay::backend::input::InputEvent;
use smithay::backend::libinput::LibinputInputBackend;
use smithay::input::keyboard::XkbConfig;

use crate::state::MetisState;

pub struct InputRuntime {
    last_check: Instant,
    cached: InputConfig,
    /// libinput devices seen on the DRM session (empty under nested winit).
    devices: Vec<Device>,
    /// Whether the pointer device that last moved/ scrolled is a touchpad.
    active_touchpad: bool,
}

impl InputRuntime {
    pub fn new() -> Self {
        Self {
            last_check: Instant::now(),
            cached: metis_config::load_input_config(),
            devices: Vec::new(),
            active_touchpad: false,
        }
    }

    pub fn initial_keyboard_config() -> InputConfig {
        metis_config::load_input_config()
    }

    pub fn on_device_added(&mut self, mut device: Device) {
        let touchpad = device.config_tap_finger_count() > 0;
        let caps = device_capabilities(&device);
        tracing::info!(
            name = %device.name(),
            touchpad,
            capabilities = %caps,
            "libinput device added — applying input.json where applicable"
        );
        apply_to_device(&self.cached, &mut device);
        self.devices.push(device);
    }

    pub fn on_device_removed(&mut self, device: &Device) {
        tracing::info!(name = %device.name(), "libinput device removed");
        self.devices.retain(|d| d != device);
    }

    pub fn note_pointer_device(&mut self, device: &Device) {
        self.active_touchpad = device.config_tap_finger_count() > 0;
    }

    /// Scroll-wheel / touchpad scroll multiplier from `input.json`.
    pub fn scroll_multiplier(&self) -> f64 {
        let speed = if self.active_touchpad {
            self.cached.touchpad.scroll_speed
        } else {
            self.cached.mouse.scroll_speed
        };
        speed.clamp(0.25, 4.0)
    }

    /// Force a reload from disk and apply to every tracked libinput device.
    /// Returns the loaded config so the caller can refresh the seat keyboard.
    pub fn reload_from_disk(&mut self) -> InputConfig {
        let cfg = metis_config::load_input_config();
        tracing::info!(
            devices = self.devices.len(),
            "reloading input.json"
        );
        self.cached = cfg.clone();
        for device in &mut self.devices {
            apply_to_device(&cfg, device);
        }
        cfg
    }

    /// Throttled re-read of `input.json` (~1s), mirroring the bar.json watcher.
    pub fn maybe_refresh(&mut self) -> Option<InputConfig> {
        if self.last_check.elapsed() < Duration::from_secs(1) {
            return None;
        }
        self.last_check = Instant::now();
        let cfg = metis_config::load_input_config();
        if cfg == self.cached {
            return None;
        }
        tracing::info!("input.json changed — reapplying device + keyboard settings");
        self.cached = cfg.clone();
        for device in &mut self.devices {
            apply_to_device(&cfg, device);
        }
        Some(cfg)
    }
}

pub fn apply_keyboard(state: &mut MetisState, cfg: &InputConfig) {
    let kb = &cfg.keyboard;
    let xkb = XkbConfig {
        rules: "",
        model: "",
        layout: &kb.layout,
        variant: &kb.variant,
        options: kb.merged_xkb_options(),
    };
    if let Some(keyboard) = state.seat.get_keyboard() {
        if let Err(err) = keyboard.set_xkb_config(state, xkb) {
            tracing::warn!(?err, "failed to apply xkb config");
        }
        keyboard.change_repeat_info(kb.repeat_rate_hz, kb.repeat_delay_ms);
    }
}

fn apply_to_device(cfg: &InputConfig, device: &mut Device) {
    if !device.has_capability(DeviceCapability::Pointer) {
        return;
    }

    let touchpad = device.config_tap_finger_count() > 0;
    let mouse = &cfg.mouse;
    let pad = &cfg.touchpad;
    let (speed, profile, natural_scroll, left_handed, tap, tap_drag, dwt) = if touchpad {
        (
            pad.speed,
            pad.accel_profile,
            pad.natural_scroll,
            mouse.left_handed,
            pad.tap_to_click,
            pad.tap_and_drag,
            pad.disable_while_typing,
        )
    } else {
        (
            mouse.speed,
            mouse.accel_profile,
            mouse.natural_scroll,
            mouse.left_handed,
            false,
            false,
            false,
        )
    };

    let name = device.name().into_owned();

    if device.config_accel_is_available() {
        let li_profile = match profile {
            AccelProfile::Flat => LiAccelProfile::Flat,
            AccelProfile::Adaptive => LiAccelProfile::Adaptive,
        };
        log_cfg(
            &name,
            "accel profile",
            device.config_accel_set_profile(li_profile),
        );
        log_cfg(
            &name,
            "pointer speed",
            device.config_accel_set_speed(speed),
        );
    }

    if device.config_left_handed_is_available() {
        log_cfg(
            &name,
            "left-handed",
            device.config_left_handed_set(left_handed),
        );
    }

    if device.config_scroll_has_natural_scroll() {
        log_cfg(
            &name,
            "natural scroll",
            device.config_scroll_set_natural_scroll_enabled(natural_scroll),
        );
    }

    if touchpad {
        log_cfg(
            &name,
            "tap-to-click",
            device.config_tap_set_enabled(tap),
        );
        if device.config_tap_finger_count() > 0 {
            log_cfg(
                &name,
                "tap-and-drag",
                device.config_tap_set_drag_enabled(tap_drag),
            );
        }
        if device.config_dwt_is_available() {
            log_cfg(
                &name,
                "disable-while-typing",
                device.config_dwt_set_enabled(dwt),
            );
        }
    }
}

fn log_cfg(name: &str, setting: &str, result: DeviceConfigResult) {
    if let Err(err) = result {
        tracing::warn!(
            name,
            %setting,
            ?err,
            "libinput setting rejected"
        );
    }
}

fn device_capabilities(device: &Device) -> String {
    let mut caps = Vec::new();
    for (cap, label) in [
        (DeviceCapability::Keyboard, "keyboard"),
        (DeviceCapability::Pointer, "pointer"),
        (DeviceCapability::Touch, "touch"),
        (DeviceCapability::TabletTool, "tablet-tool"),
        (DeviceCapability::TabletPad, "tablet-pad"),
        (DeviceCapability::Gesture, "gesture"),
        (DeviceCapability::Switch, "switch"),
    ] {
        if device.has_capability(cap) {
            caps.push(label);
        }
    }
    if caps.is_empty() {
        "none".into()
    } else {
        caps.join(",")
    }
}

/// Extract the libinput device from a libinput backend event, if any.
pub fn libinput_device_from_event(event: &InputEvent<LibinputInputBackend>) -> Option<Device> {
    use smithay::backend::input::Event;
    match event {
        InputEvent::PointerMotion { event, .. } => Some(event.device()),
        InputEvent::PointerMotionAbsolute { event, .. } => Some(event.device()),
        InputEvent::PointerButton { event, .. } => Some(event.device()),
        InputEvent::PointerAxis { event, .. } => Some(event.device()),
        _ => None,
    }
}
