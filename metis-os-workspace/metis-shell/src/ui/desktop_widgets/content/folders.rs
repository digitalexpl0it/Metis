//! Folders widget — files/folders (default `~/Desktop`), grid or list.
//!
//! - Directories open in the file manager
//! - `.desktop` entries show their app icon (name keeps the `.desktop` suffix)
//!   and launch the application
//! - Other files open via `xdg-open`
//! - Right-click: Open, Open with…, Rename, Delete, New Folder
//! - Sort: folders A–Z, then files A–Z

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk::gdk;
use gtk::gio;
use gtk::gio::prelude::{AppInfoExt, FileExt};
use gtk::glib;
use gtk::prelude::*;
use metis_config::{DesktopWidgetInstance, DesktopWidgetView};

const MAX_ENTRIES: usize = 120;
const TILE_ICON: i32 = 48;
const TILE_WIDTH: i32 = 96;
const LIST_ICON: i32 = 22;

pub fn build(inst: &DesktopWidgetInstance) -> gtk::Widget {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 4);
    root.set_hexpand(true);
    root.set_vexpand(true);

    let path_label = gtk::Label::new(None);
    path_label.add_css_class("metis-dw-hint");
    path_label.set_xalign(0.0);
    path_label.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    root.append(&path_label);

    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .vexpand(true)
        .hexpand(true)
        .build();

    let path = expand_path(&inst.path);
    path_label.set_text(&display_path(&path));
    let path_rc = Rc::new(path.clone());

    let rebuild: Rc<dyn Fn()> = match inst.view {
        DesktopWidgetView::Grid => {
            let flow = gtk::FlowBox::builder()
                .valign(gtk::Align::Start)
                .max_children_per_line(8)
                .min_children_per_line(2)
                .selection_mode(gtk::SelectionMode::None)
                .homogeneous(true)
                .column_spacing(4)
                .row_spacing(4)
                .build();
            flow.add_css_class("metis-dw-folder-grid");
            scroll.set_child(Some(&flow));
            root.append(&scroll);
            attach_background_menu_flow(&flow, path_rc.clone());
            let flow_rc = flow.clone();
            let path_for = path_rc.clone();
            Rc::new(move || rebuild_grid(&flow_rc, &path_for))
        }
        DesktopWidgetView::List => {
            let list = gtk::Box::new(gtk::Orientation::Vertical, 2);
            list.add_css_class("metis-dw-list");
            scroll.set_child(Some(&list));
            root.append(&scroll);
            attach_background_menu_box(&list, path_rc.clone());
            let list_rc = list.clone();
            let path_for = path_rc.clone();
            Rc::new(move || rebuild_list(&list_rc, &path_for))
        }
    };
    rebuild();

    let file = gio::File::for_path(&path);
    if let Ok(monitor) =
        file.monitor_directory(gio::FileMonitorFlags::NONE, None::<&gio::Cancellable>)
    {
        let rebuild = rebuild.clone();
        monitor.connect_changed(move |_, _, _, _| {
            let rebuild = rebuild.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(200), move || {
                rebuild();
            });
        });
        let keep = Rc::new(RefCell::new(Some(monitor)));
        root.connect_destroy(move |_| {
            let _ = keep.borrow_mut().take();
        });
    }

    root.upcast()
}

fn rebuild_grid(flow: &gtk::FlowBox, path: &Path) {
    while let Some(child) = flow.first_child() {
        flow.remove(&child);
    }

    if !path.exists() {
        let empty = gtk::Label::new(Some(&format!("Folder not found:\n{}", path.display())));
        empty.set_wrap(true);
        empty.set_xalign(0.0);
        empty.add_css_class("metis-dw-hint");
        flow.insert(&empty, -1);
        return;
    }

    let entries = match read_entries(path) {
        Ok(e) => e,
        Err(err) => {
            let empty = gtk::Label::new(Some(&format!("Could not read folder:\n{err}")));
            empty.set_wrap(true);
            empty.set_xalign(0.0);
            empty.add_css_class("metis-dw-hint");
            flow.insert(&empty, -1);
            return;
        }
    };

    if entries.is_empty() {
        let empty = gtk::Label::new(Some(&metis_i18n::tr(
            "This folder is empty.\nRight-click for New Folder.",
        )));
        empty.set_wrap(true);
        empty.set_xalign(0.5);
        empty.add_css_class("metis-dw-hint");
        flow.insert(&empty, -1);
        return;
    }

    let parent = path.to_path_buf();
    for (i, entry) in entries.iter().enumerate() {
        if i >= MAX_ENTRIES {
            let more = gtk::Label::new(Some(&format!(
                "…and {} more",
                entries.len().saturating_sub(MAX_ENTRIES)
            )));
            more.add_css_class("metis-dw-hint");
            flow.insert(&more, -1);
            break;
        }
        flow.insert(&entry_tile(entry, &parent), -1);
    }
}

fn rebuild_list(list: &gtk::Box, path: &Path) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }

    if !path.exists() {
        let empty = gtk::Label::new(Some(&format!("Folder not found:\n{}", path.display())));
        empty.set_wrap(true);
        empty.set_xalign(0.0);
        empty.add_css_class("metis-dw-hint");
        list.append(&empty);
        return;
    }

    let entries = match read_entries(path) {
        Ok(e) => e,
        Err(err) => {
            let empty = gtk::Label::new(Some(&format!("Could not read folder:\n{err}")));
            empty.set_wrap(true);
            empty.set_xalign(0.0);
            empty.add_css_class("metis-dw-hint");
            list.append(&empty);
            return;
        }
    };

    if entries.is_empty() {
        let empty = gtk::Label::new(Some(&metis_i18n::tr(
            "This folder is empty.\nRight-click for New Folder.",
        )));
        empty.set_wrap(true);
        empty.set_xalign(0.5);
        empty.add_css_class("metis-dw-hint");
        list.append(&empty);
        return;
    }

    let parent = path.to_path_buf();
    for (i, entry) in entries.iter().enumerate() {
        if i >= MAX_ENTRIES {
            let more = gtk::Label::new(Some(&format!(
                "…and {} more",
                entries.len().saturating_sub(MAX_ENTRIES)
            )));
            more.add_css_class("metis-dw-hint");
            list.append(&more);
            break;
        }
        list.append(&entry_row(entry, &parent));
    }
}

#[derive(Clone)]
struct DirEntry {
    path: PathBuf,
    /// On-disk basename (includes `.desktop` when present).
    name: String,
    is_dir: bool,
    is_desktop: bool,
}

fn read_entries(path: &Path) -> std::io::Result<Vec<DirEntry>> {
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    for ent in std::fs::read_dir(path)? {
        let ent = ent?;
        let name = ent.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let meta = ent.metadata()?;
        let path = ent.path();
        let is_desktop = !meta.is_dir()
            && path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("desktop"));
        let item = DirEntry {
            path,
            name,
            is_dir: meta.is_dir(),
            is_desktop,
        };
        if item.is_dir {
            dirs.push(item);
        } else {
            files.push(item);
        }
    }
    dirs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    files.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    dirs.append(&mut files);
    Ok(dirs)
}

fn entry_tile(entry: &DirEntry, parent_dir: &Path) -> gtk::Widget {
    let btn = gtk::Button::new();
    btn.add_css_class("metis-dw-folder-tile");
    btn.set_has_frame(false);
    btn.set_hexpand(true);
    btn.set_size_request(TILE_WIDTH, -1);
    btn.set_tooltip_text(Some(&entry.name));

    let col = gtk::Box::new(gtk::Orientation::Vertical, 4);
    col.set_halign(gtk::Align::Center);
    col.set_valign(gtk::Align::Start);

    let icon = resolve_icon(entry);
    icon.set_pixel_size(TILE_ICON);
    icon.set_halign(gtk::Align::Center);
    col.append(&icon);

    let label = gtk::Label::new(Some(&entry.name));
    label.add_css_class("metis-dw-folder-name");
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.set_justify(gtk::Justification::Center);
    label.set_lines(2);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    label.set_max_width_chars(12);
    label.set_xalign(0.5);
    col.append(&label);

    btn.set_child(Some(&col));

    let path = entry.path.clone();
    let is_dir = entry.is_dir;
    let is_desktop = entry.is_desktop;
    btn.connect_clicked(move |_| {
        open_path(&path, is_dir, is_desktop);
    });

    attach_entry_menu(&btn, entry, parent_dir);

    btn.upcast()
}

fn entry_row(entry: &DirEntry, parent_dir: &Path) -> gtk::Widget {
    let btn = gtk::Button::new();
    btn.add_css_class("metis-dw-row");
    btn.set_has_frame(false);
    btn.set_halign(gtk::Align::Fill);
    btn.set_hexpand(true);
    btn.set_tooltip_text(Some(&entry.name));

    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let icon = resolve_icon(entry);
    icon.set_pixel_size(LIST_ICON);
    row.append(&icon);

    let label = gtk::Label::new(Some(&entry.name));
    label.set_xalign(0.0);
    label.set_hexpand(true);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    row.append(&label);
    btn.set_child(Some(&row));

    let path = entry.path.clone();
    let is_dir = entry.is_dir;
    let is_desktop = entry.is_desktop;
    btn.connect_clicked(move |_| {
        open_path(&path, is_dir, is_desktop);
    });

    attach_entry_menu(&btn, entry, parent_dir);

    btn.upcast()
}

fn resolve_icon(entry: &DirEntry) -> gtk::Image {
    if entry.is_dir {
        return gtk::Image::from_icon_name("folder");
    }
    if entry.is_desktop {
        if let Some(info) = gio::DesktopAppInfo::from_filename(&entry.path) {
            if let Some(icon) = AppInfoExt::icon(&info) {
                return gtk::Image::from_gicon(&icon);
            }
        }
        return gtk::Image::from_icon_name("application-x-executable");
    }
    let file = gio::File::for_path(&entry.path);
    if let Ok(info) = file.query_info(
        "standard::icon",
        gio::FileQueryInfoFlags::NONE,
        None::<&gio::Cancellable>,
    ) {
        if let Some(icon) = info.icon() {
            return gtk::Image::from_gicon(&icon);
        }
    }
    let (ctype, _) = gio::content_type_guess(Some(entry.path.as_os_str()), &[]);
    gtk::Image::from_gicon(&gio::content_type_get_icon(&ctype))
}

fn open_path(path: &Path, is_dir: bool, is_desktop: bool) {
    if is_dir {
        crate::services::open_in_file_manager(path);
        return;
    }
    if is_desktop {
        launch_desktop_file(path);
        return;
    }
    open_with_default(path);
}

fn launch_desktop_file(path: &Path) {
    let Some(info) = gio::DesktopAppInfo::from_filename(path) else {
        tracing::warn!(path = %path.display(), "not a valid .desktop file");
        open_with_default(path);
        return;
    };
    let exec = info
        .commandline()
        .map(|c| clean_exec(&c.to_string_lossy()))
        .filter(|s| !s.is_empty());
    if let Some(exec) = exec {
        if let Err(err) = crate::compositor::launch_program(&exec) {
            tracing::warn!(%err, exec = %exec, "failed to launch .desktop");
        }
        return;
    }
    if let Err(err) = info.launch(&[], None::<&gio::AppLaunchContext>) {
        tracing::warn!(%err, path = %path.display(), "DesktopAppInfo::launch failed");
        open_with_default(path);
    }
}

fn clean_exec(exec: &str) -> String {
    exec.split_whitespace()
        .filter(|tok| !(tok.len() == 2 && tok.starts_with('%')))
        .collect::<Vec<_>>()
        .join(" ")
}

fn open_with_default(path: &Path) {
    let quoted = shell_dquote(&path.to_string_lossy());
    if let Err(err) = crate::compositor::launch_program(&format!("xdg-open {quoted}")) {
        tracing::warn!(%err, path = %path.display(), "xdg-open failed");
    }
}

fn open_with_picker(path: &Path) {
    let file = gio::File::for_path(path);
    let launcher = gtk::FileLauncher::new(Some(&file));
    let parent = active_window();
    launcher.launch(
        parent.as_ref(),
        None::<&gio::Cancellable>,
        |_| {},
    );
}

fn active_window() -> Option<gtk::Window> {
    gtk::Application::default().active_window()
}

fn shell_dquote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn menu_button(label: &str, popover: &gtk::Popover, on_click: Rc<dyn Fn()>) -> gtk::Button {
    let item = gtk::Button::with_label(label);
    item.set_halign(gtk::Align::Fill);
    item.add_css_class("flat");
    item.add_css_class("metis-dw-menu-item");
    let pop = popover.clone();
    item.connect_clicked(move |_| {
        pop.popdown();
        on_click();
    });
    item
}

fn attach_entry_menu(btn: &gtk::Button, entry: &DirEntry, parent_dir: &Path) {
    let gesture = gtk::GestureClick::builder()
        .button(gdk::BUTTON_SECONDARY)
        .build();

    let entry = entry.clone();
    let parent_dir = parent_dir.to_path_buf();
    let btn_weak = btn.downgrade();

    gesture.connect_pressed(move |_, _, _, _| {
        let Some(btn) = btn_weak.upgrade() else {
            return;
        };
        let popover = gtk::Popover::builder()
            .autohide(true)
            .has_arrow(true)
            .build();
        popover.set_parent(&btn);
        let panel = gtk::Box::new(gtk::Orientation::Vertical, 2);
        panel.set_margin_start(6);
        panel.set_margin_end(6);
        panel.set_margin_top(6);
        panel.set_margin_bottom(6);
        popover.set_child(Some(&panel));

        {
            let path = entry.path.clone();
            let is_dir = entry.is_dir;
            let is_desktop = entry.is_desktop;
            panel.append(&menu_button(
                &metis_i18n::tr("Open"),
                &popover,
                Rc::new(move || open_path(&path, is_dir, is_desktop)),
            ));
        }
        if !entry.is_dir {
            let path = entry.path.clone();
            panel.append(&menu_button(
                &metis_i18n::tr("Open with…"),
                &popover,
                Rc::new(move || open_with_picker(&path)),
            ));
        }
        {
            let path = entry.path.clone();
            let parent = parent_dir.clone();
            let old_name = entry.name.clone();
            panel.append(&menu_button(
                &metis_i18n::tr("Rename…"),
                &popover,
                Rc::new(move || prompt_rename(&parent, &path, &old_name)),
            ));
        }
        {
            let path = entry.path.clone();
            let name = entry.name.clone();
            panel.append(&menu_button(
                &metis_i18n::tr("Delete"),
                &popover,
                Rc::new(move || confirm_delete(&path, &name)),
            ));
        }
        {
            let parent = parent_dir.clone();
            panel.append(&menu_button(
                &metis_i18n::tr("New Folder"),
                &popover,
                Rc::new(move || create_new_folder(&parent)),
            ));
        }
        {
            let parent = parent_dir.clone();
            panel.append(&menu_button(
                &metis_i18n::tr("Open in File Manager"),
                &popover,
                Rc::new(move || crate::services::open_in_file_manager(&parent)),
            ));
        }

        popover.popup();
    });

    btn.add_controller(gesture);
}

fn attach_background_menu_flow(flow: &gtk::FlowBox, parent_dir: Rc<PathBuf>) {
    let gesture = gtk::GestureClick::builder()
        .button(gdk::BUTTON_SECONDARY)
        .propagation_phase(gtk::PropagationPhase::Bubble)
        .build();

    let flow_weak = flow.downgrade();
    gesture.connect_pressed(move |gesture, n_press, x, y| {
        if n_press != 1 {
            return;
        }
        let Some(flow) = flow_weak.upgrade() else {
            return;
        };
        if let Some(child) = flow.child_at_pos(x as i32, y as i32) {
            if child
                .child()
                .and_then(|c| c.downcast::<gtk::Button>().ok())
                .is_some()
            {
                return;
            }
        }
        show_background_menu(flow.upcast_ref::<gtk::Widget>(), &parent_dir, x, y);
        gesture.set_state(gtk::EventSequenceState::Claimed);
    });

    flow.add_controller(gesture);
}

fn attach_background_menu_box(list: &gtk::Box, parent_dir: Rc<PathBuf>) {
    let gesture = gtk::GestureClick::builder()
        .button(gdk::BUTTON_SECONDARY)
        .propagation_phase(gtk::PropagationPhase::Bubble)
        .build();

    let list_weak = list.downgrade();
    gesture.connect_pressed(move |gesture, n_press, x, y| {
        if n_press != 1 {
            return;
        }
        let Some(list) = list_weak.upgrade() else {
            return;
        };
        show_background_menu(list.upcast_ref::<gtk::Widget>(), &parent_dir, x, y);
        gesture.set_state(gtk::EventSequenceState::Claimed);
    });

    list.add_controller(gesture);
}

fn show_background_menu(parent: &impl IsA<gtk::Widget>, parent_dir: &Path, x: f64, y: f64) {
    let popover = gtk::Popover::builder()
        .autohide(true)
        .has_arrow(false)
        .build();
    popover.set_parent(parent);
    let rect = gdk::Rectangle::new(x as i32, y as i32, 1, 1);
    popover.set_pointing_to(Some(&rect));
    let panel = gtk::Box::new(gtk::Orientation::Vertical, 2);
    panel.set_margin_start(6);
    panel.set_margin_end(6);
    panel.set_margin_top(6);
    panel.set_margin_bottom(6);
    popover.set_child(Some(&panel));

    let dir = parent_dir.to_path_buf();
    panel.append(&menu_button(
        &metis_i18n::tr("New Folder"),
        &popover,
        Rc::new(move || create_new_folder(&dir)),
    ));
    let dir = parent_dir.to_path_buf();
    panel.append(&menu_button(
        &metis_i18n::tr("Open in File Manager"),
        &popover,
        Rc::new(move || crate::services::open_in_file_manager(&dir)),
    ));

    popover.popup();
}

fn create_new_folder(parent: &Path) {
    let base = parent.join("New Folder");
    let mut path = base.clone();
    let mut n = 2;
    while path.exists() {
        path = parent.join(format!("New Folder {n}"));
        n += 1;
    }
    if let Err(err) = std::fs::create_dir(&path) {
        tracing::warn!(%err, path = %path.display(), "failed to create folder");
        toast_error(&format!("Could not create folder: {err}"));
    }
}

fn confirm_delete(path: &Path, name: &str) {
    let path = path.to_path_buf();
    let name = name.to_string();
    let dialog = gtk::AlertDialog::builder()
        .modal(true)
        .message(format!("Delete \"{name}\"?"))
        .detail("This cannot be undone.")
        .buttons(["Cancel", "Delete"])
        .default_button(0)
        .cancel_button(0)
        .build();
    dialog.choose(
        active_window().as_ref(),
        None::<&gio::Cancellable>,
        move |result| {
            let Ok(idx) = result else {
                return;
            };
            if idx != 1 {
                return;
            }
            let res = if path.is_dir() {
                std::fs::remove_dir_all(&path)
            } else {
                std::fs::remove_file(&path)
            };
            if let Err(err) = res {
                tracing::warn!(%err, path = %path.display(), "delete failed");
                toast_error(&format!("Could not delete: {err}"));
            }
        },
    );
}

fn prompt_rename(parent: &Path, path: &Path, old_name: &str) {
    let parent = parent.to_path_buf();
    let path = path.to_path_buf();
    let old_name = old_name.to_string();

    let win = gtk::Window::builder()
        .title("Rename")
        .modal(true)
        .default_width(360)
        .resizable(false)
        .build();
    if let Some(app_win) = active_window() {
        win.set_transient_for(Some(&app_win));
    }

    let body = gtk::Box::new(gtk::Orientation::Vertical, 10);
    body.set_margin_start(16);
    body.set_margin_end(16);
    body.set_margin_top(16);
    body.set_margin_bottom(16);
    let entry = gtk::Entry::new();
    entry.set_text(&old_name);
    entry.set_hexpand(true);
    body.append(&entry);

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    actions.set_halign(gtk::Align::End);
    let cancel = gtk::Button::with_label("Cancel");
    let ok = gtk::Button::with_label("Rename");
    ok.add_css_class("suggested-action");
    actions.append(&cancel);
    actions.append(&ok);
    body.append(&actions);
    win.set_child(Some(&body));

    {
        let win = win.clone();
        cancel.connect_clicked(move |_| win.close());
    }
    let do_rename = Rc::new({
        let win = win.clone();
        let entry = entry.clone();
        move || {
            let new_name = entry.text().to_string();
            let new_name = new_name.trim();
            if new_name.is_empty() || new_name == old_name {
                win.close();
                return;
            }
            let dest = parent.join(new_name);
            if dest.exists() {
                toast_error("A file with that name already exists.");
                return;
            }
            if let Err(err) = std::fs::rename(&path, &dest) {
                tracing::warn!(%err, "rename failed");
                toast_error(&format!("Could not rename: {err}"));
                return;
            }
            win.close();
        }
    });
    {
        let do_rename = do_rename.clone();
        ok.connect_clicked(move |_| do_rename());
    }
    {
        let do_rename = do_rename.clone();
        entry.connect_activate(move |_| do_rename());
    }

    win.present();
    entry.grab_focus();
}

fn toast_error(message: &str) {
    crate::ui::toast::show(&crate::services::BarNotification::internal(
        crate::services::NotificationKind::Error,
        "Folders",
        message,
    ));
}

fn expand_path(raw: &str) -> PathBuf {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "~/Desktop" || trimmed == "~" {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join("Desktop");
        }
    }
    if let Some(rest) = trimmed.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(trimmed)
}

fn display_path(path: &Path) -> String {
    if let Ok(home) = std::env::var("HOME") {
        let home_path = PathBuf::from(&home);
        if let Ok(rel) = path.strip_prefix(&home_path) {
            return format!("~/{}", rel.display());
        }
    }
    path.display().to_string()
}
