//! Remote access — desktop session sharing (RDP via gnome-remote-desktop).

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use gtk::prelude::*;

use crate::remote::{self, RemoteSnapshot};
use crate::ui;

struct Sections {
    enable_sw: gtk::Switch,
    status_label: gtk::Label,
    address_label: gtk::Label,
    port_label: gtk::Label,
    username_label: gtk::Label,
    hint_label: gtk::Label,
    error_label: gtk::Label,
    action_error: Rc<RefCell<Option<String>>>,
    password_banner: gtk::Box,
    change_pw_btn: gtk::Button,
    install_banner: gtk::Box,
    toggling: Rc<Cell<bool>>,
}

pub fn build(parent: &gtk::Window) -> gtk::Widget {
    let (scroller, content) = ui::page_for("remote");

    let intro = gtk::Label::new(Some(
        "Session sharing lets another device view and control the Metis session \
         you are already logged into. Remote login to start a separate session \
         will be a different option when it is available.",
    ));
    intro.set_xalign(0.0);
    intro.set_wrap(true);
    intro.add_css_class("metis-settings-hint");
    intro.set_margin_bottom(16);
    content.append(&intro);

    let install_banner = gtk::Box::new(gtk::Orientation::Vertical, 6);
    install_banner.add_css_class("metis-settings-banner");
    install_banner.set_margin_bottom(12);
    install_banner.set_visible(false);
    let install_text = gtk::Label::new(Some(
        "Install gnome-remote-desktop to enable desktop session sharing:\n\
         sudo apt install gnome-remote-desktop",
    ));
    install_text.set_xalign(0.0);
    install_text.set_wrap(true);
    install_text.add_css_class("metis-settings-hint");
    install_banner.append(&install_text);
    content.append(&install_banner);

    let password_banner = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    password_banner.add_css_class("metis-settings-banner");
    password_banner.set_margin_bottom(12);
    password_banner.set_visible(false);
    let pw_text = gtk::Label::new(Some(
        "Set a password before enabling desktop session sharing.",
    ));
    pw_text.set_xalign(0.0);
    pw_text.set_hexpand(true);
    pw_text.set_wrap(true);
    password_banner.append(&pw_text);
    let set_pw_btn = gtk::Button::with_label("Set password…");
    set_pw_btn.add_css_class("suggested-action");
    password_banner.append(&set_pw_btn);
    content.append(&password_banner);

    let (share_card, share_body) = ui::section("Desktop session sharing");

    let error_label = gtk::Label::new(None);
    error_label.set_xalign(0.0);
    error_label.set_wrap(true);
    error_label.add_css_class("metis-settings-error");
    error_label.set_margin_bottom(12);
    error_label.set_visible(false);
    share_body.append(&error_label);

    let (enable_row, enable_sw) =
        ui::switch_row("Allow desktop session sharing");
    share_body.append(&enable_row);
    content.append(&share_card);

    let (status_card, status_body) = ui::section("Connection");
    let status_label = gtk::Label::new(Some("Checking…"));
    status_label.set_xalign(0.0);
    status_body.append(&readout_row("Status", &status_label));

    let address_label = gtk::Label::new(None);
    address_label.set_xalign(0.0);
    address_label.set_selectable(true);
    status_body.append(&readout_row("Address", &address_label));

    let port_label = gtk::Label::new(None);
    port_label.set_xalign(0.0);
    status_body.append(&readout_row("Port", &port_label));

    let username_label = gtk::Label::new(None);
    username_label.set_xalign(0.0);
    status_body.append(&readout_row("Username", &username_label));

    let change_pw_btn = gtk::Button::with_label("Change password…");
    change_pw_btn.set_halign(gtk::Align::Start);
    change_pw_btn.set_visible(false);

    let copy_btn = gtk::Button::with_label("Copy connection address");
    copy_btn.set_halign(gtk::Align::Start);

    let clients_hint = gtk::Label::new(Some(
        "Connect with Microsoft Remote Desktop, Remmina, or FreeRDP. \
         Use the username and password you set above — empty credentials will not work.",
    ));
    clients_hint.set_xalign(0.0);
    clients_hint.set_wrap(true);
    clients_hint.add_css_class("metis-settings-hint");

    let actions = gtk::Box::new(gtk::Orientation::Vertical, 8);
    actions.add_css_class("metis-settings-actions");
    actions.append(&change_pw_btn);
    actions.append(&copy_btn);
    actions.append(&clients_hint);
    status_body.append(&actions);
    content.append(&status_card);

    let (sec_card, sec_body) = ui::section("Security");
    let hint_label = gtk::Label::new(Some(
        "Keep session sharing on your local network when possible. Use a strong password. \
         Sharing pauses while the session is locked (Super+L). \
         See docs/USER_GUIDE.md for firewall setup (ufw allow 3389/tcp).",
    ));
    hint_label.set_xalign(0.0);
    hint_label.set_wrap(true);
    hint_label.add_css_class("metis-settings-hint");
    sec_body.append(&hint_label);
    content.append(&sec_card);

    let (login_card, login_body) = ui::section("Remote login");
    let login_hint = gtk::Label::new(Some(
        "Sign in remotely to start a new desktop session (for example xrdp) — planned \
         for a later milestone. This page only covers sharing the session you are \
         already in.",
    ));
    login_hint.set_xalign(0.0);
    login_hint.set_wrap(true);
    login_hint.add_css_class("metis-settings-hint");
    login_body.append(&login_hint);
    content.append(&login_card);

    let password_ui_open = Rc::new(Cell::new(false));
    let password_dialog = Rc::new(RefCell::new(None::<gtk::Window>));

    let toggling = Rc::new(Cell::new(false));
    let action_error = Rc::new(RefCell::new(None::<String>));
    let sections = Rc::new(Sections {
        enable_sw,
        status_label,
        address_label,
        port_label,
        username_label,
        hint_label,
        error_label,
        action_error: action_error.clone(),
        password_banner,
        change_pw_btn: change_pw_btn.clone(),
        install_banner,
        toggling: toggling.clone(),
    });

    let (tx, rx) = mpsc::channel::<RemoteSnapshot>();
    let (action_tx, action_rx) = mpsc::channel::<(bool, Result<(), String>)>();
    let (cred_tx, cred_rx) = mpsc::channel::<Result<(), String>>();
    let refresh = {
        let tx = tx.clone();
        Rc::new(move || {
            let tx = tx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(remote::load_snapshot());
            });
        })
    };

    {
        let sections_poll = sections.clone();
        let refresh_after_toggle = refresh.clone();
        let password_ui_open_poll = password_ui_open.clone();
        glib::timeout_add_local(Duration::from_millis(200), move || {
            while let Ok(result) = cred_rx.try_recv() {
                match result {
                    Ok(()) => refresh_after_toggle(),
                    Err(err) => {
                        *sections_poll.action_error.borrow_mut() = Some(err.clone());
                        sections_poll.error_label.set_text(&err);
                        sections_poll.error_label.set_visible(true);
                    }
                }
            }
            while let Ok((active, result)) = action_rx.try_recv() {
                if let Err(err) = result {
                    sections_poll.toggling.set(true);
                    sections_poll.enable_sw.set_active(!active);
                    sections_poll.toggling.set(false);
                    *sections_poll.action_error.borrow_mut() = Some(err.clone());
                    sections_poll.error_label.set_text(&err);
                    sections_poll.error_label.set_visible(true);
                } else {
                    *sections_poll.action_error.borrow_mut() = None;
                    sections_poll.error_label.set_visible(false);
                }
                refresh_after_toggle();
            }
            if !password_ui_open_poll.get() {
                if let Ok(snap) = rx.try_recv() {
                    render(&sections_poll, &snap);
                }
            } else {
                while rx.try_recv().is_ok() {}
            }
            glib::ControlFlow::Continue
        });
        let refresh_periodic = refresh.clone();
        let password_ui_open_periodic = password_ui_open.clone();
        glib::timeout_add_seconds_local(5, move || {
            if !password_ui_open_periodic.get() {
                refresh_periodic();
            }
            glib::ControlFlow::Continue
        });
    }

    {
        let sections_sw = sections.clone();
        let action_tx = action_tx.clone();
        ui::defer_switch_active_notify(&sections.enable_sw, move |active| {
            if sections_sw.toggling.get() {
                return;
            }
            *sections_sw.action_error.borrow_mut() = None;
            let action_tx = action_tx.clone();
            std::thread::spawn(move || {
                let result = if active {
                    remote::enable_sharing()
                } else {
                    remote::disable_sharing()
                };
                let _ = action_tx.send((active, result));
            });
        });
    }

    let open_password = {
        let parent = parent.clone();
        let cred_tx = cred_tx.clone();
        let password_ui_open = password_ui_open.clone();
        let password_dialog = password_dialog.clone();
        Rc::new(move || {
            show_password_dialog(
                &parent,
                cred_tx.clone(),
                password_ui_open.clone(),
                password_dialog.clone(),
            );
        })
    };

    for btn in [&set_pw_btn, &change_pw_btn] {
        let open_password = open_password.clone();
        btn.connect_clicked(move |_| open_password());
    }

    {
        let sections_copy = sections.clone();
        copy_btn.connect_clicked(move |_| {
            let text = remote::connection_hint(&remote::load_snapshot());
            let display = gtk::gdk::Display::default();
            if let Some(display) = display {
                display.clipboard().set_text(&text);
            }
            sections_copy
                .hint_label
                .set_text(&format!("Copied: {text}"));
        });
    }

    refresh();
    scroller.upcast()
}

fn readout_row(title: &str, value: &gtk::Label) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.add_css_class("metis-settings-row");
    let title = gtk::Label::new(Some(title));
    title.set_xalign(0.0);
    title.set_width_chars(10);
    row.append(&title);
    value.add_css_class("metis-settings-value");
    value.set_hexpand(true);
    row.append(value);
    row
}

fn render(sections: &Sections, snap: &RemoteSnapshot) {
    sections.install_banner.set_visible(!snap.available);
    sections.password_banner.set_visible(snap.available && !snap.password_set);
    sections.change_pw_btn.set_visible(snap.available && snap.password_set);

    sections.toggling.set(true);
    sections.enable_sw.set_sensitive(snap.available && snap.password_set);
    // Reflect user intent from remote.json — not only live RDP daemon state.
    sections.enable_sw.set_active(snap.config_enabled);
    sections.toggling.set(false);

    if !snap.available {
        sections.status_label.set_text("Not available");
        sections.address_label.set_text("—");
        sections.port_label.set_text("—");
        sections.username_label.set_text("—");
    } else if snap.rdp_enabled {
        sections.status_label.set_text("Running — ready for connections");
        sections.address_label.set_text(&remote::connection_hint(snap));
        sections.port_label.set_text(&snap.port.to_string());
        sections.username_label.set_text(
            snap.username
                .as_deref()
                .filter(|u| !u.eq_ignore_ascii_case("(hidden)"))
                .unwrap_or(if snap.password_set {
                    "Use your session sharing password"
                } else {
                    "—"
                }),
        );
    } else if snap.config_enabled && !snap.rdp_enabled {
        sections.status_label.set_text("Starting…");
        sections.address_label.set_text(&remote::connection_hint(snap));
        sections.port_label.set_text(&snap.port.to_string());
        sections.username_label.set_text(
            snap.username
                .as_deref()
                .filter(|u| !u.eq_ignore_ascii_case("(hidden)"))
                .unwrap_or(if snap.password_set {
                    "Use your session sharing password"
                } else {
                    "—"
                }),
        );
    } else {
        sections.status_label.set_text("Stopped");
        sections.address_label.set_text(&remote::connection_hint(snap));
        sections.port_label.set_text(&snap.port.to_string());
        sections.username_label.set_text(
            snap.username
                .as_deref()
                .filter(|u| !u.eq_ignore_ascii_case("(hidden)"))
                .unwrap_or(if snap.password_set {
                    "Use your session sharing password"
                } else {
                    "—"
                }),
        );
    }

    if let Some(err) = snap
        .error
        .as_deref()
        .or(sections.action_error.borrow().as_deref())
    {
        sections.error_label.set_text(err);
        sections.error_label.set_visible(true);
    } else {
        sections.error_label.set_visible(false);
    }
}

/// Centered modal sheet over Settings — undecorated so Metis does not add a
/// second compositor titlebar; in-dialog header supplies title + close.
fn show_password_dialog(
    parent: &gtk::Window,
    cred_tx: mpsc::Sender<Result<(), String>>,
    password_ui_open: Rc<Cell<bool>>,
    password_dialog: Rc<RefCell<Option<gtk::Window>>>,
) {
    if let Some(existing) = password_dialog.borrow().as_ref() {
        existing.present();
        return;
    }

    password_ui_open.set(true);

    let dialog = gtk::Window::builder()
        .title("Session sharing password")
        .modal(true)
        .transient_for(parent)
        .decorated(false)
        .resizable(false)
        .default_width(440)
        .build();
    dialog.add_css_class("metis-settings-window");
    dialog.add_css_class("metis-settings-password-dialog");

    let outer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    outer.set_margin_top(16);
    outer.set_margin_bottom(16);
    outer.set_margin_start(20);
    outer.set_margin_end(20);

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    header.set_margin_bottom(12);
    let heading = gtk::Label::new(Some("Session sharing password"));
    heading.set_xalign(0.0);
    heading.set_hexpand(true);
    heading.add_css_class("metis-settings-section-title");
    header.append(&heading);
    let close_btn = gtk::Button::with_label("Close");
    close_btn.add_css_class("metis-settings-secondary");
    header.append(&close_btn);
    outer.append(&header);

    let hint = gtk::Label::new(Some(
        "Choose the username and password RDP clients use to join this session.",
    ));
    hint.set_xalign(0.0);
    hint.set_wrap(true);
    hint.add_css_class("metis-settings-hint");
    hint.set_margin_bottom(12);
    outer.append(&hint);

    let user_entry = gtk::Entry::new();
    user_entry.set_placeholder_text(Some("Username"));
    user_entry.set_hexpand(true);
    if let Ok(user) = std::env::var("USER") {
        user_entry.set_text(&user);
    }
    ui::swallow_empty_backspace(&user_entry);
    outer.append(&user_entry);

    let pass_entry = gtk::Entry::new();
    pass_entry.set_placeholder_text(Some("Password"));
    pass_entry.set_visibility(false);
    pass_entry.set_input_purpose(gtk::InputPurpose::Password);
    pass_entry.set_hexpand(true);
    pass_entry.set_margin_top(8);
    ui::swallow_empty_backspace(&pass_entry);
    outer.append(&pass_entry);

    let confirm_entry = gtk::Entry::new();
    confirm_entry.set_placeholder_text(Some("Confirm password"));
    confirm_entry.set_visibility(false);
    confirm_entry.set_input_purpose(gtk::InputPurpose::Password);
    confirm_entry.set_hexpand(true);
    confirm_entry.set_margin_top(8);
    ui::swallow_empty_backspace(&confirm_entry);
    outer.append(&confirm_entry);

    let err = gtk::Label::new(None);
    err.set_xalign(0.0);
    err.set_wrap(true);
    err.add_css_class("metis-settings-error");
    err.set_margin_top(8);
    err.set_visible(false);
    outer.append(&err);

    let btn_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    btn_row.set_halign(gtk::Align::End);
    btn_row.set_margin_top(16);
    let cancel = gtk::Button::with_label("Cancel");
    cancel.add_css_class("metis-settings-secondary");
    let save = gtk::Button::with_label("Save");
    save.add_css_class("suggested-action");
    btn_row.append(&cancel);
    btn_row.append(&save);
    outer.append(&btn_row);

    dialog.set_child(Some(&outer));

    *password_dialog.borrow_mut() = Some(dialog.clone());

    let dismiss = {
        let dialog = dialog.clone();
        let password_ui_open = password_ui_open.clone();
        let password_dialog = password_dialog.clone();
        Rc::new(move || {
            password_ui_open.set(false);
            *password_dialog.borrow_mut() = None;
            dialog.destroy();
        })
    };

    close_btn.connect_clicked({
        let dismiss = dismiss.clone();
        move |_| dismiss()
    });
    cancel.connect_clicked({
        let dismiss = dismiss.clone();
        move |_| dismiss()
    });

    let (save_tx, save_rx) = mpsc::channel::<Result<(), String>>();

    save.connect_clicked({
        let save = save.clone();
        let user_entry = user_entry.clone();
        let pass_entry = pass_entry.clone();
        let confirm_entry = confirm_entry.clone();
        let err = err.clone();
        let save_tx = save_tx.clone();
        move |_| {
            let user = user_entry.text().to_string();
            let pass = pass_entry.text().to_string();
            let confirm = confirm_entry.text().to_string();
            if pass != confirm {
                err.set_text("Passwords do not match");
                err.set_visible(true);
                return;
            }
            if pass.len() < 8 {
                err.set_text("Use at least 8 characters");
                err.set_visible(true);
                return;
            }
            err.set_visible(false);
            save.set_sensitive(false);
            save.set_label("Saving…");
            let save_tx = save_tx.clone();
            std::thread::spawn(move || {
                let result = remote::set_credentials(&user, &pass);
                let _ = save_tx.send(result);
            });
        }
    });

    let cred_tx_done = cred_tx.clone();
    let dismiss_done = dismiss.clone();
    glib::timeout_add_local(Duration::from_millis(100), move || {
        let Ok(result) = save_rx.try_recv() else {
            return glib::ControlFlow::Continue;
        };
        save.set_sensitive(true);
        save.set_label("Save");
        match result {
            Ok(()) => {
                let _ = cred_tx_done.send(Ok(()));
                dismiss_done();
                glib::ControlFlow::Break
            }
            Err(e) => {
                err.set_text(&e);
                err.set_visible(true);
                glib::ControlFlow::Break
            }
        }
    });

    dialog.connect_destroy({
        let password_ui_open = password_ui_open.clone();
        let password_dialog = password_dialog.clone();
        move |_| {
            password_ui_open.set(false);
            *password_dialog.borrow_mut() = None;
        }
    });

    dialog.present();
    user_entry.grab_focus();
}
