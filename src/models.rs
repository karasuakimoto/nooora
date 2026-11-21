use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RarityTier {
    Standard,
    Rare,
    Shiny,
    #[serde(rename = "3d")]
    ThreeD,
    Event,
}

impl RarityTier {
    pub fn parse(raw: &str) -> Self {
        match raw {
            "rare" => Self::Rare,
            "shiny" => Self::Shiny,
            "3d" => Self::ThreeD,
            "event" => Self::Event,
            _ => Self::Standard,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Standard => "Standard",
            Self::Rare => "Rare",
            Self::Shiny => "Shiny",
            Self::ThreeD => "3D",
            Self::Event => "Event",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardVariant {
    Normal,
    Skin,
    Shiny,
    #[serde(rename = "3d")]
    ThreeD,
    Event,
}

impl CardVariant {
    pub fn from_rarity(rarity: RarityTier) -> Self {
        match rarity {
            RarityTier::Shiny => Self::Shiny,
            RarityTier::ThreeD => Self::ThreeD,
            RarityTier::Event => Self::Event,
            _ => Self::Normal,
        }
    }

    pub fn parse(raw: &str) -> Self {
        match raw {
            "skin" => Self::Skin,
            "shiny" => Self::Shiny,
            "3d" => Self::ThreeD,
            "event" => Self::Event,
            _ => Self::Normal,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Skin => "Skin",
            Self::Shiny => "Shiny",
            Self::ThreeD => "3D",
            Self::Event => "Event",
        }
    }

    pub fn db(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Skin => "skin",
            Self::Shiny => "shiny",
            Self::ThreeD => "3d",
            Self::Event => "event",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OutlineMode {
    #[default]
    None,
    Black,
    White,
}

impl OutlineMode {
    pub fn parse(raw: &str) -> Self {
        match raw.to_ascii_lowercase().as_str() {
            "black" => Self::Black,
            "white" => Self::White,
            _ => Self::None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Black => "Black",
            Self::White => "White",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Cosmetics {
    #[serde(default)]
    pub frame_id: Option<String>,
    #[serde(default)]
    pub filter_id: Option<String>,
    #[serde(default)]
    pub effect_3d_id: Option<String>,
    #[serde(default)]
    pub font_family: Option<String>,
    #[serde(default)]
    pub text_color_hex: Option<String>,
    #[serde(default)]
    pub outline_mode: OutlineMode,
}

impl Default for Cosmetics {
    fn default() -> Self {
        Self {
            frame_id: None,
            filter_id: None,
            effect_3d_id: None,
            font_family: None,
            text_color_hex: None,
            outline_mode: OutlineMode::Black,
        }
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct UserRow {
    #[allow(dead_code)]
    pub user_id: i64,
    pub gold: i64,
    pub total_drops: i32,
    pub drop_charges: i16,
    pub grab_charges: i16,
    pub drop_next_regen: Option<DateTime<Utc>>,
    pub grab_next_regen: Option<DateTime<Utc>>,
    pub last_daily_claim: Option<NaiveDate>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TemplateRow {
    pub template_id: i64,
    pub character_name: String,
    pub series_id: String,
    pub series_name: String,
    pub base_image_url: Option<String>,
    pub rarity_tier: String,
    #[allow(dead_code)]
    pub weight: i32,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct OwnedCard {
    pub card_uid: String,
    pub owner_id: i64,
    #[allow(dead_code)]
    pub template_id: i64,
    pub print_number: i32,
    pub variant: String,
    pub cosmetics: sqlx::types::Json<Cosmetics>,
    pub hold_kind: Option<String>,
    pub hold_ref: Option<String>,
    pub character_name: String,
    pub series_id: String,
    pub series_name: String,
    pub base_image_url: Option<String>,
    pub rarity_tier: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SeriesHit {
    pub series_id: String,
    pub series_name: String,
    pub score: f64,
}

#[derive(Debug, Clone)]
pub struct AuditEvent {
    pub tx_id: Uuid,
    pub kind: &'static str,
    pub sender_id: Option<i64>,
    pub receiver_id: Option<i64>,
    pub cards: Vec<String>,
    pub gold_delta: i64,
    pub metadata: serde_json::Value,
}

pub fn parse_hex_color(raw: &str) -> Option<[u8; 3]> {
    let hex = raw.trim().trim_start_matches('#');
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some([r, g, b])
}

pub fn valid_series_id(raw: &str) -> bool {
    raw.len() == 6 && raw.chars().all(|c| c.is_ascii_alphanumeric())
}

pub fn discord_rel(ts: DateTime<Utc>) -> String {
    format!("<t:{}:R>", ts.timestamp())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_and_series_ids() {
        assert_eq!(parse_hex_color("#ff00aa"), Some([255, 0, 170]));
        assert!(parse_hex_color("gg0000").is_none());
        assert!(valid_series_id("086c62"));
        assert!(!valid_series_id("086c6"));
        assert!(!valid_series_id("086c6!"));
    }
}
