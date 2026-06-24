use crate::connectors::openf1_historical::RawEndpoint;
use sqlx::{Row, SqlitePool};

pub async fn store_raw_bundle(pool: &SqlitePool, bundle: &[RawEndpoint]) -> anyhow::Result<()> {
    for endpoint in bundle {
        sqlx::query(
            r#"
            INSERT INTO raw_api_cache (endpoint, session_key, payload)
            VALUES (?, ?, ?)
            ON CONFLICT(endpoint, session_key)
            DO UPDATE SET payload = excluded.payload, captured_at = CURRENT_TIMESTAMP
            "#,
        )
        .bind(&endpoint.endpoint)
        .bind(endpoint.session_key)
        .bind(serde_json::to_string(&endpoint.payload)?)
        .execute(pool)
        .await?;
    }
    Ok(())
}

pub async fn load_raw_bundle(
    pool: &SqlitePool,
    session_key: i64,
) -> anyhow::Result<Vec<RawEndpoint>> {
    let rows = sqlx::query(
        "SELECT endpoint, payload FROM raw_api_cache WHERE session_key = ? ORDER BY endpoint",
    )
    .bind(session_key)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(RawEndpoint {
                endpoint: row.get("endpoint"),
                session_key,
                payload: serde_json::from_str(row.get::<String, _>("payload").as_str())?,
            })
        })
        .collect()
}
