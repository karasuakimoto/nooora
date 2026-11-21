use poise::serenity_prelude::{
    ButtonStyle, ComponentInteraction, ComponentInteractionDataKind, CreateActionRow,
    CreateInputText, CreateInteractionResponse, CreateInteractionResponseFollowup,
    CreateInteractionResponseMessage, CreateModal, EditInteractionResponse, InputTextStyle,
    Interaction, ModalInteraction,
};
use poise::FrameworkContext;

use crate::commands::{admin, collection, drop, economy, hub, quests};
use crate::db::{self, cards, cup, drops, economy as econ, guild};
use crate::error::BotError;
use crate::ids::{parse_card_list, snowflake_i64};
use crate::state::{AppState, CupTicket};
use crate::ui::{self, button};

pub async fn on_interaction(
    ctx: FrameworkContext<'_, AppState, BotError>,
    interaction: &Interaction,
) -> Result<(), BotError> {
    match interaction {
        Interaction::Component(component) => on_component(ctx, component).await,
        Interaction::Modal(modal) => on_modal(ctx, modal).await,
        _ => Ok(()),
    }
}

async fn on_component(
    ctx: FrameworkContext<'_, AppState, BotError>,
    interaction: &ComponentInteraction,
) -> Result<(), BotError> {
    let state = ctx.user_data;
    let user_id = interaction.user.id.get();
    if let Err(err) = state.hit_rate_limit(user_id) {
        return ephemeral(ctx, interaction, &err).await;
    }
    let id = interaction.data.custom_id.clone();
    let parts: Vec<&str> = id.split(':').collect();
    let result = match parts.as_slice() {
        ["grab", drop_id, slot] => grab(ctx, interaction, drop_id, slot).await,
        ["hub", owner, action] => hub_action(ctx, interaction, owner, action, "0").await,
        ["hub", owner, action, arg] => hub_action(ctx, interaction, owner, action, arg).await,
        ["invsel", owner, _] => inspect_selected(ctx, interaction, owner).await,
        ["trade", trade_id, action] => trade_action(ctx, interaction, trade_id, action).await,
        ["quest", owner, action] => quest_action(ctx, interaction, owner, action, None).await,
        ["quest", owner, "claim", quest_id] => {
            quest_action(ctx, interaction, owner, "claim", Some(quest_id)).await
        }
        ["mkt", owner, action] => market_action(ctx, interaction, owner, action, None).await,
        ["mkt", owner, action, arg] => market_action(ctx, interaction, owner, action, Some(arg)).await,
        ["admin", owner, action] => admin_action(ctx, interaction, owner, action).await,
        ["cup", owner, "join"] => cup_join(ctx, interaction, owner).await,
        ["event", owner, "claim"] => event_claim(ctx, interaction, owner).await,
        ["album", owner, "add"] => {
            owned(interaction, owner)?;
            modal(
                ctx,
                interaction,
                &format!("m:albumput:{owner}"),
                "Add to album",
                "Album id and card uid",
                "12 Ab12Cd34",
                "pair",
            )
            .await
        }
        ["album", owner, "create"] => {
            owned(interaction, owner)?;
            modal(
                ctx,
                interaction,
                &format!("m:album:{owner}"),
                "New album",
                "Name",
                "Album name",
                "name",
            )
            .await
        }
        ["cos", owner, card_uid, field] => {
            owned(interaction, owner)?;
            let (label, placeholder) = match *field {
                "style" => ("Color and outline", "#ffcc00 black"),
                _ => ("Cosmetic id", "frame_1 or - to clear"),
            };
            modal(
                ctx,
                interaction,
                &format!("m:cos:{owner}:{card_uid}:{field}"),
                "Customize card",
                label,
                placeholder,
                "value",
            )
            .await
        }
        _ => Ok(()),
    };
    if let Err(err) = result {
        ephemeral(ctx, interaction, &err).await?;
    }
    Ok(())
}

async fn on_modal(
    ctx: FrameworkContext<'_, AppState, BotError>,
    modal: &ModalInteraction,
) -> Result<(), BotError> {
    let state = ctx.user_data;
    let user_id = modal.user.id.get();
    if let Err(err) = state.hit_rate_limit(user_id) {
        return modal_ephemeral(ctx, modal, &err).await;
    }
    let value = first_input(modal);
    let parts: Vec<&str> = modal.data.custom_id.split(':').collect();
    let result = match parts.as_slice() {
        ["m", "search", owner] => {
            owned_id(modal.user.id.get(), owner)?;
            let hits = cards::search(&state.db, &value).await?;
            let body = if hits.is_empty() {
                format!("Nothing matched `{value}`.")
            } else {
                hits.iter()
                    .map(|hit| {
                        format!(
                            "**{}** · {} `{}` · {}",
                            hit.character_name, hit.series_name, hit.series_id, hit.rarity_tier
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            modal_embed(ctx, modal, "Search", &body).await
        }
        ["m", "list", owner] => {
            let owner_id = owned_id(modal.user.id.get(), owner)?;
            let mut fields = value.split_whitespace();
            let uid = fields.next().unwrap_or("").to_string();
            let price: i64 = fields
                .next()
                .unwrap_or("0")
                .parse()
                .map_err(|_| BotError::user_message("price must be a whole number of Gold"))?;
            let listing = econ::list_card(&state.db, owner_id, &uid, price, state.config.trade_gate).await?;
            modal_embed(ctx, modal, "Listed", &format!("Listing `{listing}` is up for {price} Gold.")).await
        }
        ["m", "tadd", trade_id, user] => {
            let user_id = owned_id(modal.user.id.get(), user)?;
            let row = econ::add_cards(&state.db, trade_id, user_id, &parse_card_list(&value)).await?;
            refresh_trade(ctx, modal, &row).await
        }
        ["m", "trem", trade_id, user] => {
            let user_id = owned_id(modal.user.id.get(), user)?;
            let uid = value.split_whitespace().next().unwrap_or("").trim();
            let row = econ::remove_card(&state.db, trade_id, user_id, uid).await?;
            refresh_trade(ctx, modal, &row).await
        }
        ["m", "tgold", trade_id, user] => {
            let user_id = owned_id(modal.user.id.get(), user)?;
            let gold: i64 = value
                .trim()
                .parse()
                .map_err(|_| BotError::user_message("enter a whole number of Gold"))?;
            let row = econ::set_gold(&state.db, trade_id, user_id, gold).await?;
            refresh_trade(ctx, modal, &row).await
        }
        ["m", "cos", owner, card_uid, field] => {
            let owner_id = owned_id(modal.user.id.get(), owner)?;
            collection::apply_field(state, owner_id, card_uid, field, &value).await?;
            let reply = collection::inspect_reply(state, owner_id, card_uid).await?;
            modal
                .create_response(
                    ctx.serenity_context,
                    CreateInteractionResponse::UpdateMessage(reply.into_update()),
                )
                .await?;
            Ok(())
        }
        ["m", "series", guild, user] => {
            owned_id(modal.user.id.get(), user)?;
            if !can_manage_modal(modal) {
                return modal_ephemeral(ctx, modal, &BotError::user_message("you need Manage Server")).await;
            }
            let guild_id: i64 = guild.parse().unwrap_or(0);
            let saved = guild::set_series(
                &state.db,
                guild_id,
                &value,
                chrono::Duration::seconds(state.config.series_change_cooldown.as_secs() as i64),
            )
            .await?;
            modal_embed(
                ctx,
                modal,
                "Series linked",
                &format!(
                    "`{}` {}",
                    saved.linked_series_id.as_deref().unwrap_or("-"),
                    saved.series_name.as_deref().unwrap_or("")
                ),
            )
            .await
        }
        ["m", "albumput", owner] => {
            let owner_id = owned_id(modal.user.id.get(), owner)?;
            let mut parts = value.split_whitespace();
            let album_id: i64 = parts
                .next()
                .unwrap_or("0")
                .parse()
                .map_err(|_| BotError::user_message("the album id has to be a number"))?;
            let card_uid = parts.next().unwrap_or("").trim();
            cards::add_to_album(&state.db, owner_id, album_id, card_uid).await?;
            modal_embed(ctx, modal, "Album", &format!("Added `{card_uid}` to album {album_id}.")).await
        }
        ["m", "album", owner] => {
            let owner_id = owned_id(modal.user.id.get(), owner)?;
            let id = cards::create_album(&state.db, owner_id, &value).await?;
            modal_embed(ctx, modal, "Album created", &format!("Album `{id}` is ready. Add cards with its id from the cards menu.")).await
        }
        ["m", "albumadd", owner, album] => {
            let owner_id = owned_id(modal.user.id.get(), owner)?;
            let album_id: i64 = album.parse().unwrap_or(0);
            cards::add_to_album(&state.db, owner_id, album_id, value.trim()).await?;
            modal_embed(ctx, modal, "Album", "Card added.").await
        }
        _ => Ok(()),
    };
    if let Err(err) = result {
        modal_ephemeral(ctx, modal, &err).await?;
    }
    Ok(())
}

async fn hub_action(
    ctx: FrameworkContext<'_, AppState, BotError>,
    interaction: &ComponentInteraction,
    owner: &str,
    action: &str,
    arg: &str,
) -> Result<(), BotError> {
    let state = ctx.user_data;
    let owner_id = owned(interaction, owner)?;
    match action {
        "home" => update_embed(ctx, interaction, hub::home_embed(), hub::hub_rows(owner_id, &state.config.portal_url)).await,
        "me" => {
            let user = db::users::ensure_user(&state.db, owner_id).await?;
            let now = db::utc_now();
            let (drops, grabs) = db::users::view_charges(
                &user,
                now,
                state.config.drop_max,
                state.config.grab_max,
                state.config.drop_regen,
                state.config.grab_regen,
            );
            let embed = ui::embed("Profile", format!("<@{owner_id}>"))
                .field("Gold", user.gold.to_string(), true)
                .field("Total drops", user.total_drops.to_string(), true)
                .field(
                    "Drop charges",
                    format!(
                        "{}/{} {}",
                        drops.charges,
                        state.config.drop_max,
                        drops.next_regen_at.map(|at| crate::models::discord_rel(at)).unwrap_or_else(|| "full".into())
                    ),
                    false,
                )
                .field(
                    "Grab charges",
                    format!(
                        "{}/{} {}",
                        grabs.charges,
                        state.config.grab_max,
                        grabs.next_regen_at.map(|at| crate::models::discord_rel(at)).unwrap_or_else(|| "full".into())
                    ),
                    false,
                );
            update_embed(ctx, interaction, embed, vec![hub::back_row(owner_id)]).await
        }
        "search" => {
            modal(ctx, interaction, &format!("m:search:{owner_id}"), "Search", "Query", "Name or series id", "query").await
        }
        "market" => {
            let page: i64 = arg.parse().unwrap_or(0);
            let reply = economy::market_reply(state, owner_id, page).await?;
            update_view(ctx, interaction, reply).await
        }
        "cards" => {
            let page: i64 = arg.parse().unwrap_or(0);
            let reply = collection::inventory_reply(state, owner_id, page).await?;
            update_view(ctx, interaction, reply).await
        }
        "cup" => cup_board(ctx, interaction, owner_id).await,
        "event" => event_board(ctx, interaction, owner_id).await,
        "quests" => {
            let reply = quests::board_reply(state, owner_id).await?;
            update_view(ctx, interaction, reply).await
        }
        "drop" => {
            ephemeral_text(ctx, interaction, "Drawing three cards...").await?;
            let guild_id = interaction.guild_id.map(|id| id.get() as i64);
            drop::post_to_channel(
                state,
                &ctx.serenity_context.http,
                interaction.channel_id,
                guild_id,
                owner_id,
            )
            .await?;
            let _ = interaction
                .edit_response(
                    ctx.serenity_context,
                    EditInteractionResponse::new().content("Drop posted in this channel."),
                )
                .await;
            Ok(())
        }
        _ => Ok(()),
    }
}

async fn grab(
    ctx: FrameworkContext<'_, AppState, BotError>,
    interaction: &ComponentInteraction,
    drop_raw: &str,
    slot_raw: &str,
) -> Result<(), BotError> {
    let state = ctx.user_data;
    let drop_id: i64 = drop_raw.parse().unwrap_or(0);
    let slot: i16 = slot_raw.parse().unwrap_or(0);
    let user_id = snowflake_i64(interaction.user.id.get())?;
    let local = state.lock_drop(drop_id).await;
    let _guard = local.lock().await;
    let key = format!("nooora:drop:{drop_id}");
    if !state.redis_try_lock(&key, 30).await {
        return ephemeral(ctx, interaction, &BotError::user_message("someone else is resolving this drop")).await;
    }
    let outcome = claim_and_paint(ctx, interaction, state, drop_id, slot, user_id).await;
    state.redis_unlock(&key).await;
    outcome
}

async fn claim_and_paint(
    ctx: FrameworkContext<'_, AppState, BotError>,
    interaction: &ComponentInteraction,
    state: &AppState,
    drop_id: i64,
    slot: i16,
    user_id: i64,
) -> Result<(), BotError> {
    let claimed = drops::claim_slot(&state.db, drop_id, slot, user_id, &state.config).await?;
    let expires: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT expires_at FROM drops WHERE drop_id = $1")
            .bind(drop_id)
            .fetch_optional(&state.db)
            .await?;
    let expires = expires.unwrap_or_else(db::utc_now);
    let fresh = drops::load_slots(&state.db, drop_id).await?;
    let (png, embed, rows) = drop::rerender(state, drop_id, &fresh, expires).await?;
    interaction
        .create_response(
            ctx.serenity_context,
            CreateInteractionResponse::UpdateMessage(
                CreateInteractionResponseMessage::new()
                    .embed(embed)
                    .components(rows)
                    .add_file(poise::serenity_prelude::CreateAttachment::bytes(png, "drop.png")),
            ),
        )
        .await?;
    interaction
        .create_followup(
            ctx.serenity_context,
            CreateInteractionResponseFollowup::new()
                .ephemeral(true)
                .content(format!(
                    "You grabbed **{}** from {} (`{}`) print #{}.",
                    claimed.character_name, claimed.series_name, claimed.card_uid, claimed.print_number
                )),
        )
        .await?;
    Ok(())
}

async fn inspect_selected(
    ctx: FrameworkContext<'_, AppState, BotError>,
    interaction: &ComponentInteraction,
    owner: &str,
) -> Result<(), BotError> {
    let owner_id = owned(interaction, owner)?;
    let ComponentInteractionDataKind::StringSelect { values } = &interaction.data.kind else {
        return Ok(());
    };
    let Some(uid) = values.first() else {
        return Ok(());
    };
    let reply = collection::inspect_reply(ctx.user_data, owner_id, uid).await?;
    update_view(ctx, interaction, reply).await
}

async fn trade_action(
    ctx: FrameworkContext<'_, AppState, BotError>,
    interaction: &ComponentInteraction,
    trade_id: &str,
    action: &str,
) -> Result<(), BotError> {
    let state = ctx.user_data;
    let user_id = snowflake_i64(interaction.user.id.get())?;
    let user = user_id.to_string();
    let lock = state.lock_trade(trade_id).await;
    let _guard = lock.lock().await;
    match action {
        "add" => modal(ctx, interaction, &format!("m:tadd:{trade_id}:{user}"), "Add cards", "Card uids", "uid1, uid2", "cards").await,
        "remove" => modal(ctx, interaction, &format!("m:trem:{trade_id}:{user}"), "Remove a card", "Card uid", "uid", "card").await,
        "gold" => modal(ctx, interaction, &format!("m:tgold:{trade_id}:{user}"), "Offer Gold", "Amount", "0", "gold").await,
        "lock" => {
            let row = econ::lock_side(&state.db, trade_id, user_id).await?;
            update_embed(ctx, interaction, economy::trade_embed(&row), economy::trade_rows(&row)).await
        }
        "confirm" => {
            let outcome = econ::confirm(&state.db, trade_id, user_id, state.config.trade_gate).await?;
            let row = econ::load_trade(&state.db, trade_id).await?.ok_or_else(|| BotError::user_message("trade missing"))?;
            update_embed(
                ctx,
                interaction,
                economy::trade_embed(&row).field("Result", economy::confirm_text(&outcome), false),
                economy::trade_rows(&row),
            )
            .await
        }
        "cancel" => {
            let row = econ::cancel(&state.db, trade_id, user_id).await?;
            update_embed(ctx, interaction, economy::trade_embed(&row), economy::trade_rows(&row)).await
        }
        _ => Ok(()),
    }
}

async fn quest_action(
    ctx: FrameworkContext<'_, AppState, BotError>,
    interaction: &ComponentInteraction,
    owner: &str,
    action: &str,
    quest_id: Option<&str>,
) -> Result<(), BotError> {
    let state = ctx.user_data;
    let owner_id = owned(interaction, owner)?;
    let text = match action {
        "daily" => quests::claim_daily(state, owner_id).await?,
        "claim" => quests::claim_one(state, owner_id, quest_id.unwrap_or("")).await?,
        _ => return Ok(()),
    };
    let reply = quests::board_reply(state, owner_id).await?.content(text);
    update_view(ctx, interaction, reply).await
}

async fn market_action(
    ctx: FrameworkContext<'_, AppState, BotError>,
    interaction: &ComponentInteraction,
    owner: &str,
    action: &str,
    arg: Option<&str>,
) -> Result<(), BotError> {
    let state = ctx.user_data;
    let owner_id = owned(interaction, owner)?;
    match action {
        "list" => {
            modal(ctx, interaction, &format!("m:list:{owner_id}"), "List a card", "Uid and price", "Ab12Cd34 250", "offer").await
        }
        "buy" => {
            let listing_id: i64 = arg.unwrap_or("0").parse().unwrap_or(0);
            let bought = econ::buy_listing(&state.db, owner_id, listing_id, state.config.trade_gate).await?;
            ephemeral_text(
                ctx,
                interaction,
                &format!("Bought listing #{} for {} Gold.", bought.listing_id, bought.price_gold),
            )
            .await
        }
        _ => Ok(()),
    }
}

async fn admin_action(
    ctx: FrameworkContext<'_, AppState, BotError>,
    interaction: &ComponentInteraction,
    owner: &str,
    action: &str,
) -> Result<(), BotError> {
    owned(interaction, owner)?;
    if !can_manage(interaction) {
        return Err(BotError::user_message("you need Manage Server or Administrator"));
    }
    let guild_id = interaction
        .guild_id
        .map(|id| id.get() as i64)
        .ok_or_else(|| BotError::user_message("use this in a server"))?;
    match action {
        "series" | "set" => {
            modal(
                ctx,
                interaction,
                &format!("m:series:{guild_id}:{owner}"),
                "Set series",
                "Series id",
                "086c62",
                "series_id",
            )
            .await
        }
        "show" => {
            let text = admin::describe(ctx.user_data, guild_id).await?;
            update_embed(
                ctx,
                interaction,
                ui::embed("Linked series", text),
                vec![poise::serenity_prelude::CreateActionRow::Buttons(vec![button(
                    format!("admin:{owner}:series"),
                    "Series",
                    ButtonStyle::Primary,
                )])],
            )
            .await
        }
        _ => Ok(()),
    }
}

async fn cup_board(
    ctx: FrameworkContext<'_, AppState, BotError>,
    interaction: &ComponentInteraction,
    owner: i64,
) -> Result<(), BotError> {
    let rows = cup::leaderboard(&ctx.user_data.db).await?;
    let mine = cup::stats(&ctx.user_data.db, owner).await?;
    let board = if rows.is_empty() {
        "No matches yet.".to_string()
    } else {
        rows.iter()
            .enumerate()
            .map(|(index, row)| {
                format!(
                    "{}. <@{}> rating {} · {}W/{}L",
                    index + 1,
                    row.user_id,
                    row.rating,
                    row.wins,
                    row.losses
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let embed = ui::embed("Nooora Cup", board).field(
        "You",
        format!("rating {} · {}W/{}L", mine.rating, mine.wins, mine.losses),
        false,
    );
    update_embed(
        ctx,
        interaction,
        embed,
        vec![poise::serenity_prelude::CreateActionRow::Buttons(vec![
            button(format!("cup:{owner}:join"), "Join queue", ButtonStyle::Success),
            button(format!("hub:{owner}:home"), "Menu", ButtonStyle::Secondary),
        ])],
    )
    .await
}

async fn cup_join(
    ctx: FrameworkContext<'_, AppState, BotError>,
    interaction: &ComponentInteraction,
    owner: &str,
) -> Result<(), BotError> {
    let state = ctx.user_data;
    let owner_id = owned(interaction, owner)?;
    let name = interaction.user.name.clone();
    let mut queue = state.cup.lock().await;
    if queue.iter().any(|ticket| ticket.user_id == owner_id) {
        return ephemeral_text(ctx, interaction, "You are already in the Nooora Cup queue.").await;
    }
    queue.push_back(CupTicket {
        user_id: owner_id,
        channel_id: interaction.channel_id.get(),
        name,
    });
    if queue.len() < 2 {
        let waiting = queue.len();
        drop(queue);
        return ephemeral_text(ctx, interaction, &format!("Queued. {waiting} player waiting.")).await;
    }
    let Some(first) = queue.pop_front() else {
        return Ok(());
    };
    let Some(second) = queue.pop_front() else {
        queue.push_front(first);
        return Ok(());
    };
    drop(queue);
    tracing::info!(
        channel_a = first.channel_id,
        channel_b = second.channel_id,
        "{} vs {}",
        first.name,
        second.name
    );
    let result = cup::play_match(&state.db, first.user_id, second.user_id).await?;
    let winner_name = if result.winner == first.user_id {
        first.name.as_str()
    } else {
        second.name.as_str()
    };
    let loser_name = if result.loser == first.user_id {
        first.name.as_str()
    } else {
        second.name.as_str()
    };
    interaction
        .create_response(
            ctx.serenity_context,
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new().embed(
                    ui::embed(
                        "Nooora Cup result",
                        format!(
                            "{winner_name} (<@{}>, power {}) beat {loser_name} (<@{}>, power {}).\nRatings: {} / {}.",
                            result.winner,
                            result.winner_power,
                            result.loser,
                            result.loser_power,
                            result.winner_rating,
                            result.loser_rating
                        ),
                    ),
                ),
            ),
        )
        .await?;
    Ok(())
}

async fn event_board(
    ctx: FrameworkContext<'_, AppState, BotError>,
    interaction: &ComponentInteraction,
    owner: i64,
) -> Result<(), BotError> {
    let event: Option<(String, String, chrono::DateTime<chrono::Utc>, i32, Option<String>)> = sqlx::query_as(
        r#"
        SELECT event_id, name, ends_at, bonus_gold, series_id
        FROM events
        WHERE starts_at <= now() AND ends_at > now()
        ORDER BY starts_at DESC
        LIMIT 1
        "#,
    )
    .fetch_optional(&ctx.user_data.db)
    .await?;
    let (embed, rows) = if let Some((id, name, ends, bonus, series)) = event {
        (
            ui::embed(
                name,
                format!(
                    "Event `{id}` boosts series `{}` and pays {bonus} Gold on claim.\nEnds {}.",
                    series.unwrap_or_else(|| "-".into()),
                    crate::models::discord_rel(ends)
                ),
            ),
            vec![poise::serenity_prelude::CreateActionRow::Buttons(vec![
                button(format!("event:{owner}:claim"), "Claim reward", ButtonStyle::Success),
                button(format!("hub:{owner}:home"), "Menu", ButtonStyle::Secondary),
            ])],
        )
    } else {
        (
            ui::embed("Events", "No seasonal event is live."),
            vec![hub::back_row(owner)],
        )
    };
    update_embed(ctx, interaction, embed, rows).await
}

async fn event_claim(
    ctx: FrameworkContext<'_, AppState, BotError>,
    interaction: &ComponentInteraction,
    owner: &str,
) -> Result<(), BotError> {
    let state = ctx.user_data;
    let owner_id = owned(interaction, owner)?;
    db::users::ensure_user(&state.db, owner_id).await?;
    let event: Option<(String, i32)> = sqlx::query_as(
        "SELECT event_id, bonus_gold FROM events WHERE starts_at <= now() AND ends_at > now() ORDER BY starts_at DESC LIMIT 1",
    )
    .fetch_optional(&state.db)
    .await?;
    let Some((event_id, bonus)) = event else {
        return ephemeral_text(ctx, interaction, "No event is live.").await;
    };
    let mut tx = db::begin(&state.db).await?;
    let inserted = sqlx::query(
        "INSERT INTO event_claims (user_id, event_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
    )
    .bind(owner_id)
    .bind(&event_id)
    .execute(&mut *tx)
    .await?;
    if inserted.rows_affected() == 0 {
        drop(tx);
        return ephemeral_text(ctx, interaction, "You already claimed this event.").await;
    }
    let balance = db::users::add_gold(&mut tx, owner_id, bonus as i64).await?;
    db::write_audit(
        &mut tx,
        &crate::models::AuditEvent {
            tx_id: db::new_tx_id(),
            kind: "event",
            sender_id: None,
            receiver_id: Some(owner_id),
            cards: Vec::new(),
            gold_delta: bonus as i64,
            metadata: serde_json::json!({ "event_id": event_id }),
        },
    )
    .await?;
    tx.commit().await?;
    ephemeral_text(
        ctx,
        interaction,
        &format!("Event reward added {bonus} Gold. Balance: {balance}."),
    )
    .await
}

fn owned(interaction: &ComponentInteraction, owner: &str) -> Result<i64, BotError> {
    owned_id(interaction.user.id.get(), owner)
}

fn owned_id(actual: u64, owner: &str) -> Result<i64, BotError> {
    let owner_id: i64 = owner.parse().unwrap_or(0);
    if actual as i64 != owner_id {
        return Err(BotError::NotYourMenu);
    }
    Ok(owner_id)
}

fn can_manage(interaction: &ComponentInteraction) -> bool {
    interaction
        .member
        .as_ref()
        .and_then(|member| member.permissions)
        .map(|perms| perms.administrator() || perms.manage_guild())
        .unwrap_or(false)
}

fn can_manage_modal(modal: &ModalInteraction) -> bool {
    modal
        .member
        .as_ref()
        .and_then(|member| member.permissions)
        .map(|perms| perms.administrator() || perms.manage_guild())
        .unwrap_or(false)
}

async fn modal(
    ctx: FrameworkContext<'_, AppState, BotError>,
    interaction: &ComponentInteraction,
    custom_id: &str,
    title: &str,
    label: &str,
    placeholder: &str,
    field: &str,
) -> Result<(), BotError> {
    let input = CreateInputText::new(InputTextStyle::Short, label, field)
        .placeholder(placeholder)
        .required(true)
        .max_length(80);
    interaction
        .create_response(
            ctx.serenity_context,
            CreateInteractionResponse::Modal(
                CreateModal::new(custom_id, title).components(vec![CreateActionRow::InputText(input)]),
            ),
        )
        .await?;
    Ok(())
}

async fn update_embed(
    ctx: FrameworkContext<'_, AppState, BotError>,
    interaction: &ComponentInteraction,
    embed: poise::serenity_prelude::CreateEmbed,
    components: Vec<CreateActionRow>,
) -> Result<(), BotError> {
    interaction
        .create_response(
            ctx.serenity_context,
            CreateInteractionResponse::UpdateMessage(
                CreateInteractionResponseMessage::new()
                    .content("")
                    .embed(embed)
                    .components(components),
            ),
        )
        .await?;
    Ok(())
}

async fn update_view(
    ctx: FrameworkContext<'_, AppState, BotError>,
    interaction: &ComponentInteraction,
    view: crate::ui::View,
) -> Result<(), BotError> {
    interaction
        .create_response(
            ctx.serenity_context,
            CreateInteractionResponse::UpdateMessage(view.into_update()),
        )
        .await?;
    Ok(())
}

async fn ephemeral(
    ctx: FrameworkContext<'_, AppState, BotError>,
    interaction: &ComponentInteraction,
    err: &BotError,
) -> Result<(), BotError> {
    interaction
        .create_response(
            ctx.serenity_context,
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .ephemeral(true)
                    .embed(err.user_embed()),
            ),
        )
        .await?;
    Ok(())
}

async fn ephemeral_text(
    ctx: FrameworkContext<'_, AppState, BotError>,
    interaction: &ComponentInteraction,
    text: &str,
) -> Result<(), BotError> {
    interaction
        .create_response(
            ctx.serenity_context,
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .ephemeral(true)
                    .content(text),
            ),
        )
        .await?;
    Ok(())
}

async fn modal_ephemeral(
    ctx: FrameworkContext<'_, AppState, BotError>,
    modal: &ModalInteraction,
    err: &BotError,
) -> Result<(), BotError> {
    modal
        .create_response(
            ctx.serenity_context,
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .ephemeral(true)
                    .embed(err.user_embed()),
            ),
        )
        .await?;
    Ok(())
}

async fn modal_embed(
    ctx: FrameworkContext<'_, AppState, BotError>,
    modal: &ModalInteraction,
    title: &str,
    body: &str,
) -> Result<(), BotError> {
    modal
        .create_response(
            ctx.serenity_context,
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new().embed(ui::embed(title, body)),
            ),
        )
        .await?;
    Ok(())
}

async fn refresh_trade(
    ctx: FrameworkContext<'_, AppState, BotError>,
    modal: &ModalInteraction,
    row: &econ::TradeRow,
) -> Result<(), BotError> {
    if let Some(message) = &modal.message {
        message
            .channel_id
            .edit_message(
                ctx.serenity_context,
                message.id,
                poise::serenity_prelude::EditMessage::new()
                    .embed(economy::trade_embed(row))
                    .components(economy::trade_rows(row)),
            )
            .await?;
    }
    modal
        .create_response(
            ctx.serenity_context,
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .ephemeral(true)
                    .content("Offer updated. Both locks were cleared."),
            ),
        )
        .await?;
    Ok(())
}

fn first_input(modal: &ModalInteraction) -> String {
    for row in &modal.data.components {
        for component in row.components.iter() {
            if let poise::serenity_prelude::ActionRowComponent::InputText(input) = component {
                if let Some(value) = &input.value {
                    return value.clone();
                }
            }
        }
    }
    String::new()
}


