mod commands;
mod components;
mod config;
mod db;
mod engine;
mod error;
mod ids;
mod images;
mod models;
mod prefix;
mod state;
mod ui;

use std::time::Duration;

use poise::serenity_prelude as serenity;
use state::AppState;

use crate::error::BotError;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("nooora=info,serenity=warn")),
        )
        .init();

    if let Err(err) = run().await {
        tracing::error!(error = %err, "nooora stopped");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), BotError> {
    let config = config::Config::from_env()?;
    let state = AppState::connect(config).await?;
    sqlx::migrate!("./migrations")
        .run(&state.db)
        .await
        .map_err(|err| BotError::user_message(format!("migration failed: {err}")))?;
    log_catalog(&state).await?;

    let setup_state = state.clone();
    let dev_guild = state.config.dev_guild_id;
    let register_global = state.config.register_global;
    let token = state.config.discord_token.clone();

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: commands::all(),
            on_error: |error| Box::pin(on_error(error)),
            command_check: Some(|ctx| {
                Box::pin(async move { commands::rate_check(ctx).await })
            }),
            event_handler: |ctx, event| Box::pin(on_event(ctx, event)),
            prefix_options: poise::PrefixFrameworkOptions {
                prefix: None,
                mention_as_prefix: false,
                case_insensitive_commands: true,
                stripped_dynamic_prefix: Some(prefix::strip),
                ..Default::default()
            },
            ..Default::default()
        })
        .setup(move |ctx, ready, framework| {
            let state = setup_state.clone();
            Box::pin(async move {
                state.set_bot_id(ready.user.id.get());
                tracing::info!(user = %ready.user.name, id = %ready.user.id, "logged in");
                if let Some(guild_id) = dev_guild {
                    poise::builtins::register_in_guild(
                        ctx,
                        &framework.options().commands,
                        serenity::GuildId::new(guild_id),
                    )
                    .await?;
                    tracing::info!(guild_id, "registered guild slash commands");
                }
                if register_global {
                    poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                    tracing::info!("registered global slash commands");
                }
                Ok(state)
            })
        })
        .build();

    let intents = serenity::GatewayIntents::GUILDS
        | serenity::GatewayIntents::GUILD_MESSAGES
        | serenity::GatewayIntents::MESSAGE_CONTENT;
    let mut client = serenity::ClientBuilder::new(token, intents)
        .framework(framework)
        .await?;
    let manager = client.shard_manager.clone();
    tokio::spawn(async move {
        shutdown_signal().await;
        tracing::info!("shutdown signal received");
        manager.shutdown_all().await;
    });
    client.start().await?;
    Ok(())
}

async fn log_catalog(state: &AppState) -> Result<(), BotError> {
    let templates: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM card_templates")
        .fetch_one(&state.db)
        .await?;
    let series: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM series")
        .fetch_one(&state.db)
        .await?;
    let cosmetics: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cosmetics")
        .fetch_one(&state.db)
        .await?;
    tracing::info!(templates, series, cosmetics, "catalog ready");
    if templates < 40_000 {
        tracing::warn!("card catalog is below the 40,000 template target");
    }
    if cosmetics < 12_000 {
        tracing::warn!("cosmetics registry is below the 12,000 target");
    }
    Ok(())
}

fn on_event<'a>(
    ctx: poise::FrameworkContext<'a, AppState, BotError>,
    event: &'a serenity::FullEvent,
) -> poise::BoxFuture<'a, Result<(), BotError>> {
    Box::pin(async move {
        if let serenity::FullEvent::InteractionCreate { interaction } = event {
            if let Err(err) = components::on_interaction(ctx, interaction).await {
                tracing::error!(error = %err, "interaction handler failed");
            }
        }
        Ok(())
    })
}

fn on_error(error: poise::FrameworkError<'_, AppState, BotError>) -> poise::BoxFuture<'_, ()> {
    Box::pin(async move {
        match error {
            poise::FrameworkError::Command { error, ctx, .. } => {
                let embed = if error.is_user_facing() {
                    error.user_embed()
                } else {
                    BotError::user_message("That command failed. The details are in the bot log.").user_embed()
                };
                if !error.is_user_facing() {
                    tracing::error!(error = %error, "command failed");
                }
                let _ = ctx.send(poise::CreateReply::default().embed(embed)).await;
            }
            poise::FrameworkError::CommandCheckFailed { error, ctx, .. } => {
                if let Some(error) = error {
                    let _ = ctx
                        .send(poise::CreateReply::default().embed(error.user_embed()))
                        .await;
                }
            }
            other => {
                tracing::warn!("framework error");
                let _ = other;
            }
        }
    })
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    let _ = Duration::from_millis(1);
}
