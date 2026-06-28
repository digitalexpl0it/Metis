mod settings;

use futures_util::future::pending;
use settings::MetisSettings;

const DBUS_NAME: &str = "org.freedesktop.impl.portal.desktop.metis";

#[tokio::main]
async fn main() -> ashpd::Result<()> {
    tracing_subscriber::fmt::init();
    tracing::info!("starting Metis portal Settings backend");

    ashpd::backend::Builder::new(DBUS_NAME)?
        .settings(MetisSettings)
        .build()
        .await?;

    pending::<()>().await;
    Ok(())
}
