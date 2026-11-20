//! Desktop launcher and standalone runtime options parsed from `AppConfig`'s environment map.

use std::{collections::BTreeMap, path::PathBuf};

use common::secret::Secret;

use super::{optional_non_empty, parse_bool_var};

/// Default local proxy/desktop bearer fallback when `OWNER_API_BEARER_TOKEN` is omitted in standalone dev.
pub const DEFAULT_DESKTOP_BEARER_SENTINEL: &str = "dev-owner-bearer-token-change-me";

/// Minimum 17-digit `Discord` snowflake threshold distinguishing real IDs from placeholders.
pub const MIN_REAL_DISCORD_SNOWFLAKE: u64 = 10_000_000_000_000_000;

/// Computes `(user_install_url, bot_dm_url)` when `app_id` is a valid 17-20 digit `Discord` snowflake.
#[must_use]
pub fn discord_oauth_urls_for(app_id: u64) -> Option<(String, String)> {
    if app_id < MIN_REAL_DISCORD_SNOWFLAKE {
        return None;
    }
    let user_install = format!(
        "https://discord.com/oauth2/authorize?client_id={app_id}&integration_type=1&scope=applications.commands"
    );
    let bot_dm = format!(
        "https://discord.com/oauth2/authorize?client_id={app_id}&integration_type=0&scope=bot+applications.commands&permissions=274877910016"
    );
    Some((user_install, bot_dm))
}

/// Typed host environment settings consumed by `voice-clone-studio` (`desktop-launcher`).
#[derive(Debug, Clone, Default)]
pub struct DesktopLauncherEnv {
    pub portable_flag: bool,
    pub home_override: Option<PathBuf>,
    pub local_appdata: Option<PathBuf>,
    pub xdg_data_home: Option<PathBuf>,
    pub user_home: Option<PathBuf>,
    pub system_path: String,
    pub hf_token: Option<Secret<String>>,
}

impl DesktopLauncherEnv {
    /// Collects host environment variables via `super::collect_process_vars` and parses launcher paths.
    #[must_use]
    pub fn from_env() -> Self {
        let vars = super::collect_process_vars();
        Self::from_map(&vars)
    }

    /// Pure deterministic parser over an explicit environment map.
    #[must_use]
    pub fn from_map(vars: &BTreeMap<String, String>) -> Self {
        let portable_flag = parse_bool_var(vars, "VOICE_STUDIO_PORTABLE", false);
        let home_override = optional_non_empty(vars, "VOICE_STUDIO_HOME").map(PathBuf::from);
        let local_appdata = optional_non_empty(vars, "LOCALAPPDATA").map(PathBuf::from);
        let xdg_data_home = optional_non_empty(vars, "XDG_DATA_HOME").map(PathBuf::from);
        let user_home = optional_non_empty(vars, "USERPROFILE")
            .or_else(|| optional_non_empty(vars, "HOME"))
            .map(PathBuf::from);
        let system_path = optional_non_empty(vars, "PATH")
            .or_else(|| optional_non_empty(vars, "Path"))
            .unwrap_or("")
            .to_owned();
        let hf_token = optional_non_empty(vars, "HF_TOKEN").map(|t| Secret::new(t.to_owned()));

        Self {
            portable_flag,
            home_override,
            local_appdata,
            xdg_data_home,
            user_home,
            system_path,
            hf_token,
        }
    }
}

/// Self-contained desktop runtime options (static `SPA` hosting, auto-migration, `studio.env` path).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopRuntimeOptions {
    pub standalone_mode: bool,
    pub auto_migrate: bool,
    pub web_dist_dir: Option<PathBuf>,
    pub studio_env_path: Option<PathBuf>,
}

impl Default for DesktopRuntimeOptions {
    fn default() -> Self {
        Self {
            standalone_mode: false,
            auto_migrate: true,
            web_dist_dir: None,
            studio_env_path: None,
        }
    }
}

pub(super) fn parse_desktop_options(vars: &BTreeMap<String, String>) -> DesktopRuntimeOptions {
    DesktopRuntimeOptions {
        standalone_mode: parse_bool_var(vars, "DESKTOP_STANDALONE_MODE", false),
        auto_migrate: parse_bool_var(vars, "AUTO_MIGRATE", true),
        web_dist_dir: optional_non_empty(vars, "WEB_DIST_DIR").map(PathBuf::from),
        studio_env_path: optional_non_empty(vars, "STUDIO_ENV_PATH").map(PathBuf::from),
    }
}

/// Reloads `DISCORD_*` fields onto `config` when `config.desktop.studio_env_path` contains saved overrides.
pub fn refresh_discord_from_studio_env(config: &mut super::AppConfig) -> bool {
    let Some(path) = config.desktop.studio_env_path.as_deref() else {
        return config.is_discord_configured();
    };
    let Ok(overrides) = crate::desktop::dotenv::read_dotenv_file(path) else {
        return config.is_discord_configured();
    };
    if let Some(app_id) = overrides
        .get("DISCORD_APPLICATION_ID")
        .and_then(|v| v.parse::<u64>().ok())
    {
        config.discord_application_id = app_id;
    }
    if let Some(owner_id) = overrides
        .get("DISCORD_OWNER_USER_ID")
        .and_then(|v| v.parse::<u64>().ok())
    {
        config.discord_owner_user_id = owner_id;
    }
    if let Some(token) = overrides.get("DISCORD_BOT_TOKEN")
        && !token.trim().is_empty()
    {
        config.discord_bot_token = Secret::new(token.trim().to_owned());
    }
    config.is_discord_configured()
}
