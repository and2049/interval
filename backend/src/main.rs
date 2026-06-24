use interval_backend::{api, connectors::openf1_historical::HistoricalClient, storage};
use std::net::SocketAddr;
use tower_http::{cors::CorsLayer, trace::TraceLayer};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "interval_backend=info,tower_http=info".to_string()),
        )
        .init();

    let database_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://interval.db".to_string());
    let pool = storage::connect(&database_url).await?;
    storage::migrate(&pool).await?;
    storage::seed_demo_session(&pool).await?;
    storage::seed_mvp_fixture(&pool).await?;

    let state = api::AppState::new(pool, HistoricalClient::default());
    let app = api::router(state)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    let addr: SocketAddr = std::env::var("INTERVAL_BIND")
        .unwrap_or_else(|_| "127.0.0.1:4000".to_string())
        .parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("interval backend listening on http://{addr}");

    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;

    Ok(())
}
