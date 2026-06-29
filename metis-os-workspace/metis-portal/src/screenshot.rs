use std::sync::Arc;

use ashpd::{
    MaybeAppID, PortalError, WindowIdentifierType,
    backend::{
        request::RequestImpl,
        screenshot::ScreenshotImpl,
    },
    desktop::{
        Color, HandleToken,
        screenshot::{ColorOptions, Screenshot, ScreenshotOptions},
    },
};
use async_trait::async_trait;

use crate::capture::CaptureHub;

pub struct MetisScreenshot {
    capture: Arc<CaptureHub>,
}

impl MetisScreenshot {
    pub fn new(capture: Arc<CaptureHub>) -> Self {
        Self { capture }
    }
}

#[async_trait]
impl RequestImpl for MetisScreenshot {
    async fn close(&self, _token: HandleToken) {}
}

#[async_trait]
impl ScreenshotImpl for MetisScreenshot {
    async fn screenshot(
        &self,
        _token: HandleToken,
        app_id: Option<MaybeAppID>,
        _window_identifier: Option<WindowIdentifierType>,
        _options: ScreenshotOptions,
    ) -> ashpd::backend::Result<Screenshot> {
        tracing::info!(?app_id, "portal screenshot request");
        let path = self.capture.screenshot_png().await?;
        let uri = format!("file://{}", path.display())
            .parse()
            .map_err(|err| PortalError::Failed(format!("invalid screenshot uri: {err}")))?;
        Ok(Screenshot::new(uri))
    }

    async fn pick_color(
        &self,
        _token: HandleToken,
        _app_id: Option<MaybeAppID>,
        _window_identifier: Option<WindowIdentifierType>,
        _options: ColorOptions,
    ) -> ashpd::backend::Result<Color> {
        Err(PortalError::Failed(
            "PickColor is not implemented in the Metis portal yet".into(),
        ))
    }
}
