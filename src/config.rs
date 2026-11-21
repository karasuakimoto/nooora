use crate::error::BotError;
use std::env;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub discord_token: String,
    pub redis_url: Option<String>,
    pub portal_url: String,
    pub drop_max: i32,
    pub grab_max: i32,
    pub drop_regen: Duration,
    pub grab_regen: Duration,
    pub series_boost: f64,
    pub event_boost: f64,
    pub trade_gate: i32,
    pub series_change_cooldown: Duration,
    pub dev_guild_id: Option<u64>,
    pub register_global: bool,
    pub drop_ttl: Duration,
    pub daily_gold: i64,
}

impl Config {
    pub fn from_env() -> Result<Self, BotError> {
        let _ = dotenvy::dotenv();
        let database_url = required("DATABASE_URL")?;
        let discord_token = required("DISCORD_TOKEN")?;
        let redis_url = env::var("REDIS_URL")
            .ok()
            .filter(|s| !s.trim().is_empty());
        let portal_url =
            env::var("NOOORA_PORTAL_URL").unwrap_or_else(|_| "https://nooora.example".to_string());
        Ok(Self {
            database_url,
            discord_token,
            redis_url,
            portal_url,
            drop_max: env_i32("NOOORA_DROP_MAX", 3).clamp(1, 10),
            grab_max: env_i32("NOOORA_GRAB_MAX", 1).clamp(1, 5),
            drop_regen: Duration::from_secs(env_u64("NOOORA_DROP_REGEN_SECS", 1800).max(1)),
            grab_regen: Duration::from_secs(env_u64("NOOORA_GRAB_REGEN_SECS", 900).max(1)),
            series_boost: env_f64("NOOORA_SERIES_BOOST", 3.0).clamp(1.0, 9.0),
            event_boost: env_f64("NOOORA_EVENT_BOOST", 2.0).clamp(1.0, 9.0),
            trade_gate: env_i32("NOOORA_TRADE_GATE", 100).max(0),
            series_change_cooldown: Duration::from_secs(
                env_u64("NOOORA_SERIES_COOLDOWN_SECS", 6 * 24 * 60 * 60).max(1),
            ),
            dev_guild_id: env::var("NOOORA_DEV_GUILD_ID")
                .ok()
                .filter(|s| !s.is_empty())
                .and_then(|s| s.parse().ok()),
            register_global: env::var("NOOORA_REGISTER_GLOBAL")
                .map(|v| v != "0" && !v.eq_ignore_ascii_case("false"))
                .unwrap_or(true),
            drop_ttl: Duration::from_secs(env_u64("NOOORA_DROP_TTL_SECS", 180).max(30)),
            daily_gold: 4,
        })
    }
}

fn required(key: &str) -> Result<String, BotError> {
    env::var(key).map_err(|_| {
        BotError::user_message(format!("missing environment variable {key}"))
    })
}

fn env_i32(key: &str, default: i32) -> i32 {
    env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_u64(key: &str, default: u64) -> u64 {
    env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_f64(key: &str, default: f64) -> f64 {
    env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}
