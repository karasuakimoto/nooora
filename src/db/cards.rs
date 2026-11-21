use sqlx::PgPool;

use crate::db::{self, quests};
use crate::error::BotError;
use crate::models::{Cosmetics, OwnedCard};

pub const PAGE_SIZE: i64 = 8;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SearchHit {
    pub character_name: String,
    pub series_name: String,
    pub series_id: String,
    pub rarity_tier: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AlbumRow {
    pub album_id: i64,
    pub name: String,
    pub cards: i64,
}

pub async fn inventory(
    pool: &PgPool,
    owner_id: i64,
    page: i64,
) -> Result<(Vec<OwnedCard>, i64), BotError> {
    let page = page.max(0);
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM card_instances WHERE owner_id = $1")
        .bind(owner_id)
        .fetch_one(pool)
        .await?;
    let cards = sqlx::query_as::<_, OwnedCard>(
        r#"
        SELECT i.card_uid, i.owner_id, i.template_id, i.print_number, i.variant, i.cosmetics,
               i.hold_kind, i.hold_ref,
               t.character_name, t.series_id, t.series_name, t.base_image_url, t.rarity_tier
        FROM card_instances i
        JOIN card_templates t ON t.template_id = i.template_id
        WHERE i.owner_id = $1
        ORDER BY i.created_at DESC, i.card_uid
        LIMIT $2 OFFSET $3
        "#,
    )
    .bind(owner_id)
    .bind(PAGE_SIZE)
    .bind(page * PAGE_SIZE)
    .fetch_all(pool)
    .await?;
    Ok((cards, total))
}

pub async fn find_card(pool: &PgPool, card_uid: &str) -> Result<Option<OwnedCard>, BotError> {
    sqlx::query_as::<_, OwnedCard>(
        r#"
        SELECT i.card_uid, i.owner_id, i.template_id, i.print_number, i.variant, i.cosmetics,
               i.hold_kind, i.hold_ref,
               t.character_name, t.series_id, t.series_name, t.base_image_url, t.rarity_tier
        FROM card_instances i
        JOIN card_templates t ON t.template_id = i.template_id
        WHERE i.card_uid = $1
        "#,
    )
    .bind(card_uid)
    .fetch_optional(pool)
    .await
    .map_err(BotError::from)
}

pub async fn search(pool: &PgPool, query: &str) -> Result<Vec<SearchHit>, BotError> {
    let pattern = like_pattern(query);
    let series_id = if query.len() == 6 {
        query.to_ascii_lowercase()
    } else {
        String::new()
    };
    sqlx::query_as::<_, SearchHit>(
        r#"
        SELECT character_name, series_name, series_id, rarity_tier
        FROM card_templates
        WHERE character_name ILIKE $1 ESCAPE '\'
           OR series_name ILIKE $1 ESCAPE '\'
           OR series_id = $2
        ORDER BY (series_id = $2) DESC, weight DESC, character_name
        LIMIT 10
        "#,
    )
    .bind(&pattern)
    .bind(&series_id)
    .fetch_all(pool)
    .await
    .map_err(BotError::from)
}

pub async fn update_cosmetics(
    pool: &PgPool,
    owner_id: i64,
    card_uid: &str,
    cosmetics: Cosmetics,
) -> Result<(), BotError> {
    let mut tx = db::begin(pool).await?;
    let updated = sqlx::query(
        r#"
        UPDATE card_instances
        SET cosmetics = $3
        WHERE card_uid = $1 AND owner_id = $2
        "#,
    )
    .bind(card_uid)
    .bind(owner_id)
    .bind(sqlx::types::Json(&cosmetics))
    .execute(&mut *tx)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(BotError::user_message("that card is not in your inventory"));
    }
    for id in [
        cosmetics.frame_id.as_deref(),
        cosmetics.filter_id.as_deref(),
        cosmetics.effect_3d_id.as_deref(),
        cosmetics.font_family.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        let owned: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM user_cosmetics WHERE user_id = $1 AND cosmetic_id = $2)",
        )
        .bind(owner_id)
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        if !owned {
            return Err(BotError::user_message(format!(
                "you do not own cosmetic `{id}`"
            )));
        }
    }
    quests::bump(&mut tx, owner_id, "apply_cosmetic", 1, chrono::Utc::now()).await?;
    tx.commit().await?;
    Ok(())
}

pub async fn albums(pool: &PgPool, owner_id: i64) -> Result<Vec<AlbumRow>, BotError> {
    sqlx::query_as::<_, AlbumRow>(
        r#"
        SELECT a.album_id, a.name, COUNT(c.card_uid) AS cards
        FROM albums a
        LEFT JOIN album_cards c ON c.album_id = a.album_id
        WHERE a.owner_id = $1
        GROUP BY a.album_id, a.name
        ORDER BY a.name
        "#,
    )
    .bind(owner_id)
    .fetch_all(pool)
    .await
    .map_err(BotError::from)
}

pub async fn create_album(pool: &PgPool, owner_id: i64, name: &str) -> Result<i64, BotError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 40 {
        return Err(BotError::user_message("album names must be 1 to 40 characters"));
    }
    sqlx::query("INSERT INTO users (user_id) VALUES ($1) ON CONFLICT DO NOTHING")
        .bind(owner_id)
        .execute(pool)
        .await?;
    sqlx::query_scalar(
        "INSERT INTO albums (owner_id, name) VALUES ($1, $2) RETURNING album_id",
    )
    .bind(owner_id)
    .bind(name)
    .fetch_one(pool)
    .await
    .map_err(|err| {
        if let sqlx::Error::Database(db) = &err {
            if db.constraint() == Some("albums_owner_name_unique") {
                return BotError::user_message("you already have an album with that name");
            }
        }
        BotError::from(err)
    })
}

pub async fn add_to_album(
    pool: &PgPool,
    owner_id: i64,
    album_id: i64,
    card_uid: &str,
) -> Result<(), BotError> {
    let owns_album: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM albums WHERE album_id = $1 AND owner_id = $2)",
    )
    .bind(album_id)
    .bind(owner_id)
    .fetch_one(pool)
    .await?;
    if !owns_album {
        return Err(BotError::user_message("that album is not yours"));
    }
    let owns_card: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM card_instances WHERE card_uid = $1 AND owner_id = $2)",
    )
    .bind(card_uid)
    .bind(owner_id)
    .fetch_one(pool)
    .await?;
    if !owns_card {
        return Err(BotError::user_message("that card is not yours"));
    }
    let position: i32 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(position), 0) + 1 FROM album_cards WHERE album_id = $1",
    )
    .bind(album_id)
    .fetch_one(pool)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO album_cards (album_id, card_uid, position)
        VALUES ($1, $2, $3)
        ON CONFLICT DO NOTHING
        "#,
    )
    .bind(album_id)
    .bind(card_uid)
    .bind(position)
    .execute(pool)
    .await?;
    Ok(())
}

pub fn like_pattern(query: &str) -> String {
    let escaped = query
        .trim()
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}
