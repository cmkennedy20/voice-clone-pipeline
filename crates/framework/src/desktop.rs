//! Self-contained desktop runtime primitives: portable paths, `.env` persistence, hardware probing,
//! `SPA` static asset loader, resumable `HuggingFace` model downloads, and `Discord` verification.

pub mod discord_verify;
pub mod dotenv;
pub mod models;
pub mod paths;
pub mod probe;
pub mod spa;

/// Typed errors emitted by desktop bootstrap, hardware probing, `.env` updates, and model pulls.
#[derive(Debug, thiserror::Error)]
pub enum DesktopError {
    #[error("filesystem I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("model download failed for `{artifact}` (HTTP {status}): {detail}")]
    DownloadStatus {
        artifact: String,
        status: u16,
        detail: String,
    },
    #[error("Discord Bot Token verification failed (HTTP {status}): {detail}")]
    DiscordVerificationFailed { status: u16, detail: String },
    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),
}
