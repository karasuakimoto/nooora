use sqlx::PgPool;

use crate::db::{self, quests, users};
use crate::error::BotError;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct CupRow {
    pub user_id: i64,
    pub wins: i32,
    pub losses: i32,
    pub rating: i32,
}

#[derive(Debug, Clone)]
pub struct MatchResult {
    pub winner: i64,
    pub loser: i64,
    pub winner_power: i32,
    pub loser_power: i32,
    pub winner_rating: i32,
    pub loser_rating: i32,
}

pub async fn leaderboard(pool: &PgPool) -> Result<Vec<CupRow>, BotError> {
    sqlx::query_as::<_, CupRow>(
        "SELECT user_id, wins, losses, rating FROM cup_stats ORDER BY rating DESC, wins DESC LIMIT 10",
    )
    .fetch_all(pool)
    .await
    .map_err(BotError::from)
}

pub async fn stats(pool: &PgPool, user_id: i64) -> Result<CupRow, BotError> {
    sqlx::query("INSERT INTO users (user_id) VALUES ($1) ON CONFLICT DO NOTHING")
        .bind(user_id)
        .execute(pool)
        .await?;
    sqlx::query(
        "INSERT INTO cup_stats (user_id) VALUES ($1) ON CONFLICT DO NOTHING",
    )
    .bind(user_id)
    .execute(pool)
    .await?;
    sqlx::query_as::<_, CupRow>("SELECT user_id, wins, losses, rating FROM cup_stats WHERE user_id = $1")
        .bind(user_id)
        .fetch_one(pool)
        .await
        .map_err(BotError::from)
}

pub async fn play_match(pool: &PgPool, a: i64, b: i64) -> Result<MatchResult, BotError> {
    if a == b {
        return Err(BotError::user_message("you cannot match yourself"));
    }
    let mut tx = db::begin(pool).await?;
    let (first, second) = if a < b { (a, b) } else { (b, a) };
    users::lock_user(&mut tx, first).await?;
    users::lock_user(&mut tx, second).await?;
    for id in [a, b] {
        sqlx::query("INSERT INTO cup_stats (user_id) VALUES ($1) ON CONFLICT DO NOTHING")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    let power_a = power(&mut tx, a).await?;
    let power_b = power(&mut tx, b).await?;
    let roll_a = power_a + rand_bonus();
    let roll_b = power_b + rand_bonus();
    let (winner, loser, winner_power, loser_power) = if roll_a == roll_b {
        if rand_bonus() % 2 == 0 {
            (a, b, power_a, power_b)
        } else {
            (b, a, power_b, power_a)
        }
    } else if roll_a > roll_b {
        (a, b, power_a, power_b)
    } else {
        (b, a, power_b, power_a)
    };
    let winner_rating: i32 = sqlx::query_scalar(
        r#"
        UPDATE cup_stats
        SET wins = wins + 1, rating = rating + 15
        WHERE user_id = $1
        RETURNING rating
        "#,
    )
    .bind(winner)
    .fetch_one(&mut *tx)
    .await?;
    let loser_rating: i32 = sqlx::query_scalar(
        r#"
        UPDATE cup_stats
        SET losses = losses + 1, rating = GREATEST(rating - 10, 0)
        WHERE user_id = $1
        RETURNING rating
        "#,
    )
    .bind(loser)
    .fetch_one(&mut *tx)
    .await?;
    quests::bump(&mut tx, winner, "cup_wins", 1, chrono::Utc::now()).await?;
    tx.commit().await?;
    Ok(MatchResult {
        winner,
        loser,
        winner_power,
        loser_power,
        winner_rating,
        loser_rating,
    })
}

async fn power(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, user_id: i64) -> Result<i32, BotError> {
    let score: i32 = sqlx::query_scalar(
        r#"
        SELECT COALESCE(SUM(score), 0)::int FROM (
            SELECT CASE t.rarity_tier
                WHEN 'standard' THEN 10
                WHEN 'rare' THEN 25
                WHEN 'shiny' THEN 60
                WHEN '3d' THEN 120
                WHEN 'event' THEN 200
                ELSE 10
            END AS score
            FROM card_instances i
            JOIN card_templates t ON t.template_id = i.template_id
            WHERE i.owner_id = $1
            ORDER BY score DESC
            LIMIT 3
        ) AS top_cards
        "#,
    )
    .bind(user_id)
    .fetch_one(&mut **tx)
    .await?;
    Ok(score)
}

fn rand_bonus() -> i32 {
    rand::Rng::random_range(&mut rand::rng(), 0..21)
}
