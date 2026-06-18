use tokio::sync::mpsc::UnboundedReceiver;

use crate::briefing::BriefingScheduler;
use crate::state::{EventPublisher, UiCommand};

/// Routes UI commands into async subsystem handlers and publishes status on the event bus.
pub fn spawn_core_dispatcher(mut command_rx: UnboundedReceiver<UiCommand>, events: EventPublisher) {
    std::thread::Builder::new()
        .name("metis-dispatcher".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .thread_name("metis-tokio")
                .build();

            let Ok(runtime) = runtime else {
                events.publish_status("Failed to start async runtime.");
                return;
            };

            runtime.block_on(async move {
                while let Some(command) = command_rx.recv().await {
                    match command {
                        UiCommand::RefreshBriefing => {
                            BriefingScheduler::spawn(events.clone());
                            events.publish_status("Briefing refresh scheduled.");
                        }
                    }
                }
            });
        })
        .expect("failed to spawn metis-dispatcher thread");
}
