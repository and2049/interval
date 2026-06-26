use sqlx::SqlitePool;

const REBUILD_SESSION_ENV: &str = "INTERVAL_REBUILD_SESSION_ON_START";

pub async fn rebuild_cached_replay_on_start(pool: &SqlitePool) -> anyhow::Result<()> {
    let Some(session_key) = rebuild_session_key(std::env::var(REBUILD_SESSION_ENV).ok())? else {
        return Ok(());
    };

    tracing::info!(session_key, "rebuilding cached replay before startup");
    crate::replay::rebuild_from_cache(pool, session_key).await?;
    Ok(())
}

fn rebuild_session_key(value: Option<String>) -> anyhow::Result<Option<i64>> {
    let Some(value) = value else {
        return Ok(None);
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    Ok(Some(trimmed.parse::<i64>()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rebuild_session_key_ignores_missing_or_empty_values() {
        assert_eq!(rebuild_session_key(None).unwrap(), None);
        assert_eq!(rebuild_session_key(Some(" ".to_string())).unwrap(), None);
    }

    #[test]
    fn rebuild_session_key_parses_session_key() {
        assert_eq!(
            rebuild_session_key(Some(" 9472 ".to_string())).unwrap(),
            Some(9472)
        );
    }

    #[test]
    fn rebuild_session_key_rejects_invalid_values() {
        assert!(rebuild_session_key(Some("bahrain".to_string())).is_err());
    }
}
