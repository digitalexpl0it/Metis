use std::cell::RefCell;
use std::rc::Rc;

use chrono::{DateTime, Local, Offset, Utc};
use chrono_tz::Tz;
use gtk::prelude::*;

use super::Store;

pub struct WorldClocksPage {
    pub widget: gtk::Widget,
    inner: Rc<WorldInner>,
}

struct WorldInner {
    store: Store,
    cards: gtk::FlowBox,
    times: RefCell<Vec<(Tz, gtk::Label)>>,
}

impl WorldClocksPage {
    pub fn new(store: Store) -> Self {
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(10)
            .build();
        root.set_width_request(600);

        // Add row: timezone search entry + button.
        let add_row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .build();
        let entry = gtk::Entry::builder()
            .placeholder_text("Add a city / timezone (e.g. Europe/London)")
            .hexpand(true)
            .build();
        entry.add_css_class("metis-clock-tz-entry");
        attach_tz_completion(&entry);
        let add_btn = gtk::Button::builder().label("Add").build();
        add_btn.add_css_class("metis-cal-add-btn");
        add_row.append(&entry);
        add_row.append(&add_btn);
        root.append(&add_row);

        let cards = gtk::FlowBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .min_children_per_line(2)
            .max_children_per_line(3)
            .column_spacing(10)
            .row_spacing(10)
            .homogeneous(true)
            .build();
        cards.add_css_class("metis-clock-cards");
        root.append(&cards);

        let inner = Rc::new(WorldInner {
            store,
            cards,
            times: RefCell::new(Vec::new()),
        });

        {
            let inner = inner.clone();
            let entry = entry.clone();
            add_btn.connect_clicked(move |_| inner.add_from_entry(&entry));
        }
        {
            let inner = inner.clone();
            entry.connect_activate(move |entry| inner.add_from_entry(entry));
        }

        inner.rebuild();

        Self {
            widget: root.upcast(),
            inner,
        }
    }

    pub fn refresh(&self) {
        let now = Utc::now();
        for (tz, label) in self.inner.times.borrow().iter() {
            let local: DateTime<Tz> = now.with_timezone(tz);
            label.set_label(&local.format("%-I:%M %p").to_string());
        }
    }
}

impl WorldInner {
    fn add_from_entry(self: &Rc<Self>, entry: &gtk::Entry) {
        let text = entry.text().to_string();
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return;
        }
        let Ok(tz) = trimmed.parse::<Tz>() else {
            entry.add_css_class("metis-entry-error");
            return;
        };
        entry.remove_css_class("metis-entry-error");
        {
            let mut cfg = self.store.borrow_mut();
            if !cfg.world_clocks.iter().any(|z| z == tz.name()) {
                cfg.world_clocks.push(tz.name().to_string());
            }
        }
        self.store.save();
        entry.set_text("");
        self.rebuild();
    }

    fn remove(self: &Rc<Self>, zone: &str) {
        self.store.borrow_mut().world_clocks.retain(|z| z != zone);
        self.store.save();
        self.rebuild();
    }

    fn rebuild(self: &Rc<Self>) {
        while let Some(child) = self.cards.first_child() {
            self.cards.remove(&child);
        }
        self.times.borrow_mut().clear();

        let zones = self.store.borrow().world_clocks.clone();
        if zones.is_empty() {
            let empty = gtk::Label::builder().label("No world clocks yet").build();
            empty.add_css_class("metis-cal-empty");
            self.cards.append(&empty);
            return;
        }

        let now = Utc::now();
        for zone in zones {
            let Ok(tz) = zone.parse::<Tz>() else { continue };
            let card = gtk::Box::builder()
                .orientation(gtk::Orientation::Horizontal)
                .spacing(8)
                .build();
            card.add_css_class("metis-clock-card");

            let info = gtk::Box::new(gtk::Orientation::Vertical, 2);
            info.set_hexpand(true);
            let name = gtk::Label::builder()
                .label(&pretty_zone(&tz))
                .halign(gtk::Align::Start)
                .build();
            name.add_css_class("metis-clock-card-name");
            let offset = gtk::Label::builder()
                .label(&offset_label(&tz, now))
                .halign(gtk::Align::Start)
                .build();
            offset.add_css_class("metis-clock-card-offset");
            info.append(&name);
            info.append(&offset);
            card.append(&info);

            let time = gtk::Label::new(Some(
                &now.with_timezone(&tz).format("%-I:%M %p").to_string(),
            ));
            time.add_css_class("metis-clock-card-time");
            time.set_valign(gtk::Align::Center);
            card.append(&time);

            let remove = gtk::Button::from_icon_name("window-close-symbolic");
            remove.add_css_class("metis-cal-event-action");
            remove.set_tooltip_text(Some("Remove"));
            remove.set_valign(gtk::Align::Center);
            {
                let inner = self.clone();
                let zone = tz.name().to_string();
                remove.connect_clicked(move |_| inner.remove(&zone));
            }
            card.append(&remove);

            self.cards.append(&card);
            self.times.borrow_mut().push((tz, time));
        }
    }
}

fn pretty_zone(tz: &Tz) -> String {
    tz.name()
        .rsplit('/')
        .next()
        .unwrap_or(tz.name())
        .replace('_', " ")
}

fn offset_label(tz: &Tz, now: DateTime<Utc>) -> String {
    let tz_secs = now.with_timezone(tz).offset().fix().local_minus_utc();
    let local_secs = Local::now().offset().fix().local_minus_utc();
    let diff = tz_secs - local_secs;
    let sign = if diff < 0 { "-" } else { "+" };
    let abs = diff.abs();
    let h = abs / 3600;
    let m = (abs % 3600) / 60;
    if diff == 0 {
        "Same as local".to_string()
    } else if m == 0 {
        format!("{sign}{h}h from local")
    } else {
        format!("{sign}{h}h{m:02} from local")
    }
}

// EntryCompletion is deprecated in GTK 4.10 but remains the simplest inline
// autocomplete; the replacement (GtkColumnView popovers) is far heavier.
#[allow(deprecated)]
fn attach_tz_completion(entry: &gtk::Entry) {
    let store = gtk::ListStore::new(&[glib::Type::STRING]);
    for tz in chrono_tz::TZ_VARIANTS.iter() {
        let iter = store.append();
        store.set_value(&iter, 0, &tz.name().to_value());
    }
    let completion = gtk::EntryCompletion::builder()
        .model(&store)
        .text_column(0)
        .inline_completion(false)
        .popup_completion(true)
        .build();
    completion.set_match_func(|completion, key, iter| {
        let key = key.to_lowercase();
        let Some(model) = completion.model() else {
            return false;
        };
        let value: String = model.get_value(iter, 0).get().unwrap_or_default();
        value.to_lowercase().contains(&key)
    });
    entry.set_completion(Some(&completion));
}
