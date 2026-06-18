use std::sync::atomic::{AtomicU32, Ordering};

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

static ACTIVE_WORKSPACE: AtomicU32 = AtomicU32::new(1);

pub fn workspace_count() -> u32 {
    crate::config::load_bar_config()
        .workspace_count
        .clamp(1, 12)
}

pub fn workspace_snapshot() -> WorkspaceSnapshot {
    let count = workspace_count();
    let active_id = ACTIVE_WORKSPACE
        .load(Ordering::Relaxed)
        .clamp(1, count);
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

pub fn dispatch_workspace(id: u32) {
    let count = workspace_count();
    if !(1..=count).contains(&id) {
        return;
    }
    ACTIVE_WORKSPACE.store(id, Ordering::Relaxed);
    tracing::debug!(id, "workspace selected (compositor switch pending)");
}
