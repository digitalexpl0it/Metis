mod briefing;
mod compositor;
mod config;
mod services;
mod state;
mod ui;

/// Metis Shell — configurable edge bar.
fn main() {
    tracing_subscriber::fmt().init();

    if let Err(err) = config::ensure_config_dirs() {
        tracing::warn!("config dirs: {err}");
    }
    if let Err(err) = ui::theme::export_embedded_themes_to_config() {
        tracing::warn!("theme export: {err}");
    }
    if let Err(err) = config::save_default_bar_config() {
        tracing::warn!("bar config: {err}");
    }

    let (init, handles) = state::bootstrap();

    compositor::spawn_listener(handles.clone());
    if std::env::var("METIS_NO_BRIEFING").is_err() {
        briefing::BriefingScheduler::spawn(handles.events.clone());
    }

    gtk::glib::set_application_name("Metis Shell");

    if std::panic::catch_unwind(|| ui::app::run(init)).is_err() {
        eprintln!(
            "Metis shell failed to initialize GTK/Wayland.\n\
             Run under the Metis compositor: ./run-metis.sh --session"
        );
        std::process::exit(1);
    }
}
