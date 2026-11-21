use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};

use crate::config::Config;
use crate::db::{self, quests, users};
use crate::engine::charges::{self, Pool};
use crate::error::BotError;
use crate::ids::card_uid;
use crate::models::{CardVariant, RarityTier, TemplateRow};

#[derive(Debug, Clone)]
pub struct OpenDrop {
    pub drop_id: i64,
    pub expires_at: DateTime<Utc>,
    pub templates: Vec<TemplateRow>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SlotView {
    pub slot: i16,
    pub claimed_by: Option<i64>,
    pub card_uid: Option<String>,
    pub character_name: String,
    pub series_id: String,
    pub series_name: String,
    pub base_image_url: Option<String>,
    pub rarity_tier: String,
    pub print_number: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct ClaimedCard {
    pub card_uid: String,
    pub print_number: i32,
    pub character_name: String,
    pub series_name: String,
}

pub async fn open_drop(
    pool: &PgPool,
    user_id: i64,
    channel_id: i64,
    guild_id: Option<i64>,
    config: &Config,
) -> Result<OpenDrop, BotError> {
    let now = Utc::now();
    let mut tx = db::begin(pool).await?;
    let user = users::lock_user(&mut tx, user_id).await?;
    let (drops, grabs) = users::view_charges(
        &user,
        now,
        config.drop_max,
        config.grab_max,
        config.drop_regen,
        config.grab_regen,
    );
    let drops = charges::try_spend(drops, now, config.drop_max, config.drop_regen).map_err(
        |ready_at| BotError::Charge {
            kind: "drop",
            ready_at,
        },
    )?;
    let total_drops = user.total_drops + 1;
    users::save_charges(&mut tx, user_id, drops, grabs, total_drops).await?;
    quests::bump(&mut tx, user_id, "drops", 1, now).await?;

    let guild_series = match guild_id {
        Some(id) => guild_series(&mut tx, id).await?,
        None => None,
    };
    let event_series = event_series(&mut tx, now).await?;
    let templates = roll_three(
        &mut tx,
        guild_series.as_deref(),
        config.series_boost,
        event_series.as_deref(),
        config.event_boost,
    )
    .await?;
    let expires_at = now + chrono::Duration::seconds(config.drop_ttl.as_secs() as i64);
    let drop_id: i64 = sqlx::query_scalar(
        r#"
        INSERT INTO drops (channel_id, guild_id, dropper_id, expires_at)
        VALUES ($1, $2, $3, $4)
        RETURNING drop_id
        "#,
    )
    .bind(channel_id)
    .bind(guild_id)
    .bind(user_id)
    .bind(expires_at)
    .fetch_one(&mut *tx)
    .await?;
    for (index, template) in templates.iter().enumerate() {
        sqlx::query(
            "INSERT INTO drop_slots (drop_id, slot, template_id) VALUES ($1, $2, $3)",
        )
        .bind(drop_id)
        .bind((index as i16) + 1)
        .bind(template.template_id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(OpenDrop {
        drop_id,
        expires_at,
        templates,
    })
}

pub async fn attach_message(pool: &PgPool, drop_id: i64, message_id: i64) -> Result<(), BotError> {
    sqlx::query("UPDATE drops SET message_id = $2 WHERE drop_id = $1")
        .bind(drop_id)
        .bind(message_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn refund_unclaimed(pool: &PgPool, drop_id: i64, user_id: i64) -> Result<(), BotError> {
    let mut tx = db::begin(pool).await?;
    let claimed: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM drop_slots WHERE drop_id = $1 AND claimed_by IS NOT NULL",
    )
    .bind(drop_id)
    .fetch_one(&mut *tx)
    .await?;
    if claimed == 0 {
        let user = users::lock_user(&mut tx, user_id).await?;
        let restored = Pool {
            charges: user.drop_charges as i32 + 1,
            next_regen_at: user.drop_next_regen,
        };
        let grabs = Pool {
            charges: user.grab_charges as i32,
            next_regen_at: user.grab_next_regen,
        };
        users::save_charges(
            &mut tx,
            user_id,
            restored,
            grabs,
            (user.total_drops - 1).max(0),
        )
        .await?;
        quests::bump(&mut tx, user_id, "drops", -1, Utc::now()).await?;
        sqlx::query("DELETE FROM drops WHERE drop_id = $1")
            .bind(drop_id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn load_slots(pool: &PgPool, drop_id: i64) -> Result<Vec<SlotView>, BotError> {
    sqlx::query_as::<_, SlotView>(
        r#"
        SELECT s.slot, s.claimed_by, s.card_uid,
               t.character_name, t.series_id, t.series_name, t.base_image_url, t.rarity_tier,
               i.print_number
        FROM drop_slots s
        JOIN card_templates t ON t.template_id = s.template_id
        LEFT JOIN card_instances i ON i.card_uid = s.card_uid
        WHERE s.drop_id = $1
        ORDER BY s.slot
        "#,
    )
    .bind(drop_id)
    .fetch_all(pool)
    .await
    .map_err(BotError::from)
}

pub async fn claim_slot(
    pool: &PgPool,
    drop_id: i64,
    slot: i16,
    user_id: i64,
    config: &Config,
) -> Result<ClaimedCard, BotError> {
    let now = Utc::now();
    let mut tx = db::begin(pool).await?;
    let expires_at: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT expires_at FROM drops WHERE drop_id = $1 FOR UPDATE")
            .bind(drop_id)
            .fetch_optional(&mut *tx)
            .await?;
    let Some(expires_at) = expires_at else {
        return Err(BotError::user_message("that drop no longer exists"));
    };
    if now >= expires_at {
        return Err(BotError::DropExpired);
    }
    let user = users::lock_user(&mut tx, user_id).await?;
    let (drops, grabs) = users::view_charges(
        &user,
        now,
        config.drop_max,
        config.grab_max,
        config.drop_regen,
        config.grab_regen,
    );
    let grabs = charges::try_spend(grabs, now, config.grab_max, config.grab_regen).map_err(
        |ready_at| BotError::Charge {
            kind: "grab",
            ready_at,
        },
    )?;
    let template_id: Option<i64> = sqlx::query_scalar(
        r#"
        SELECT template_id FROM drop_slots
        WHERE drop_id = $1 AND slot = $2 AND claimed_by IS NULL
        FOR UPDATE
        "#,
    )
    .bind(drop_id)
    .bind(slot)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(template_id) = template_id else {
        return Err(BotError::SlotTaken);
    };
    let template = load_template(&mut tx, template_id).await?;
    let print_number = next_print(&mut tx, template_id).await?;
    let uid = insert_instance(
        &mut tx,
        user_id,
        &template,
        print_number,
    )
    .await?;
    sqlx::query(
        "UPDATE drop_slots SET claimed_by = $3, card_uid = $4 WHERE drop_id = $1 AND slot = $2",
    )
    .bind(drop_id)
    .bind(slot)
    .bind(user_id)
    .bind(&uid)
    .execute(&mut *tx)
    .await?;
    users::save_charges(&mut tx, user_id, drops, grabs, user.total_drops).await?;
    db::write_audit(
        &mut tx,
        &crate::models::AuditEvent {
            tx_id: db::new_tx_id(),
            kind: "mint",
            sender_id: None,
            receiver_id: Some(user_id),
            cards: vec![uid.clone()],
            gold_delta: 0,
            metadata: serde_json::json!({
                "drop_id": drop_id,
                "slot": slot,
                "print_number": print_number,
                "template_id": template_id
            }),
        },
    )
    .await?;
    tx.commit().await?;
    Ok(ClaimedCard {
        card_uid: uid,
        print_number,
        character_name: template.character_name,
        series_name: template.series_name,
    })
}

async fn roll_three(
    tx: &mut Transaction<'_, Postgres>,
    guild_series: Option<&str>,
    guild_boost: f64,
    event_series: Option<&str>,
    event_boost: f64,
) -> Result<Vec<TemplateRow>, BotError> {
    let mut picked = Vec::new();
    let mut excluded: Vec<i64> = Vec::new();
    for _ in 0..3 {
        let row = sqlx::query_as::<_, TemplateRow>(
            r#"
            SELECT template_id, character_name, series_id, series_name, base_image_url, rarity_tier, weight
            FROM card_templates
            WHERE NOT (template_id = ANY($1))
            ORDER BY -ln(GREATEST(random(), 1e-12)) / GREATEST(
                LEAST(
                    weight::float8
                        * CASE WHEN $2::text IS NOT NULL AND series_id = $2 THEN $3 ELSE 1 END
                        * CASE WHEN $4::text IS NOT NULL AND series_id = $4 THEN $5 ELSE 1 END,
                    weight::float8 * 9
                ),
                0.000001
            )
            LIMIT 1
            "#,
        )
        .bind(&excluded)
        .bind(guild_series)
        .bind(guild_boost)
        .bind(event_series)
        .bind(event_boost)
        .fetch_one(&mut **tx)
        .await?;
        excluded.push(row.template_id);
        picked.push(row);
    }
    Ok(picked)
}

async fn guild_series(
    tx: &mut Transaction<'_, Postgres>,
    guild_id: i64,
) -> Result<Option<String>, BotError> {
    sqlx::query_scalar("SELECT linked_series_id FROM guild_settings WHERE guild_id = $1")
        .bind(guild_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(BotError::from)
}

async fn event_series(
    tx: &mut Transaction<'_, Postgres>,
    now: DateTime<Utc>,
) -> Result<Option<String>, BotError> {
    sqlx::query_scalar(
        r#"
        SELECT series_id FROM events
        WHERE starts_at <= $1 AND ends_at > $1 AND series_id IS NOT NULL
        ORDER BY starts_at DESC
        LIMIT 1
        "#,
    )
    .bind(now)
    .fetch_optional(&mut **tx)
    .await
    .map_err(BotError::from)
}

async fn load_template(
    tx: &mut Transaction<'_, Postgres>,
    template_id: i64,
) -> Result<TemplateRow, BotError> {
    sqlx::query_as::<_, TemplateRow>(
        r#"
        SELECT template_id, character_name, series_id, series_name, base_image_url, rarity_tier, weight
        FROM card_templates WHERE template_id = $1
        "#,
    )
    .bind(template_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(BotError::from)
}

async fn next_print(
    tx: &mut Transaction<'_, Postgres>,
    template_id: i64,
) -> Result<i32, BotError> {
    sqlx::query_scalar(
        r#"
        INSERT INTO template_print_counters (template_id, next_print)
        VALUES ($1, 2)
        ON CONFLICT (template_id) DO UPDATE
        SET next_print = template_print_counters.next_print + 1
        RETURNING next_print - 1
        "#,
    )
    .bind(template_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(BotError::from)
}

async fn insert_instance(
    tx: &mut Transaction<'_, Postgres>,
    owner_id: i64,
    template: &TemplateRow,
    print_number: i32,
) -> Result<String, BotError> {
    let variant = CardVariant::from_rarity(RarityTier::parse(&template.rarity_tier)).db();
    let cosmetics = sqlx::types::Json(crate::models::Cosmetics::default());
    for _ in 0..6 {
        let uid = card_uid();
        let inserted = sqlx::query(
            r#"
            INSERT INTO card_instances
                (card_uid, owner_id, template_id, print_number, variant, cosmetics)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (card_uid) DO NOTHING
            "#,
        )
        .bind(&uid)
        .bind(owner_id)
        .bind(template.template_id)
        .bind(print_number)
        .bind(variant)
        .bind(&cosmetics)
        .execute(&mut **tx)
        .await?;
        if inserted.rows_affected() == 1 {
            return Ok(uid);
        }
    }
    Err(BotError::user_message("could not mint a unique card id"))
}
