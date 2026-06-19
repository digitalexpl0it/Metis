use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

/// A concrete, expanded calendar occurrence ready for display.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    /// Stable id used for de-dupe and dismiss. For recurring events this includes
    /// the occurrence start so each instance is addressable.
    pub id: String,
    pub account_id: String,
    pub calendar_id: String,
    pub summary: String,
    pub start: DateTime<Local>,
    pub end: DateTime<Local>,
    pub all_day: bool,
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    /// Provider-native handle (CalDAV href, Graph event id, local file/uid) for deletion.
    #[serde(default)]
    pub source_ref: Option<String>,
    #[serde(default)]
    pub etag: Option<String>,
    #[serde(default)]
    pub can_delete: bool,
}
