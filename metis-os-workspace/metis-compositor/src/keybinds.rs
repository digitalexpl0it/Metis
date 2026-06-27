use smithay::input::keyboard::ModifiersState;

/// Which modifier key activates Metis compositor shortcuts (`Super`+… by default).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeybindMod {
    Super,
    Alt,
    Ctrl,
}

/// Read once from `METIS_MOD` (`super` | `alt` | `ctrl`). Defaults to `Super`.
pub fn keybind_mod() -> KeybindMod {
    static MOD: std::sync::OnceLock<KeybindMod> = std::sync::OnceLock::new();
    *MOD.get_or_init(|| {
        match std::env::var("METIS_MOD")
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str()
        {
            "alt" => KeybindMod::Alt,
            "ctrl" | "control" => KeybindMod::Ctrl,
            _ => KeybindMod::Super,
        }
    })
}

pub fn keybind_mod_label() -> &'static str {
    match keybind_mod() {
        KeybindMod::Super => "Super",
        KeybindMod::Alt => "Alt",
        KeybindMod::Ctrl => "Ctrl",
    }
}

/// True when the configured Metis modifier is held.
pub fn mod_active(modifiers: &ModifiersState) -> bool {
    match keybind_mod() {
        KeybindMod::Super => modifiers.logo,
        KeybindMod::Alt => modifiers.alt,
        KeybindMod::Ctrl => modifiers.ctrl,
    }
}
