use crate::domain::TrackGeometry;
use sqlx::{Row, SqlitePool};

pub async fn replace_track_geometry(
    pool: &SqlitePool,
    geometry: &TrackGeometry,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO track_geometry (session_key, payload)
        VALUES (?, ?)
        ON CONFLICT(session_key) DO UPDATE SET payload = excluded.payload
        "#,
    )
    .bind(geometry.session_key)
    .bind(serde_json::to_string(geometry)?)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_track_geometry(
    pool: &SqlitePool,
    session_key: i64,
) -> sqlx::Result<Option<TrackGeometry>> {
    let row = sqlx::query("SELECT payload FROM track_geometry WHERE session_key = ?")
        .bind(session_key)
        .fetch_optional(pool)
        .await?;
    row.map(|row| super::decode(row.get::<String, _>("payload")))
        .transpose()
}
