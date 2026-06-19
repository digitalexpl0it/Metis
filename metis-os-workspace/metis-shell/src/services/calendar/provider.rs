use async_trait::async_trait;
use chrono::{DateTime, Local};

use super::model::Event;

pub type ProviderError = Box<dyn std::error::Error + Send + Sync>;
pub type ProviderResult<T> = Result<T, ProviderError>;

/// A single calendar source. Implementations live behind the aggregator and are
/// run on the calendar service's background runtime. Per-event delete capability
/// is carried on each [`Event`](super::model::Event) via `can_delete`.
#[async_trait]
pub trait EventProvider: Send + Sync {
    fn account_id(&self) -> &str;

    async fn fetch(
        &self,
        since: DateTime<Local>,
        until: DateTime<Local>,
    ) -> ProviderResult<Vec<Event>>;

    async fn delete(&self, _event: &Event) -> ProviderResult<()> {
        Err("This calendar is read-only".into())
    }
}
