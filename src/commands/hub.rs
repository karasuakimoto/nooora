use poise::serenity_prelude::{ButtonStyle, CreateActionRow};

use crate::db::users;
use crate::error::BotError;
use crate::ids::snowflake_i64;
use crate::models::discord_rel;
use crate::state::Context;
use crate::ui::{self, button, link_button, row};

/// Open the Nooora menu.
#[poise::command(prefix_command, slash_command, rename = "nooora", aliases("k"))]
pub async fn nooora(ctx: Context<'_>) -> Result<(), BotError> {
    let owner = snowflake_i64(ctx.author().id.get())?;
    ctx.send(
        poise::CreateReply::default()
            .embed(home_embed())
            .components(hub_rows(owner, &ctx.data().config.portal_url)),
    )
    .await?;
    Ok(())
}

pub fn home_embed() -> poise::serenity_prelude::CreateEmbed {
    ui::embed(
        "Nooora",
        "Pick a desk. Grab buttons are the only public controls. Everything else stays with the person who opened the menu.",
    )
    .field("Me", "Profile, Gold, drops, charges", true)
    .field("Search", "Character or series lookup", true)
    .field("Market", "Listings priced in Gold", true)
    .field("Cards", "Inventory, albums, cosmetics", true)
    .field("Cup", "Queue and leaderboard", true)
    .field("Event", "The live seasonal drop", true)
}

pub fn hub_rows(owner: i64, portal: &str) -> Vec<CreateActionRow> {
    vec![
        row(vec![
            button(format!("hub:{owner}:me"), "Me", ButtonStyle::Primary),
            button(format!("hub:{owner}:search"), "Search", ButtonStyle::Secondary),
            button(format!("hub:{owner}:market"), "Market", ButtonStyle::Secondary),
            button(format!("hub:{owner}:cards"), "Cards", ButtonStyle::Secondary),
            button(format!("hub:{owner}:cup"), "Cup", ButtonStyle::Secondary),
        ]),
        row(vec![
            button(format!("hub:{owner}:event"), "Event", ButtonStyle::Secondary),
            button(format!("hub:{owner}:drop"), "Drop", ButtonStyle::Success),
            button(format!("hub:{owner}:quests"), "Quests", ButtonStyle::Secondary),
            link_button(portal, "Links"),
            button(format!("hub:{owner}:home"), "Home", ButtonStyle::Secondary),
        ]),
    ]
}

pub async fn profile_embed(ctx: Context<'_>) -> Result<poise::serenity_prelude::CreateEmbed, BotError> {
    let state = ctx.data();
    let user_id = snowflake_i64(ctx.author().id.get())?;
    let user = users::ensure_user(&state.db, user_id).await?;
    let now = chrono::Utc::now();
    let (drops, grabs) = users::view_charges(
        &user,
        now,
        state.config.drop_max,
        state.config.grab_max,
        state.config.drop_regen,
        state.config.grab_regen,
    );
    let drop_line = match drops.next_regen_at {
        Some(at) if drops.charges < state.config.drop_max => {
            format!("{}/{}  next {}", drops.charges, state.config.drop_max, discord_rel(at))
        }
        _ => format!("{}/{}  full", drops.charges, state.config.drop_max),
    };
    let grab_line = match grabs.next_regen_at {
        Some(at) if grabs.charges < state.config.grab_max => {
            format!("{}/{}  next {}", grabs.charges, state.config.grab_max, discord_rel(at))
        }
        _ => format!("{}/{}  full", grabs.charges, state.config.grab_max),
    };
    let gate = state.config.trade_gate;
    let gate_line = if user.total_drops >= gate {
        "Economy unlocked".to_string()
    } else {
        format!("Economy locks until {}/{} drops", user.total_drops, gate)
    };
    Ok(ui::embed(format!("{}'s desk", ctx.author().name), gate_line)
        .field("Gold", user.gold.to_string(), true)
        .field("Drops", user.total_drops.to_string(), true)
        .field("Drop charges", drop_line, false)
        .field("Grab charges", grab_line, false)
        .field(
            "Daily claim",
            user.last_daily_claim
                .map(|day| day.to_string())
                .unwrap_or_else(|| "not claimed".into()),
            true,
        ))
}

pub fn back_row(owner: i64) -> CreateActionRow {
    row(vec![button(
        format!("hub:{owner}:home"),
        "Back",
        ButtonStyle::Secondary,
    )])
}


