use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use gtk::prelude::*;

use crate::config::{
    load_calendars_config, save_calendars_config, AccountKind, CalendarAccount, CalendarsConfig,
};
use crate::services::calendar::{complete_device_login, start_device_login};
use crate::services::secrets;
use crate::services::CalCommand;

type Tx = std::sync::mpsc::Sender<CalCommand>;

pub struct AccountsPage {
    pub widget: gtk::Widget,
}

struct Inner {
    tx: Tx,
    config: RefCell<CalendarsConfig>,
    list: gtk::Box,
}

impl AccountsPage {
    pub fn new(tx: Tx) -> Self {
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .build();
        root.set_width_request(600);

        let list = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .build();
        root.append(&list);

        let inner = Rc::new(Inner {
            tx,
            config: RefCell::new(load_calendars_config()),
            list,
        });

        root.append(&build_add_form(&inner));
        inner.rebuild();

        Self {
            widget: root.upcast(),
        }
    }
}

fn build_add_form(inner: &Rc<Inner>) -> gtk::Widget {
    let frame = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .build();
    frame.add_css_class("metis-acct-form");

    let title = gtk::Label::builder()
        .label("Add account")
        .halign(gtk::Align::Start)
        .build();
    title.add_css_class("metis-bar-section-title");
    frame.append(&title);

    let kinds = ["CalDAV", "Thunderbird", "Microsoft 365", "Local"];
    let kind_dd = gtk::DropDown::from_strings(&kinds);
    frame.append(&kind_dd);

    let name = entry("Display name");
    let url = entry("CalDAV URL (server, calendar, or principal)");
    let username = entry("Username");
    let tenant = entry("MS tenant (e.g. common or your tenant id)");
    let client_id = entry("MS application (client) id");
    let color = entry("Color (e.g. #22d3ee) — optional");
    frame.append(&name);
    frame.append(&url);
    frame.append(&username);
    frame.append(&tenant);
    frame.append(&client_id);
    frame.append(&color);

    let add_btn = gtk::Button::with_label("Add account");
    add_btn.add_css_class("metis-cal-add-btn");
    add_btn.set_halign(gtk::Align::End);
    frame.append(&add_btn);

    let update_visibility = {
        let url = url.clone();
        let username = username.clone();
        let tenant = tenant.clone();
        let client_id = client_id.clone();
        move |idx: u32| {
            let is_caldav = idx == 0;
            let is_ms = idx == 2;
            url.set_visible(is_caldav);
            username.set_visible(is_caldav);
            tenant.set_visible(is_ms);
            client_id.set_visible(is_ms);
        }
    };
    update_visibility(kind_dd.selected());
    {
        let update_visibility = update_visibility.clone();
        kind_dd.connect_selected_notify(move |dd| update_visibility(dd.selected()));
    }

    {
        let inner = inner.clone();
        let kind_dd = kind_dd.clone();
        let name = name.clone();
        let url = url.clone();
        let username = username.clone();
        let tenant = tenant.clone();
        let client_id = client_id.clone();
        let color = color.clone();
        add_btn.connect_clicked(move |_| {
            let kind = match kind_dd.selected() {
                0 => AccountKind::Caldav,
                1 => AccountKind::Thunderbird,
                2 => AccountKind::Ms365,
                _ => AccountKind::Local,
            };
            let display = name.text().trim().to_string();
            let display = if display.is_empty() {
                format!("{kind:?}")
            } else {
                display
            };
            let account = CalendarAccount {
                id: new_account_id(&display),
                kind,
                name: display,
                url: opt(&url),
                username: opt(&username),
                tenant: opt(&tenant),
                client_id: opt(&client_id),
                color: opt(&color),
                enabled: true,
                read_only: false,
            };
            inner.add_account(account);
            for e in [&name, &url, &username, &tenant, &client_id, &color] {
                e.set_text("");
            }
        });
    }

    frame.upcast()
}

impl Inner {
    fn persist_and_reload(&self) {
        let _ = save_calendars_config(&self.config.borrow());
        let _ = self.tx.send(CalCommand::Reload);
    }

    fn add_account(self: &Rc<Self>, account: CalendarAccount) {
        self.config.borrow_mut().accounts.push(account);
        self.persist_and_reload();
        self.rebuild();
    }

    fn remove_account(self: &Rc<Self>, id: &str) {
        self.config.borrow_mut().accounts.retain(|a| a.id != id);
        self.persist_and_reload();
        self.rebuild();
    }

    fn set_enabled(self: &Rc<Self>, id: &str, enabled: bool) {
        if let Some(a) = self
            .config
            .borrow_mut()
            .accounts
            .iter_mut()
            .find(|a| a.id == id)
        {
            a.enabled = enabled;
        }
        self.persist_and_reload();
    }

    fn rebuild(self: &Rc<Self>) {
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        let accounts = self.config.borrow().accounts.clone();
        if accounts.is_empty() {
            let empty = gtk::Label::builder().label("No calendar accounts").build();
            empty.add_css_class("metis-cal-empty");
            self.list.append(&empty);
            return;
        }
        for account in accounts {
            self.list.append(&self.build_row(&account));
        }
    }

    fn build_row(self: &Rc<Self>, account: &CalendarAccount) -> gtk::Widget {
        let row = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(4)
            .build();
        row.add_css_class("metis-acct-row");

        let header = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .build();
        let label = gtk::Label::builder()
            .label(&format!("{}  ·  {:?}", account.name, account.kind))
            .halign(gtk::Align::Start)
            .hexpand(true)
            .build();
        label.add_css_class("metis-acct-name");
        header.append(&label);

        let toggle = gtk::Switch::new();
        toggle.set_active(account.enabled);
        toggle.set_valign(gtk::Align::Center);
        {
            let inner = self.clone();
            let id = account.id.clone();
            toggle.connect_state_set(move |_, state| {
                inner.set_enabled(&id, state);
                glib::Propagation::Proceed
            });
        }
        header.append(&toggle);

        let remove = gtk::Button::from_icon_name("user-trash-symbolic");
        remove.add_css_class("metis-cal-event-action");
        remove.set_valign(gtk::Align::Center);
        {
            let inner = self.clone();
            let id = account.id.clone();
            remove.connect_clicked(move |_| inner.remove_account(&id));
        }
        header.append(&remove);
        row.append(&header);

        match account.kind {
            AccountKind::Caldav => row.append(&self.caldav_password_row(account)),
            AccountKind::Ms365 => row.append(&self.ms_login_row(account)),
            _ => {}
        }

        row.upcast()
    }

    fn caldav_password_row(self: &Rc<Self>, account: &CalendarAccount) -> gtk::Widget {
        let row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(6)
            .build();
        let entry = gtk::PasswordEntry::builder().hexpand(true).build();
        entry.set_show_peek_icon(true);
        let save = gtk::Button::with_label("Save password");
        save.add_css_class("metis-clock-btn");
        let status = gtk::Label::new(None);
        status.add_css_class("metis-acct-status");
        row.append(&entry);
        row.append(&save);

        {
            let id = account.id.clone();
            let tx = self.tx.clone();
            let entry = entry.clone();
            let status = status.clone();
            save.connect_clicked(move |_| {
                let pw = entry.text().to_string();
                if pw.is_empty() {
                    return;
                }
                let id = id.clone();
                let tx = tx.clone();
                let shared = Arc::new(Mutex::new(String::new()));
                run_async({
                    let shared = shared.clone();
                    async move {
                        let msg = match secrets::store(&id, secrets::CALDAV_PASSWORD, &pw).await {
                            Ok(()) => "Password saved".to_string(),
                            Err(e) => format!("Failed: {e}"),
                        };
                        if let Ok(mut g) = shared.lock() {
                            *g = msg;
                        }
                    }
                });
                entry.set_text("");
                poll_status(&status, shared, move || {
                    let _ = tx.send(CalCommand::Refresh);
                });
            });
        }

        let outer = gtk::Box::new(gtk::Orientation::Vertical, 2);
        outer.append(&row);
        outer.append(&status);
        outer.upcast()
    }

    fn ms_login_row(self: &Rc<Self>, account: &CalendarAccount) -> gtk::Widget {
        let outer = gtk::Box::new(gtk::Orientation::Vertical, 4);
        let sign_in = gtk::Button::with_label("Sign in with Microsoft");
        sign_in.add_css_class("metis-clock-btn");
        sign_in.set_halign(gtk::Align::Start);
        let status = gtk::Label::new(None);
        status.add_css_class("metis-acct-status");
        status.set_wrap(true);
        status.set_halign(gtk::Align::Start);
        outer.append(&sign_in);
        outer.append(&status);

        let tenant = account.tenant.clone().unwrap_or_else(|| "common".into());
        let client_id = account.client_id.clone().unwrap_or_default();
        let id = account.id.clone();
        let tx = self.tx.clone();
        {
            let status = status.clone();
            sign_in.connect_clicked(move |btn| {
                if client_id.is_empty() {
                    status.set_label("Set the application (client) id first.");
                    return;
                }
                btn.set_sensitive(false);
                let shared = Arc::new(Mutex::new("Starting sign-in…".to_string()));
                run_async({
                    let shared = shared.clone();
                    let tenant = tenant.clone();
                    let client_id = client_id.clone();
                    let id = id.clone();
                    async move {
                        let msg = match start_device_login(&tenant, &client_id).await {
                            Ok(code) => {
                                if let Ok(mut g) = shared.lock() {
                                    *g = code.message.clone();
                                }
                                match complete_device_login(&id, &tenant, &client_id, &code).await {
                                    Ok(()) => "Signed in.".to_string(),
                                    Err(e) => format!("Sign-in failed: {e}"),
                                }
                            }
                            Err(e) => format!("Could not start sign-in: {e}"),
                        };
                        if let Ok(mut g) = shared.lock() {
                            *g = msg;
                        }
                    }
                });
                let btn = btn.clone();
                let tx = tx.clone();
                poll_status(&status, shared, move || {
                    btn.set_sensitive(true);
                    let _ = tx.send(CalCommand::Reload);
                });
            });
        }

        outer.upcast()
    }
}

fn entry(placeholder: &str) -> gtk::Entry {
    gtk::Entry::builder().placeholder_text(placeholder).build()
}

fn opt(entry: &gtk::Entry) -> Option<String> {
    let t = entry.text().trim().to_string();
    if t.is_empty() {
        None
    } else {
        Some(t)
    }
}

fn new_account_id(name: &str) -> String {
    let slug: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    use std::time::{SystemTime, UNIX_EPOCH};
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{}-{n}", slug.trim_matches('-'))
}

/// Run a future to completion on a throwaway tokio runtime off the GTK thread.
fn run_async<F>(fut: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    std::thread::spawn(move || {
        if let Ok(rt) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            rt.block_on(fut);
        }
    });
}

/// Mirror a worker thread's status string into a label until it stops changing,
/// then fire `on_done` once.
fn poll_status<F: Fn() + 'static>(label: &gtk::Label, shared: Arc<Mutex<String>>, on_done: F) {
    let label = label.clone();
    let last = Rc::new(RefCell::new(String::new()));
    let stable_ticks = Rc::new(std::cell::Cell::new(0u32));
    let done = Rc::new(std::cell::Cell::new(false));
    glib::timeout_add_local(std::time::Duration::from_millis(600), move || {
        let current = shared.lock().map(|g| g.clone()).unwrap_or_default();
        if !current.is_empty() && current != *last.borrow() {
            label.set_label(&current);
            *last.borrow_mut() = current.clone();
            stable_ticks.set(0);
        } else {
            stable_ticks.set(stable_ticks.get() + 1);
        }
        let terminal = current.starts_with("Signed in")
            || current.starts_with("Password saved")
            || current.starts_with("Failed")
            || current.contains("failed");
        if terminal && stable_ticks.get() >= 1 && !done.get() {
            done.set(true);
            on_done();
            return glib::ControlFlow::Break;
        }
        glib::ControlFlow::Continue
    });
}
