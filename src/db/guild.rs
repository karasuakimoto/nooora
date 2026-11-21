use chrono::{DateTime, Utc};
use sqlx::PgPool;

use crate::db::cards::like_pattern;
use crate::error::BotError;
use crate::models::{valid_series_id, SeriesHit};

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct GuildRow {
    pub guild_id: i64,
    pub linked_series_id: Option<String>,
    pub series_name: Option<String>,
    pub series_boost: f64,
    pub last_series_change_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RosterEntry {
    pub character_name: String,
    pub rarity_tier: String,
}

pub async fn search_series(pool: &PgPool, query: &str) -> Result<Vec<SeriesHit>, BotError> {
    let trimmed = query.trim();
    let id = if valid_series_id(trimmed) {
        trimmed.to_ascii_lowercase()
    } else {
        String::new()
    };
    let pattern = like_pattern(trimmed);
    let trgm = sqlx::query_as::<_, SeriesHit>(
        r#"
        SELECT series_id, series_name, similarity(series_name, $1) AS score
        FROM series
        WHERE series_id = $2
           OR series_name ILIKE $3 ESCAPE '\'
           OR similarity(series_name, $1) > 0.15
        ORDER BY (series_id = $2) DESC, score DESC, series_name
        LIMIT 8
        "#,
    )
    .bind(trimmed)
    .bind(&id)
    .bind(&pattern)
    .fetch_all(pool)
    .await;
    match trgm {
        Ok(rows) => Ok(rows),
        Err(err) if missing_similarity(&err) => {
            sqlx::query_as::<_, SeriesHit>(
                r#"
                SELECT series_id, series_name,
                       CASE WHEN series_id = $2 THEN 1 ELSE 0.5 END AS score
                FROM series
                WHERE series_id = $2 OR series_name ILIKE $3 ESCAPE '\'
                ORDER BY score DESC, series_name
                LIMIT 8
                "#,
            )
            .bind(trimmed)
            .bind(&id)
            .bind(&pattern)
            .fetch_all(pool)
            .await
            .map_err(BotError::from)
        }
        Err(err) => Err(BotError::from(err)),
    }
}

pub async fn roster(pool: &PgPool, series_id: &str) -> Result<Vec<RosterEntry>, BotError> {
    sqlx::query_as::<_, RosterEntry>(
        r#"
        SELECT character_name, rarity_tier
        FROM card_templates
        WHERE series_id = $1
        ORDER BY weight DESC, character_name
        LIMIT 20
        "#,
    )
    .bind(series_id)
    .fetch_all(pool)
    .await
    .map_err(BotError::from)
}

pub async fn settings(pool: &PgPool, guild_id: i64) -> Result<Option<GuildRow>, BotError> {
    sqlx::query_as::<_, GuildRow>(
        r#"
        SELECT g.guild_id, g.linked_series_id, s.series_name, g.series_boost, g.last_series_change_at
        FROM guild_settings g
        LEFT JOIN series s ON s.series_id = g.linked_series_id
        WHERE g.guild_id = $1
        "#,
    )
    .bind(guild_id)
    .fetch_optional(pool)
    .await
    .map_err(BotError::from)
}

pub async fn set_series(
    pool: &PgPool,
    guild_id: i64,
    series_id: &str,
    cooldown: chrono::Duration,
) -> Result<GuildRow, BotError> {
    let series_id = series_id.trim().to_ascii_lowercase();
    if !valid_series_id(&series_id) {
        return Err(BotError::user_message(
            "series ids are 6 letters or digits, for example `086c62`",
        ));
    }
    let mut tx = crate::db::begin(pool).await?;
    sqlx::query(
        "INSERT INTO guild_settings (guild_id) VALUES ($1) ON CONFLICT (guild_id) DO NOTHING",
    )
    .bind(guild_id)
    .execute(&mut *tx)
    .await?;
    let current: (Option<String>, Option<DateTime<Utc>>) = sqlx::query_as(
        "SELECT linked_series_id, last_series_change_at FROM guild_settings WHERE guild_id = $1 FOR UPDATE",
    )
    .bind(guild_id)
    .fetch_one(&mut *tx)
    .await?;
    if let Some(changed) = current.1 {
        let ready = changed + cooldown;
        if Utc::now() < ready {
            return Err(BotError::user_message(format!(
                "This server can change series again <t:{}:R>.",
                ready.timestamp()
            )));
        }
    }
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM series WHERE series_id = $1)")
            .bind(&series_id)
            .fetch_one(&mut *tx)
            .await?;
    if !exists {
        return Err(BotError::user_message(format!(
            "no series is registered with id `{series_id}`"
        )));
    }
    sqlx::query(
        r#"
        UPDATE guild_settings
        SET linked_series_id = $2,
            last_series_change_at = now()
        WHERE guild_id = $1
        "#,
    )
    .bind(guild_id)
    .bind(&series_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    settings(pool, guild_id)
        .await?
        .ok_or_else(|| BotError::user_message("guild settings disappeared"))
}

fn missing_similarity(err: &sqlx::Error) -> bool {
    match err {
        sqlx::Error::Database(db) => db.code().as_deref() == Some("42883"),
        _ => false,
    }
}
