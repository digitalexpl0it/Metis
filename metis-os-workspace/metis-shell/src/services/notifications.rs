use std::cell::RefCell;
use std::rc::Rc;

/// A notification plus how many identical copies have arrived (for de-duped
/// grouping with a count badge).
#[derive(Debug, Clone)]
pub struct NotificationEntry {
    pub notification: BarNotification,
    pub count: u32,
}

thread_local! {
    /// Notifications raised at runtime (timers, alarms, calendar reminders),
    /// newest-first, with identical messages coalesced into a single entry.
    static RUNTIME: RefCell<Vec<NotificationEntry>> = const { RefCell::new(Vec::new()) };
    /// Repaint hook installed by the bar's notifications widget.
    static REFRESH: RefCell<Option<Rc<dyn Fn()>>> = const { RefCell::new(None) };
}

const RUNTIME_CAP: usize = 50;

/// Register a callback the runtime queue invokes whenever it changes so the bar
/// can repaint its notification badge and list.
pub fn register_refresh(cb: Rc<dyn Fn()>) {
    REFRESH.with(|r| *r.borrow_mut() = Some(cb));
}

fn fire_refresh() {
    let cb = REFRESH.with(|r| r.borrow().clone());
    if let Some(cb) = cb {
        cb();
    }
}

/// Push a notification into the in-bar notification popup. Identical
/// notifications (same kind/title/message) are coalesced: the existing entry's
/// count is bumped and it moves to the top instead of stacking duplicates.
pub fn push_notification(notification: BarNotification) {
    RUNTIME.with(|r| {
        let mut list = r.borrow_mut();
        if let Some(pos) = list
            .iter()
            .position(|e| e.notification == notification)
        {
            let mut entry = list.remove(pos);
            entry.count = entry.count.saturating_add(1);
            list.insert(0, entry);
        } else {
            list.insert(0, NotificationEntry { notification, count: 1 });
            list.truncate(RUNTIME_CAP);
        }
    });
    fire_refresh();
}

/// Remove all runtime notifications.
pub fn clear_notifications() {
    RUNTIME.with(|r| r.borrow_mut().clear());
    fire_refresh();
}

/// Snapshot of the runtime notification queue (newest first), grouped.
pub fn runtime_notifications() -> Vec<NotificationEntry> {
    RUNTIME.with(|r| r.borrow().clone())
}

/// Total number of notifications including coalesced duplicates.
pub fn notification_count() -> u32 {
    RUNTIME.with(|r| r.borrow().iter().map(|e| e.count).sum())
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

    /// Symbolic icon name that matches the notification's nature.
    pub fn icon_name(self) -> &'static str {
        match self {
            Self::Error => "dialog-error-symbolic",
            Self::Notification => "alarm-symbolic",
            Self::Success => "emblem-ok-symbolic",
            Self::Information => "dialog-information-symbolic",
            Self::Payment => "emblem-synchronizing-symbolic",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BarNotification {
    pub kind: NotificationKind,
    pub title: String,
    pub message: String,
}

