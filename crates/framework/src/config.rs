//! Typed application configuration parsed exclusively from `std::env::vars()`.

pub mod desktop_env;

use std::{collections::BTreeMap, path::PathBuf};

use common::{basis_points::BasisPoints, secret::Secret};

pub use self::desktop_env::{
    DEFAULT_DESKTOP_BEARER_SENTINEL, DesktopLauncherEnv, DesktopRuntimeOptions,
    MIN_REAL_DISCORD_SNOWFLAKE, discord_oauth_urls_for,
};

pub(crate) fn collect_process_vars() -> BTreeMap<String, String> {
    std::env::vars().collect()
}

/// Configuration parsing and validation failures.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("missing required environment variable `{key}`")]
    MissingRequired { key: &'static str },
    #[error("environment variable `{key}` cannot be empty")]
    EmptyRequired { key: &'static str },
    #[error("invalid integer in environment variable `{key}`: `{value}`")]
    InvalidInteger { key: &'static str, value: String },
    #[error("environment variable `{key}` value `{value}` is out of range ({expected})")]
    OutOfRange {
        key: &'static str,
        value: String,
        expected: &'static str,
    },
}

/// Validated process configuration for `api-server`, `discord-worker`, `cli`, and `voice-clone-studio`.
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub rust_log: String,
    pub api_bind_addr: String,
    pub database_url: Secret<String>,
    pub owner_api_bearer_token: Secret<String>,
    pub discord_bot_token: Secret<String>,
    pub discord_application_id: u64,
    pub discord_owner_user_id: u64,
    pub whisper_model_path: PathBuf,
    pub whisper_use_gpu: bool,
    pub silero_vad_model_path: Option<PathBuf>,
    pub translator_base_url: String,
    pub tts_sidecar_url: String,
    pub api_server_internal_url: String,
    pub storage_dir: PathBuf,
    pub max_cer_bps: BasisPoints,
    pub max_tts_attempts: u8,
    pub desktop: DesktopRuntimeOptions,
}

fn require_non_empty<'a>(
    vars: &'a BTreeMap<String, String>,
    key: &'static str,
) -> Result<&'a str, ConfigError> {
    let raw = vars
        .get(key)
        .ok_or(ConfigError::MissingRequired { key })?
        .trim();
    if raw.is_empty() {
        return Err(ConfigError::EmptyRequired { key });
    }
    Ok(raw)
}

pub(crate) fn optional_non_empty<'a>(
    vars: &'a BTreeMap<String, String>,
    key: &'static str,
) -> Option<&'a str> {
    vars.get(key).map(|v| v.trim()).filter(|v| !v.is_empty())
}

pub(crate) fn parse_bool_var(
    vars: &BTreeMap<String, String>,
    key: &'static str,
    default_val: bool,
) -> bool {
    let Some(raw) = optional_non_empty(vars, key) else {
        return default_val;
    };
    matches!(
        raw.to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn parse_u64_var(raw: &str, key: &'static str) -> Result<u64, ConfigError> {
    raw.parse::<u64>().map_err(|_| ConfigError::InvalidInteger {
        key,
        value: raw.to_owned(),
    })
}

fn parse_max_cer_bps(vars: &BTreeMap<String, String>) -> Result<BasisPoints, ConfigError> {
    const KEY: &str = "MAX_CER_BPS";
    let Some(raw) = optional_non_empty(vars, KEY) else {
        return Ok(BasisPoints::from_raw(1_200));
    };
    let parsed = raw
        .parse::<u32>()
        .map_err(|_| ConfigError::InvalidInteger {
            key: KEY,
            value: raw.to_owned(),
        })?;
    if parsed > BasisPoints::MAX_PROPORTION.value() {
        return Err(ConfigError::OutOfRange {
            key: KEY,
            value: raw.to_owned(),
            expected: "0..=10000",
        });
    }
    Ok(BasisPoints::from_raw(parsed))
}

fn parse_max_tts_attempts(vars: &BTreeMap<String, String>) -> Result<u8, ConfigError> {
    const KEY: &str = "MAX_TTS_ATTEMPTS";
    let Some(raw) = optional_non_empty(vars, KEY) else {
        return Ok(3);
    };
    let parsed = raw.parse::<u8>().map_err(|_| ConfigError::InvalidInteger {
        key: KEY,
        value: raw.to_owned(),
    })?;
    if !(1..=10).contains(&parsed) {
        return Err(ConfigError::OutOfRange {
            key: KEY,
            value: raw.to_owned(),
            expected: "1..=10",
        });
    }
    Ok(parsed)
}

impl AppConfig {
    /// Collects process environment pairs once via `std::env::vars()` and delegates to [`Self::from_map`].
    pub fn from_env() -> Result<Self, ConfigError> {
        let vars = collect_process_vars();
        Self::from_map(&vars)
    }

    /// Reads only `DATABASE_URL` via `std::env::vars()` for isolated test pool bootstrap.
    pub fn database_url_from_env() -> Result<Secret<String>, ConfigError> {
        let vars = collect_process_vars();
        Ok(Secret::new(
            require_non_empty(&vars, "DATABASE_URL")?.to_owned(),
        ))
    }

    /// Pure deterministic configuration parser over an explicit key-value map.
    pub fn from_map(vars: &BTreeMap<String, String>) -> Result<Self, ConfigError> {
        let desktop = desktop_env::parse_desktop_options(vars);
        let database_url = Secret::new(require_non_empty(vars, "DATABASE_URL")?.to_owned());

        let owner_api_bearer_token = if desktop.standalone_mode {
            Secret::new(
                optional_non_empty(vars, "OWNER_API_BEARER_TOKEN")
                    .unwrap_or(DEFAULT_DESKTOP_BEARER_SENTINEL)
                    .to_owned(),
            )
        } else {
            Secret::new(require_non_empty(vars, "OWNER_API_BEARER_TOKEN")?.to_owned())
        };

        let discord_bot_token = if desktop.standalone_mode {
            Secret::new(
                optional_non_empty(vars, "DISCORD_BOT_TOKEN")
                    .unwrap_or("unconfigured")
                    .to_owned(),
            )
        } else {
            Secret::new(require_non_empty(vars, "DISCORD_BOT_TOKEN")?.to_owned())
        };

        let discord_owner_user_id = if desktop.standalone_mode {
            match optional_non_empty(vars, "DISCORD_OWNER_USER_ID") {
                Some(raw) => parse_u64_var(raw, "DISCORD_OWNER_USER_ID")?,
                None => 1,
            }
        } else {
            parse_u64_var(
                require_non_empty(vars, "DISCORD_OWNER_USER_ID")?,
                "DISCORD_OWNER_USER_ID",
            )?
        };

        let discord_application_id = match optional_non_empty(vars, "DISCORD_APPLICATION_ID") {
            Some(raw) => parse_u64_var(raw, "DISCORD_APPLICATION_ID")?,
            None => 0,
        };

        let default_bind = if desktop.standalone_mode {
            "127.0.0.1:8080"
        } else {
            "0.0.0.0:8080"
        };

        Ok(Self {
            rust_log: optional_non_empty(vars, "RUST_LOG")
                .unwrap_or("info")
                .to_owned(),
            api_bind_addr: optional_non_empty(vars, "API_BIND_ADDR")
                .unwrap_or(default_bind)
                .to_owned(),
            database_url,
            owner_api_bearer_token,
            discord_bot_token,
            discord_application_id,
            discord_owner_user_id,
            whisper_model_path: PathBuf::from(
                optional_non_empty(vars, "WHISPER_MODEL_PATH")
                    .unwrap_or("/models/whisper/ggml-large-v3.bin"),
            ),
            whisper_use_gpu: parse_bool_var(vars, "WHISPER_USE_GPU", true),
            silero_vad_model_path: optional_non_empty(vars, "SILERO_VAD_MODEL_PATH")
                .map(PathBuf::from),
            translator_base_url: optional_non_empty(vars, "TRANSLATOR_BASE_URL")
                .unwrap_or("http://qwen-translator:8081")
                .to_owned(),
            tts_sidecar_url: optional_non_empty(vars, "TTS_SIDECAR_URL")
                .unwrap_or("http://tts-clone-sidecar:8082")
                .to_owned(),
            api_server_internal_url: optional_non_empty(vars, "API_SERVER_INTERNAL_URL")
                .unwrap_or("http://api-server:8080")
                .to_owned(),
            storage_dir: PathBuf::from(
                optional_non_empty(vars, "STORAGE_DIR").unwrap_or("/storage"),
            ),
            max_cer_bps: parse_max_cer_bps(vars)?,
            max_tts_attempts: parse_max_tts_attempts(vars)?,
            desktop,
        })
    }

    /// Returns `true` when non-placeholder `Discord` bot token and owner snowflake are configured.
    #[must_use]
    pub fn is_discord_configured(&self) -> bool {
        let token = self.discord_bot_token.expose_secret().trim();
        let real_token = !token.is_empty()
            && token != "unconfigured"
            && !token.starts_with("replace-with-")
            && !token.starts_with("dev-discord-bot-token")
            && !token.starts_with("test-bot-token");
        real_token && self.discord_owner_user_id >= MIN_REAL_DISCORD_SNOWFLAKE
    }

    /// Computes `(user_install_url, bot_dm_url)` when `discord_application_id` is a valid snowflake.
    #[must_use]
    pub fn discord_oauth_urls(&self) -> Option<(String, String)> {
        discord_oauth_urls_for(self.discord_application_id)
    }

    /// Deterministic baseline configuration for automated workflow and API test suites.
    #[must_use]
    pub fn test_default() -> Self {
        Self {
            rust_log: "info".to_owned(),
            api_bind_addr: "0.0.0.0:8080".to_owned(),
            database_url: Secret::new(
                "postgres://test_user:test_password@postgres-test:5432/voice_pipe_test".to_owned(),
            ),
            owner_api_bearer_token: Secret::new("test-owner-bearer-token".to_owned()),
            discord_bot_token: Secret::new("test-bot-token".to_owned()),
            discord_application_id: 111_222_333,
            discord_owner_user_id: 777_888_999,
            whisper_model_path: PathBuf::from("/models/whisper/ggml-large-v3.bin"),
            whisper_use_gpu: true,
            silero_vad_model_path: None,
            translator_base_url: "http://qwen-translator:8081".to_owned(),
            tts_sidecar_url: "http://tts-clone-sidecar:8082".to_owned(),
            api_server_internal_url: "http://api-server:8080".to_owned(),
            storage_dir: PathBuf::from("/tmp/voice-clone-pipeline-test-storage"),
            max_cer_bps: BasisPoints::from_raw(1_200),
            max_tts_attempts: 3,
            desktop: DesktopRuntimeOptions::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, path::PathBuf};

    use common::basis_points::BasisPoints;

    use super::{AppConfig, ConfigError};

    fn base_map() -> BTreeMap<String, String> {
        BTreeMap::from([
            (
                "DATABASE_URL".to_owned(),
                "postgres://voice:secret_pw@localhost:5432/voice_db".to_owned(),
            ),
            (
                "OWNER_API_BEARER_TOKEN".to_owned(),
                "owner-token-secret-123".to_owned(),
            ),
            (
                "DISCORD_BOT_TOKEN".to_owned(),
                "discord-bot-secret-456".to_owned(),
            ),
            ("DISCORD_OWNER_USER_ID".to_owned(), "777888999".to_owned()),
        ])
    }

    #[test]
    fn parses_defaults_and_explicit_overrides_from_map() {
        let vars = base_map();
        let cfg = AppConfig::from_map(&vars).unwrap();

        assert_eq!(cfg.rust_log, "info");
        assert_eq!(cfg.api_bind_addr, "0.0.0.0:8080");
        assert_eq!(cfg.discord_application_id, 0);
        assert_eq!(cfg.discord_owner_user_id, 777_888_999);
        assert_eq!(
            cfg.whisper_model_path,
            PathBuf::from("/models/whisper/ggml-large-v3.bin")
        );
        assert!(cfg.whisper_use_gpu);
        assert_eq!(cfg.silero_vad_model_path, None);
        assert_eq!(cfg.translator_base_url, "http://qwen-translator:8081");
        assert_eq!(cfg.tts_sidecar_url, "http://tts-clone-sidecar:8082");
        assert_eq!(cfg.api_server_internal_url, "http://api-server:8080");
        assert_eq!(cfg.storage_dir, PathBuf::from("/storage"));
        assert_eq!(cfg.max_cer_bps, BasisPoints::from_raw(1_200));
        assert_eq!(cfg.max_tts_attempts, 3);
        assert_eq!(
            AppConfig::test_default().storage_dir,
            PathBuf::from("/tmp/voice-clone-pipeline-test-storage")
        );
    }

    #[test]
    fn rejects_missing_or_empty_required_keys() {
        let mut missing_db = base_map();
        missing_db.remove("DATABASE_URL");
        assert_eq!(
            AppConfig::from_map(&missing_db).unwrap_err(),
            ConfigError::MissingRequired {
                key: "DATABASE_URL"
            }
        );

        let mut empty_token = base_map();
        empty_token.insert("OWNER_API_BEARER_TOKEN".to_owned(), "   ".to_owned());
        assert_eq!(
            AppConfig::from_map(&empty_token).unwrap_err(),
            ConfigError::EmptyRequired {
                key: "OWNER_API_BEARER_TOKEN"
            }
        );
    }

    #[test]
    fn rejects_invalid_integers_and_out_of_range_attempts() {
        let mut bad_owner = base_map();
        bad_owner.insert("DISCORD_OWNER_USER_ID".to_owned(), "not-a-u64".to_owned());
        assert_eq!(
            AppConfig::from_map(&bad_owner).unwrap_err(),
            ConfigError::InvalidInteger {
                key: "DISCORD_OWNER_USER_ID",
                value: "not-a-u64".to_owned(),
            }
        );

        let mut zero_attempts = base_map();
        zero_attempts.insert("MAX_TTS_ATTEMPTS".to_owned(), "0".to_owned());
        assert_eq!(
            AppConfig::from_map(&zero_attempts).unwrap_err(),
            ConfigError::OutOfRange {
                key: "MAX_TTS_ATTEMPTS",
                value: "0".to_owned(),
                expected: "1..=10",
            }
        );
    }

    #[test]
    fn debug_output_redacts_all_secret_credentials() {
        let cfg = AppConfig::from_map(&base_map()).unwrap();
        let debug_str = format!("{cfg:?}");

        assert!(debug_str.contains("[REDACTED]"));
        assert!(!debug_str.contains("secret_pw"));
        assert!(!debug_str.contains("owner-token-secret-123"));
        assert!(!debug_str.contains("discord-bot-secret-456"));
    }
}
