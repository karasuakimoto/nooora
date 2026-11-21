use sqlx::{PgPool, Postgres, Transaction};

use crate::db::{self, quests, users};
use crate::error::BotError;
use crate::ids::trade_id;
use crate::models::AuditEvent;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TradeRow {
    pub trade_id: String,
    pub user_a: i64,
    pub user_b: i64,
    pub cards_a: sqlx::types::Json<Vec<String>>,
    pub cards_b: sqlx::types::Json<Vec<String>>,
    pub gold_a: i64,
    pub gold_b: i64,
    pub lock_a: bool,
    pub lock_b: bool,
    pub confirm_a: bool,
    pub confirm_b: bool,
    pub status: String,
    #[allow(dead_code)]
    pub channel_id: i64,
    #[allow(dead_code)]
    pub message_id: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    A,
    B,
}

#[derive(Debug, Clone)]
pub enum ConfirmOutcome {
    LockedIn,
    Completed,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ListingRow {
    pub listing_id: i64,
    pub seller_id: i64,
    pub kind: String,
    pub card_uid: Option<String>,
    pub cosmetic_id: Option<String>,
    pub price_gold: i64,
    pub label: Option<String>,
}

pub async fn create_trade(
    pool: &PgPool,
    user_a: i64,
    user_b: i64,
    channel_id: i64,
    gate: i32,
) -> Result<TradeRow, BotError> {
    if user_a == user_b {
        return Err(BotError::user_message("you cannot trade with yourself"));
    }
    let mut tx = db::begin_serializable(pool).await?;
    gate_pair(&mut tx, user_a, user_b, gate).await?;
    let id = trade_id();
    sqlx::query(
        r#"
        INSERT INTO trades (trade_id, user_a, user_b, channel_id)
        VALUES ($1, $2, $3, $4)
        "#,
    )
    .bind(&id)
    .bind(user_a)
    .bind(user_b)
    .bind(channel_id)
    .execute(&mut *tx)
    .await?;
    let row = load_trade_tx(&mut tx, &id).await?;
    tx.commit().await?;
    Ok(row)
}

pub async fn set_message(pool: &PgPool, trade_id: &str, message_id: i64) -> Result<(), BotError> {
    sqlx::query("UPDATE trades SET message_id = $2 WHERE trade_id = $1")
        .bind(trade_id)
        .bind(message_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn load_trade(pool: &PgPool, trade_id: &str) -> Result<Option<TradeRow>, BotError> {
    sqlx::query_as::<_, TradeRow>(TRADE_SQL)
        .bind(trade_id)
        .fetch_optional(pool)
        .await
        .map_err(BotError::from)
}

pub async fn add_cards(
    pool: &PgPool,
    trade_id: &str,
    user_id: i64,
    card_uids: &[String],
) -> Result<TradeRow, BotError> {
    if card_uids.is_empty() {
        return Err(BotError::user_message("name at least one card uid"));
    }
    let mut tx = db::begin_serializable(pool).await?;
    let trade = lock_trade(&mut tx, trade_id).await?;
    ensure_open(&trade)?;
    let side = side_of(&trade, user_id)?;
    let mut cards = cards_of(&trade, side);
    for uid in card_uids {
        if !cards.iter().any(|existing| existing == uid) {
            cards.push(uid.clone());
        }
    }
    if cards.len() > 8 {
        return Err(BotError::user_message("a trade offer can hold at most 8 cards"));
    }
    for uid in card_uids {
        let held = sqlx::query(
            r#"
            UPDATE card_instances
            SET hold_kind = 'trade', hold_ref = $3
            WHERE card_uid = $1 AND owner_id = $2 AND hold_kind IS NULL
            "#,
        )
        .bind(uid)
        .bind(user_id)
        .bind(trade_id)
        .execute(&mut *tx)
        .await?;
        if held.rows_affected() == 0 && !already_held_by_trade(&mut tx, uid, user_id, trade_id).await?
        {
            return Err(BotError::user_message(format!(
                "`{uid}` is not yours, or it is already in a listing or another trade"
            )));
        }
    }
    save_offer(&mut tx, &trade, side, &cards, gold_of(&trade, side)).await?;
    let row = load_trade_tx(&mut tx, trade_id).await?;
    tx.commit().await?;
    Ok(row)
}

pub async fn remove_card(
    pool: &PgPool,
    trade_id: &str,
    user_id: i64,
    card_uid: &str,
) -> Result<TradeRow, BotError> {
    let mut tx = db::begin_serializable(pool).await?;
    let trade = lock_trade(&mut tx, trade_id).await?;
    ensure_open(&trade)?;
    let side = side_of(&trade, user_id)?;
    let mut cards = cards_of(&trade, side);
    let before = cards.len();
    cards.retain(|uid| uid != card_uid);
    if cards.len() == before {
        return Err(BotError::user_message("that card is not in your offer"));
    }
    release_hold(&mut tx, card_uid, trade_id).await?;
    save_offer(&mut tx, &trade, side, &cards, gold_of(&trade, side)).await?;
    let row = load_trade_tx(&mut tx, trade_id).await?;
    tx.commit().await?;
    Ok(row)
}

pub async fn set_gold(
    pool: &PgPool,
    trade_id: &str,
    user_id: i64,
    gold: i64,
) -> Result<TradeRow, BotError> {
    if gold < 0 {
        return Err(BotError::user_message("Gold offers cannot be negative"));
    }
    let mut tx = db::begin_serializable(pool).await?;
    let trade = lock_trade(&mut tx, trade_id).await?;
    ensure_open(&trade)?;
    let side = side_of(&trade, user_id)?;
    let user = users::lock_user(&mut tx, user_id).await?;
    if user.gold < gold {
        return Err(BotError::user_message(format!(
            "you have {} Gold, which is short of {gold}",
            user.gold
        )));
    }
    save_offer(&mut tx, &trade, side, &cards_of(&trade, side), gold).await?;
    let row = load_trade_tx(&mut tx, trade_id).await?;
    tx.commit().await?;
    Ok(row)
}

pub async fn lock_side(
    pool: &PgPool,
    trade_id: &str,
    user_id: i64,
) -> Result<TradeRow, BotError> {
    let mut tx = db::begin(pool).await?;
    let trade = lock_trade(&mut tx, trade_id).await?;
    ensure_open(&trade)?;
    let side = side_of(&trade, user_id)?;
    match side {
        Side::A => {
            sqlx::query(
                "UPDATE trades SET lock_a = true, confirm_a = false, updated_at = now() WHERE trade_id = $1",
            )
            .bind(trade_id)
            .execute(&mut *tx)
            .await?;
        }
        Side::B => {
            sqlx::query(
                "UPDATE trades SET lock_b = true, confirm_b = false, updated_at = now() WHERE trade_id = $1",
            )
            .bind(trade_id)
            .execute(&mut *tx)
            .await?;
        }
    }
    let row = load_trade_tx(&mut tx, trade_id).await?;
    tx.commit().await?;
    Ok(row)
}

pub async fn confirm(
    pool: &PgPool,
    trade_id: &str,
    user_id: i64,
    gate: i32,
) -> Result<ConfirmOutcome, BotError> {
    let mut tx = db::begin_serializable(pool).await?;
    let trade = lock_trade(&mut tx, trade_id).await?;
    ensure_open(&trade)?;
    let side = side_of(&trade, user_id)?;
    if !trade.lock_a || !trade.lock_b {
        return Err(BotError::user_message(
            "both players have to lock their offers before either can confirm",
        ));
    }
    let (confirm_a, confirm_b) = match side {
        Side::A => (true, trade.confirm_b),
        Side::B => (trade.confirm_a, true),
    };
    if confirm_a && confirm_b {
        execute_trade(&mut tx, &trade, gate).await?;
        sqlx::query(
            "UPDATE trades SET confirm_a = true, confirm_b = true, status = 'completed', updated_at = now() WHERE trade_id = $1",
        )
        .bind(trade_id)
        .execute(&mut *tx)
        .await?;
        let row = load_trade_tx(&mut tx, trade_id).await?;
        tx.commit().await?;
        let _ = row;
        return Ok(ConfirmOutcome::Completed);
    }
    match side {
        Side::A => {
            sqlx::query("UPDATE trades SET confirm_a = true, updated_at = now() WHERE trade_id = $1")
                .bind(trade_id)
                .execute(&mut *tx)
                .await?;
        }
        Side::B => {
            sqlx::query("UPDATE trades SET confirm_b = true, updated_at = now() WHERE trade_id = $1")
                .bind(trade_id)
                .execute(&mut *tx)
                .await?;
        }
    }
    tx.commit().await?;
    Ok(ConfirmOutcome::LockedIn)
}

pub async fn cancel(pool: &PgPool, trade_id: &str, user_id: i64) -> Result<TradeRow, BotError> {
    let mut tx = db::begin(pool).await?;
    let trade = lock_trade(&mut tx, trade_id).await?;
    ensure_open(&trade)?;
    side_of(&trade, user_id)?;
    release_all(&mut tx, &trade).await?;
    sqlx::query("UPDATE trades SET status = 'cancelled', updated_at = now() WHERE trade_id = $1")
        .bind(trade_id)
        .execute(&mut *tx)
        .await?;
    let row = load_trade_tx(&mut tx, trade_id).await?;
    tx.commit().await?;
    Ok(row)
}

pub async fn gift(
    pool: &PgPool,
    sender: i64,
    receiver: i64,
    card_uids: &[String],
    gate: i32,
) -> Result<Vec<String>, BotError> {
    if sender == receiver {
        return Err(BotError::user_message("you cannot gift cards to yourself"));
    }
    let mut uids = card_uids.to_vec();
    uids.sort();
    uids.dedup();
    if uids.is_empty() {
        return Err(BotError::user_message("provide at least one card uid"));
    }
    if uids.len() > 15 {
        return Err(BotError::user_message("you can gift at most 15 cards at once"));
    }
    let mut tx = db::begin_serializable(pool).await?;
    gate_pair(&mut tx, sender, receiver, gate).await?;
    let rows: Vec<(String, i64, Option<String>)> = sqlx::query_as(
        r#"
        SELECT card_uid, owner_id, hold_kind
        FROM card_instances
        WHERE card_uid = ANY($1)
        ORDER BY card_uid
        FOR UPDATE
        "#,
    )
    .bind(&uids)
    .fetch_all(&mut *tx)
    .await?;
    if rows.len() != uids.len() {
        return Err(BotError::user_message("one or more card uids do not exist"));
    }
    for (uid, owner, hold) in &rows {
        if *owner != sender {
            return Err(BotError::user_message(format!("`{uid}` is not yours")));
        }
        if hold.is_some() {
            return Err(BotError::user_message(format!(
                "`{uid}` is locked in a trade or market listing"
            )));
        }
    }
    sqlx::query("UPDATE card_instances SET owner_id = $1 WHERE card_uid = ANY($2)")
        .bind(receiver)
        .bind(&uids)
        .execute(&mut *tx)
        .await?;
    db::write_audit(
        &mut tx,
        &AuditEvent {
            tx_id: db::new_tx_id(),
            kind: "gift",
            sender_id: Some(sender),
            receiver_id: Some(receiver),
            cards: uids.clone(),
            gold_delta: 0,
            metadata: serde_json::json!({}),
        },
    )
    .await?;
    tx.commit().await?;
    Ok(uids)
}

pub async fn market_page(pool: &PgPool, page: i64) -> Result<(Vec<ListingRow>, i64), BotError> {
    let page = page.max(0);
    let total: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM market_listings WHERE status = 'active'")
            .fetch_one(pool)
            .await?;
    let rows = sqlx::query_as::<_, ListingRow>(
        r#"
        SELECT l.listing_id, l.seller_id, l.kind, l.card_uid, l.cosmetic_id, l.price_gold,
               COALESCE(t.character_name, c.name) AS label
        FROM market_listings l
        LEFT JOIN card_instances i ON i.card_uid = l.card_uid
        LEFT JOIN card_templates t ON t.template_id = i.template_id
        LEFT JOIN cosmetics c ON c.cosmetic_id = l.cosmetic_id
        WHERE l.status = 'active'
        ORDER BY l.created_at DESC
        LIMIT 5 OFFSET $1
        "#,
    )
    .bind(page * 5)
    .fetch_all(pool)
    .await?;
    Ok((rows, total))
}

pub async fn list_card(
    pool: &PgPool,
    seller: i64,
    card_uid: &str,
    price: i64,
    gate: i32,
) -> Result<i64, BotError> {
    if price <= 0 {
        return Err(BotError::user_message("the price has to be at least 1 Gold"));
    }
    let mut tx = db::begin_serializable(pool).await?;
    db::assert_gate(&mut tx, seller, gate).await?;
    let listing_id: i64 = sqlx::query_scalar(
        r#"
        INSERT INTO market_listings (seller_id, kind, card_uid, price_gold)
        VALUES ($1, 'card', $2, $3)
        RETURNING listing_id
        "#,
    )
    .bind(seller)
    .bind(card_uid)
    .bind(price)
    .fetch_one(&mut *tx)
    .await?;
    let held = sqlx::query(
        r#"
        UPDATE card_instances
        SET hold_kind = 'market', hold_ref = $3
        WHERE card_uid = $1 AND owner_id = $2 AND hold_kind IS NULL
        "#,
    )
    .bind(card_uid)
    .bind(seller)
    .bind(listing_id.to_string())
    .execute(&mut *tx)
    .await?;
    if held.rows_affected() == 0 {
        return Err(BotError::user_message(
            "that card is not yours, or it is already listed or in a trade",
        ));
    }
    db::write_audit(
        &mut tx,
        &AuditEvent {
            tx_id: db::new_tx_id(),
            kind: "list",
            sender_id: Some(seller),
            receiver_id: None,
            cards: vec![card_uid.to_string()],
            gold_delta: price,
            metadata: serde_json::json!({ "listing_id": listing_id }),
        },
    )
    .await?;
    tx.commit().await?;
    Ok(listing_id)
}

pub async fn buy_listing(
    pool: &PgPool,
    buyer: i64,
    listing_id: i64,
    gate: i32,
) -> Result<ListingRow, BotError> {
    let mut tx = db::begin_serializable(pool).await?;
    let listing: Option<(i64, String, Option<String>, Option<String>, i64, String)> = sqlx::query_as(
        r#"
        SELECT seller_id, kind, card_uid, cosmetic_id, price_gold, status
        FROM market_listings
        WHERE listing_id = $1
        FOR UPDATE
        "#,
    )
    .bind(listing_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((seller, kind, card_uid, cosmetic_id, price, status)) = listing else {
        return Err(BotError::user_message("that listing does not exist"));
    };
    if status != "active" {
        return Err(BotError::user_message("that listing is no longer active"));
    }
    if seller == buyer {
        return Err(BotError::user_message("you cannot buy your own listing"));
    }
    gate_pair(&mut tx, buyer, seller, gate).await?;
    pay(&mut tx, buyer, seller, price).await?;
    let mut cards = Vec::new();
    if kind == "card" {
        let uid = card_uid.clone().unwrap_or_default();
        let moved = sqlx::query(
            r#"
            UPDATE card_instances
            SET owner_id = $1, hold_kind = NULL, hold_ref = NULL
            WHERE card_uid = $2 AND owner_id = $3 AND hold_kind = 'market' AND hold_ref = $4
            "#,
        )
        .bind(buyer)
        .bind(&uid)
        .bind(seller)
        .bind(listing_id.to_string())
        .execute(&mut *tx)
        .await?;
        if moved.rows_affected() != 1 {
            return Err(BotError::user_message("the listed card is no longer available"));
        }
        cards.push(uid);
    } else {
        let cosmetic = cosmetic_id.clone().unwrap_or_default();
        let already: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM user_cosmetics WHERE user_id = $1 AND cosmetic_id = $2)",
        )
        .bind(buyer)
        .bind(&cosmetic)
        .fetch_one(&mut *tx)
        .await?;
        if already {
            return Err(BotError::user_message("you already own that cosmetic"));
        }
        let removed = sqlx::query(
            "DELETE FROM user_cosmetics WHERE user_id = $1 AND cosmetic_id = $2",
        )
        .bind(seller)
        .bind(&cosmetic)
        .execute(&mut *tx)
        .await?;
        if removed.rows_affected() != 1 {
            return Err(BotError::user_message("the seller no longer has that cosmetic"));
        }
        sqlx::query("INSERT INTO user_cosmetics (user_id, cosmetic_id) VALUES ($1, $2)")
            .bind(buyer)
            .bind(&cosmetic)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query(
        r#"
        UPDATE market_listings
        SET status = 'sold', buyer_id = $2, sold_at = now()
        WHERE listing_id = $1
        "#,
    )
    .bind(listing_id)
    .bind(buyer)
    .execute(&mut *tx)
    .await?;
    db::write_audit(
        &mut tx,
        &AuditEvent {
            tx_id: db::new_tx_id(),
            kind: "market_sale",
            sender_id: Some(seller),
            receiver_id: Some(buyer),
            cards,
            gold_delta: price,
            metadata: serde_json::json!({
                "listing_id": listing_id,
                "kind": kind,
                "cosmetic_id": cosmetic_id
            }),
        },
    )
    .await?;
    tx.commit().await?;
    Ok(ListingRow {
        listing_id,
        seller_id: seller,
        kind,
        card_uid,
        cosmetic_id,
        price_gold: price,
        label: None,
    })
}

async fn execute_trade(
    tx: &mut Transaction<'_, Postgres>,
    trade: &TradeRow,
    gate: i32,
) -> Result<(), BotError> {
    gate_pair(tx, trade.user_a, trade.user_b, gate).await?;
    let mut all = cards_of(trade, Side::A);
    all.extend(cards_of(trade, Side::B));
    all.sort();
    if all.len() != all.iter().collect::<std::collections::HashSet<_>>().len() {
        return Err(BotError::user_message("the same card is on both sides of the trade"));
    }
    if !all.is_empty() {
        let rows: Vec<(String, i64, Option<String>, Option<String>)> = sqlx::query_as(
            r#"
            SELECT card_uid, owner_id, hold_kind, hold_ref
            FROM card_instances
            WHERE card_uid = ANY($1)
            ORDER BY card_uid
            FOR UPDATE
            "#,
        )
        .bind(&all)
        .fetch_all(&mut **tx)
        .await?;
        if rows.len() != all.len() {
            return Err(BotError::user_message("a card in this trade no longer exists"));
        }
        for (uid, owner, hold_kind, hold_ref) in &rows {
            let expected = if cards_of(trade, Side::A).iter().any(|c| c == uid) {
                trade.user_a
            } else {
                trade.user_b
            };
            if *owner != expected {
                return Err(BotError::user_message(format!("`{uid}` changed owners")));
            }
            let held_here = hold_kind.as_deref() == Some("trade") && hold_ref.as_deref() == Some(trade.trade_id.as_str());
            if !held_here {
                return Err(BotError::user_message(format!("`{uid}` is not locked to this trade")));
            }
        }
    }
    // Settle against each player's original balance. A sequential pay would let
    // one side's incoming Gold fund the other side's outgoing offer.
    settle_gold(tx, trade.user_a, trade.gold_a, trade.gold_b).await?;
    settle_gold(tx, trade.user_b, trade.gold_b, trade.gold_a).await?;
    move_cards(tx, &cards_of(trade, Side::A), trade.user_b).await?;
    move_cards(tx, &cards_of(trade, Side::B), trade.user_a).await?;
    let now = chrono::Utc::now();
    quests::bump(tx, trade.user_a, "trades", 1, now).await?;
    quests::bump(tx, trade.user_b, "trades", 1, now).await?;
    db::write_audit(
        tx,
        &AuditEvent {
            tx_id: db::new_tx_id(),
            kind: "trade",
            sender_id: Some(trade.user_a),
            receiver_id: Some(trade.user_b),
            cards: all,
            gold_delta: trade.gold_a - trade.gold_b,
            metadata: serde_json::json!({
                "cards_a": trade.cards_a.0,
                "cards_b": trade.cards_b.0,
                "gold_a": trade.gold_a,
                "gold_b": trade.gold_b
            }),
        },
    )
    .await?;
    Ok(())
}

async fn settle_gold(
    tx: &mut Transaction<'_, Postgres>,
    user_id: i64,
    pay: i64,
    receive: i64,
) -> Result<(), BotError> {
    if pay == 0 && receive == 0 {
        return Ok(());
    }
    let updated = sqlx::query(
        r#"
        UPDATE users
        SET gold = gold - $2 + $3, updated_at = now()
        WHERE user_id = $1 AND gold >= $2
        "#,
    )
    .bind(user_id)
    .bind(pay)
    .bind(receive)
    .execute(&mut **tx)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(BotError::user_message(
            "a player no longer has enough Gold for this offer",
        ));
    }
    Ok(())
}

async fn pay(
    tx: &mut Transaction<'_, Postgres>,
    payer: i64,
    payee: i64,
    amount: i64,
) -> Result<(), BotError> {
    if amount == 0 || payer == payee {
        return Ok(());
    }
    let paid = sqlx::query(
        "UPDATE users SET gold = gold - $2, updated_at = now() WHERE user_id = $1 AND gold >= $2",
    )
    .bind(payer)
    .bind(amount)
    .execute(&mut **tx)
    .await?;
    if paid.rows_affected() != 1 {
        return Err(BotError::user_message(
            "a player no longer has enough Gold for this offer",
        ));
    }
    sqlx::query("UPDATE users SET gold = gold + $2, updated_at = now() WHERE user_id = $1")
        .bind(payee)
        .bind(amount)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn move_cards(
    tx: &mut Transaction<'_, Postgres>,
    cards: &[String],
    new_owner: i64,
) -> Result<(), BotError> {
    if cards.is_empty() {
        return Ok(());
    }
    sqlx::query(
        r#"
        UPDATE card_instances
        SET owner_id = $1, hold_kind = NULL, hold_ref = NULL
        WHERE card_uid = ANY($2)
        "#,
    )
    .bind(new_owner)
    .bind(cards)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn gate_pair(
    tx: &mut Transaction<'_, Postgres>,
    a: i64,
    b: i64,
    gate: i32,
) -> Result<(), BotError> {
    let (first, second) = if a < b { (a, b) } else { (b, a) };
    db::assert_gate(tx, first, gate).await?;
    db::assert_gate(tx, second, gate).await?;
    Ok(())
}

async fn lock_trade(
    tx: &mut Transaction<'_, Postgres>,
    trade_id: &str,
) -> Result<TradeRow, BotError> {
    sqlx::query_as::<_, TradeRow>(TRADE_SQL_LOCK)
        .bind(trade_id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| BotError::user_message("that trade no longer exists"))
}

async fn load_trade_tx(
    tx: &mut Transaction<'_, Postgres>,
    trade_id: &str,
) -> Result<TradeRow, BotError> {
    sqlx::query_as::<_, TradeRow>(TRADE_SQL)
        .bind(trade_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(BotError::from)
}

fn ensure_open(trade: &TradeRow) -> Result<(), BotError> {
    if trade.status != "open" {
        Err(BotError::user_message("that trade is already closed"))
    } else {
        Ok(())
    }
}

fn side_of(trade: &TradeRow, user_id: i64) -> Result<Side, BotError> {
    if trade.user_a == user_id {
        Ok(Side::A)
    } else if trade.user_b == user_id {
        Ok(Side::B)
    } else {
        Err(BotError::NotYourMenu)
    }
}

fn cards_of(trade: &TradeRow, side: Side) -> Vec<String> {
    match side {
        Side::A => trade.cards_a.0.clone(),
        Side::B => trade.cards_b.0.clone(),
    }
}

fn gold_of(trade: &TradeRow, side: Side) -> i64 {
    match side {
        Side::A => trade.gold_a,
        Side::B => trade.gold_b,
    }
}

async fn save_offer(
    tx: &mut Transaction<'_, Postgres>,
    trade: &TradeRow,
    side: Side,
    cards: &[String],
    gold: i64,
) -> Result<(), BotError> {
    let sql = match side {
        Side::A => {
            "UPDATE trades SET cards_a = $2, gold_a = $3, lock_a = false, lock_b = false, confirm_a = false, confirm_b = false, updated_at = now() WHERE trade_id = $1"
        }
        Side::B => {
            "UPDATE trades SET cards_b = $2, gold_b = $3, lock_a = false, lock_b = false, confirm_a = false, confirm_b = false, updated_at = now() WHERE trade_id = $1"
        }
    };
    sqlx::query(sql)
        .bind(&trade.trade_id)
        .bind(sqlx::types::Json(cards))
        .bind(gold)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn already_held_by_trade(
    tx: &mut Transaction<'_, Postgres>,
    uid: &str,
    user_id: i64,
    trade_id: &str,
) -> Result<bool, BotError> {
    sqlx::query_scalar(
        r#"
        SELECT EXISTS(
            SELECT 1 FROM card_instances
            WHERE card_uid = $1 AND owner_id = $2 AND hold_kind = 'trade' AND hold_ref = $3
        )
        "#,
    )
    .bind(uid)
    .bind(user_id)
    .bind(trade_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(BotError::from)
}

async fn release_hold(
    tx: &mut Transaction<'_, Postgres>,
    card_uid: &str,
    trade_id: &str,
) -> Result<(), BotError> {
    sqlx::query(
        r#"
        UPDATE card_instances
        SET hold_kind = NULL, hold_ref = NULL
        WHERE card_uid = $1 AND hold_kind = 'trade' AND hold_ref = $2
        "#,
    )
    .bind(card_uid)
    .bind(trade_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn release_all(tx: &mut Transaction<'_, Postgres>, trade: &TradeRow) -> Result<(), BotError> {
    let mut cards = cards_of(trade, Side::A);
    cards.extend(cards_of(trade, Side::B));
    if cards.is_empty() {
        return Ok(());
    }
    sqlx::query(
        r#"
        UPDATE card_instances
        SET hold_kind = NULL, hold_ref = NULL
        WHERE card_uid = ANY($1) AND hold_kind = 'trade' AND hold_ref = $2
        "#,
    )
    .bind(&cards)
    .bind(&trade.trade_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

const TRADE_SQL: &str = r#"
    SELECT trade_id, user_a, user_b, cards_a, cards_b, gold_a, gold_b,
           lock_a, lock_b, confirm_a, confirm_b, status, channel_id, message_id
    FROM trades
    WHERE trade_id = $1
"#;

const TRADE_SQL_LOCK: &str = r#"
    SELECT trade_id, user_a, user_b, cards_a, cards_b, gold_a, gold_b,
           lock_a, lock_b, confirm_a, confirm_b, status, channel_id, message_id
    FROM trades
    WHERE trade_id = $1
    FOR UPDATE
"#;
