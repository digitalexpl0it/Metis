---
name: Calendar Clocks Popover
overview: Turn the clock popover into an interactive calendar with a DE-agnostic, pluggable events backend (local .ics/JSON, Basic-auth CalDAV, Thunderbird, Microsoft 365 device-code OAuth), supporting local dismiss, real delete write-back, and incremental sync, plus inline world clocks, alarms, a timer, and a stopwatch. Email is out of scope.
todos:
  - id: self-contained-popover
    content: Restructure clock.rs into clock/ module with ScrolledWindow + StackSwitcher/Stack (Calendar/Clocks/Timers); interactive calendar (day select, month+year nav, Today); world clocks add/remove; stopwatch; timer; alarms. No event/network deps yet.
    status: completed
  - id: clock-config
    content: Add clock.json persistence (world_clocks + alarms) seeded from ClockConfig.timezones; per-minute alarm check + notify-send firing.
    status: completed
  - id: event-core
    content: Event model (+ source_ref/etag) + EventProvider trait (fetch/capabilities/delete) + aggregator thread (Receiver<Vec<Event>>); ics.rs (ical) + recurrence.rs (rrule); disk cache; dismissed-events overlay filter.
    status: completed
  - id: provider-local
    content: Local provider - parse .ics + events.json; create + delete local events (write/remove VEVENT).
    status: completed
  - id: events-ui
    content: Calendar dots + selected-day list with color tags; per-event dismiss (hide) and delete (write-back) actions; add-event form; empty state.
    status: completed
  - id: sync
    content: Incremental sync + auto-update - periodic refresh, refresh-on-open, manual refresh, file/mtime watch (local + Thunderbird), CalDAV sync-token, MS Graph delta.
    status: completed
  - id: secrets
    content: services/secrets.rs using oo7 (freedesktop Secret Service) to store/fetch credentials + OAuth tokens.
    status: completed
  - id: provider-caldav
    content: Basic-auth CalDAV provider (reqwest + quick-xml) - discovery, calendar-query REPORT, sync-collection, DELETE write-back (href/etag).
    status: completed
  - id: provider-thunderbird
    content: Thunderbird provider (read-only/dismiss-only) - read calendar-data/*.sqlite (rusqlite) AND import prefs.js calendar.registry.* CalDAV/ICS URLs.
    status: completed
  - id: provider-ms365
    content: Microsoft 365 provider - device-code OAuth2 (Calendars.ReadWrite + offline_access), token refresh in Secret Service, Graph calendarView fetch + delta + DELETE.
    status: completed
  - id: calendars-config-ui
    content: calendars.json + inline Calendars management page (add/remove/enable accounts; CalDAV password; MS device-code login).
    status: completed
  - id: css-clock
    content: CSS for the wide multi-column layout, selected day, event dots, event row actions, world-clock cards, stopwatch/timer digits, alarms, account rows, stack switcher.
    status: completed
  - id: sqlite-vec-datalayer
    content: "DEFERRED/future: unified SQLite + sqlite-vec store (events/clocks/alarms/accounts/dismissed + embeddings) for AI-ready metadata; swap persistence under the same provider/aggregator interfaces."
    status: cancelled
isProject: false
---

## Calendar / Clocks Popover (DE-agnostic, pluggable providers)

Rebuild the clock dropdown ([clock.rs](metis-os-workspace/metis-shell/src/ui/bar/widgets/clock.rs)) into a compact multi-view popover (inline, no separate window). Events come from our own pluggable backend - no GNOME, no EDS. Notifications via `notify-send` (freedesktop); secrets via the freedesktop Secret Service (gnome-keyring/KWallet/KeePassXC). Email is out of scope; calendars/events only.

### Provider model (read + dismiss + delete)

```rust
// services/calendar/model.rs
pub struct Event {
    pub id: String,          // stable UID for dismiss/dedupe
    pub calendar_id: String,
    pub provider: ProviderId,
    pub summary: String,
    pub start: DateTime<Local>,
    pub end: DateTime<Local>,
    pub all_day: bool,
    pub location: Option<String>,
    pub color: Option<String>,
    pub source_ref: Option<String>, // CalDAV href / Graph event id - needed to delete
    pub etag: Option<String>,
}

// services/calendar/provider.rs
pub struct Caps { pub can_delete: bool }
#[async_trait]
pub trait EventProvider {
    fn id(&self) -> &str;
    fn caps(&self) -> Caps;
    async fn fetch(&self, since: DateTime<Local>, until: DateTime<Local>) -> Result<Vec<Event>>;
    async fn delete(&self, ev: &Event) -> Result<()>; // err if !can_delete
}
```

- Dismiss = local hide only (works for every provider, including read-only ones). Stored in `~/.config/metis/dismissed.json` (`[{ uid, dismissed_at }]`) and applied as an aggregator filter.
- Delete = real write-back via `provider.delete()`: Local remove, CalDAV `DELETE`, MS365 `DELETE`. Thunderbird is dismiss-only (`can_delete=false`) to avoid corrupting its DB while running.

### Architecture / data flow

```mermaid
flowchart LR
  subgraph shell [metis-shell GTK main loop]
    calWidget[Clock popover]
    sched[glib timers: alarms/timer/stopwatch]
    clockCfg[clock.json]
    calCfg[calendars.json]
    dis[dismissed.json]
  end
  subgraph svc [services/calendar background - tokio]
    agg[Aggregator + cache + dedupe]
    local[LocalProvider]
    caldav[CalDavProvider]
    tbird[ThunderbirdProvider]
    ms[Microsoft365Provider]
  end
  secrets[Secret Service - oo7]
  remoteDav[CalDAV servers]
  graph[Microsoft Graph]
  tbprofile[~/.thunderbird profile]

  calWidget -->|range / refresh / delete / dismiss| agg
  agg --> local
  agg --> caldav
  agg --> tbird
  agg --> ms
  caldav -->|pw| secrets
  ms -->|token| secrets
  caldav -->|REPORT / sync / DELETE| remoteDav
  ms -->|device-code + calendarView/delta + DELETE| graph
  tbird -->|sqlite + prefs.js| tbprofile
  agg -->|Vec Event via mpsc| calWidget
  calWidget <-->|read/write| clockCfg
  calWidget <-->|read/write| calCfg
  calWidget <-->|read/write| dis
  sched -->|notify-send| calWidget
```

### Popover layout (wide, multi-column)
A wider popover (~660-720px) that organizes content side-by-side instead of stacking tall. Top `gtk::StackSwitcher`/`gtk::Stack`; each page lays out in columns via `gtk::Grid`/`gtk::Box` so little vertical scrolling is needed:
- Calendar: two columns - left = selected-date header + interactive month grid (event dots); right = selected-day events list (each row: title/time + dismiss + delete) + add-event.
- Clocks: world-clock cards in a 2-column flow (`gtk::FlowBox`) + add/remove.
- Timers: three columns - Stopwatch | Timer | Alarms.
- Calendars: provider/account list + add/remove/login.
Content still wrapped in a `gtk::ScrolledWindow` with a generous max height as a safety net, but the default layout fits without scrolling.

### Phase 1 - Self-contained popover (no events/network)
Split `clock.rs` into `clock/` module (`mod.rs`, `calendar.rs`, `world.rs`, `timers.rs`). Interactive calendar (`RefCell` shown-month + selected-day; click select; month/year nav; Today). World clocks (name + offset + live time, add/remove). Stopwatch/Timer/Alarms via `glib::timeout`; timer/alarm fire `notify-send` (+ best-effort `canberra-gtk-play`/`paplay`).

### Phase 2 - Persistence + scheduling
`~/.config/metis/clock.json` (`world_clocks`, `alarms`) seeded from `ClockConfig.timezones` (extend [config/mod.rs](metis-os-workspace/metis-shell/src/config/mod.rs)). Persistent once-per-minute alarm check (fires when popover closed).

### Phase 3 - Event core
`services/calendar/` mirroring the `AUDIO_CMD_TX` + `Receiver<BarSnapshot>` pattern in [poll.rs](metis-os-workspace/metis-shell/src/services/poll.rs): `model.rs`, `provider.rs`, `ics.rs` (ical), `recurrence.rs` (rrule), `mod.rs` (aggregator, dedupe by uid, dismissed filter, `spawn_calendar_service() -> (Sender<CalCommand>, Receiver<Vec<Event>>)` where `CalCommand` = SetRange/Refresh/Delete/Dismiss). Disk cache under `~/.cache/metis/calendars/`. Deps: `ical = "0.11"`, `rrule = "0.13"`.

### Phase 4 - Local provider + events UI
`local.rs`: parse `.ics` from `~/.local/share/metis/calendars/` + `events.json`; create + delete local events. UI: dots on days w/ events, selected-day list with color tags, per-row dismiss (always) and delete (only when `caps.can_delete`), inline add-event form, "No Events" empty state.

### Phase 5 - Incremental sync + auto-update
Periodic background refresh (e.g., every 5 min), refresh-on-popover-open, manual refresh button. Watch mtimes for `events.json`/local `.ics` and Thunderbird sqlite. CalDAV `sync-collection` (sync-token) for deltas; MS Graph `calendarView/delta`. New results flow through the existing `Receiver`, so the open popover updates live.

### Phase 6 - Secret Service
`services/secrets.rs` via `oo7 = "0.3"` (tokio): secrets keyed by `{ app:"metis", account:<id> }` (CalDAV passwords, MS refresh tokens). Pulls `zbus` transitively as a Secret Service client only (DE-agnostic).

### Phase 7 - CalDAV provider
`caldav.rs` (reqwest + `quick-xml = "0.36"`): `PROPFIND` principal -> calendar-home -> calendar list; `REPORT` calendar-query w/ `time-range`; `sync-collection` for deltas; track `href`+`etag` per event; `DELETE` href for write-back; `Authorization: Basic` from Secret Service; offline cache.

### Phase 8 - Thunderbird provider (read-only / dismiss-only)
`thunderbird.rs`: read `~/.thunderbird/<profile>/calendar-data/local.sqlite` (+ `cache.sqlite`) read-only via `rusqlite = { version="0.31", features=["bundled"] }` (`cal_events`/`cal_properties`); parse `prefs.js` `calendar.registry.<id>.{uri,type,name,username}` -> `storage` uses sqlite, `caldav|ics` registers a CalDAV/ICS account (creds via Secret Service). `caps.can_delete=false`.

### Phase 9 - Microsoft 365 provider
`ms365.rs`: device-code OAuth2 via `oauth2 = "4"` (scopes `Calendars.ReadWrite offline_access`), tenant + client_id from config (user-supplied app registration); tokens in Secret Service w/ refresh; `GET /me/calendarView` for window + `/calendarView/delta` for sync; `DELETE /me/events/{id}` for write-back. Login UX shows user-code + URL in the Calendars page.

### Phase 10 - Calendars config + management UI
`~/.config/metis/calendars.json`: `{ accounts:[{ id, kind: local|caldav|thunderbird|ms365, name, url?, username?, tenant?, client_id?, color, enabled, read_only }], local_dir }` (no secrets here). Inline "Calendars" page: list/add/remove/enable accounts; CalDAV password entry; MS device-code login; manual refresh.

### Phase 11 - CSS
Classes in [css.rs](metis-os-workspace/metis-shell/src/ui/theme/css.rs) for selected day (cyan ring, coexists with today's marker), event dots, event-row actions (dismiss/delete), world-clock rows, stopwatch/timer digits, alarm rows, account rows, StackSwitcher.

### Deferred (future pass, per your call)
- SQLite + sqlite-vec data layer: replace the JSON/file caches with a unified SQLite store (`~/.local/share/metis/metis.db`) holding events, world clocks, alarms, accounts, and dismissed state, plus a `sqlite-vec` virtual table of embeddings over event/metadata text so the data is AI-ready (semantic search, summarization, "what's my week look like"). The `EventProvider`/aggregator interfaces stay the same; only the persistence layer swaps underneath. Would add the `sqlite-vec` extension + an embedding step.
- Full calendar app: an expand button opening a larger surface with Day / 3-Day / Week / Month views of all events. Built on the same aggregator. Not in this scope.
- CalDAV/Graph event creation + edit write-back (currently create is local-only; remote is delete + read).
- Google (OAuth2) provider.

### Notes / decisions
- DE-agnostic: no GNOME/EDS. HTTPS/CalDAV/Graph, standard Secret Service, freedesktop notifications.
- Dismiss is universal + non-destructive; delete is real write-back where supported (Thunderbird excluded for safety).
- MS365 uses device-code flow with `Calendars.ReadWrite`; user provides the Azure app registration. Google excluded.
- Self-contained parts (Phases 1-2) land first and are usable before any provider/network work.
- New crates: `ical`, `rrule`, `quick-xml`, `oo7`, `rusqlite` (bundled), `oauth2` (`reqwest` already present).
