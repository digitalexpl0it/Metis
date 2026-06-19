use std::path::{Path, PathBuf};

use async_trait::async_trait;
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

use super::ics::parse_ics;
use super::model::Event;
use super::provider::{EventProvider, ProviderResult};
use super::recurrence::expand;

/// A user-created event stored in `events.json` (always deletable).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LocalEvent {
    pub id: String,
    pub summary: String,
    pub start: DateTime<Local>,
    pub end: DateTime<Local>,
    #[serde(default)]
    pub all_day: bool,
    #[serde(default)]
    pub location: Option<String>,
}

pub struct LocalProvider {
    account_id: String,
    dir: PathBuf,
    color: Option<String>,
}

impl LocalProvider {
    pub fn new(account_id: impl Into<String>, dir: PathBuf, color: Option<String>) -> Self {
        Self {
            account_id: account_id.into(),
            dir,
            color,
        }
    }
}

pub fn events_json_path(dir: &Path) -> PathBuf {
    dir.join("events.json")
}

pub fn load_local_events(dir: &Path) -> Vec<LocalEvent> {
    let path = events_json_path(dir);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save_local_events(dir: &Path, events: &[LocalEvent]) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let json = serde_json::to_string_pretty(events).map_err(std::io::Error::other)?;
    std::fs::write(events_json_path(dir), json)
}

pub fn add_local_event(dir: &Path, event: LocalEvent) -> std::io::Result<()> {
    let mut events = load_local_events(dir);
    events.push(event);
    save_local_events(dir, &events)
}

pub fn delete_local_event(dir: &Path, id: &str) -> std::io::Result<()> {
    let mut events = load_local_events(dir);
    events.retain(|e| e.id != id);
    save_local_events(dir, &events)
}

#[async_trait]
impl EventProvider for LocalProvider {
    fn account_id(&self) -> &str {
        &self.account_id
    }

    async fn fetch(
        &self,
        since: DateTime<Local>,
        until: DateTime<Local>,
    ) -> ProviderResult<Vec<Event>> {
        let mut out = Vec::new();

        // User events from events.json (deletable).
        for ev in load_local_events(&self.dir) {
            if ev.end >= since && ev.start <= until {
                out.push(Event {
                    id: format!("{}:{}", self.account_id, ev.id),
                    account_id: self.account_id.clone(),
                    calendar_id: "events.json".into(),
                    summary: ev.summary,
                    start: ev.start,
                    end: ev.end,
                    all_day: ev.all_day,
                    location: ev.location,
                    color: self.color.clone(),
                    source_ref: Some(ev.id),
                    etag: None,
                    can_delete: true,
                });
            }
        }

        // Read-only .ics files dropped into the calendar dir.
        if let Ok(entries) = std::fs::read_dir(&self.dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("ics") {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(&path) else {
                    continue;
                };
                let file = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("ics")
                    .to_string();
                for raw in parse_ics(&text) {
                    for (start, end) in expand(&raw, since, until) {
                        out.push(Event {
                            id: format!("{}:{}:{}", self.account_id, raw.uid, start.timestamp()),
                            account_id: self.account_id.clone(),
                            calendar_id: file.clone(),
                            summary: raw.summary.clone(),
                            start,
                            end,
                            all_day: raw.all_day,
                            location: raw.location.clone(),
                            color: self.color.clone(),
                            source_ref: Some(raw.uid.clone()),
                            etag: None,
                            can_delete: false,
                        });
                    }
                }
            }
        }

        Ok(out)
    }

    async fn delete(&self, event: &Event) -> ProviderResult<()> {
        if !event.can_delete {
            return Err("This event is read-only".into());
        }
        if let Some(id) = &event.source_ref {
            delete_local_event(&self.dir, id)?;
        }
        Ok(())
    }
}
