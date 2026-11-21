use chrono::{DateTime, Datelike, Utc};
use sqlx::{PgPool, Postgres, Transaction};

use crate::db::{self, users};
use crate::error::BotError;
use crate::models::AuditEvent;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct QuestView {
    pub quest_id: String,
    pub period: String,
    pub title: String,
    pub target: i32,
    pub reward_gold: i32,
    pub reward_box: bool,
    pub progress: i32,
    pub claimed: bool,
}

#[derive(Debug, Clone)]
pub struct QuestReward {
    pub gold: i64,
    pub cosmetic: Option<CosmeticGrant>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct CosmeticGrant {
    pub cosmetic_id: String,
    pub kind: String,
    pub name: String,
    pub rarity: String,
}

pub fn period_key(period: &str, now: DateTime<Utc>) -> String {
    match period {
        "weekly" => {
            let iso = now.iso_week();
            format!("{:04}-W{:02}", iso.year(), iso.week())
        }
        "monthly" => now.format("%Y-%m").to_string(),
        _ => now.format("%Y-%m-%d").to_string(),
    }
}

pub async fn bump(
    tx: &mut Transaction<'_, Postgres>,
    user_id: i64,
    metric: &str,
    amount: i32,
    now: DateTime<Utc>,
) -> Result<(), BotError> {
    sqlx::query(
        r#"
        INSERT INTO quest_progress (user_id, quest_id, period_key, progress, claimed)
        SELECT $1, q.quest_id,
               CASE q.period
                   WHEN 'weekly' THEN $3
                   WHEN 'monthly' THEN $4
                   ELSE $5
               END,
               GREATEST($6, 0),
               false
        FROM quest_defs q
        WHERE q.metric = $2
        ON CONFLICT (user_id, quest_id, period_key)
        DO UPDATE SET progress = GREATEST(quest_progress.progress + $6, 0)
        "#,
    )
    .bind(user_id)
    .bind(metric)
    .bind(period_key("weekly", now))
    .bind(period_key("monthly", now))
    .bind(period_key("daily", now))
    .bind(amount)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub async fn board(pool: &PgPool, user_id: i64) -> Result<Vec<QuestView>, BotError> {
    let now = Utc::now();
    sqlx::query_as::<_, QuestView>(
        r#"
        SELECT q.quest_id, q.period, q.title, q.target, q.reward_gold, q.reward_box,
               COALESCE(p.progress, 0) AS progress,
               COALESCE(p.claimed, false) AS claimed
        FROM quest_defs q
        LEFT JOIN quest_progress p
          ON p.quest_id = q.quest_id
         AND p.user_id = $1
         AND p.period_key = CASE q.period
             WHEN 'weekly' THEN $2
             WHEN 'monthly' THEN $3
             ELSE $4
         END
        ORDER BY CASE q.period WHEN 'daily' THEN 0 WHEN 'weekly' THEN 1 ELSE 2 END, q.quest_id
        "#,
    )
    .bind(user_id)
    .bind(period_key("weekly", now))
    .bind(period_key("monthly", now))
    .bind(period_key("daily", now))
    .fetch_all(pool)
    .await
    .map_err(BotError::from)
}

pub async fn claim_daily(pool: &PgPool, user_id: i64, bonus: i64) -> Result<i64, BotError> {
    let mut tx = db::begin(pool).await?;
    users::lock_user(&mut tx, user_id).await?;
    let gold: Option<i64> = sqlx::query_scalar(
        r#"
        UPDATE users
        SET gold = gold + $2,
            last_daily_claim = (now() AT TIME ZONE 'UTC')::date,
            updated_at = now()
        WHERE user_id = $1
          AND (last_daily_claim IS NULL OR last_daily_claim < (now() AT TIME ZONE 'UTC')::date)
        RETURNING gold
        "#,
    )
    .bind(user_id)
    .bind(bonus)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(gold) = gold else {
        return Err(BotError::user_message(
            "Daily Gold were already claimed. The next claim opens at 00:00 UTC.",
        ));
    };
    db::write_audit(
        &mut tx,
        &AuditEvent {
            tx_id: db::new_tx_id(),
            kind: "daily",
            sender_id: None,
            receiver_id: Some(user_id),
            cards: Vec::new(),
            gold_delta: bonus,
            metadata: serde_json::json!({ "source": "daily_claim" }),
        },
    )
    .await?;
    tx.commit().await?;
    Ok(gold)
}

pub async fn claim_quest(
    pool: &PgPool,
    user_id: i64,
    quest_id: &str,
) -> Result<QuestReward, BotError> {
    let now = Utc::now();
    let mut tx = db::begin(pool).await?;
    users::lock_user(&mut tx, user_id).await?;
    let def: Option<(String, i32, i32, bool)> = sqlx::query_as(
        "SELECT period, target, reward_gold, reward_box FROM quest_defs WHERE quest_id = $1",
    )
    .bind(quest_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((period, target, reward_gold, reward_box)) = def else {
        return Err(BotError::user_message("that quest does not exist"));
    };
    let key = period_key(&period, now);
    let progress: Option<(i32, bool)> = sqlx::query_as(
        r#"
        SELECT progress, claimed FROM quest_progress
        WHERE user_id = $1 AND quest_id = $2 AND period_key = $3
        FOR UPDATE
        "#,
    )
    .bind(user_id)
    .bind(quest_id)
    .bind(&key)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((progress, claimed)) = progress else {
        return Err(BotError::user_message(format!(
            "progress is 0/{target} for this period"
        )));
    };
    if claimed {
        return Err(BotError::user_message("that quest reward was already claimed"));
    }
    if progress < target {
        return Err(BotError::user_message(format!(
            "progress is {progress}/{target}"
        )));
    }
    sqlx::query(
        r#"
        UPDATE quest_progress SET claimed = true
        WHERE user_id = $1 AND quest_id = $2 AND period_key = $3
        "#,
    )
    .bind(user_id)
    .bind(quest_id)
    .bind(&key)
    .execute(&mut *tx)
    .await?;
    let gold = if reward_gold > 0 {
        users::add_gold(&mut tx, user_id, reward_gold as i64).await?
    } else {
        sqlx::query_scalar("SELECT gold FROM users WHERE user_id = $1")
            .bind(user_id)
            .fetch_one(&mut *tx)
            .await?
    };
    let cosmetic = if reward_box {
        Some(grant_box(&mut tx, user_id).await?)
    } else {
        None
    };
    db::write_audit(
        &mut tx,
        &AuditEvent {
            tx_id: db::new_tx_id(),
            kind: "quest",
            sender_id: None,
            receiver_id: Some(user_id),
            cards: Vec::new(),
            gold_delta: reward_gold as i64,
            metadata: serde_json::json!({
                "quest_id": quest_id,
                "cosmetic": cosmetic.as_ref().map(|c| &c.cosmetic_id)
            }),
        },
    )
    .await?;
    tx.commit().await?;
    Ok(QuestReward { gold, cosmetic })
}

async fn grant_box(
    tx: &mut Transaction<'_, Postgres>,
    user_id: i64,
) -> Result<CosmeticGrant, BotError> {
    for _ in 0..8 {
        let rolled: Option<CosmeticGrant> = sqlx::query_as(
            r#"
            SELECT cosmetic_id, kind, name, rarity
            FROM cosmetics
            ORDER BY -ln(GREATEST(random(), 1e-12)) / weight
            LIMIT 1
            "#,
        )
        .fetch_optional(&mut **tx)
        .await?;
        let Some(rolled) = rolled else {
            return Err(BotError::user_message("the cosmetics registry is empty"));
        };
        let inserted = sqlx::query(
            r#"
            INSERT INTO user_cosmetics (user_id, cosmetic_id)
            VALUES ($1, $2)
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(user_id)
        .bind(&rolled.cosmetic_id)
        .execute(&mut **tx)
        .await?;
        if inserted.rows_affected() == 1 {
            return Ok(rolled);
        }
    }
    Err(BotError::user_message(
        "the box kept rolling cosmetics you already own",
    ))
}
