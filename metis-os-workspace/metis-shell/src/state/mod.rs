mod dispatcher;

use std::sync::mpsc::{self, Receiver, Sender};

pub use dispatcher::spawn_core_dispatcher;

use metis_protocol::CompositorEvent;

/// Events emitted by backend subsystems and consumed by the GTK shell.
#[derive(Debug, Clone)]
pub enum SystemEvent {
    Status(String),
    BriefingReady(Vec<crate::briefing::BriefingItem>),
    CompositorConnected,
    Compositor(CompositorEvent),
}

/// Commands issued by the UI and handled asynchronously on a Tokio runtime.
#[derive(Debug, Clone)]
pub enum UiCommand {
    RefreshBriefing,
}

#[derive(Clone)]
pub struct EventPublisher {
    tx: Sender<SystemEvent>,
}

impl EventPublisher {
    pub fn publish(&self, event: SystemEvent) {
        let _ = self.tx.send(event);
    }

    pub fn publish_status(&self, message: impl Into<String>) {
        self.publish(SystemEvent::Status(message.into()));
    }
}

#[derive(Clone)]
pub struct StateHandles {
    pub events: EventPublisher,
}

pub struct MetisInit {
    pub event_rx: Receiver<SystemEvent>,
    pub command_tx: tokio::sync::mpsc::UnboundedSender<UiCommand>,
}

pub fn bootstrap() -> (MetisInit, StateHandles) {
    let (event_tx, event_rx) = mpsc::channel();
    let (command_tx, command_rx) = tokio::sync::mpsc::unbounded_channel();

    let handles = StateHandles {
        events: EventPublisher { tx: event_tx },
    };

    spawn_core_dispatcher(command_rx, handles.events.clone());

    let init = MetisInit {
        event_rx,
        command_tx,
    };

    (init, handles)
}
