pub mod applications;
pub mod calendar;
mod notifications;
mod notify_dbus;
mod poll;
pub mod secrets;
pub mod weather;
mod workspaces;

pub use applications::AppEntry;
pub use calendar::{
    reload_calendars, spawn_calendar_service, CalCommand, Event as CalendarEvent, LocalEvent,
};
pub use notifications::{
    clear_notifications, notification_count, push_notification, register_refresh,
    runtime_notifications, BarNotification, NotificationEntry, NotificationKind,
};
pub use notify_dbus::spawn_notification_service;
pub use poll::{
    set_mic_mute, set_mic_volume_absolute, set_mute, set_volume_absolute, set_volume_relative,
    spawn_bar_pollers, wifi_connect, wifi_scan, wifi_set_radio, BarSnapshot, EthernetStatus,
    WifiNetwork,
};
pub use weather::{spawn_weather_service, weather_refresh, LocationWeather, WeatherSnapshot};
pub use workspaces::{dispatch_workspace, workspace_snapshot, WorkspaceSnapshot};
