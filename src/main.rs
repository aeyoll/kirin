use anyhow::Context;
use kirin::app::create_app;
use kirin::config::AppConfig;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config.toml".into());
    let cfg = Arc::new(
        AppConfig::load_path(&config_path)
            .with_context(|| format!("failed to load config from {config_path}"))?,
    );
    let addr: SocketAddr = cfg.socket_addr()?;
    let app = create_app(cfg)?;

    let listener = TcpListener::bind(addr).await?;
    tracing::info!(%addr, "listening");
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
        .await?;
    Ok(())
}
