use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    SqlitePool,
};
use std::str::FromStr;

pub async fn connect(database_url: &str) -> anyhow::Result<SqlitePool> {
    let options = SqliteConnectOptions::from_str(database_url)?.create_if_missing(true);
    Ok(SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?)
}

pub async fn migrate(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS meetings (
            meeting_key INTEGER PRIMARY KEY,
            year INTEGER NOT NULL,
            name TEXT NOT NULL,
            country TEXT NOT NULL,
            location TEXT NOT NULL
        );
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS sessions (
            session_key INTEGER PRIMARY KEY,
            meeting_key INTEGER NOT NULL,
            year INTEGER NOT NULL,
            name TEXT NOT NULL,
            session_type TEXT NOT NULL,
            start_time TEXT NOT NULL,
            end_time TEXT NOT NULL,
            total_laps INTEGER NOT NULL,
            FOREIGN KEY(meeting_key) REFERENCES meetings(meeting_key)
        );
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS ingest_status (
            session_key INTEGER PRIMARY KEY,
            status TEXT NOT NULL,
            last_error TEXT,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY(session_key) REFERENCES sessions(session_key)
        );
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS replay_metadata (
            session_key INTEGER PRIMARY KEY,
            payload TEXT NOT NULL,
            FOREIGN KEY(session_key) REFERENCES sessions(session_key)
        );
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS track_geometry (
            session_key INTEGER PRIMARY KEY,
            payload TEXT NOT NULL,
            FOREIGN KEY(session_key) REFERENCES sessions(session_key)
        );
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS replay_snapshots (
            session_key INTEGER NOT NULL,
            t REAL NOT NULL,
            payload TEXT NOT NULL,
            PRIMARY KEY(session_key, t),
            FOREIGN KEY(session_key) REFERENCES sessions(session_key)
        );
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS replay_events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_key INTEGER NOT NULL,
            t REAL NOT NULL,
            category TEXT NOT NULL,
            message TEXT NOT NULL,
            payload TEXT NOT NULL,
            FOREIGN KEY(session_key) REFERENCES sessions(session_key)
        );
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS raw_api_cache (
            endpoint TEXT NOT NULL,
            session_key INTEGER NOT NULL,
            captured_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            payload TEXT NOT NULL,
            PRIMARY KEY(endpoint, session_key)
        );
        "#,
    )
    .execute(pool)
    .await?;

    Ok(())
}
