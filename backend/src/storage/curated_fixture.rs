use crate::domain::{IngestStatus, Meeting, Session, SessionType};
use sqlx::SqlitePool;

pub const MVP_SESSION_KEY: i64 = 9472;

pub async fn seed_mvp_fixture(pool: &SqlitePool) -> anyhow::Result<()> {
    let meeting = Meeting {
        meeting_key: 1229,
        year: 2024,
        name: "Bahrain Grand Prix".to_string(),
        country: "Bahrain".to_string(),
        location: "Sakhir".to_string(),
    };
    let session = Session {
        session_key: MVP_SESSION_KEY,
        meeting_key: meeting.meeting_key,
        year: meeting.year,
        name: "Race".to_string(),
        session_type: SessionType::Race,
        start_time: "2024-03-02T15:00:00Z".to_string(),
        end_time: "2024-03-02T17:00:00Z".to_string(),
        total_laps: 57,
    };

    super::upsert_meetings(pool, &[meeting]).await?;
    super::upsert_sessions(pool, &[session]).await?;
    let (status, _) = super::get_ingest_status(pool, MVP_SESSION_KEY).await?;
    if matches!(status, IngestStatus::Fetching | IngestStatus::Normalizing) {
        super::set_ingest_status(
            pool,
            MVP_SESSION_KEY,
            IngestStatus::Failed,
            Some("Previous ingest was interrupted before completion."),
        )
        .await?;
    }
    Ok(())
}
