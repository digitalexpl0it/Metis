//! Background: desktop wallpaper as a picture, solid colour, or gradient
//! (written to `wallpaper.json`), plus per-display overrides when more than one
//! output is connected. The compositor applies changes live via `ApplyBackground`
//! and re-reads `wallpaper.json` on next start.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk::gio;
use gtk::prelude::*;

use crate::pages::appearance_common::{
    color_dialog_button, current_wallpaper, hex_to_rgba, list_wallpapers, rgba_to_hex,
};
use crate::{runtime, ui};

pub fn build() -> gtk::Widget {
    let (scroller, content) = ui::page_for("background");

    let current_wp = current_wallpaper();
    let bgcfg = Rc::new(RefCell::new(metis_config::load_wallpaper_config()));

    // ---- Background (picture / solid / gradient) -------------------------
    let (bg_card, bg_body) =
        ui::section_with_icon("Background", "preferences-desktop-wallpaper-symbolic");

    let type_dd = gtk::DropDown::from_strings(&["Picture", "Solid color", "Gradient"]);
    type_dd.set_selected(match bgcfg.borrow().kind {
        metis_config::BackgroundKind::Image => 0,
        metis_config::BackgroundKind::Solid => 1,
        metis_config::BackgroundKind::Gradient => 2,
    });
    bg_body.append(&ui::row_with_icon("view-paged-symbolic", "Type", &type_dd));

    // -- Picture controls --
    let picture_box = gtk::Box::new(gtk::Orientation::Vertical, 12);
    let add_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    add_row.set_halign(gtk::Align::End);
    let add_btn = gtk::Button::new();
    add_btn.add_css_class("flat");
    let add_content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    add_content.append(&gtk::Image::from_icon_name("list-add-symbolic"));
    add_content.append(&gtk::Label::new(Some("Add Picture…")));
    add_btn.set_child(Some(&add_content));
    add_row.append(&add_btn);
    picture_box.append(&add_row);

    let flow = gtk::FlowBox::new();
    flow.set_selection_mode(gtk::SelectionMode::None);
    flow.set_max_children_per_line(3);
    flow.set_min_children_per_line(2);
    flow.set_column_spacing(12);
    flow.set_row_spacing(12);
    flow.set_homogeneous(true);
    flow.add_css_class("metis-wallpaper-grid");
    picture_box.append(&flow);
    bg_body.append(&picture_box);
    populate_wallpapers(&flow, current_wp.as_deref(), &bgcfg);

    // -- Solid colour controls --
    let solid_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let solid_btn = color_dialog_button();
    solid_btn.set_rgba(&hex_to_rgba(&bgcfg.borrow().color));
    solid_box.append(&ui::row_with_icon(
        "applications-graphics-symbolic",
        "Color",
        &solid_btn,
    ));
    bg_body.append(&solid_box);

    // -- Gradient controls --
    let gradient_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let grad_start = color_dialog_button();
    grad_start.set_rgba(&hex_to_rgba(&bgcfg.borrow().gradient_start));
    let grad_end = color_dialog_button();
    grad_end.set_rgba(&hex_to_rgba(&bgcfg.borrow().gradient_end));
    let dir_dd = gtk::DropDown::from_strings(&[
        "Top → Bottom",
        "Bottom → Top",
        "Left → Right",
        "Right → Left",
        "Diagonal ↘",
        "Diagonal ↗",
    ]);
    dir_dd.set_selected(direction_to_index(bgcfg.borrow().gradient_direction));
    gradient_box.append(&ui::row_with_icon("starred-symbolic", "Start color", &grad_start));
    gradient_box.append(&ui::row_with_icon("starred-symbolic", "End color", &grad_end));
    gradient_box.append(&ui::row_with_icon("object-rotate-right-symbolic", "Direction", &dir_dd));
    bg_body.append(&gradient_box);

    content.append(&bg_card);

    // ---- Per-display background (only with 2+ displays) -------------------
    // Lets the user override the wallpaper on an individual output. Outputs not
    // overridden fall back to the global background above; either way each
    // display is cover-cropped to its own resolution by the compositor.
    let outputs = runtime::list_outputs();
    if outputs.len() >= 2 {
        let (pd_card, pd_body) =
            ui::section_with_icon("Per-display background", "video-display-symbolic");
        let hint = gtk::Label::new(Some(
            "Pick a different picture for a specific display. Leave a display on \
             “Default” to use the background above.",
        ));
        hint.set_wrap(true);
        hint.set_xalign(0.0);
        hint.add_css_class("dim-label");
        pd_body.append(&hint);

        for (i, out) in outputs.iter().enumerate() {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            let label = gtk::Label::new(Some(&format!(
                "Display {} · {}×{}",
                i + 1,
                out.rect.width,
                out.rect.height
            )));
            label.set_xalign(0.0);
            label.set_hexpand(true);

            let status = gtk::Label::new(None);
            status.add_css_class("dim-label");
            status.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
            status.set_max_width_chars(18);

            let update_status: Rc<dyn Fn()> = {
                let status = status.clone();
                let bgcfg = bgcfg.clone();
                let name = out.name.clone();
                Rc::new(move || {
                    let text = bgcfg
                        .borrow()
                        .per_output
                        .get(&name)
                        .map(|p| {
                            Path::new(p)
                                .file_name()
                                .map(|f| f.to_string_lossy().to_string())
                                .unwrap_or_else(|| p.clone())
                        })
                        .unwrap_or_else(|| "Default".to_string());
                    status.set_text(&text);
                })
            };
            update_status();

            let set_btn = gtk::Button::with_label("Set…");
            set_btn.add_css_class("flat");
            let clear_btn = gtk::Button::with_label("Clear");
            clear_btn.add_css_class("flat");

            {
                let bgcfg = bgcfg.clone();
                let name = out.name.clone();
                let update_status = update_status.clone();
                set_btn.connect_clicked(move |btn| {
                    let bgcfg = bgcfg.clone();
                    let name = name.clone();
                    let update_status = update_status.clone();
                    let root = btn.root().and_then(|r| r.downcast::<gtk::Window>().ok());
                    pick_picture(root.as_ref(), move |path| {
                        bgcfg
                            .borrow_mut()
                            .per_output
                            .insert(name.clone(), path.to_string_lossy().to_string());
                        save_and_apply(&bgcfg.borrow());
                        update_status();
                    });
                });
            }
            {
                let bgcfg = bgcfg.clone();
                let name = out.name.clone();
                let update_status = update_status.clone();
                clear_btn.connect_clicked(move |_| {
                    bgcfg.borrow_mut().per_output.remove(&name);
                    save_and_apply(&bgcfg.borrow());
                    update_status();
                });
            }

            row.append(&label);
            row.append(&status);
            row.append(&set_btn);
            row.append(&clear_btn);
            pd_body.append(&row);
        }
        content.append(&pd_card);
    }

    // Show only the controls for the active background kind.
    let update_visibility = {
        let picture_box = picture_box.clone();
        let solid_box = solid_box.clone();
        let gradient_box = gradient_box.clone();
        Rc::new(move |kind: metis_config::BackgroundKind| {
            picture_box.set_visible(kind == metis_config::BackgroundKind::Image);
            solid_box.set_visible(kind == metis_config::BackgroundKind::Solid);
            gradient_box.set_visible(kind == metis_config::BackgroundKind::Gradient);
        })
    };
    update_visibility(bgcfg.borrow().kind);

    // Type chooser.
    {
        let bgcfg = bgcfg.clone();
        let update_visibility = update_visibility.clone();
        type_dd.connect_selected_notify(move |dd| {
            let kind = match dd.selected() {
                1 => metis_config::BackgroundKind::Solid,
                2 => metis_config::BackgroundKind::Gradient,
                _ => metis_config::BackgroundKind::Image,
            };
            bgcfg.borrow_mut().kind = kind;
            update_visibility(kind);
            save_and_apply(&bgcfg.borrow());
        });
    }
    // Solid colour.
    {
        let bgcfg = bgcfg.clone();
        solid_btn.connect_rgba_notify(move |b| {
            bgcfg.borrow_mut().color = rgba_to_hex(&b.rgba());
            save_and_apply(&bgcfg.borrow());
        });
    }
    // Gradient stops + direction.
    {
        let bgcfg = bgcfg.clone();
        grad_start.connect_rgba_notify(move |b| {
            bgcfg.borrow_mut().gradient_start = rgba_to_hex(&b.rgba());
            save_and_apply(&bgcfg.borrow());
        });
    }
    {
        let bgcfg = bgcfg.clone();
        grad_end.connect_rgba_notify(move |b| {
            bgcfg.borrow_mut().gradient_end = rgba_to_hex(&b.rgba());
            save_and_apply(&bgcfg.borrow());
        });
    }
    {
        let bgcfg = bgcfg.clone();
        dir_dd.connect_selected_notify(move |dd| {
            bgcfg.borrow_mut().gradient_direction = index_to_direction(dd.selected());
            save_and_apply(&bgcfg.borrow());
        });
    }
    // Add Picture… → import + select.
    {
        let flow = flow.clone();
        let bgcfg = bgcfg.clone();
        add_btn.connect_clicked(move |btn| {
            let flow = flow.clone();
            let bgcfg = bgcfg.clone();
            let root = btn.root().and_then(|r| r.downcast::<gtk::Window>().ok());
            pick_picture(root.as_ref(), move |path| {
                select_picture(&bgcfg, &path);
                populate_wallpapers(&flow, Some(&path), &bgcfg);
            });
        });
    }

    scroller.upcast()
}

// ---- Wallpaper discovery + selection --------------------------------------

fn populate_wallpapers(
    flow: &gtk::FlowBox,
    selected: Option<&Path>,
    bgcfg: &Rc<RefCell<metis_config::WallpaperConfig>>,
) {
    while let Some(child) = flow.first_child() {
        flow.remove(&child);
    }
    let selected_canon = selected.and_then(|p| p.canonicalize().ok());
    for path in list_wallpapers() {
        let is_selected = path
            .canonicalize()
            .ok()
            .zip(selected_canon.clone())
            .map(|(a, b)| a == b)
            .unwrap_or(false);
        flow.insert(&wallpaper_thumb(&path, is_selected, flow, bgcfg), -1);
    }
}

fn wallpaper_thumb(
    path: &Path,
    selected: bool,
    flow: &gtk::FlowBox,
    bgcfg: &Rc<RefCell<metis_config::WallpaperConfig>>,
) -> gtk::Widget {
    let btn = gtk::Button::new();
    btn.add_css_class("metis-wallpaper-thumb");
    btn.add_css_class("flat");
    if selected {
        btn.add_css_class("selected");
    }

    let overlay = gtk::Overlay::new();
    let pic = gtk::Picture::for_filename(path);
    pic.set_content_fit(gtk::ContentFit::Cover);
    pic.set_size_request(150, 92);
    pic.add_css_class("metis-wallpaper-image");
    overlay.set_child(Some(&pic));

    if selected {
        let check = gtk::Image::from_icon_name("emblem-ok-symbolic");
        check.add_css_class("metis-wallpaper-check");
        check.set_halign(gtk::Align::End);
        check.set_valign(gtk::Align::End);
        check.set_margin_end(6);
        check.set_margin_bottom(6);
        overlay.add_overlay(&check);
    }
    btn.set_child(Some(&overlay));

    {
        let path = path.to_path_buf();
        let flow = flow.clone();
        let bgcfg = bgcfg.clone();
        btn.connect_clicked(move |_| {
            select_picture(&bgcfg, &path);
            populate_wallpapers(&flow, Some(&path), &bgcfg);
        });
    }
    btn.upcast()
}

/// Switch the background to the given picture (preserving solid/gradient fields)
/// and persist + apply it.
fn select_picture(bgcfg: &Rc<RefCell<metis_config::WallpaperConfig>>, path: &Path) {
    {
        let mut cfg = bgcfg.borrow_mut();
        cfg.kind = metis_config::BackgroundKind::Image;
        cfg.path = Some(path.to_string_lossy().to_string());
    }
    save_and_apply(&bgcfg.borrow());
}

/// Persist the background config (live via the compositor, durable via
/// `wallpaper.json` which the compositor also reads on next start).
fn save_and_apply(cfg: &metis_config::WallpaperConfig) {
    if let Err(err) = metis_config::save_wallpaper_config(cfg) {
        tracing::warn!(%err, "failed to save wallpaper.json");
    }
    runtime::apply_background();
}

fn direction_to_index(dir: metis_config::GradientDirection) -> u32 {
    use metis_config::GradientDirection as D;
    match dir {
        D::Vertical => 0,
        D::VerticalReverse => 1,
        D::Horizontal => 2,
        D::HorizontalReverse => 3,
        D::Diagonal => 4,
        D::DiagonalReverse => 5,
    }
}

fn index_to_direction(idx: u32) -> metis_config::GradientDirection {
    use metis_config::GradientDirection as D;
    match idx {
        1 => D::VerticalReverse,
        2 => D::Horizontal,
        3 => D::HorizontalReverse,
        4 => D::Diagonal,
        5 => D::DiagonalReverse,
        _ => D::Vertical,
    }
}

/// Open a file chooser for a custom picture; copies it into the wallpaper store
/// then invokes `on_pick` with the stored copy's path.
fn pick_picture<F>(parent: Option<&gtk::Window>, on_pick: F)
where
    F: Fn(PathBuf) + 'static,
{
    let dialog = gtk::FileDialog::new();
    dialog.set_title("Choose a picture");
    let filter = gtk::FileFilter::new();
    filter.set_name(Some("Images"));
    for ext in metis_config::WALLPAPER_IMAGE_EXTS {
        filter.add_pattern(&format!("*.{ext}"));
        filter.add_pattern(&format!("*.{}", ext.to_ascii_uppercase()));
    }
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    dialog.set_filters(Some(&filters));

    dialog.open(parent, gio::Cancellable::NONE, move |res| {
        let Ok(file) = res else { return };
        let Some(src) = file.path() else { return };
        match import_picture(&src) {
            Ok(stored) => on_pick(stored),
            Err(err) => tracing::warn!(%err, "failed to import wallpaper"),
        }
    });
}

fn import_picture(src: &Path) -> std::io::Result<PathBuf> {
    let dir = metis_config::wallpaper_store_dir();
    std::fs::create_dir_all(&dir)?;
    let name = src
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "wallpaper".to_string());
    let mut dest = dir.join(&name);
    // Avoid clobbering an existing import with the same name.
    if dest.exists() && std::fs::canonicalize(&dest).ok() != std::fs::canonicalize(src).ok() {
        let stem = src.file_stem().map(|s| s.to_string_lossy().to_string());
        let ext = src.extension().map(|e| e.to_string_lossy().to_string());
        let unique = format!(
            "{}-{}",
            stem.as_deref().unwrap_or("wallpaper"),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        );
        dest = match ext {
            Some(ext) => dir.join(format!("{unique}.{ext}")),
            None => dir.join(unique),
        };
    }
    if std::fs::canonicalize(&dest).ok() != std::fs::canonicalize(src).ok() {
        std::fs::copy(src, &dest)?;
    }
    Ok(dest)
}
