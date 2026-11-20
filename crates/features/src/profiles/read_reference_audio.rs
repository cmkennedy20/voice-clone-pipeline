//! Owner-verified reference `WAV` reader (`read_profile_reference_audio`).

use std::io::ErrorKind;

use framework::context::AppContext;

use crate::{
    error::AppError,
    shared_queries::profile_loader::{ensure_caller_is_owner, load_owned_profile_by_slug},
};

/// Verifies ownership of `slug` and reads its enrolled `24 kHz` reference `WAV` bytes from disk.
pub async fn read_profile_reference_audio(
    ctx: &AppContext,
    caller_discord_user_id: u64,
    slug: &str,
) -> Result<Vec<u8>, AppError> {
    ensure_caller_is_owner(ctx, caller_discord_user_id)?;
    let profile = load_owned_profile_by_slug(&ctx.db, slug, caller_discord_user_id).await?;

    tokio::fs::read(&profile.reference_wav_path)
        .await
        .map_err(|err| match err.kind() {
            ErrorKind::NotFound => AppError::NotFound(format!(
                "reference audio file for profile '{slug}' was not found"
            )),
            _ => AppError::Internal(format!(
                "failed reading reference audio for profile '{slug}': {err}"
            )),
        })
}
