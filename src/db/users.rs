use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};

use crate::engine::charges::{self, Pool};
use crate::error::BotError;
use crate::models::UserRow;

const SELECT_USER: &str = "SELECT user_id, gold, total_drops, drop_charges, grab_charges, drop_next_regen, grab_next_regen, last_daily_claim FROM users WHERE user_id = $1";
const LOCK_USER: &str = "SELECT user_id, gold, total_drops, drop_charges, grab_charges, drop_next_regen, grab_next_regen, last_daily_claim FROM users WHERE user_id = $1 FOR UPDATE";

pub async fn ensure_user(pool: &PgPool, user_id: i64) -> Result<UserRow, BotError> {
    sqlx::query("INSERT INTO users (user_id) VALUES ($1) ON CONFLICT (user_id) DO NOTHING")
        .bind(user_id)
        .execute(pool)
        .await?;
    sqlx::query_as::<_, UserRow>(SELECT_USER)
        .bind(user_id)
        .fetch_one(pool)
        .await
        .map_err(BotError::from)
}

pub async fn lock_user(
    tx: &mut Transaction<'_, Postgres>,
    user_id: i64,
) -> Result<UserRow, BotError> {
    sqlx::query(
        "INSERT INTO users (user_id) VALUES ($1) ON CONFLICT (user_id) DO NOTHING",
    )
    .bind(user_id)
    .execute(&mut **tx)
    .await?;
    sqlx::query_as::<_, UserRow>(LOCK_USER)
    .bind(user_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(BotError::from)
}

pub fn view_charges(user: &UserRow, now: DateTime<Utc>, drop_max: i32, grab_max: i32, drop_every: std::time::Duration, grab_every: std::time::Duration) -> (Pool, Pool) {
    let drops = charges::reconcile(
        Pool {
            charges: user.drop_charges as i32,
            next_regen_at: user.drop_next_regen,
        },
        now,
        drop_max,
        drop_every,
    );
    let grabs = charges::reconcile(
        Pool {
            charges: user.grab_charges as i32,
            next_regen_at: user.grab_next_regen,
        },
        now,
        grab_max,
        grab_every,
    );
    (drops, grabs)
}

pub async fn save_charges(
    tx: &mut Transaction<'_, Postgres>,
    user_id: i64,
    drops: Pool,
    grabs: Pool,
    total_drops: i32,
) -> Result<(), BotError> {
    sqlx::query(
        r#"
        UPDATE users
        SET drop_charges = $2,
            grab_charges = $3,
            drop_next_regen = $4,
            grab_next_regen = $5,
            total_drops = $6,
            updated_at = now()
        WHERE user_id = $1
        "#,
    )
    .bind(user_id)
    .bind(drops.charges as i16)
    .bind(grabs.charges as i16)
    .bind(drops.next_regen_at)
    .bind(grabs.next_regen_at)
    .bind(total_drops)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub async fn add_gold(
    tx: &mut Transaction<'_, Postgres>,
    user_id: i64,
    amount: i64,
) -> Result<i64, BotError> {
    let gold = sqlx::query_scalar::<_, i64>(
        "UPDATE users SET gold = gold + $2, updated_at = now() WHERE user_id = $1 RETURNING gold",
    )
    .bind(user_id)
    .bind(amount)
    .fetch_one(&mut **tx)
    .await?;
    Ok(gold)
}
