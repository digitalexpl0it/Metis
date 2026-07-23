//! Catalog filesystem layout and discovery.

use std::path::PathBuf;

pub const GETTEXT_DOMAIN: &str = "metis";

/// Directories that contain `<lang>/LC_MESSAGES/metis.mo` and `<lang>/compositor/metis.ftl`.
pub fn catalog_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    if let Ok(dir) = std::env::var("METIS_LOCALE_DIR") {
        let p = PathBuf::from(dir);
        if p.is_dir() {
            roots.push(p);
        }
    }

    for p in [
        PathBuf::from("/usr/local/share/metis/locale"),
        PathBuf::from("/usr/share/metis/locale"),
    ] {
        if p.is_dir() {
            roots.push(p);
        }
    }

    // Dev checkout: workspace assets/locale next to the binary or via CARGO_MANIFEST_DIR.
    if let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") {
        // metis-i18n or an app crate → walk up to workspace assets.
        let mut dir = PathBuf::from(manifest);
        for _ in 0..4 {
            let candidate = dir.join("assets/locale");
            if candidate.is_dir() {
                roots.push(candidate);
                break;
            }
            let workspace = dir.join("../assets/locale");
            if workspace.is_dir() {
                roots.push(workspace.canonicalize().unwrap_or(workspace));
                break;
            }
            if !dir.pop() {
                break;
            }
        }
    }

    // Relative to cwd (run-metis from workspace root).
    for rel in ["assets/locale", "metis-os-workspace/assets/locale"] {
        let p = PathBuf::from(rel);
        if p.is_dir() {
            roots.push(p);
        }
    }

    roots
}

pub fn primary_share_locale() -> PathBuf {
    catalog_roots()
        .into_iter()
        .next()
        .unwrap_or_else(|| PathBuf::from("/usr/local/share/metis/locale"))
}

/// Language tags that have at least a gettext or Fluent catalog on disk.
pub fn discover_installed_languages() -> Vec<String> {
    let mut tags = Vec::new();
    for root in catalog_roots() {
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let has_mo = path
                .join("LC_MESSAGES")
                .join(format!("{GETTEXT_DOMAIN}.mo"))
                .is_file()
                || path
                    .join("LC_MESSAGES")
                    .join(format!("{GETTEXT_DOMAIN}.po"))
                    .is_file();
            let has_ftl = path.join("compositor").join("metis.ftl").is_file();
            if has_mo || has_ftl {
                if !tags.iter().any(|t| t == &name) {
                    tags.push(name);
                }
            }
        }
    }
    tags.sort();
    tags
}
