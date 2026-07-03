use crate::domain::{Meeting, TrackGeometry, TrackGeometryQuality, TrackGeometrySource};
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

pub async fn get_reusable_track_geometry_for_meeting(
    pool: &SqlitePool,
    meeting: &Meeting,
    target_session_key: i64,
) -> sqlx::Result<Option<TrackGeometry>> {
    let rows = sqlx::query(
        r#"
        SELECT tg.payload
        FROM track_geometry tg
        JOIN sessions s ON s.session_key = tg.session_key
        JOIN meetings m ON m.meeting_key = s.meeting_key
        WHERE s.session_key != ?
          AND LOWER(m.country) = LOWER(?)
          AND LOWER(m.location) = LOWER(?)
        ORDER BY s.year DESC, s.start_time DESC
        "#,
    )
    .bind(target_session_key)
    .bind(&meeting.country)
    .bind(&meeting.location)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .filter_map(|row| super::decode::<TrackGeometry>(row.get::<String, _>("payload")).ok())
        .find(|geometry| is_reusable_geometry(geometry))
        .map(|mut geometry| {
            geometry.session_key = target_session_key;
            geometry
        }))
}

fn is_reusable_geometry(geometry: &TrackGeometry) -> bool {
    geometry.quality == TrackGeometryQuality::Ready
        && matches!(
            geometry.source,
            TrackGeometrySource::FastF1Telemetry
                | TrackGeometrySource::OpenF1Location
                | TrackGeometrySource::CuratedStatic
        )
        && !geometry.centerline.is_empty()
}
