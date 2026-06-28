pub mod applications;
pub mod calendar;
mod notifications;
mod notify_dbus;
mod poll;
mod tray;
mod tray_dbus_types;
mod tray_menu;
mod tray_watcher;
pub mod secrets;
pub mod weather;
pub mod windows;
mod workspaces;

pub use applications::{watch_app_index, AppEntry};
pub use calendar::{
    reload_calendars, spawn_calendar_service, CalCommand, Event as CalendarEvent, LocalEvent,
};
pub use notifications::{
    clear_notifications, close_notification, dismiss_notification, invoke_action,
    notification_count, play_notification_sound, push_notification, register_refresh,
    runtime_notifications, set_action_sender, BarNotification, NotificationEntry, NotificationKind,
};
pub use notify_dbus::{spawn_notification_service, NotifyChannels};
pub use poll::{
    set_mic_mute, set_mic_volume_absolute, set_mute, set_volume_absolute, set_volume_relative,
    spawn_bar_pollers, wifi_connect, wifi_scan, wifi_set_radio, BarSnapshot, BluetoothDevice,
    BluetoothStatus, EthernetStatus, WifiNetwork,
};
pub use tray_menu::{MenuItem as TrayMenuItem, MenuType, TrayMenu};
pub use tray::{
    apply_event, register_refresh as register_tray_refresh, send_command, set_command_sender,
    snapshot as tray_snapshot, spawn_tray_service, sync_tray, IconPixmap, TrayCommand, TrayEvent,
    TrayItem, TraySnapshot,
};
pub use weather::{spawn_weather_service, weather_refresh, LocationWeather, WeatherSnapshot};
pub use workspaces::{
    active_workspace_for, dispatch_workspace, set_active_workspace, workspace_snapshot,
    workspace_snapshot_for, WorkspaceSnapshot,
};
