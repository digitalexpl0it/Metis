//! Sidebar structure — single source of truth for nav order, icons, and page metadata.

use std::sync::OnceLock;

/// Accent hue for the macOS-style icon badge in the sidebar.
#[derive(Clone, Copy)]
pub enum NavHue {
    Blue,
    Purple,
    Pink,
    Orange,
    Teal,
    Green,
    Gray,
    Yellow,
}

impl NavHue {
    pub fn css_class(self) -> &'static str {
        match self {
            Self::Blue => "metis-nav-hue-blue",
            Self::Purple => "metis-nav-hue-purple",
            Self::Pink => "metis-nav-hue-pink",
            Self::Orange => "metis-nav-hue-orange",
            Self::Teal => "metis-nav-hue-teal",
            Self::Green => "metis-nav-hue-green",
            Self::Gray => "metis-nav-hue-gray",
            Self::Yellow => "metis-nav-hue-yellow",
        }
    }
}

pub struct NavItem {
    pub page_id: Option<&'static str>,
    pub title: &'static str,
    pub icon: Option<&'static str>,
    pub hue: Option<NavHue>,
    /// Shown under the page title in the content area.
    pub subtitle: Option<&'static str>,
}

pub const NAV: &[NavItem] = &[
    NavItem {
        page_id: None,
        title: "Displays",
        icon: None,
        hue: None,
        subtitle: None,
    },
    NavItem {
        page_id: Some("display"),
        title: "Display",
        icon: Some("video-display-symbolic"),
        hue: Some(NavHue::Blue),
        subtitle: Some("Arrangement, resolution, scale, and night light"),
    },
    NavItem {
        page_id: None,
        title: "Desktop",
        icon: None,
        hue: None,
        subtitle: None,
    },
    NavItem {
        page_id: Some("appearance"),
        title: "Appearance",
        icon: Some("preferences-desktop-appearance-symbolic"),
        hue: Some(NavHue::Pink),
        subtitle: Some("Theme, colours, wallpaper, edge bar, and windows"),
    },
    NavItem {
        page_id: Some("menu"),
        title: "Metis Menu",
        icon: Some("view-app-grid-symbolic"),
        hue: Some(NavHue::Purple),
        subtitle: Some("Launcher apps and menu panel look"),
    },
    NavItem {
        page_id: Some("weather"),
        title: "Weather",
        icon: Some("weather-few-clouds-symbolic"),
        hue: Some(NavHue::Teal),
        subtitle: Some("Briefing weather card on the edge bar"),
    },
    NavItem {
        page_id: Some("calendars"),
        title: "Calendars",
        icon: Some("x-office-calendar-symbolic"),
        hue: Some(NavHue::Orange),
        subtitle: Some("Calendar accounts for the briefing"),
    },
    NavItem {
        page_id: None,
        title: "Connectivity",
        icon: None,
        hue: None,
        subtitle: None,
    },
    NavItem {
        page_id: Some("network"),
        title: "Network",
        icon: Some("network-wireless-symbolic"),
        hue: Some(NavHue::Blue),
        subtitle: Some("Wi-Fi, Ethernet, DNS, and proxy"),
    },
    NavItem {
        page_id: Some("bluetooth"),
        title: "Bluetooth",
        icon: Some("bluetooth-symbolic"),
        hue: Some(NavHue::Blue),
        subtitle: Some("Pair and manage Bluetooth devices"),
    },
    NavItem {
        page_id: None,
        title: "Input",
        icon: None,
        hue: None,
        subtitle: None,
    },
    NavItem {
        page_id: Some("mouse"),
        title: "Mouse",
        icon: Some("input-mouse-symbolic"),
        hue: Some(NavHue::Gray),
        subtitle: Some("Pointer speed, acceleration, and scrolling"),
    },
    NavItem {
        page_id: Some("touchpad"),
        title: "Touchpad",
        icon: Some("input-touchpad-symbolic"),
        hue: Some(NavHue::Gray),
        subtitle: Some("Gestures, tap-to-click, and natural scroll"),
    },
    NavItem {
        page_id: Some("keyboard"),
        title: "Keyboard",
        icon: Some("input-keyboard-symbolic"),
        hue: Some(NavHue::Gray),
        subtitle: Some("Repeat rate and layout preferences"),
    },
    NavItem {
        page_id: None,
        title: "System",
        icon: None,
        hue: None,
        subtitle: None,
    },
    NavItem {
        page_id: Some("sound"),
        title: "Sound",
        icon: Some("audio-volume-high-symbolic"),
        hue: Some(NavHue::Pink),
        subtitle: Some("Output and input audio devices"),
    },
    NavItem {
        page_id: Some("power"),
        title: "Power",
        icon: Some("battery-level-100-symbolic"),
        hue: Some(NavHue::Green),
        subtitle: Some("Battery, profiles, and idle behaviour"),
    },
    NavItem {
        page_id: Some("printers"),
        title: "Printers",
        icon: Some("printer-symbolic"),
        hue: Some(NavHue::Gray),
        subtitle: Some("Installed printers and system print settings"),
    },
];

pub fn page_ids() -> Vec<&'static str> {
    NAV.iter().filter_map(|item| item.page_id).collect()
}

pub fn meta_for(page_id: &str) -> Option<&'static NavItem> {
    NAV.iter().find(|item| item.page_id == Some(page_id))
}

fn lowercase_titles() -> &'static [String] {
    static TITLES: OnceLock<Vec<String>> = OnceLock::new();
    TITLES.get_or_init(|| {
        NAV.iter()
            .map(|item| {
                if item.page_id.is_some() {
                    item.title.to_ascii_lowercase()
                } else {
                    String::new()
                }
            })
            .collect()
    })
}

/// Whether a sidebar row at `index` should stay visible for `query`.
pub fn row_visible_for_search(index: usize, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let Some(item) = NAV.get(index) else {
        return false;
    };
    let titles = lowercase_titles();
    if item.page_id.is_some() {
        return titles[index].contains(query);
    }
    section_visible_for_search(index, query, titles)
}

fn section_visible_for_search(section_index: usize, query: &str, titles: &[String]) -> bool {
    for (index, item) in NAV.iter().enumerate().skip(section_index + 1) {
        if item.page_id.is_none() {
            break;
        }
        if titles[index].contains(query) {
            return true;
        }
    }
    false
}
