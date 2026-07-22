pub mod applications;
pub mod audio_viz;
pub mod calendar;
mod dashboard;
pub mod hardware;
mod notifications;
mod clipboard;
mod notify_dbus;
mod poll;
mod tray;
mod tray_dbus_types;
mod tray_menu;
mod tray_watcher;
pub mod secrets;
pub mod weather;
pub mod windows;
mod volumes;
mod workspaces;

pub use applications::{watch_app_index, AppEntry};
pub use audio_viz::{audio_viz_frame, ensure_audio_viz, release_audio_viz, VizFrame};
pub use calendar::{
    reload_calendars, spawn_calendar_service, CalCommand, Event as CalendarEvent, LocalEvent,
};
pub use clipboard::{
    apply_clipboard_event, clear_history, delete_entry, filtered_entries, load_history,
    private_mode, recall_entry, register_refresh as register_clipboard_refresh, page_size,
    runtime_clipboard_entries, set_page_size, set_private_mode, toggle_favorite,
    active_entry_id, ClipboardEntry, ClipboardPage,
};
pub use dashboard::{
    format_bytes, format_rate, format_uptime, kill_process, kill_process_tree, polling_active,
    set_polling_active, short_kernel_version, spawn_dashboard_pollers, DashboardSnapshot, DiskMount,
    GpuTempReading, HealthLevel, ProcessClass, ProcessRow,
};
pub use notifications::{
    clear_notifications, close_notification, dismiss_notification, do_not_disturb,
    invoke_action, notification_count, notify_store_changed, play_notification_sound,
    push_notification, register_refresh, runtime_notifications, set_action_sender,
    set_do_not_disturb, BarNotification, NotificationEntry, NotificationKind,
};
pub use notify_dbus::{spawn_notification_service, NotifyChannels};
pub use poll::{
    bluetooth_set_powered, set_mic_mute, set_mic_volume_absolute, set_mute, set_volume_absolute,
    set_volume_relative, spawn_bar_pollers, vpn_down, vpn_up, wifi_connect, wifi_scan,
    wifi_set_radio, BarSnapshot, BluetoothDevice, BluetoothStatus, EthernetStatus, VpnFeedback,
    VpnStatus, WifiNetwork,
};
pub use tray_menu::{MenuItem as TrayMenuItem, MenuType, TrayMenu};
pub use tray::{
    apply_event, register_context_menu_ready, register_refresh as register_tray_refresh,
    send_command, set_command_sender, snapshot as tray_snapshot, spawn_tray_service, sync_tray,
    IconPixmap, TrayCommand, TrayEvent, TrayItem, TraySnapshot,
};
pub use volumes::{
    activate as volumes_activate, eject as volumes_eject, icon_name as volumes_icon_name,
    mount_volume as volumes_mount, open_in_file_manager, register_refresh as register_volumes_refresh,
    snapshot as volumes_snapshot, unmount as volumes_unmount, VolumeEntry, VolumeKind,
};
pub use weather::{
    last_weather_snapshot, remember_snapshot, spawn_weather_service, weather_refresh,
    LocationWeather, WeatherSnapshot,
};
pub use windows::refresh_taskbars;
pub use workspaces::{
    active_workspace_for, dispatch_workspace, set_active_workspace, workspace_snapshot,
    workspace_snapshot_for, WorkspaceSnapshot,
};
