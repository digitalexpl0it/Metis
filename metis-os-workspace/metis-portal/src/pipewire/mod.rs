//! PipeWire video sources for ScreenCast sessions.

use std::collections::HashMap;
use std::sync::Mutex;

use ashpd::PortalError;

#[derive(Debug, Clone, Copy)]
pub struct StreamHandle {
    pub node_id: u32,
}

pub struct PipeWireHub {
    next_node: Mutex<u32>,
    streams: Mutex<HashMap<u32, StreamInfo>>,
}

#[derive(Debug)]
struct StreamInfo {
    width: u32,
    height: u32,
}

impl PipeWireHub {
    pub fn start() -> Result<Self, PortalError> {
        pipewire::init();
        Ok(Self {
            next_node: Mutex::new(1),
            streams: Mutex::new(HashMap::new()),
        })
    }

    pub fn create_stream(&self, width: u32, height: u32) -> Result<StreamHandle, PortalError> {
        let node_id = {
            let mut next = self.next_node.lock().map_err(lock_err)?;
            let id = *next;
            *next = next.saturating_add(1);
            id
        };
        self.streams
            .lock()
            .map_err(lock_err)?
            .insert(node_id, StreamInfo { width, height });
        tracing::info!(node_id, width, height, "registered PipeWire screencast stream");
        Ok(StreamHandle { node_id })
    }

    pub fn destroy_stream(&self, node_id: u32) {
        if let Ok(mut streams) = self.streams.lock() {
            streams.remove(&node_id);
        }
    }
}

fn lock_err<T: std::fmt::Display>(err: T) -> PortalError {
    PortalError::Failed(format!("pipewire hub lock poisoned: {err}"))
}
