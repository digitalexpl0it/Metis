use chrono::{DateTime, Local, Utc};
use rrule::{RRuleSet, Tz as RruleTz};

use super::ics::RawEvent;

const MAX_OCCURRENCES: u16 = 366;

/// Expand a raw event into concrete (start, end) occurrences inside the window.
/// Non-recurring events yield a single occurrence if they overlap the window.
pub fn expand(
    raw: &RawEvent,
    win_start: DateTime<Local>,
    win_end: DateTime<Local>,
) -> Vec<(DateTime<Local>, DateTime<Local>)> {
    let duration = raw.end - raw.start;

    let Some(rrule_payload) = &raw.rrule else {
        if raw.end >= win_start && raw.start <= win_end {
            return vec![(raw.start, raw.end)];
        }
        return Vec::new();
    };

    match expand_rrule(raw, rrule_payload, duration, win_start, win_end) {
        Some(occ) if !occ.is_empty() => occ,
        _ => {
            // Fall back to the base occurrence so a parse failure never hides the event.
            if raw.end >= win_start && raw.start <= win_end {
                vec![(raw.start, raw.end)]
            } else {
                Vec::new()
            }
        }
    }
}

fn expand_rrule(
    raw: &RawEvent,
    rrule_payload: &str,
    duration: chrono::Duration,
    win_start: DateTime<Local>,
    win_end: DateTime<Local>,
) -> Option<Vec<(DateTime<Local>, DateTime<Local>)>> {
    let dtstart_utc = raw.start.with_timezone(&Utc);
    let payload = rrule_payload
        .strip_prefix("RRULE:")
        .unwrap_or(rrule_payload);
    let spec = format!(
        "DTSTART:{}\nRRULE:{}",
        dtstart_utc.format("%Y%m%dT%H%M%SZ"),
        payload
    );

    let set: RRuleSet = spec.parse().ok()?;
    let after = win_start.with_timezone(&RruleTz::UTC);
    let before = win_end.with_timezone(&RruleTz::UTC);
    let result = set.after(after).before(before).all(MAX_OCCURRENCES);

    let occurrences = result
        .dates
        .into_iter()
        .map(|dt| {
            let start = dt.with_timezone(&Local);
            (start, start + duration)
        })
        .collect();
    Some(occurrences)
}
