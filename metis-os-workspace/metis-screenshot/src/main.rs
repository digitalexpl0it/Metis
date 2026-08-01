//! Metis screenshot editor, pin window, and recording controller.

mod editor;
mod icons;
mod ocr;
mod pin;
mod record;
mod theme;

use std::path::PathBuf;

use gtk::prelude::*;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Mode {
    #[default]
    Edit,
    Pin,
    Record,
}

#[derive(Debug, Clone, Default)]
struct Cli {
    path: Option<PathBuf>,
    mode: Mode,
    connector: Option<String>,
    crop: Option<Crop>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Crop {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "metis_screenshot=info,warn".into()),
        )
        .init();

    if effective_uid() == 0 {
        eprintln!("metis-screenshot: refuse to run as root; launch it as your normal user.");
        std::process::exit(1);
    }

    let cli = parse_cli();
    metis_i18n::init();
    theme::sync_gtk_theme_env();

    let app = gtk::Application::builder()
        .application_id("com.metis.Screenshot")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_activate(move |app| match cli.mode {
        Mode::Edit => editor::show(app, cli.clone()),
        Mode::Pin => pin::show(app, cli.clone()),
        Mode::Record => record::show(app, cli.clone()),
    });
    let argv0: Vec<String> = std::env::args().take(1).collect();
    app.run_with_args(&argv0);
}

/// Metis draws server-side decorations for its own apps, so the GTK titlebar has
/// to be removed here or the window shows two stacked titlebars.
pub(crate) fn running_under_metis() -> bool {
    if std::env::var_os("METIS_SESSION").is_some() {
        return true;
    }
    std::env::var("XDG_CURRENT_DESKTOP")
        .map(|desktop| desktop.to_ascii_lowercase().contains("metis"))
        .unwrap_or(false)
}

fn effective_uid() -> u32 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|line| line.starts_with("Uid:"))
                .and_then(|line| line.split_whitespace().nth(2))
                .and_then(|uid| uid.parse().ok())
        })
        .unwrap_or(0)
}

fn parse_cli() -> Cli {
    let mut cli = Cli::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--path" => cli.path = args.next().map(PathBuf::from),
            "--mode" => {
                cli.mode = match args.next().as_deref() {
                    Some("edit") => Mode::Edit,
                    Some("pin") => Mode::Pin,
                    Some("record") => Mode::Record,
                    _ => usage(2, Some("--mode must be edit, pin, or record")),
                }
            }
            "--connector" => cli.connector = args.next(),
            "--crop" => {
                let Some(value) = args.next() else {
                    usage(2, Some("--crop requires X,Y,W,H"));
                };
                cli.crop =
                    Some(parse_crop(&value).unwrap_or_else(|message| usage(2, Some(&message))));
            }
            "-h" | "--help" => usage(0, None),
            other => usage(2, Some(&format!("unknown argument {other}"))),
        }
    }
    if cli.mode != Mode::Record && cli.path.is_none() {
        usage(2, Some("--path FILE is required for edit and pin modes"));
    }
    cli
}

fn parse_crop(value: &str) -> Result<Crop, String> {
    let values: Result<Vec<i32>, _> = value.split(',').map(|part| part.trim().parse()).collect();
    let values = values.map_err(|_| "--crop must be X,Y,W,H in logical pixels".to_string())?;
    match values.as_slice() {
        [x, y, width, height] if *width > 0 && *height > 0 => Ok(Crop {
            x: *x,
            y: *y,
            width: *width,
            height: *height,
        }),
        _ => Err("--crop must be X,Y,W,H with positive width and height".into()),
    }
}

fn usage(code: i32, error: Option<&str>) -> ! {
    if let Some(error) = error {
        eprintln!("metis-screenshot: {error}");
    }
    eprintln!(
        "Usage: metis-screenshot --path FILE [--mode edit|pin|record] [--connector NAME] [--crop X,Y,W,H]\n\
         --crop is used by record mode."
    );
    std::process::exit(code);
}
