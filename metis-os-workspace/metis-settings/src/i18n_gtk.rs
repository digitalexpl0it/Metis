//! GTK direction helpers and live UI rebuild after a language change.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;

thread_local! {
    static REBUILD_UI: RefCell<Option<Rc<dyn Fn(String)>>> = const { RefCell::new(None) };
}

/// Set the default text direction for the display from the active Metis locale.
pub fn apply_gtk_direction() {
    let dir = if metis_i18n::is_rtl() {
        gtk::TextDirection::Rtl
    } else {
        gtk::TextDirection::Ltr
    };
    if let Some(settings) = gtk::Settings::default() {
        // gtk-settings has gtk-enable-primary-paste etc.; direction is per-widget /
        // per-display via Widget::set_default_direction.
        let _ = settings;
    }
    gtk::Widget::set_default_direction(dir);
}

/// Register the Settings shell rebuild callback (called once from `build_ui`).
pub fn register_ui_rebuild(rebuild: Rc<dyn Fn(String)>) {
    REBUILD_UI.with(|slot| {
        *slot.borrow_mut() = Some(rebuild);
    });
}

/// After `locale.json` + gettext reload: rebuild chrome/pages in the new language.
///
/// Deferred to idle so we never destroy widgets mid-click handler, and so we do
/// not call `register_ui_rebuild` while `REBUILD_UI` is still borrowed.
pub fn rebuild_ui_for_locale(page_id: &str) {
    apply_gtk_direction();
    let page = page_id.to_string();
    let rebuild = REBUILD_UI.with(|slot| slot.borrow().clone());
    let Some(rebuild) = rebuild else {
        return;
    };
    glib::idle_add_local_once(move || {
        rebuild(page);
    });
}
