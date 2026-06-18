#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationKind {
    Error,
    Notification,
    Success,
    Information,
    Payment,
}

impl NotificationKind {
    pub fn css_suffix(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Notification => "notify",
            Self::Success => "success",
            Self::Information => "info",
            Self::Payment => "payment",
        }
    }

    pub fn icon_glyph(self) -> &'static str {
        match self {
            Self::Error => "!",
            Self::Notification => "🔔",
            Self::Success => "✓",
            Self::Information => "i",
            Self::Payment => "$",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BarNotification {
    pub kind: NotificationKind,
    pub title: String,
    pub message: String,
}

/// Placeholder feed until freedesktop notification D-Bus is wired.
pub fn demo_notifications() -> Vec<BarNotification> {
    vec![
        BarNotification {
            kind: NotificationKind::Error,
            title: "Error".into(),
            message: "System error occurred".into(),
        },
        BarNotification {
            kind: NotificationKind::Notification,
            title: "Notification".into(),
            message: "You've been hired as a driver".into(),
        },
        BarNotification {
            kind: NotificationKind::Success,
            title: "Success".into(),
            message: "You've completed all the tasks".into(),
        },
        BarNotification {
            kind: NotificationKind::Information,
            title: "Information".into(),
            message: "You received a fine of $10,000".into(),
        },
        BarNotification {
            kind: NotificationKind::Payment,
            title: "Payment".into(),
            message: "You purchased 10x bread".into(),
        },
        BarNotification {
            kind: NotificationKind::Success,
            title: "Success".into(),
            message: "Workspace layout saved".into(),
        },
        BarNotification {
            kind: NotificationKind::Information,
            title: "Information".into(),
            message: "System update available — restart when convenient".into(),
        },
        BarNotification {
            kind: NotificationKind::Notification,
            title: "Notification".into(),
            message: "New message from Metis Core".into(),
        },
    ]
}
