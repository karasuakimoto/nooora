use chrono::{DateTime, Utc};
use poise::serenity_prelude::{Colour, CreateEmbed};
use std::time::Duration;

pub const EMBED_COLOR: u32 = 0x8B5C_F6;

#[derive(Debug, thiserror::Error)]
pub enum BotError {
    #[error(transparent)]
    Db(#[from] sqlx::Error),
    #[error(transparent)]
    Discord(#[from] poise::serenity_prelude::Error),
    #[error(transparent)]
    Render(#[from] image::ImageError),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    #[error("{0}")]
    User(String),
    #[error("trade gate {have}/{need}")]
    TradeGate { have: i32, need: i32 },
    #[error("{kind} charge empty until {ready_at}")]
    Charge {
        kind: &'static str,
        ready_at: DateTime<Utc>,
    },
    #[error("rate limited")]
    RateLimited { retry_after: Duration },
    #[error("this menu belongs to someone else")]
    NotYourMenu,
    #[error("that drop slot was already taken")]
    SlotTaken,
    #[error("that drop has expired")]
    DropExpired,
}

impl BotError {
    pub fn user_message(msg: impl Into<String>) -> Self {
        Self::User(msg.into())
    }

    pub fn is_user_facing(&self) -> bool {
        matches!(
            self,
            Self::User(_)
                | Self::TradeGate { .. }
                | Self::Charge { .. }
                | Self::RateLimited { .. }
                | Self::NotYourMenu
                | Self::SlotTaken
                | Self::DropExpired
        )
    }

    pub fn user_text(&self) -> String {
        match self {
            Self::TradeGate { have, need } => format!(
                "Economy actions unlock at **{need}** drops. Progress: **{have}/{need}**."
            ),
            Self::Charge { kind, ready_at } => format!(
                "No {kind} charges left. Next charge <t:{}:R>.",
                ready_at.timestamp()
            ),
            Self::RateLimited { retry_after } => {
                format!(
                    "Slow down. Try again in {}s.",
                    retry_after.as_secs().max(1)
                )
            }
            Self::User(msg) => msg.clone(),
            Self::NotYourMenu => "This menu belongs to someone else.".to_string(),
            Self::SlotTaken => "Someone else already grabbed that card.".to_string(),
            Self::DropExpired => "This drop has expired.".to_string(),
            other => format!("Something broke while handling that. ({other})"),
        }
    }

    pub fn user_embed(&self) -> CreateEmbed {
        let title = match self {
            Self::TradeGate { .. } => "Drops required",
            Self::Charge { .. } => "Recharging",
            Self::RateLimited { .. } => "Slow down",
            Self::NotYourMenu => "Not your menu",
            Self::SlotTaken => "Too late",
            Self::DropExpired => "Drop closed",
            Self::User(_) => "Nooora",
            _ => "Error",
        };
        CreateEmbed::new()
            .title(title)
            .description(self.user_text())
            .color(Colour::new(EMBED_COLOR))
    }
}

pub fn map_sql(err: sqlx::Error) -> BotError {
    if let sqlx::Error::Database(db) = &err {
        let message = db.message();
        if let Some(rest) = message.strip_prefix("trade_gate:") {
            let mut parts = rest.split('/');
            let have = parts.next().unwrap_or("0").trim().parse().unwrap_or(0);
            let need = parts.next().unwrap_or("100").trim().parse().unwrap_or(100);
            return BotError::TradeGate { have, need };
        }
        if db.constraint() == Some("users_gold_nonnegative")
            || message.contains("gold") && message.contains("check")
        {
            return BotError::user_message("That transfer would make a Gold balance negative.");
        }
    }
    BotError::Db(err)
}
