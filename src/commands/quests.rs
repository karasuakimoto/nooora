use poise::serenity_prelude::ButtonStyle;

use crate::db::quests::{self, QuestView};
use crate::error::BotError;
use crate::ids::snowflake_i64;
use crate::state::{AppState, Context};
use crate::ui::{self, button, chunk_buttons};

/// Daily Gold, quest progress, and cosmetic boxes.
#[poise::command(prefix_command, slash_command, rename = "quest")]
pub async fn quest(ctx: Context<'_>) -> Result<(), BotError> {
    let owner = snowflake_i64(ctx.author().id.get())?;
    let reply = board_reply(ctx.data(), owner).await?;
    ctx.send(reply.into_reply()).await?;
    Ok(())
}

pub async fn board_reply(state: &AppState, owner: i64) -> Result<crate::ui::View, BotError> {
    crate::db::users::ensure_user(&state.db, owner).await?;
    let quests = quests::board(&state.db, owner).await?;
    let body = if quests.is_empty() {
        "No quests are configured.".to_string()
    } else {
        quests.iter().map(format_quest).collect::<Vec<_>>().join("\n")
    };
    let mut buttons = vec![button(
        format!("quest:{owner}:daily"),
        "Claim daily +4",
        ButtonStyle::Success,
    )];
    for quest in quests.iter().filter(|quest| quest.progress >= quest.target && !quest.claimed) {
        buttons.push(button(
            format!("quest:{owner}:claim:{}", quest.quest_id),
            ui::trunc(&format!("Claim {}", quest.title), 80),
            ButtonStyle::Success,
        ));
    }
    buttons.push(button(format!("hub:{owner}:home"), "Menu", ButtonStyle::Secondary));
    Ok(ui::View::new(ui::embed("Quests", body)).components(chunk_buttons(buttons)))
}

fn format_quest(quest: &QuestView) -> String {
    let mark = if quest.claimed {
        "claimed"
    } else if quest.progress >= quest.target {
        "ready"
    } else {
        "open"
    };
    let reward = if quest.reward_box {
        format!("{} Gold + box", quest.reward_gold)
    } else {
        format!("{} Gold", quest.reward_gold)
    };
    format!(
        "**{}** ({}) {}/{} · {reward} · {mark}",
        quest.title, quest.period, quest.progress.min(quest.target), quest.target
    )
}

pub async fn claim_daily(state: &AppState, owner: i64) -> Result<String, BotError> {
    let gold = quests::claim_daily(&state.db, owner, state.config.daily_gold).await?;
    Ok(format!("Daily claim added {} Gold. Balance: **{gold}**.", state.config.daily_gold))
}

pub async fn claim_one(state: &AppState, owner: i64, quest_id: &str) -> Result<String, BotError> {
    let reward = quests::claim_quest(&state.db, owner, quest_id).await?;
    let mut text = format!("Quest claimed. Balance: **{}** Gold.", reward.gold);
    if let Some(cosmetic) = reward.cosmetic {
        text.push_str(&format!(
            "\nBox rolled **{}** (`{}`, {}, {}).",
            cosmetic.name, cosmetic.cosmetic_id, cosmetic.kind, cosmetic.rarity
        ));
    }
    Ok(text)
}


