use poise::serenity_prelude::ButtonStyle;

use crate::db::guild::{self, GuildRow};
use crate::error::BotError;
use crate::ids::snowflake_i64;
use crate::models::discord_rel;
use crate::state::{AppState, Context};
use crate::ui::{self, button, row};

/// Look up a series id and a slice of its roster.
#[poise::command(prefix_command, slash_command, rename = "series")]
pub async fn series(
    ctx: Context<'_>,
    #[description = "Series name or 6-character id"]
    #[rest]
    query: String,
) -> Result<(), BotError> {
    let hits = guild::search_series(&ctx.data().db, &query).await?;
    if hits.is_empty() {
        ctx.send(
            poise::CreateReply::default().embed(ui::embed("No series", format!("Nothing matched `{query}`."))),
        )
        .await?;
        return Ok(());
    }
    let mut blocks = Vec::new();
    for hit in hits.iter().take(5) {
        let roster = guild::roster(&ctx.data().db, &hit.series_id).await?;
        let names = if roster.is_empty() {
            "_no characters_".to_string()
        } else {
            roster
                .iter()
                .take(8)
                .map(|entry| format!("{} ({})", entry.character_name, entry.rarity_tier))
                .collect::<Vec<_>>()
                .join(", ")
        };
        blocks.push(format!(
            "**{}** `{}` · match {:.2}\n{names}",
            hit.series_name, hit.series_id, hit.score
        ));
    }
    ctx.send(
        poise::CreateReply::default().embed(ui::embed(format!("Series · {query}"), blocks.join("\n\n"))),
    )
    .await?;
    Ok(())
}

/// Server controls. Discord requires a subcommand, so `/admin panel` opens the buttons.
#[poise::command(
    prefix_command,
    slash_command,
    rename = "admin",
    subcommands("panel", "series_group"),
    required_permissions = "MANAGE_GUILD",
    guild_only
)]
pub async fn admin(ctx: Context<'_>) -> Result<(), BotError> {
    render_panel(ctx).await
}

/// Open the series linking buttons.
#[poise::command(
    prefix_command,
    slash_command,
    rename = "panel",
    required_permissions = "MANAGE_GUILD",
    guild_only
)]
pub async fn panel(ctx: Context<'_>) -> Result<(), BotError> {
    render_panel(ctx).await
}

async fn render_panel(ctx: Context<'_>) -> Result<(), BotError> {
    let owner = snowflake_i64(ctx.author().id.get())?;
    let guild_id = require_guild(ctx)?;
    let current = describe(ctx.data(), guild_id).await?;
    ctx.send(
        poise::CreateReply::default()
            .embed(ui::embed("Server admin", current))
            .components(vec![row(vec![
                button(format!("admin:{owner}:series"), "Series", ButtonStyle::Primary),
                button(format!("admin:{owner}:show"), "Show link", ButtonStyle::Secondary),
            ])]),
    )
    .await?;
    Ok(())
}

/// Series linking commands.
#[poise::command(
    prefix_command,
    slash_command,
    rename = "series",
    subcommands("set_series", "show_series"),
    required_permissions = "MANAGE_GUILD",
    guild_only
)]
pub async fn series_group(ctx: Context<'_>) -> Result<(), BotError> {
    render_show(ctx).await
}

/// Set the server-linked series. Enforces the 6-day cooldown.
#[poise::command(
    prefix_command,
    slash_command,
    rename = "set",
    required_permissions = "MANAGE_GUILD",
    guild_only
)]
pub async fn set_series(
    ctx: Context<'_>,
    #[description = "6-character series id"] series_id: Option<String>,
) -> Result<(), BotError> {
    let Some(series_id) = series_id else {
        let owner = snowflake_i64(ctx.author().id.get())?;
        ctx.send(
            poise::CreateReply::default()
                .embed(ui::embed(
                    "Set series",
                    "Pass a 6-character id, or press the button and type it.",
                ))
                .components(vec![row(vec![button(
                    format!("admin:{owner}:set"),
                    "Enter series id",
                    ButtonStyle::Primary,
                )])]),
        )
        .await?;
        return Ok(());
    };
    let guild_id = require_guild(ctx)?;
    let saved = guild::set_series(
        &ctx.data().db,
        guild_id,
        &series_id,
        chrono::Duration::seconds(ctx.data().config.series_change_cooldown.as_secs() as i64),
    )
    .await?;
    ctx.send(poise::CreateReply::default().embed(settings_embed(&saved)))
        .await?;
    Ok(())
}

/// Show the server-linked series and the next time it can change.
#[poise::command(
    prefix_command,
    slash_command,
    rename = "show",
    required_permissions = "MANAGE_GUILD",
    guild_only
)]
pub async fn show_series(ctx: Context<'_>) -> Result<(), BotError> {
    render_show(ctx).await
}

async fn render_show(ctx: Context<'_>) -> Result<(), BotError> {
    let guild_id = require_guild(ctx)?;
    ctx.send(
        poise::CreateReply::default().embed(ui::embed(
            "Linked series",
            describe(ctx.data(), guild_id).await?,
        )),
    )
    .await?;
    Ok(())
}

pub async fn describe(state: &AppState, guild_id: i64) -> Result<String, BotError> {
    match guild::settings(&state.db, guild_id).await? {
        None => Ok("This server has no linked series.".into()),
        Some(row) => Ok(settings_text(&row, state)),
    }
}

pub fn settings_embed(row: &GuildRow) -> poise::serenity_prelude::CreateEmbed {
    ui::embed(
        "Series linked",
        format!(
            "`{}` {}",
            row.linked_series_id.as_deref().unwrap_or("-"),
            row.series_name.as_deref().unwrap_or("")
        ),
    )
}

fn settings_text(row: &GuildRow, state: &AppState) -> String {
    let ready = row.last_series_change_at.map(|changed| {
        changed + chrono::Duration::seconds(state.config.series_change_cooldown.as_secs() as i64)
    });
    let cooldown = match ready {
        Some(at) if chrono::Utc::now() < at => format!("Next change {}.", discord_rel(at)),
        Some(_) => "The cooldown has elapsed.".into(),
        None => "No change has been recorded yet.".into(),
    };
    format!(
        "Guild `{}`\nLinked series: `{}` {}\nBoost: {}x\n{cooldown}",
        row.guild_id,
        row.linked_series_id.as_deref().unwrap_or("none"),
        row.series_name.as_deref().unwrap_or(""),
        row.series_boost
    )
}

pub fn require_guild(ctx: Context<'_>) -> Result<i64, BotError> {
    let guild = ctx
        .guild_id()
        .ok_or_else(|| BotError::user_message("use this command in a server"))?;
    snowflake_i64(guild.get())
}


