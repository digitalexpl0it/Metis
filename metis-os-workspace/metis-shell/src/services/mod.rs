pub mod calendar;
mod notifications;
mod poll;
pub mod secrets;
mod workspaces;

pub use calendar::{spawn_calendar_service, CalCommand, Event as CalendarEvent, LocalEvent};
pub use notifications::BarNotification;
pub use poll::{
    set_mic_mute, set_mic_volume_absolute, set_mute, set_volume_absolute, set_volume_relative,
    spawn_bar_pollers, BarSnapshot,
};
pub use workspaces::{dispatch_workspace, workspace_snapshot, WorkspaceSnapshot};
