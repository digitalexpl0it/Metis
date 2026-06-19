use std::path::PathBuf;

use async_trait::async_trait;
use chrono::{DateTime, Local, Utc};
use rusqlite::{Connection, OpenFlags};

use super::model::Event;
use super::provider::{EventProvider, ProviderResult};

/// Thunderbird `EVENT_ALLDAY` item flag.
const FLAG_ALLDAY: i64 = 4;

pub struct ThunderbirdProvider {
    account_id: String,
    color: Option<String>,
}

impl ThunderbirdProvider {
    pub fn new(account_id: impl Into<String>, color: Option<String>) -> Self {
        Self {
            account_id: account_id.into(),
            color,
        }
    }
}

#[async_trait]
impl EventProvider for ThunderbirdProvider {
    fn account_id(&self) -> &str {
        &self.account_id
    }

    async fn fetch(
        &self,
        since: DateTime<Local>,
        until: DateTime<Local>,
    ) -> ProviderResult<Vec<Event>> {
        let since_us = since.with_timezone(&Utc).timestamp_micros();
        let until_us = until.with_timezone(&Utc).timestamp_micros();
        let account_id = self.account_id.clone();
        let color = self.color.clone();

        // Run the blocking sqlite work off the async runtime.
        let events = tokio::task::spawn_blocking(move || {
            let mut out = Vec::new();
            for db in storage_sqlite_paths() {
                if let Ok(rows) = read_db(&db, since_us, until_us) {
                    for row in rows {
                        let Some(start) = micros_to_local(row.start) else {
                            continue;
                        };
                        let end = row
                            .end
                            .and_then(micros_to_local)
                            .unwrap_or(start + chrono::Duration::hours(1));
                        out.push(Event {
                            id: format!("{account_id}:tb:{}", row.id),
                            account_id: account_id.clone(),
                            calendar_id: row.cal_id,
                            summary: row.title.unwrap_or_else(|| "(no title)".into()),
                            start,
                            end,
                            all_day: row.flags & FLAG_ALLDAY != 0,
                            location: None,
                            color: color.clone(),
                            source_ref: None,
                            etag: None,
                            can_delete: false,
                        });
                    }
                }
            }
            out
        })
        .await
        .unwrap_or_default();

        Ok(events)
    }
}

struct TbRow {
    id: String,
    cal_id: String,
    title: Option<String>,
    start: i64,
    end: Option<i64>,
    flags: i64,
}

fn read_db(path: &PathBuf, since_us: i64, until_us: i64) -> rusqlite::Result<Vec<TbRow>> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )?;
    let mut stmt = conn.prepare(
        "SELECT id, cal_id, title, event_start, event_end, flags \
         FROM cal_events \
         WHERE event_start <= ?2 AND (event_end IS NULL OR event_end >= ?1)",
    )?;
    let rows = stmt
        .query_map([since_us, until_us], |row| {
            Ok(TbRow {
                id: row.get::<_, String>(0).unwrap_or_default(),
                cal_id: row.get::<_, String>(1).unwrap_or_default(),
                title: row.get::<_, Option<String>>(2).unwrap_or(None),
                start: row.get::<_, i64>(3).unwrap_or(0),
                end: row.get::<_, Option<i64>>(4).unwrap_or(None),
                flags: row.get::<_, i64>(5).unwrap_or(0),
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

fn micros_to_local(micros: i64) -> Option<DateTime<Local>> {
    let secs = micros.div_euclid(1_000_000);
    let nanos = (micros.rem_euclid(1_000_000) * 1000) as u32;
    DateTime::<Utc>::from_timestamp(secs, nanos).map(|dt| dt.with_timezone(&Local))
}

/// Discover `calendar-data/*.sqlite` files across Thunderbird/Icedove profiles.
fn storage_sqlite_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for root in profile_roots() {
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let data = entry.path().join("calendar-data");
            for name in ["local.sqlite", "cache.sqlite"] {
                let candidate = data.join(name);
                if candidate.exists() {
                    out.push(candidate);
                }
            }
        }
    }
    out
}

fn profile_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(home) = std::env::var("HOME") {
        for dir in [".thunderbird", ".icedove"] {
            let p = PathBuf::from(&home).join(dir);
            if p.is_dir() {
                roots.push(p);
            }
        }
    }
    roots
}

// ---- prefs.js network calendar import (CalDAV/ICS) ----

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct TbNetworkCalendar {
    pub id: String,
    pub kind: String,
    pub uri: String,
    pub name: String,
    pub username: Option<String>,
}

/// Parse `prefs.js` across profiles for `calendar.registry.<id>.*` network calendars.
pub fn network_calendars() -> Vec<TbNetworkCalendar> {
    use std::collections::HashMap;
    let mut by_id: HashMap<String, HashMap<String, String>> = HashMap::new();

    for root in profile_roots() {
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let prefs = entry.path().join("prefs.js");
            let Ok(text) = std::fs::read_to_string(&prefs) else {
                continue;
            };
            for line in text.lines() {
                let line = line.trim();
                if !line.starts_with("user_pref(\"calendar.registry.") {
                    continue;
                }
                let Some((key, value)) = parse_pref(line) else {
                    continue;
                };
                // key = calendar.registry.<id>.<field>
                let rest = &key["calendar.registry.".len()..];
                let Some((id, field)) = rest.split_once('.') else {
                    continue;
                };
                by_id
                    .entry(id.to_string())
                    .or_default()
                    .insert(field.to_string(), value);
            }
        }
    }

    by_id
        .into_iter()
        .filter_map(|(id, fields)| {
            let kind = fields.get("type")?.clone();
            if kind != "caldav" && kind != "ics" {
                return None;
            }
            let uri = fields.get("uri")?.clone();
            Some(TbNetworkCalendar {
                name: fields.get("name").cloned().unwrap_or_else(|| id.clone()),
                username: fields.get("username").cloned(),
                id,
                kind,
                uri,
            })
        })
        .collect()
}

/// Parse a `user_pref("KEY", VALUE);` line into (key, value) with quotes stripped.
fn parse_pref(line: &str) -> Option<(String, String)> {
    let inner = line.strip_prefix("user_pref(")?.strip_suffix(");")?;
    let (key_part, value_part) = inner.split_once(',')?;
    let key = key_part.trim().trim_matches('"').to_string();
    let value = value_part.trim().trim_matches('"').to_string();
    Some((key, value))
}
