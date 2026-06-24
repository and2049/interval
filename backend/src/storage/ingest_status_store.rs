use crate::domain::IngestStatus;
use sqlx::{Row, SqlitePool};

pub async fn set_ingest_status(
    pool: &SqlitePool,
    session_key: i64,
    status: IngestStatus,
    last_error: Option<&str>,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO ingest_status (session_key, status, last_error, updated_at)
        VALUES (?, ?, ?, CURRENT_TIMESTAMP)
        ON CONFLICT(session_key)
        DO UPDATE SET status = excluded.status, last_error = excluded.last_error, updated_at = CURRENT_TIMESTAMP
        "#,
    )
    .bind(session_key)
    .bind(status_wire(&status))
    .bind(last_error)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_ingest_status(
    pool: &SqlitePool,
    session_key: i64,
) -> sqlx::Result<(IngestStatus, Option<String>)> {
    let row = sqlx::query("SELECT status, last_error FROM ingest_status WHERE session_key = ?")
        .bind(session_key)
        .fetch_optional(pool)
        .await?;
    Ok(row.map_or((IngestStatus::NotIngested, None), |row| {
        (
            status_from_wire(row.get::<String, _>("status").as_str()),
            row.get("last_error"),
        )
    }))
}

pub(crate) fn status_wire(status: &IngestStatus) -> &'static str {
    match status {
        IngestStatus::NotIngested => "not_ingested",
        IngestStatus::Fetching => "fetching",
        IngestStatus::Normalizing => "normalizing",
        IngestStatus::Ready => "ready",
        IngestStatus::Failed => "failed",
    }
}

pub(crate) fn status_from_wire(value: &str) -> IngestStatus {
    match value {
        "fetching" => IngestStatus::Fetching,
        "normalizing" => IngestStatus::Normalizing,
        "ready" => IngestStatus::Ready,
        "failed" => IngestStatus::Failed,
        _ => IngestStatus::NotIngested,
    }
}
