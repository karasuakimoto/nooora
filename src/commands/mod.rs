pub mod admin;
pub mod collection;
pub mod drop;
pub mod economy;
pub mod hub;
pub mod quests;

use crate::error::BotError;
use crate::state::{AppState, Context};

pub fn all() -> Vec<poise::Command<AppState, BotError>> {
    vec![
        hub::nooora(),
        drop::drop(),
        drop::cooldown(),
        collection::cards(),
        quests::quest(),
        economy::trade(),
        economy::gift(),
        economy::market(),
        admin::series(),
        admin::admin(),
    ]
}

pub async fn rate_check(ctx: Context<'_>) -> Result<bool, BotError> {
    ctx.data().hit_rate_limit(ctx.author().id.get())?;
    Ok(true)
}
