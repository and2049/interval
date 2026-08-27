use interval_backend::server::{self, ServeOptions};
use std::net::SocketAddr;
use std::path::PathBuf;

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

    let bind: SocketAddr = std::env::var("INTERVAL_BIND")
        .unwrap_or_else(|_| "127.0.0.1:4000".to_string())
        .parse()?;

    let bound = server::serve(
        ServeOptions {
            bind,
            database_url,
            static_dir: std::env::var_os("INTERVAL_STATIC_DIR").map(PathBuf::from),
            enable_settings_api: std::env::var_os("INTERVAL_ENABLE_SETTINGS_API").is_some(),
        },
        shutdown_signal(),
    )
    .await?;

    bound.task.await??;
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
