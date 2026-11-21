use poise::serenity_prelude::{
    ButtonStyle, Colour, CreateActionRow, CreateAttachment, CreateEmbed, CreateMessage,
};

use crate::db::drops;
use crate::engine::renderer::CardFace;
use crate::error::BotError;
use crate::ids::snowflake_i64;
use crate::images;
use crate::models::{discord_rel, Cosmetics};
use crate::state::{AppState, Context};
use crate::ui::{self, button, row};

/// Spend a drop charge and post three cards to grab.
#[poise::command(prefix_command, slash_command, rename = "drop", aliases("d"))]
pub async fn drop(ctx: Context<'_>) -> Result<(), BotError> {
    issue(ctx).await
}

/// Show drop and grab charges with live timestamps.
#[poise::command(prefix_command, slash_command, rename = "cooldown", aliases("cd"))]
pub async fn cooldown(ctx: Context<'_>) -> Result<(), BotError> {
    let embed = crate::commands::hub::profile_embed(ctx).await?;
    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}

struct DropPost {
    drop_id: i64,
    user_id: i64,
    png: Vec<u8>,
    embed: CreateEmbed,
    components: Vec<CreateActionRow>,
}

async fn build_drop(
    state: &AppState,
    user_id: i64,
    channel_id: i64,
    guild_id: Option<i64>,
) -> Result<DropPost, BotError> {
    let opened = drops::open_drop(&state.db, user_id, channel_id, guild_id, &state.config).await?;
    let faces = faces_for_templates(state, &opened).await;
    let fonts = state.fonts.clone();
    let rendered = tokio::task::spawn_blocking(move || {
        crate::engine::renderer::render_drop_strip(&fonts, &faces)
    })
    .await
    .map_err(|err| BotError::user_message(format!("renderer task failed: {err}")));
    let png = match rendered {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(err)) | Err(err) => {
            drops::refund_unclaimed(&state.db, opened.drop_id, user_id).await?;
            return Err(err);
        }
    };
    let slots = drops::load_slots(&state.db, opened.drop_id).await?;
    Ok(DropPost {
        drop_id: opened.drop_id,
        user_id,
        embed: drop_embed(&opened, &slots),
        components: grab_rows(opened.drop_id, &slots),
        png,
    })
}

pub async fn issue(ctx: Context<'_>) -> Result<(), BotError> {
    let state = ctx.data().clone();
    let user_id = snowflake_i64(ctx.author().id.get())?;
    let channel_id = snowflake_i64(ctx.channel_id().get())?;
    let guild_id = ctx.guild_id().map(|id| id.get() as i64);
    let post = build_drop(&state, user_id, channel_id, guild_id).await?;
    let message = match ctx
        .send(
            poise::CreateReply::default()
                .embed(post.embed)
                .attachment(CreateAttachment::bytes(post.png, "drop.png"))
                .components(post.components),
        )
        .await
    {
        Ok(handle) => handle,
        Err(err) => {
            drops::refund_unclaimed(&state.db, post.drop_id, post.user_id).await?;
            return Err(BotError::Discord(err));
        }
    };
    if let Ok(sent) = message.message().await {
        drops::attach_message(&state.db, post.drop_id, sent.id.get() as i64).await?;
    }
    Ok(())
}

pub async fn post_to_channel(
    state: &AppState,
    http: &poise::serenity_prelude::Http,
    channel_id: poise::serenity_prelude::ChannelId,
    guild_id: Option<i64>,
    user_id: i64,
) -> Result<(), BotError> {
    let post = build_drop(state, user_id, channel_id.get() as i64, guild_id).await?;
    let message = channel_id
        .send_message(
            http,
            CreateMessage::new()
                .embed(post.embed)
                .add_file(CreateAttachment::bytes(post.png, "drop.png"))
                .components(post.components),
        )
        .await;
    match message {
        Ok(sent) => {
            drops::attach_message(&state.db, post.drop_id, sent.id.get() as i64).await?;
            Ok(())
        }
        Err(err) => {
            drops::refund_unclaimed(&state.db, post.drop_id, post.user_id).await?;
            Err(BotError::Discord(err))
        }
    }
}

pub async fn faces_for_templates(state: &AppState, opened: &drops::OpenDrop) -> [CardFace; 3] {
    let mut faces = Vec::new();
    for (index, template) in opened.templates.iter().enumerate() {
        faces.push(CardFace {
            character_name: template.character_name.clone(),
            series_name: template.series_name.clone(),
            series_id: template.series_id.clone(),
            rarity: template.rarity_tier.clone(),
            variant: "normal".into(),
            print_number: None,
            card_uid: None,
            base_png: images::load_base(state, template.base_image_url.as_deref()).await,
            cosmetics: Cosmetics::default(),
            slot_label: Some((index as u8) + 1),
            claimed: false,
        });
    }
    faces.try_into().unwrap_or_else(|_| {
        panic!("drop always rolls 3 templates");
    })
}

pub fn drop_embed(opened: &drops::OpenDrop, slots: &[drops::SlotView]) -> CreateEmbed {
    let lines: Vec<String> = slots
        .iter()
        .map(|slot| {
            let state = if slot.claimed_by.is_some() {
                "claimed"
            } else {
                "open"
            };
            format!(
                "**{}.** {} · {} · `{}` · {state}",
                slot.slot, slot.character_name, slot.series_name, slot.series_id
            )
        })
        .collect();
    ui::embed("Drop", lines.join("\n"))
        .color(Colour::new(crate::error::EMBED_COLOR))
        .image("attachment://drop.png")
        .field("Closes", discord_rel(opened.expires_at), true)
}

pub fn grab_rows(drop_id: i64, slots: &[drops::SlotView]) -> Vec<CreateActionRow> {
    let buttons = slots
        .iter()
        .map(|slot| {
            let label = if slot.claimed_by.is_some() {
                format!("{} taken", slot.slot)
            } else {
                slot.slot.to_string()
            };
            button(format!("grab:{drop_id}:{}", slot.slot), label, ButtonStyle::Primary)
                .disabled(slot.claimed_by.is_some())
        })
        .collect();
    vec![row(buttons)]
}

pub async fn rerender(
    state: &AppState,
    drop_id: i64,
    slots: &[drops::SlotView],
    expires_at: chrono::DateTime<chrono::Utc>,
) -> Result<(Vec<u8>, CreateEmbed, Vec<CreateActionRow>), BotError> {
    let mut faces = Vec::new();
    for slot in slots {
        faces.push(CardFace {
            character_name: slot.character_name.clone(),
            series_name: slot.series_name.clone(),
            series_id: slot.series_id.clone(),
            rarity: slot.rarity_tier.clone(),
            variant: "normal".into(),
            print_number: slot.print_number,
            card_uid: slot.card_uid.clone(),
            base_png: images::load_base(state, slot.base_image_url.as_deref()).await,
            cosmetics: Cosmetics::default(),
            slot_label: Some(slot.slot as u8),
            claimed: slot.claimed_by.is_some(),
        });
    }
    let faces: [CardFace; 3] = faces
        .try_into()
        .map_err(|_| BotError::user_message("drop is missing a slot"))?;
    let fonts = state.fonts.clone();
    let png = tokio::task::spawn_blocking(move || {
        crate::engine::renderer::render_drop_strip(&fonts, &faces)
    })
    .await
    .map_err(|err| BotError::user_message(format!("renderer task failed: {err}")))??;
    let opened = drops::OpenDrop {
        drop_id: 0,
        expires_at,
        templates: Vec::new(),
    };
    Ok((png, drop_embed(&opened, slots), grab_rows(drop_id, slots)))
}


