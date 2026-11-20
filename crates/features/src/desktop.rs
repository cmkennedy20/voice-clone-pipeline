//! Transport-agnostic workflows for agent-first discovery (`get_agent_manifest`), desktop
//! hardware/setup inspection (`get_desktop_system_status`), and live `Discord` pairing (`save_discord_config`).

use std::collections::BTreeMap;

use domain::{
    hardware::{HardwarePlan, SystemSpecs, recommend_hardware_plan},
    translation::types::TargetLanguage,
};
use framework::{
    config::{MIN_REAL_DISCORD_SNOWFLAKE, discord_oauth_urls_for},
    context::AppContext,
    desktop::{
        discord_verify::verify_discord_bot_token_default,
        dotenv::{read_dotenv_file, upsert_dotenv_file},
        probe::probe_system_specs,
    },
};
use serde::{Deserialize, Serialize};

use crate::error::AppError;

/// Single CLI command descriptor advertised to AI agents via `GET /api/v1/agent/manifest`.
#[derive(Debug, Clone, Serialize)]
pub struct AgentCliCommandSpec {
    pub command: &'static str,
    pub summary: &'static str,
}

/// Unauthenticated agent-first capability & onboarding manifest.
#[derive(Debug, Clone, Serialize)]
pub struct AgentManifestResponse {
    pub service_name: &'static str,
    pub version: &'static str,
    pub standalone_desktop_mode: bool,
    pub discord_configured: bool,
    pub cli_binary: &'static str,
    pub supported_languages: Vec<String>,
    pub cli_commands: Vec<AgentCliCommandSpec>,
    pub onboarding_endpoints: [&'static str; 3],
}

/// Authenticated desktop hardware, `Discord` onboarding, and `.env` persistence status.
#[derive(Debug, Clone, Serialize)]
pub struct DesktopSystemStatusResponse {
    pub standalone_desktop_mode: bool,
    pub discord_configured: bool,
    pub discord_application_id_str: String,
    pub discord_owner_user_id_str: String,
    pub user_install_url: Option<String>,
    pub bot_dm_url: Option<String>,
    pub studio_env_writable: bool,
    pub hardware_specs: SystemSpecs,
    pub hardware_plan: HardwarePlan,
}

/// Request payload for saving `Discord` credentials from Studio UI or an HTTP agent.
#[derive(Debug, Clone, Deserialize)]
pub struct SaveDiscordConfigRequest {
    pub discord_application_id: String,
    pub discord_bot_token: String,
    pub discord_owner_user_id: String,
    pub verify_online: Option<bool>,
}

/// Receipt returned after persisting `Discord` credentials into `studio.env`.
#[derive(Debug, Clone, Serialize)]
pub struct SaveDiscordConfigResponse {
    pub saved: bool,
    pub verified_bot_username: Option<String>,
    pub user_install_url: String,
    pub bot_dm_url: String,
}

fn parse_snowflake_field(raw: &str, field: &str) -> Result<u64, AppError> {
    let trimmed = raw.trim();
    let val = trimmed.parse::<u64>().map_err(|_| {
        AppError::Validation(format!(
            "`{field}` must be a 17-20 digit numeric Discord snowflake"
        ))
    })?;
    if val < MIN_REAL_DISCORD_SNOWFLAKE {
        return Err(AppError::Validation(format!(
            "`{field}` must be a real 17-20 digit Discord snowflake"
        )));
    }
    Ok(val)
}

/// Builds the unauthenticated agent capability manifest for `ctx`.
#[must_use]
pub fn get_agent_manifest(ctx: &AppContext) -> AgentManifestResponse {
    let langs = TargetLanguage::all_metadata()
        .into_iter()
        .map(|m| m.code)
        .collect();
    let cli_commands = vec![
        AgentCliCommandSpec {
            command: "make setup ARGS=\"--non-interactive --json\"",
            summary: "Probe WSL2/Linux GPUs, VRAM, RAM, and Windows LAN IP, then write hardware-matched .env.",
        },
        AgentCliCommandSpec {
            command: "make pull-models",
            summary: "Resumably download tier-matched Whisper, Silero VAD, Qwen3, XTTS-v2, and CosyVoice weights.",
        },
        AgentCliCommandSpec {
            command: "make up",
            summary: "Build Docker containers locally, run SQLx migrations, and start Web Studio UI + API.",
        },
        AgentCliCommandSpec {
            command: "make doctor ARGS=\"--json\"",
            summary: "Verify WSL2 Docker integration, GPU pass-through, model weights, and /health/ready.",
        },
        AgentCliCommandSpec {
            command: "make setup-discord",
            summary: "Configure Discord 1-on-1 bot credentials in .env and hot-reload discord-worker.",
        },
        AgentCliCommandSpec {
            command: "make verify",
            summary: "Run full Rust fmt, Clippy, SQLx nextest, cargo-deny, Python pytest, and Web Vitest suites.",
        },
    ];

    AgentManifestResponse {
        service_name: "voice-clone-pipeline",
        version: env!("CARGO_PKG_VERSION"),
        standalone_desktop_mode: ctx.config.desktop.standalone_mode,
        discord_configured: ctx.config.is_discord_configured(),
        cli_binary: "make",
        supported_languages: langs,
        cli_commands,
        onboarding_endpoints: [
            "/api/v1/agent/manifest",
            "/api/v1/system/status",
            "/api/v1/system/discord-config",
        ],
    }
}

/// Probes host hardware and reads `studio.env` onboarding status for the Studio UI and agents.
pub async fn get_desktop_system_status(
    ctx: &AppContext,
) -> Result<DesktopSystemStatusResponse, AppError> {
    let storage_dir = ctx.config.storage_dir.clone();
    let env_file_opt = ctx.config.desktop.studio_env_path.clone();

    let (specs, env_overrides) = tokio::task::spawn_blocking(move || {
        let probed = probe_system_specs(&storage_dir);
        let overrides = env_file_opt
            .as_deref()
            .and_then(|p| read_dotenv_file(p).ok())
            .unwrap_or_default();
        (probed, overrides)
    })
    .await
    .map_err(|err| AppError::Internal(format!("hardware probe task failed: {err}")))?;

    let plan = recommend_hardware_plan(&specs, None, None);

    let effective_app_id = env_overrides
        .get("DISCORD_APPLICATION_ID")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(ctx.config.discord_application_id);
    let effective_owner_id = env_overrides
        .get("DISCORD_OWNER_USER_ID")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(ctx.config.discord_owner_user_id);
    let has_env_bot_token = env_overrides.get("DISCORD_BOT_TOKEN").is_some_and(|t| {
        let trimmed = t.trim();
        !trimmed.is_empty()
            && trimmed != "unconfigured"
            && !trimmed.starts_with("replace-with-")
            && !trimmed.starts_with("dev-discord-bot-token")
    });

    let discord_configured = ctx.config.is_discord_configured()
        || (has_env_bot_token && effective_owner_id >= MIN_REAL_DISCORD_SNOWFLAKE);
    let oauth_pair = discord_oauth_urls_for(effective_app_id);

    Ok(DesktopSystemStatusResponse {
        standalone_desktop_mode: ctx.config.desktop.standalone_mode,
        discord_configured,
        discord_application_id_str: effective_app_id.to_string(),
        discord_owner_user_id_str: effective_owner_id.to_string(),
        user_install_url: oauth_pair.as_ref().map(|(u, _)| u.clone()),
        bot_dm_url: oauth_pair.map(|(_, b)| b),
        studio_env_writable: ctx.config.desktop.studio_env_path.is_some(),
        hardware_specs: specs,
        hardware_plan: plan,
    })
}

/// Validates and atomically writes `Discord` bot credentials into `studio.env`.
pub async fn save_discord_config(
    ctx: &AppContext,
    req: SaveDiscordConfigRequest,
) -> Result<SaveDiscordConfigResponse, AppError> {
    let Some(env_path) = ctx.config.desktop.studio_env_path.clone() else {
        return Err(AppError::Validation(
            "STUDIO_ENV_PATH is not configured on this server instance".to_owned(),
        ));
    };

    let app_id = parse_snowflake_field(&req.discord_application_id, "discord_application_id")?;
    let owner_id = parse_snowflake_field(&req.discord_owner_user_id, "discord_owner_user_id")?;
    let bot_token = req.discord_bot_token.trim().to_owned();
    if bot_token.is_empty() || bot_token == "unconfigured" || bot_token.starts_with("replace-with-")
    {
        return Err(AppError::Validation(
            "`discord_bot_token` cannot be empty or a placeholder".to_owned(),
        ));
    }

    let verified_bot_username = if req.verify_online.unwrap_or(false) {
        let verified = verify_discord_bot_token_default(&bot_token)
            .await
            .map_err(|err| AppError::Validation(err.to_string()))?;
        Some(verified.username)
    } else {
        None
    };

    let updates = BTreeMap::from([
        ("DISCORD_APPLICATION_ID".to_owned(), app_id.to_string()),
        ("DISCORD_BOT_TOKEN".to_owned(), bot_token),
        ("DISCORD_OWNER_USER_ID".to_owned(), owner_id.to_string()),
    ]);

    tokio::task::spawn_blocking(move || upsert_dotenv_file(&env_path, &updates))
        .await
        .map_err(|err| AppError::Internal(format!("dotenv write task panicked: {err}")))?
        .map_err(|err| AppError::Internal(format!("failed saving studio.env: {err}")))?;

    let (user_install_url, bot_dm_url) = discord_oauth_urls_for(app_id).ok_or_else(|| {
        AppError::Validation("failed deriving Discord OAuth2 install links".to_owned())
    })?;

    Ok(SaveDiscordConfigResponse {
        saved: true,
        verified_bot_username,
        user_install_url,
        bot_dm_url,
    })
}
