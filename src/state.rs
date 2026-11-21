use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use redis::AsyncCommands;
use moka::future::Cache;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tokio::sync::Mutex;

use crate::config::Config;
use crate::engine::renderer::Fonts;
use crate::error::BotError;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub redis: Option<redis::aio::ConnectionManager>,
    pub http: reqwest::Client,
    pub config: Arc<Config>,
    pub fonts: Fonts,
    pub images: Cache<String, Arc<Vec<u8>>>,
    pub drop_locks: Arc<DashMap<i64, Arc<Mutex<()>>>>,
    pub trade_locks: Arc<DashMap<String, Arc<Mutex<()>>>>,
    pub cup: Arc<Mutex<VecDeque<CupTicket>>>,
    pub rates: Arc<RateLimiter>,
    bot_id: Arc<AtomicU64>,
}

#[derive(Debug, Clone)]
pub struct CupTicket {
    pub user_id: i64,
    pub channel_id: u64,
    pub name: String,
}

pub struct RateLimiter {
    hits: DashMap<u64, VecDeque<Instant>>,
    limit: usize,
    window: Duration,
}

impl RateLimiter {
    pub fn new(limit: usize, window: Duration) -> Self {
        Self {
            hits: DashMap::new(),
            limit,
            window,
        }
    }

    pub fn check(&self, user_id: u64) -> Result<(), Duration> {
        let now = Instant::now();
        let mut entry = self.hits.entry(user_id).or_default();
        while entry.front().is_some_and(|at| now.duration_since(*at) > self.window) {
            entry.pop_front();
        }
        if entry.len() >= self.limit {
            let oldest = entry.front().copied().unwrap_or(now);
            let retry = self.window.saturating_sub(now.duration_since(oldest));
            return Err(retry);
        }
        entry.push_back(now);
        Ok(())
    }
}

impl AppState {
    pub async fn connect(config: Config) -> Result<Self, BotError> {
        let config = Arc::new(config);
        let db = PgPoolOptions::new()
            .max_connections(20)
            .acquire_timeout(Duration::from_secs(10))
            .after_connect(|conn, _meta| {
                Box::pin(async move {
                    sqlx::query("SET TIME ZONE 'UTC'")
                        .execute(&mut *conn)
                        .await?;
                    Ok(())
                })
            })
            .connect(&config.database_url)
            .await?;

        let redis = if let Some(url) = &config.redis_url {
            match redis::Client::open(url.as_str()) {
                Ok(client) => match client.get_connection_manager().await {
                    Ok(manager) => {
                        tracing::info!("redis connected");
                        Some(manager)
                    }
                    Err(err) => {
                        tracing::warn!(error = %err, "redis unavailable, using in-process locks");
                        None
                    }
                },
                Err(err) => {
                    tracing::warn!(error = %err, "redis url rejected, using in-process locks");
                    None
                }
            }
        } else {
            None
        };

        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(6))
            .build()
            .map_err(|err| BotError::user_message(format!("http client: {err}")))?;

        Ok(Self {
            db,
            redis,
            http,
            config,
            fonts: Fonts::load()?,
            images: Cache::builder()
                .max_capacity(512)
                .time_to_live(Duration::from_secs(60 * 60))
                .build(),
            drop_locks: Arc::new(DashMap::new()),
            trade_locks: Arc::new(DashMap::new()),
            cup: Arc::new(Mutex::new(VecDeque::new())),
            rates: Arc::new(RateLimiter::new(8, Duration::from_secs(10))),
            bot_id: Arc::new(AtomicU64::new(0)),
        })
    }

    pub fn set_bot_id(&self, id: u64) {
        self.bot_id.store(id, Ordering::Relaxed);
    }

    pub fn bot_id(&self) -> u64 {
        self.bot_id.load(Ordering::Relaxed)
    }

    pub fn hit_rate_limit(&self, user_id: u64) -> Result<(), BotError> {
        self.rates.check(user_id).map_err(|retry_after| BotError::RateLimited {
            retry_after,
        })
    }

    pub async fn lock_drop(&self, drop_id: i64) -> Arc<Mutex<()>> {
        self.drop_locks
            .entry(drop_id)
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    pub async fn lock_trade(&self, trade_id: &str) -> Arc<Mutex<()>> {
        self.trade_locks
            .entry(trade_id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    pub async fn redis_try_lock(&self, key: &str, ttl_secs: u64) -> bool {
        let Some(manager) = &self.redis else {
            return true;
        };
        let mut conn = manager.clone();
        let opts = redis::SetOptions::default()
            .conditional_set(redis::ExistenceCheck::NX)
            .with_expiration(redis::SetExpiry::EX(ttl_secs));
        match conn.set_options::<_, _, Option<String>>(key, "1", opts).await {
            Ok(Some(_)) => true,
            Ok(None) => false,
            Err(err) => {
                tracing::warn!(error = %err, "redis lock failed open");
                true
            }
        }
    }

    pub async fn redis_unlock(&self, key: &str) {
        let Some(manager) = &self.redis else {
            return;
        };
        let mut conn = manager.clone();
        let _: Result<(), _> = conn.del(key).await;
    }
}

pub type Context<'a> = poise::Context<'a, AppState, BotError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limiter_blocks_the_ninth_hit() {
        let limiter = RateLimiter::new(3, Duration::from_secs(30));
        assert!(limiter.check(1).is_ok());
        assert!(limiter.check(1).is_ok());
        assert!(limiter.check(1).is_ok());
        assert!(limiter.check(1).is_err());
        assert!(limiter.check(2).is_ok());
    }
}
