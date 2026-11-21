use poise::serenity_prelude::{ButtonStyle, CreateActionRow, CreateSelectMenu, CreateSelectMenuKind, CreateSelectMenuOption};

use crate::db::cards;
use crate::engine::renderer::CardFace;
use crate::error::BotError;
use crate::ids::snowflake_i64;
use crate::images;
use crate::models::{CardVariant, Cosmetics, OwnedCard, RarityTier};
use crate::state::{AppState, Context};
use crate::ui::{self, button, row};

/// Browse your cards, or inspect one uid.
#[poise::command(prefix_command, slash_command, rename = "cards", aliases("c"))]
pub async fn cards(
    ctx: Context<'_>,
    #[description = "Card uid to inspect"] card: Option<String>,
) -> Result<(), BotError> {
    let owner = snowflake_i64(ctx.author().id.get())?;
    if let Some(uid) = card {
        let view = inspect_reply(&ctx.data().clone(), owner, uid.trim()).await?;
        ctx.send(view.into_reply()).await?;
        return Ok(());
    }
    let reply = inventory_reply(&ctx.data().clone(), owner, 0).await?;
    ctx.send(reply.into_reply()).await?;
    Ok(())
}

pub async fn inventory_reply(
    state: &AppState,
    owner: i64,
    page: i64,
) -> Result<ui::View, BotError> {
    let (items, total) = cards::inventory(&state.db, owner, page).await?;
    let pages = pages(total);
    let page = page.clamp(0, pages.saturating_sub(1));
    let mut lines = Vec::new();
    for card in &items {
        let hold = match (&card.hold_kind, &card.hold_ref) {
            (Some(kind), Some(reference)) => format!(" · locked:{kind}:{reference}"),
            (Some(kind), None) => format!(" · locked:{kind}"),
            _ => String::new(),
        };
        lines.push(format!(
            "`{}` {} · {} · #{}{}",
            card.card_uid, card.character_name, card.series_id, card.print_number, hold
        ));
    }
    if lines.is_empty() {
        lines.push("No cards yet. Use `fd` or `/drop`.".into());
    }
    let albums = cards::albums(&state.db, owner).await?;
    if !albums.is_empty() {
        lines.push(String::new());
        lines.push("Albums".into());
        for album in &albums {
            lines.push(format!(
                "`{}` {} ({} cards)",
                album.album_id, album.name, album.cards
            ));
        }
    }
    let mut components = vec![nav_row(owner, page, pages)];
    if !items.is_empty() {
        components.push(select_row(owner, page, &items));
        components.push(row(vec![
            button(format!("album:{owner}:create"), "New album", ButtonStyle::Secondary),
            button(format!("album:{owner}:add"), "Add to album", ButtonStyle::Secondary),
        ]));
    }
    Ok(ui::View::new(ui::embed(
        format!("Cards {}/{}", page + 1, pages.max(1)),
        format!("{}\n\n{} owned", lines.join("\n"), total),
    ))
    .components(components))
}

pub async fn inspect_reply(
    state: &AppState,
    viewer: i64,
    card_uid: &str,
) -> Result<ui::View, BotError> {
    let Some(card) = cards::find_card(&state.db, card_uid).await? else {
        return Ok(ui::View::new(ui::embed(
            "Missing card",
            format!("No card `{card_uid}`"),
        )));
    };
    let png = render_owned(state, &card).await?;
    let cosmetics = &card.cosmetics.0;
    let description = format!(
        "**{}** · {} (`{}`)\nRarity {} · {} · print #{}\nOwner <@{}>\nFrame `{}` · Filter `{}` · 3D `{}`\nFont `{}` · Color `{}` · Outline {}",
        card.character_name,
        card.series_name,
        card.series_id,
        RarityTier::parse(&card.rarity_tier).as_str(),
        CardVariant::parse(&card.variant).as_str(),
        card.print_number,
        card.owner_id,
        cosmetics.frame_id.as_deref().unwrap_or("none"),
        cosmetics.filter_id.as_deref().unwrap_or("none"),
        cosmetics.effect_3d_id.as_deref().unwrap_or("none"),
        cosmetics.font_family.as_deref().unwrap_or("default"),
        cosmetics.text_color_hex.as_deref().unwrap_or("default"),
        cosmetics.outline_mode.as_str(),
    );
    let mut rows = Vec::new();
    if card.owner_id == viewer {
        rows.push(row(vec![
            button(format!("cos:{viewer}:{}:frame", card.card_uid), "Frame", ButtonStyle::Secondary),
            button(format!("cos:{viewer}:{}:filter", card.card_uid), "Filter", ButtonStyle::Secondary),
            button(format!("cos:{viewer}:{}:effect", card.card_uid), "3D", ButtonStyle::Secondary),
            button(format!("cos:{viewer}:{}:font", card.card_uid), "Font", ButtonStyle::Secondary),
            button(format!("cos:{viewer}:{}:style", card.card_uid), "Text", ButtonStyle::Secondary),
        ]));
    }
    rows.push(row(vec![button(
        format!("hub:{viewer}:cards"),
        "Back to cards",
        ButtonStyle::Secondary,
    )]));
    Ok(ui::View::new(
        ui::embed(format!("Card {}", card.card_uid), description).image("attachment://card.png"),
    )
    .file(png, "card.png")
    .components(rows))
}

pub async fn render_owned(state: &AppState, card: &OwnedCard) -> Result<Vec<u8>, BotError> {
    let face = CardFace {
        character_name: card.character_name.clone(),
        series_name: card.series_name.clone(),
        series_id: card.series_id.clone(),
        rarity: card.rarity_tier.clone(),
        variant: card.variant.clone(),
        print_number: Some(card.print_number),
        card_uid: Some(card.card_uid.clone()),
        base_png: images::load_base(state, card.base_image_url.as_deref()).await,
        cosmetics: card.cosmetics.0.clone(),
        slot_label: None,
        claimed: false,
    };
    let fonts = state.fonts.clone();
    tokio::task::spawn_blocking(move || crate::engine::renderer::render_card(&fonts, &face))
        .await
        .map_err(|err| BotError::user_message(format!("renderer task failed: {err}")))?
}

pub async fn apply_field(
    state: &AppState,
    owner: i64,
    card_uid: &str,
    field: &str,
    value: &str,
) -> Result<Cosmetics, BotError> {
    let Some(card) = cards::find_card(&state.db, card_uid).await? else {
        return Err(BotError::user_message("that card does not exist"));
    };
    if card.owner_id != owner {
        return Err(BotError::user_message("that card is not yours"));
    }
    let mut cosmetics = card.cosmetics.0.clone();
    let cleared = value.trim() == "-" || value.trim().is_empty();
    match field {
        "frame" => cosmetics.frame_id = if cleared { None } else { Some(value.trim().to_string()) },
        "filter" => cosmetics.filter_id = if cleared { None } else { Some(value.trim().to_string()) },
        "effect" => cosmetics.effect_3d_id = if cleared { None } else { Some(value.trim().to_string()) },
        "font" => cosmetics.font_family = if cleared { None } else { Some(value.trim().to_string()) },
        "style" => {
            let mut parts = value.split_whitespace();
            if let Some(color) = parts.next() {
                if color == "-" {
                    cosmetics.text_color_hex = None;
                } else {
                    if crate::models::parse_hex_color(color).is_none() {
                        return Err(BotError::user_message("color must look like `#ffcc00`"));
                    }
                    cosmetics.text_color_hex = Some(color.to_string());
                }
            }
            if let Some(outline) = parts.next() {
                cosmetics.outline_mode = crate::models::OutlineMode::parse(outline);
            }
        }
        _ => return Err(BotError::user_message("unknown cosmetic field")),
    }
    cards::update_cosmetics(&state.db, owner, card_uid, cosmetics.clone()).await?;
    Ok(cosmetics)
}

fn nav_row(owner: i64, page: i64, pages: i64) -> CreateActionRow {
    row(vec![
        button(format!("hub:{owner}:cards:{}", page.saturating_sub(1)), "Prev", ButtonStyle::Secondary)
            .disabled(page <= 0),
        button(format!("hub:{owner}:cards:{}", page + 1), "Next", ButtonStyle::Secondary)
            .disabled(page + 1 >= pages),
        button(format!("hub:{owner}:home"), "Menu", ButtonStyle::Primary),
    ])
}

fn select_row(owner: i64, page: i64, items: &[OwnedCard]) -> CreateActionRow {
    let options = items
        .iter()
        .map(|card| {
            CreateSelectMenuOption::new(
                ui::trunc(&format!("{} #{}", card.character_name, card.print_number), 100),
                card.card_uid.clone(),
            )
            .description(ui::trunc(&format!("{} · {}", card.series_name, card.card_uid), 100))
        })
        .collect();
    CreateActionRow::SelectMenu(
        CreateSelectMenu::new(
            format!("invsel:{owner}:{page}"),
            CreateSelectMenuKind::String { options },
        )
        .placeholder("Inspect a card"),
    )
}

fn pages(total: i64) -> i64 {
    ((total.max(1) + cards::PAGE_SIZE - 1) / cards::PAGE_SIZE).max(1)
}


