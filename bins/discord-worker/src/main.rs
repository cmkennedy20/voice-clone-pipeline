//! Tier 6 CPU-only `Discord` interaction & 1-on-1 `DM` worker for `voice-clone-pipeline`.

mod commands;
mod dm_relay;

use std::{process::ExitCode, time::Duration};

use framework::{
    config::{AppConfig, desktop_env::refresh_discord_from_studio_env},
    telemetry::init_telemetry,
};
use serenity::prelude::GatewayIntents;

async fn run() -> Result<(), commands::WorkerError> {
    let mut config = AppConfig::from_env()?;
    init_telemetry(&config.rust_log);

    if !refresh_discord_from_studio_env(&mut config) {
        tracing::info!(
            "Discord credentials are not configured yet; standing by until configured via Studio UI or setup-discord"
        );
        loop {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => return Ok(()),
                () = tokio::time::sleep(Duration::from_secs(3)) => {
                    if refresh_discord_from_studio_env(&mut config) {
                        tracing::info!("detected newly paired Discord credentials; connecting gateway");
                        break;
                    }
                }
            }
        }
    }

    let bot_token = config.discord_bot_token.expose_secret().clone();
    let state = commands::WorkerState::from_config(&config)?;
    let framework = commands::build_framework(state);
    let intents = GatewayIntents::non_privileged() | GatewayIntents::DIRECT_MESSAGES;

    let mut client = serenity::Client::builder(bot_token, intents)
        .framework(framework)
        .await?;

    let shard_manager = client.shard_manager.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            tracing::info!("received Ctrl-C; shutting down Discord gateway shards");
            shard_manager.shutdown_all().await;
        }
    });

    tracing::info!("discord-worker starting gateway client (slash commands + 1-on-1 DM relay)");
    client.start().await?;
    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    if let Err(err) = run().await {
        init_telemetry("info");
        tracing::error!(error = %err, "discord-worker terminated with fatal error");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
