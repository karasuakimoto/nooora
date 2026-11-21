use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::error::{map_sql, BotError};
use crate::models::AuditEvent;

pub mod cards;
pub mod cup;
pub mod drops;
pub mod economy;
pub mod guild;
pub mod quests;
pub mod users;

pub async fn begin(pool: &PgPool) -> Result<Transaction<'static, Postgres>, BotError> {
    pool.begin().await.map_err(BotError::from)
}

pub async fn begin_serializable(
    pool: &PgPool,
) -> Result<Transaction<'static, Postgres>, BotError> {
    pool.begin_with("BEGIN ISOLATION LEVEL SERIALIZABLE")
        .await
        .map_err(BotError::from)
}

pub async fn assert_gate(
    tx: &mut Transaction<'_, Postgres>,
    user_id: i64,
    need: i32,
) -> Result<i32, BotError> {
    sqlx::query_scalar::<_, i32>("SELECT nooora_assert_trade_gate($1, $2)")
        .bind(user_id)
        .bind(need)
        .fetch_one(&mut **tx)
        .await
        .map_err(map_sql)
}

pub async fn write_audit(
    tx: &mut Transaction<'_, Postgres>,
    event: &AuditEvent,
) -> Result<(), BotError> {
    sqlx::query(
        r#"
        INSERT INTO economy_audit_log
            (tx_id, kind, sender_id, receiver_id, cards_json, gold_delta, metadata)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
    )
    .bind(event.tx_id)
    .bind(event.kind)
    .bind(event.sender_id)
    .bind(event.receiver_id)
    .bind(sqlx::types::Json(&event.cards))
    .bind(event.gold_delta)
    .bind(sqlx::types::Json(&event.metadata))
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub fn new_tx_id() -> Uuid {
    Uuid::new_v4()
}

pub fn utc_now() -> DateTime<Utc> {
    Utc::now()
}
