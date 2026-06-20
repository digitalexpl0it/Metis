use std::cell::RefCell;
use std::rc::Rc;

thread_local! {
    /// Notifications raised at runtime (timers, alarms, calendar reminders).
    /// Kept newest-first and merged ahead of any poll-provided notifications.
    static RUNTIME: RefCell<Vec<BarNotification>> = const { RefCell::new(Vec::new()) };
    /// Repaint hook installed by the bar's notifications widget.
    static REFRESH: RefCell<Option<Rc<dyn Fn()>>> = const { RefCell::new(None) };
}

const RUNTIME_CAP: usize = 50;

/// Register a callback the runtime queue invokes whenever it changes so the bar
/// can repaint its notification badge and list.
pub fn register_refresh(cb: Rc<dyn Fn()>) {
    REFRESH.with(|r| *r.borrow_mut() = Some(cb));
}

/// Push a notification into the in-bar notification popup (newest first).
pub fn push_notification(notification: BarNotification) {
    RUNTIME.with(|r| {
        let mut list = r.borrow_mut();
        list.insert(0, notification);
        list.truncate(RUNTIME_CAP);
    });
    let cb = REFRESH.with(|r| r.borrow().clone());
    if let Some(cb) = cb {
        cb();
    }
}

/// Snapshot of the runtime notification queue (newest first).
pub fn runtime_notifications() -> Vec<BarNotification> {
    RUNTIME.with(|r| r.borrow().clone())
}

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
