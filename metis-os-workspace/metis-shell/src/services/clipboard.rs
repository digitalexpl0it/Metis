use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use serde::{Deserialize, Serialize};

const DEFAULT_MAX_ENTRIES: usize = 50;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardEntry {
    pub id: u64,
    pub mime: String,
    #[serde(default)]
    pub preview_text: Option<String>,
    #[serde(default)]
    pub image_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ClipboardStore {
    #[serde(default = "default_max_entries")]
    max_entries: usize,
    #[serde(default)]
    entries: Vec<ClipboardEntry>,
}

fn default_max_entries() -> usize {
    DEFAULT_MAX_ENTRIES
}

thread_local! {
    static ENTRIES: RefCell<Vec<ClipboardEntry>> = const { RefCell::new(Vec::new()) };
    static NEXT_ID: RefCell<u64> = const { RefCell::new(1) };
    static REFRESH: RefCell<Vec<std::rc::Weak<dyn Fn()>>> = const { RefCell::new(Vec::new()) };
}

pub fn register_refresh(f: Rc<dyn Fn()>) {
    REFRESH.with(|cell| cell.borrow_mut().push(Rc::downgrade(&f)));
}

fn notify_refresh() {
    glib::idle_add_local_once(|| {
        REFRESH.with(|cell| {
            cell.borrow_mut().retain(|weak| {
                if let Some(f) = weak.upgrade() {
                    f();
                    true
                } else {
                    false
                }
            });
        });
    });
}

pub fn load_history() {
    let path = clipboard_json_path();
    let store: ClipboardStore = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let max_id = store.entries.iter().map(|e| e.id).max();
    ENTRIES.with(|cell| {
        *cell.borrow_mut() = store.entries;
    });
    if let Some(max) = max_id {
        NEXT_ID.with(|id| {
            *id.borrow_mut() = max.saturating_add(1);
        });
    }
}

fn persist_history() {
    let path = clipboard_json_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let entries = runtime_clipboard_entries();
    let store = ClipboardStore {
        max_entries: DEFAULT_MAX_ENTRIES,
        entries,
    };
    if let Ok(json) = serde_json::to_string_pretty(&store) {
        let _ = std::fs::write(path, json);
    }
}

pub fn clipboard_state_dir() -> PathBuf {
    directories::ProjectDirs::from("com", "metis", "metis")
        .and_then(|d| d.state_dir().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| {
            std::env::var("HOME")
                .map(|h| PathBuf::from(h).join(".local/state/metis"))
                .unwrap_or_else(|_| PathBuf::from(".local/state/metis"))
        })
}

fn clipboard_json_path() -> PathBuf {
    clipboard_state_dir().join("clipboard.json")
}

pub fn runtime_clipboard_entries() -> Vec<ClipboardEntry> {
    ENTRIES.with(|cell| cell.borrow().clone())
}

pub fn clipboard_count() -> usize {
    ENTRIES.with(|cell| cell.borrow().len())
}

pub fn apply_clipboard_event(
    mime: &str,
    preview_text: Option<String>,
    image_path: Option<String>,
) {
    if preview_text.as_deref().is_some_and(|t| t.is_empty()) && image_path.is_none() {
        return;
    }

    ENTRIES.with(|cell| {
        let mut entries = cell.borrow_mut();
        if let Some(first) = entries.first() {
            if first.mime == mime
                && first.preview_text == preview_text
                && first.image_path == image_path
            {
                return;
            }
        }
        let id = NEXT_ID.with(|n| {
            let mut id = n.borrow_mut();
            let current = *id;
            *id = id.saturating_add(1);
            current
        });
        entries.insert(
            0,
            ClipboardEntry {
                id,
                mime: mime.to_string(),
                preview_text,
                image_path,
            },
        );
        while entries.len() > DEFAULT_MAX_ENTRIES {
            entries.pop();
        }
    });
    persist_history();
    notify_refresh();
}

pub fn clear_history() {
    ENTRIES.with(|cell| cell.borrow_mut().clear());
    persist_history();
    notify_refresh();
}

pub fn recall_entry(entry: &ClipboardEntry) -> Result<(), String> {
    if let Some(text) = entry.preview_text.as_deref() {
        crate::compositor::client::set_clipboard(entry.mime.clone(), Some(text.to_string()), None)
            .map_err(|e| e.to_string())
    } else if let Some(path) = entry.image_path.as_deref() {
        if !Path::new(path).exists() {
            return Err("image no longer available".into());
        }
        crate::compositor::client::set_clipboard(
            entry.mime.clone(),
            None,
            Some(path.to_string()),
        )
        .map_err(|e| e.to_string())
    } else {
        Err("empty clipboard entry".into())
    }
}

impl Default for ClipboardStore {
    fn default() -> Self {
        Self {
            max_entries: DEFAULT_MAX_ENTRIES,
            entries: Vec::new(),
        }
    }
}
