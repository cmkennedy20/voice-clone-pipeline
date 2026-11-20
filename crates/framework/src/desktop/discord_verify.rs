//! Live `Discord` `API v10` (`GET /api/v10/users/@me`) bot token validator for onboarding & CLI setup.

use reqwest::header::AUTHORIZATION;
use serde::{Deserialize, Serialize};

use super::DesktopError;

/// Canonical `Discord` `API` base origin.
pub const DISCORD_API_ORIGIN: &str = "https://discord.com";

/// Verified bot identity returned by `Discord` `GET /api/v10/users/@me`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifiedDiscordBot {
    pub id: String,
    pub username: String,
}

/// Validates `bot_token` against `Discord` `API v10` at `api_origin`.
pub async fn verify_discord_bot_token_at(
    http: &reqwest::Client,
    api_origin: &str,
    bot_token: &str,
) -> Result<VerifiedDiscordBot, DesktopError> {
    let clean_origin = api_origin.trim_end_matches('/');
    let clean_token = bot_token.trim();
    let resp = http
        .get(format!("{clean_origin}/api/v10/users/@me"))
        .header(AUTHORIZATION, format!("Bot {clean_token}"))
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let detail = resp.text().await.unwrap_or_default();
        return Err(DesktopError::DiscordVerificationFailed { status, detail });
    }
    Ok(resp.json::<VerifiedDiscordBot>().await?)
}

/// Validates `bot_token` against production `https://discord.com/api/v10/users/@me` using a bounded client.
pub async fn verify_discord_bot_token_default(
    bot_token: &str,
) -> Result<VerifiedDiscordBot, DesktopError> {
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;
    verify_discord_bot_token(&http, bot_token).await
}

/// Validates `bot_token` against production `https://discord.com/api/v10/users/@me`.
pub async fn verify_discord_bot_token(
    http: &reqwest::Client,
    bot_token: &str,
) -> Result<VerifiedDiscordBot, DesktopError> {
    verify_discord_bot_token_at(http, DISCORD_API_ORIGIN, bot_token).await
}
