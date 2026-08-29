mod data;
mod search;
mod tools;

use std::sync::Arc;

use anyhow::Result;
use axum::Router;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use tools::TgAutoDocs;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

const DEFAULT_DATA_PATH: &str = "data/bot-api.json";
const DEFAULT_LISTEN_ADDR: &str = "0.0.0.0:8000";

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".to_string().into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let data_path =
        std::env::var("TG_AUTODOCS_DATA").unwrap_or_else(|_| DEFAULT_DATA_PATH.to_string());
    let loaded = Arc::new(data::LoadedData::load(&data_path)?);
    tracing::info!(
        path = %data_path,
        methods = loaded.methods.len(),
        types = loaded.types.len(),
        topics = loaded.topics.len(),
        "dataset loaded"
    );

    let addr = std::env::var("TG_AUTODOCS_LISTEN_ADDR").unwrap_or_else(|_| {
        std::env::var("PORT")
            .map(|p| format!("0.0.0.0:{p}"))
            .unwrap_or_else(|_| DEFAULT_LISTEN_ADDR.to_string())
    });

    // Streamable HTTP rejects Host headers outside an allowlist (DNS rebinding
    // protection). By default only loopback hosts are allowed; set
    // TG_AUTODOCS_ALLOWED_HOSTS to a single hostname for a public deployment.
    let allowed_host = std::env::var("TG_AUTODOCS_ALLOWED_HOSTS")
        .ok()
        .filter(|h| !h.trim().is_empty());

    let mut config = StreamableHttpServerConfig::default().with_json_response(true);
    if let Some(host) = allowed_host {
        config = config.with_allowed_hosts([host]);
    }

    let service = StreamableHttpService::new(
        {
            let loaded = Arc::clone(&loaded);
            move || Ok(TgAutoDocs::new(Arc::clone(&loaded)))
        },
        LocalSessionManager::default().into(),
        config,
    );

    let router = Router::new().nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!(%addr, "tgautodocs MCP server listening on http://{addr}/mcp");
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
