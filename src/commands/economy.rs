use poise::serenity_prelude::ButtonStyle;

use crate::db::economy::{self, ConfirmOutcome, ListingRow, TradeRow};
use crate::error::BotError;
use crate::ids::{parse_card_list, snowflake_i64};
use crate::state::{AppState, Context};
use crate::ui::{self, button, chunk_buttons, row};

pub async fn economy_check(ctx: Context<'_>) -> Result<bool, BotError> {
    let user_id = snowflake_i64(ctx.author().id.get())?;
    let user = crate::db::users::ensure_user(&ctx.data().db, user_id).await?;
    let need = ctx.data().config.trade_gate;
    if user.total_drops < need {
        return Err(BotError::TradeGate {
            have: user.total_drops,
            need,
        });
    }
    Ok(true)
}

/// Open a two-sided trade. Both players must have cleared the drop gate.
#[poise::command(prefix_command, slash_command, rename = "trade", check = "economy_check")]
pub async fn trade(
    ctx: Context<'_>,
    #[description = "The other player"] user: poise::serenity_prelude::User,
) -> Result<(), BotError> {
    let state = ctx.data();
    let a = snowflake_i64(ctx.author().id.get())?;
    let b = snowflake_i64(user.id.get())?;
    let channel = snowflake_i64(ctx.channel_id().get())?;
    let row = economy::create_trade(&state.db, a, b, channel, state.config.trade_gate).await?;
    let sent = ctx
        .send(
            poise::CreateReply::default()
                .content(format!("<@{}> <@{}>", row.user_a, row.user_b))
                .embed(trade_embed(&row))
                .components(trade_rows(&row)),
        )
        .await?;
    if let Ok(message) = sent.message().await {
        economy::set_message(&state.db, &row.trade_id, message.id.get() as i64).await?;
    }
    Ok(())
}

/// Gift card uids. Both players must have cleared the drop gate.
#[poise::command(prefix_command, slash_command, rename = "gift", aliases("g"), check = "economy_check")]
pub async fn gift(
    ctx: Context<'_>,
    #[description = "Who receives the cards"] user: poise::serenity_prelude::User,
    #[description = "Card uids, separated by spaces or commas"]
    #[rest]
    cards: String,
) -> Result<(), BotError> {
    let sender = snowflake_i64(ctx.author().id.get())?;
    let receiver = snowflake_i64(user.id.get())?;
    let uids = parse_card_list(&cards);
    let moved = economy::gift(
        &ctx.data().db,
        sender,
        receiver,
        &uids,
        ctx.data().config.trade_gate,
    )
    .await?;
    ctx.send(
        poise::CreateReply::default().embed(ui::embed(
            "Gift sent",
            format!("Moved {} card(s) to <@{}>: `{}`", moved.len(), receiver, moved.join("`, `")),
        )),
    )
    .await?;
    Ok(())
}

/// Browse Gold listings.
#[poise::command(prefix_command, slash_command, rename = "market", check = "economy_check")]
pub async fn market(ctx: Context<'_>) -> Result<(), BotError> {
    let owner = snowflake_i64(ctx.author().id.get())?;
    ctx.send(market_reply(ctx.data(), owner, 0).await?.into_reply()).await?;
    Ok(())
}

pub fn trade_embed(trade: &TradeRow) -> poise::serenity_prelude::CreateEmbed {
    ui::embed(
        format!("Trade {}", trade.trade_id),
        format!(
            "**A** <@{}>  lock {}  confirm {}\n{}\nGold: {}\n\n**B** <@{}>  lock {}  confirm {}\n{}\nGold: {}\n\nStatus: {}",
            trade.user_a,
            flag(trade.lock_a),
            flag(trade.confirm_a),
            list_cards(&trade.cards_a.0),
            trade.gold_a,
            trade.user_b,
            flag(trade.lock_b),
            flag(trade.confirm_b),
            list_cards(&trade.cards_b.0),
            trade.gold_b,
            trade.status
        ),
    )
}

pub fn trade_rows(trade: &TradeRow) -> Vec<poise::serenity_prelude::CreateActionRow> {
    let id = &trade.trade_id;
    let open = trade.status == "open";
    vec![row(vec![
        button(format!("trade:{id}:add"), "Add cards", ButtonStyle::Secondary).disabled(!open),
        button(format!("trade:{id}:remove"), "Remove", ButtonStyle::Secondary).disabled(!open),
        button(format!("trade:{id}:gold"), "Set Gold", ButtonStyle::Secondary).disabled(!open),
        button(format!("trade:{id}:lock"), "Lock", ButtonStyle::Primary).disabled(!open),
        button(format!("trade:{id}:confirm"), "Confirm", ButtonStyle::Success).disabled(!open),
    ]), row(vec![button(format!("trade:{id}:cancel"), "Cancel", ButtonStyle::Danger).disabled(!open)])]
}

pub async fn market_reply(state: &AppState, owner: i64, page: i64) -> Result<crate::ui::View, BotError> {
    let (rows, total) = economy::market_page(&state.db, page).await?;
    let pages = ((total.max(1) + 4) / 5).max(1);
    let body = if rows.is_empty() {
        "No active listings.".to_string()
    } else {
        rows.iter().map(format_listing).collect::<Vec<_>>().join("\n")
    };
    let mut buttons = vec![
        button(format!("hub:{owner}:market:{}", page.saturating_sub(1)), "Prev", ButtonStyle::Secondary)
            .disabled(page <= 0),
        button(format!("hub:{owner}:market:{}", page + 1), "Next", ButtonStyle::Secondary)
            .disabled(page + 1 >= pages),
        button(format!("mkt:{owner}:list"), "List a card", ButtonStyle::Primary),
        button(format!("hub:{owner}:home"), "Menu", ButtonStyle::Secondary),
    ];
    for listing in rows.iter().take(5) {
        buttons.push(button(
            format!("mkt:{owner}:buy:{}", listing.listing_id),
            ui::trunc(&format!("Buy #{} · {}", listing.listing_id, listing.price_gold), 80),
            ButtonStyle::Success,
        ));
    }
    Ok(ui::View::new(ui::embed(format!("Market {}/{}", page + 1, pages), body))
        .components(chunk_buttons(buttons)))
}

pub fn confirm_text(outcome: &ConfirmOutcome) -> String {
    match outcome {
        ConfirmOutcome::LockedIn => "Confirmation recorded. The other player still has to confirm.".into(),
        ConfirmOutcome::Completed => "Trade completed. Ownership and Gold moved in one transaction.".into(),
    }
}

fn format_listing(listing: &ListingRow) -> String {
    format!(
        "`#{}` {} · {} · **{}** Gold · seller <@{}>",
        listing.listing_id,
        listing.kind,
        listing.label.as_deref().unwrap_or(
            listing.card_uid.as_deref().or(listing.cosmetic_id.as_deref()).unwrap_or("item")
        ),
        listing.price_gold,
        listing.seller_id
    )
}

fn list_cards(cards: &[String]) -> String {
    if cards.is_empty() {
        "_no cards_".into()
    } else {
        cards.iter().map(|uid| format!("`{uid}`")).collect::<Vec<_>>().join(" ")
    }
}

fn flag(on: bool) -> &'static str {
    if on { "yes" } else { "no" }
}
