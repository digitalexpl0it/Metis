use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Workspace strip for the Metis compositor session.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WorkspaceSnapshot {
    pub workspaces: Vec<Workspace>,
    pub active_id: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Workspace {
    pub id: u32,
    pub name: String,
}

/// Active workspace per output (output name → 1-based workspace id). Each output
/// owns an independent set of workspaces, so the bar on every monitor tracks its
/// own active workspace.
fn active_map() -> &'static Mutex<HashMap<String, u32>> {
    static MAP: OnceLock<Mutex<HashMap<String, u32>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn workspace_count() -> u32 {
    crate::config::load_bar_config()
        .workspace_count
        .clamp(1, 12)
}

/// The active workspace on a given output. `None` (a bar not bound to a specific
/// output, i.e. single-monitor sessions) falls back to any known active value.
pub fn active_workspace_for(output: Option<&str>) -> u32 {
    let map = match active_map().lock() {
        Ok(m) => m,
        Err(_) => return 1,
    };
    let count = workspace_count();
    let id = match output {
        Some(o) if !o.is_empty() => map.get(o).copied().unwrap_or(1),
        _ => map.values().next().copied().unwrap_or(1),
    };
    id.clamp(1, count)
}

/// Build a snapshot for an output. The dot list is the configured count; the
/// active id is that output's current workspace.
pub fn workspace_snapshot_for(output: Option<&str>) -> WorkspaceSnapshot {
    let count = workspace_count();
    let active_id = active_workspace_for(output);
    let workspaces = (1..=count)
        .map(|id| Workspace {
            id,
            name: format!("Desktop {id}"),
        })
        .collect();
    WorkspaceSnapshot {
        workspaces,
        active_id,
    }
}

/// Global snapshot (output-agnostic), used by the background poller and as a
/// fallback. The active id reflects any output's current workspace.
pub fn workspace_snapshot() -> WorkspaceSnapshot {
    workspace_snapshot_for(None)
}

/// Switch a specific output to workspace `id`. `output` is the compositor output
/// name (`None` lets the compositor target the output under the pointer).
pub fn dispatch_workspace(output: Option<String>, id: u32) {
    let count = workspace_count();
    if !(1..=count).contains(&id) {
        return;
    }
    // Optimistic local update for snappy dot feedback; the compositor's
    // `WorkspaceChanged` event is authoritative and reconciles this.
    if let Some(o) = output.as_deref() {
        set_active_workspace(o, id);
    }
    if let Err(err) = crate::compositor::switch_workspace(output.clone(), id) {
        tracing::warn!(?output, id, %err, "failed to switch workspace");
    }
}

/// Reconcile an output's active workspace from a compositor `WorkspaceChanged`
/// event (or an optimistic local update).
pub fn set_active_workspace(output: &str, id: u32) {
    if output.is_empty() {
        return;
    }
    if let Ok(mut map) = active_map().lock() {
        map.insert(output.to_string(), id.max(1));
    }
}
