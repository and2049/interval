use crate::domain::{ReplayEvent, ReplayMetadata, ReplaySnapshot};
use sqlx::{Row, SqlitePool};

pub async fn replace_replay(
    pool: &SqlitePool,
    metadata: &ReplayMetadata,
    snapshots: &[ReplaySnapshot],
    events: &[ReplayEvent],
) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM replay_metadata WHERE session_key = ?")
        .bind(metadata.session.session_key)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM replay_snapshots WHERE session_key = ?")
        .bind(metadata.session.session_key)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM replay_events WHERE session_key = ?")
        .bind(metadata.session.session_key)
        .execute(&mut *tx)
        .await?;

    sqlx::query("INSERT INTO replay_metadata (session_key, payload) VALUES (?, ?)")
        .bind(metadata.session.session_key)
        .bind(serde_json::to_string(metadata)?)
        .execute(&mut *tx)
        .await?;

    for snapshot in snapshots {
        sqlx::query("INSERT INTO replay_snapshots (session_key, t, payload) VALUES (?, ?, ?)")
            .bind(metadata.session.session_key)
            .bind(snapshot.cursor.t)
            .bind(serde_json::to_string(snapshot)?)
            .execute(&mut *tx)
            .await?;
    }

    for event in events {
        sqlx::query(
            "INSERT INTO replay_events (session_key, t, category, message, payload) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(metadata.session.session_key)
        .bind(event.t)
        .bind(format!("{:?}", event.kind))
        .bind(&event.message)
        .bind(serde_json::to_string(event)?)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(())
}

pub async fn get_replay_metadata(
    pool: &SqlitePool,
    session_key: i64,
) -> sqlx::Result<Option<ReplayMetadata>> {
    let row = sqlx::query("SELECT payload FROM replay_metadata WHERE session_key = ?")
        .bind(session_key)
        .fetch_optional(pool)
        .await?;
    row.map(|row| super::decode(row.get::<String, _>("payload")))
        .transpose()
}

pub async fn get_replay_snapshot(
    pool: &SqlitePool,
    session_key: i64,
    t: f64,
) -> sqlx::Result<Option<ReplaySnapshot>> {
    let prior = sqlx::query(
        "SELECT payload FROM replay_snapshots WHERE session_key = ? AND t <= ? ORDER BY t DESC LIMIT 1",
    )
    .bind(session_key)
    .bind(t)
    .fetch_optional(pool)
    .await?;

    let row =
        match prior {
            Some(row) => Some(row),
            None => sqlx::query(
                "SELECT payload FROM replay_snapshots WHERE session_key = ? ORDER BY t ASC LIMIT 1",
            )
            .bind(session_key)
            .fetch_optional(pool)
            .await?,
        };

    row.map(|row| super::decode(row.get::<String, _>("payload")))
        .transpose()
}

pub async fn list_replay_snapshots_from(
    pool: &SqlitePool,
    session_key: i64,
    from_t: f64,
) -> sqlx::Result<Vec<ReplaySnapshot>> {
    let rows = sqlx::query(
        "SELECT payload FROM replay_snapshots WHERE session_key = ? AND t >= ? ORDER BY t ASC",
    )
    .bind(session_key)
    .bind(from_t)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| super::decode(row.get::<String, _>("payload")))
        .collect()
}

pub async fn list_replay_snapshots_page(
    pool: &SqlitePool,
    session_key: i64,
    from_t: f64,
    limit: i64,
) -> sqlx::Result<Vec<ReplaySnapshot>> {
    let rows = sqlx::query(
        "SELECT payload FROM replay_snapshots WHERE session_key = ? AND t >= ? ORDER BY t ASC LIMIT ?",
    )
    .bind(session_key)
    .bind(from_t)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| super::decode(row.get::<String, _>("payload")))
        .collect()
}

pub async fn get_replay_events(
    pool: &SqlitePool,
    session_key: i64,
) -> sqlx::Result<Vec<ReplayEvent>> {
    let rows = sqlx::query("SELECT payload FROM replay_events WHERE session_key = ? ORDER BY t")
        .bind(session_key)
        .fetch_all(pool)
        .await?;
    rows.into_iter()
        .map(|row| super::decode(row.get::<String, _>("payload")))
        .collect()
}
