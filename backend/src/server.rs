//! Embeddable server assembly: everything `main.rs` used to do between reading the
//! environment and blocking on axum, factored out so the GPUI desktop app can run the
//! backend in-process. The binary stays the env-driven wrapper; the desktop passes
//! explicit [`ServeOptions`] (bind `127.0.0.1:0`, settings API on) and learns the
//! bound address from the returned [`BoundServer`].

use crate::connectors::openf1_historical::HistoricalClient;
use crate::{api, startup, storage};
use std::future::Future;
use std::net::SocketAddr;
use tokio::task::JoinHandle;
use tower_http::{cors::CorsLayer, trace::TraceLayer};

pub struct ServeOptions {
    pub bind: SocketAddr,
    pub database_url: String,
    /// The settings routes read and write an API credential and this service has no
    /// authentication, so they exist only for the desktop app.
    pub enable_settings_api: bool,
}

pub struct BoundServer {
    /// The actual bound address — meaningful when `bind` used port 0.
    pub addr: SocketAddr,
    /// Resolves when the server has drained after the shutdown future completes.
    pub task: JoinHandle<Result<(), std::io::Error>>,
}

/// Migrates and seeds the database, assembles the router, binds, and spawns
/// `axum::serve` on the current tokio runtime. `shutdown` completing triggers a
/// graceful drain; await `BoundServer::task` to observe completion.
pub async fn serve(
    opts: ServeOptions,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> anyhow::Result<BoundServer> {
    let pool = storage::connect(&opts.database_url).await?;
    storage::migrate(&pool).await?;
    storage::seed_demo_session(&pool).await?;
    storage::seed_mvp_fixture(&pool).await?;
    startup::rebuild_cached_replay_on_start(&pool).await?;

    let state = api::AppState::new(pool, HistoricalClient::default());
    let app = api::router(state.clone());

    // `Router::layer` wraps only the routes registered so far, so the settings merge
    // below deliberately lands outside this permissive CORS layer.
    let mut app = app.layer(CorsLayer::permissive());

    // Being outside the CORS layer means a cross-origin request gets no
    // Access-Control-Allow-Origin and no OPTIONS handler, so the browser's preflight
    // fails and the request is never delivered.
    if opts.enable_settings_api {
        tracing::info!("settings API enabled");
        app = app.merge(api::settings_router(state));
    }

    let app = app.layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(opts.bind).await?;
    let addr = listener.local_addr()?;
    tracing::info!("interval backend listening on http://{addr}");

    let task = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown)
            .await
    });

    Ok(BoundServer { addr, task })
}
