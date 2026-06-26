use crate::domain::{IngestStatus, Meeting, Season, Session, SessionReadiness, SessionType};
use sqlx::{Row, SqlitePool};

pub async fn upsert_meetings(pool: &SqlitePool, meetings: &[Meeting]) -> anyhow::Result<()> {
    for meeting in meetings {
        sqlx::query(
            r#"
            INSERT INTO meetings (meeting_key, year, name, country, location)
            VALUES (?, ?, ?, ?, ?)
            ON CONFLICT(meeting_key)
            DO UPDATE SET year = excluded.year, name = excluded.name, country = excluded.country, location = excluded.location
            "#,
        )
        .bind(meeting.meeting_key)
        .bind(meeting.year)
        .bind(&meeting.name)
        .bind(&meeting.country)
        .bind(&meeting.location)
        .execute(pool)
        .await?;
    }
    Ok(())
}

pub async fn upsert_sessions(pool: &SqlitePool, sessions: &[Session]) -> anyhow::Result<()> {
    for session in sessions {
        sqlx::query(
            r#"
            INSERT INTO sessions (session_key, meeting_key, year, name, session_type, start_time, end_time, total_laps)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(session_key)
            DO UPDATE SET meeting_key = excluded.meeting_key, year = excluded.year, name = excluded.name,
              session_type = excluded.session_type, start_time = excluded.start_time, end_time = excluded.end_time,
              total_laps = excluded.total_laps
            "#,
        )
        .bind(session.session_key)
        .bind(session.meeting_key)
        .bind(session.year)
        .bind(&session.name)
        .bind(match session.session_type {
            SessionType::Race => "race",
        })
        .bind(&session.start_time)
        .bind(&session.end_time)
        .bind(session.total_laps)
        .execute(pool)
        .await?;
    }
    Ok(())
}

pub async fn list_seasons(pool: &SqlitePool) -> sqlx::Result<Vec<Season>> {
    sqlx::query("SELECT DISTINCT year FROM meetings ORDER BY year DESC")
        .fetch_all(pool)
        .await
        .map(|rows| {
            rows.into_iter()
                .map(|row| Season {
                    year: row.get("year"),
                })
                .collect()
        })
}

pub async fn list_meetings(pool: &SqlitePool, year: i32) -> sqlx::Result<Vec<Meeting>> {
    sqlx::query("SELECT meeting_key, year, name, country, location FROM meetings WHERE year = ? ORDER BY meeting_key")
        .bind(year)
        .fetch_all(pool)
        .await
        .map(|rows| {
            rows.into_iter()
                .map(|row| Meeting {
                    meeting_key: row.get("meeting_key"),
                    year: row.get("year"),
                    name: row.get("name"),
                    country: row.get("country"),
                    location: row.get("location"),
                })
                .collect()
        })
}

pub async fn get_meeting(pool: &SqlitePool, meeting_key: i64) -> sqlx::Result<Option<Meeting>> {
    sqlx::query(
        "SELECT meeting_key, year, name, country, location FROM meetings WHERE meeting_key = ?",
    )
    .bind(meeting_key)
    .fetch_optional(pool)
    .await
    .map(|row| {
        row.map(|row| Meeting {
            meeting_key: row.get("meeting_key"),
            year: row.get("year"),
            name: row.get("name"),
            country: row.get("country"),
            location: row.get("location"),
        })
    })
}

pub async fn list_sessions(pool: &SqlitePool, meeting_key: i64) -> sqlx::Result<Vec<Session>> {
    sqlx::query("SELECT session_key, meeting_key, year, name, session_type, start_time, end_time, total_laps FROM sessions WHERE meeting_key = ? AND session_type = 'race' ORDER BY start_time")
        .bind(meeting_key)
        .fetch_all(pool)
        .await
        .map(|rows| rows.into_iter().map(session_from_row).collect())
}

pub async fn list_session_readiness(
    pool: &SqlitePool,
    meeting_key: i64,
) -> sqlx::Result<Vec<SessionReadiness>> {
    let rows = sqlx::query(
        r#"
        SELECT
            s.session_key, s.meeting_key, s.year, s.name, s.session_type, s.start_time, s.end_time, s.total_laps,
            COALESCE(i.status, 'not_ingested') AS ingest_status,
            i.last_error,
            CASE WHEN m.session_key IS NULL THEN 0 ELSE 1 END AS replay_ready
        FROM sessions s
        LEFT JOIN ingest_status i ON i.session_key = s.session_key
        LEFT JOIN replay_metadata m ON m.session_key = s.session_key
        WHERE s.meeting_key = ? AND s.session_type = 'race'
        ORDER BY s.start_time
        "#,
    )
    .bind(meeting_key)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(session_readiness_from_row).collect())
}

pub async fn get_session(pool: &SqlitePool, session_key: i64) -> sqlx::Result<Option<Session>> {
    sqlx::query("SELECT session_key, meeting_key, year, name, session_type, start_time, end_time, total_laps FROM sessions WHERE session_key = ?")
        .bind(session_key)
        .fetch_optional(pool)
        .await
        .map(|row| row.map(session_from_row))
}

fn session_from_row(row: sqlx::sqlite::SqliteRow) -> Session {
    Session {
        session_key: row.get("session_key"),
        meeting_key: row.get("meeting_key"),
        year: row.get("year"),
        name: row.get("name"),
        session_type: SessionType::Race,
        start_time: row.get("start_time"),
        end_time: row.get("end_time"),
        total_laps: row.get("total_laps"),
    }
}

fn session_readiness_from_row(row: sqlx::sqlite::SqliteRow) -> SessionReadiness {
    let session = Session {
        session_key: row.get("session_key"),
        meeting_key: row.get("meeting_key"),
        year: row.get("year"),
        name: row.get("name"),
        session_type: SessionType::Race,
        start_time: row.get("start_time"),
        end_time: row.get("end_time"),
        total_laps: row.get("total_laps"),
    };
    let status = super::ingest_status_store::status_from_wire(
        row.get::<String, _>("ingest_status").as_str(),
    );
    let replay_ready = row.get::<i64, _>("replay_ready") == 1;
    SessionReadiness {
        is_demo: session.session_key == crate::storage::DEMO_SESSION_KEY,
        ingest_status: if replay_ready && status == IngestStatus::NotIngested {
            IngestStatus::Ready
        } else {
            status
        },
        replay_ready,
        last_error: row.get("last_error"),
        session,
    }
}
