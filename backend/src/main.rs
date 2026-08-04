use interval_backend::{api, connectors::openf1_historical::HistoricalClient, startup, storage};
use std::net::SocketAddr;
use std::path::PathBuf;
use tower_http::{cors::CorsLayer, services::ServeDir, trace::TraceLayer};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    interval_backend::env::load_dotenv();

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
    startup::rebuild_cached_replay_on_start(&pool).await?;

    let state = api::AppState::new(pool, HistoricalClient::default());
    let mut app = api::router(state.clone());

    // Optional: serve a built frontend from the same origin as the API. Unset in the
    // web deployment and in local dev (Vite serves the UI there); set by the desktop
    // shell so the renderer's root-relative /api calls and SSE streams stay same-origin.
    // The fallback must be attached before `.layer(...)` so static responses are traced.
    if let Some(dir) = std::env::var_os("INTERVAL_STATIC_DIR") {
        let dir = PathBuf::from(dir);
        if !dir.join("index.html").is_file() {
            anyhow::bail!(
                "INTERVAL_STATIC_DIR={} does not contain index.html",
                dir.display()
            );
        }
        tracing::info!(dir = %dir.display(), "serving static UI");
        // A router fallback only runs when no route matched, so /healthz and every
        // /api path still win. Deliberately no index.html catch-all: it would turn a
        // mistyped /api path into a 200 text/html that the frontend cannot parse.
        app = app.fallback_service(ServeDir::new(&dir));
    }

    // `Router::layer` wraps only the routes registered so far, so the settings merge
    // below deliberately lands outside this permissive CORS layer.
    let mut app = app.layer(CorsLayer::permissive());

    // The settings routes read and write an API credential and this service has no
    // authentication, so they exist only for the desktop shell, which sets this flag.
    // Being outside the CORS layer means a cross-origin request gets no
    // Access-Control-Allow-Origin and no OPTIONS handler, so the browser's preflight
    // fails and the request is never delivered.
    if std::env::var_os("INTERVAL_ENABLE_SETTINGS_API").is_some() {
        tracing::info!("settings API enabled");
        app = app.merge(api::settings_router(state));
    }

    let app = app.layer(TraceLayer::new_for_http());

    let addr: SocketAddr = std::env::var("INTERVAL_BIND")
        .unwrap_or_else(|_| "127.0.0.1:4000".to_string())
        .parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let bound = listener.local_addr()?;
    tracing::info!("interval backend listening on http://{bound}");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

async fn shutdown_signal() {
    // When supervised by a parent process (the desktop shell), stdin is a pipe whose
    // write end closes if the parent exits for any reason, including a crash. Reading
    // it to EOF is the one shutdown trigger that works the same on every platform;
    // a hard kill from the parent would never run this handler at all.
    if std::env::var_os("INTERVAL_SHUTDOWN_ON_STDIN_EOF").is_some() {
        tokio::select! {
            _ = ctrl_c_signal() => {}
            _ = stdin_closed() => tracing::info!("stdin closed; shutting down"),
        }
    } else {
        ctrl_c_signal().await;
    }
}

async fn ctrl_c_signal() {
    match tokio::signal::ctrl_c().await {
        Ok(()) => tracing::info!("shutdown signal received"),
        Err(error) => {
            tracing::warn!(
                error = %error,
                "ctrl-c handler unavailable; continuing until process termination"
            );
            std::future::pending::<()>().await;
        }
    }
}

async fn stdin_closed() {
    let _ = tokio::task::spawn_blocking(|| {
        use std::io::Read;
        let mut buf = [0u8; 256];
        while !matches!(std::io::stdin().read(&mut buf), Ok(0) | Err(_)) {}
    })
    .await;
}
