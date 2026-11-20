//! Cascading profile deletion workflow (`delete_profile`) with transactional `voice_jobs` and sample directory cleanup.

use framework::context::AppContext;

use crate::{
    error::AppError,
    shared_queries::profile_loader::{ensure_caller_is_owner, load_owned_profile_by_slug},
};

/// Deletes an owned voice profile, removing associated `voice_jobs` (plus `ON DELETE CASCADE`
/// samples and fine-tune jobs) inside a `PostgreSQL` transaction and unlinking stored files best-effort.
pub async fn delete_profile(
    ctx: &AppContext,
    caller_discord_user_id: u64,
    slug: &str,
) -> Result<(), AppError> {
    ensure_caller_is_owner(ctx, caller_discord_user_id)?;
    let profile = load_owned_profile_by_slug(&ctx.db, slug, caller_discord_user_id).await?;

    let mut tx =
        ctx.db.begin().await.map_err(|err| {
            AppError::from_sqlx(&err, "failed starting profile deletion transaction")
        })?;

    let job_audio_paths: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT output_ogg_path, output_wav_path
           FROM voice_jobs
          WHERE profile_id = $1",
    )
    .bind(profile.id.as_uuid())
    .fetch_all(&mut *tx)
    .await
    .map_err(|err| AppError::from_sqlx(&err, "failed listing associated voice_jobs audio paths"))?;

    sqlx::query("DELETE FROM voice_jobs WHERE profile_id = $1")
        .bind(profile.id.as_uuid())
        .execute(&mut *tx)
        .await
        .map_err(|err| AppError::from_sqlx(&err, "failed deleting associated voice_jobs"))?;

    let deleted = sqlx::query("DELETE FROM voice_profiles WHERE id = $1")
        .bind(profile.id.as_uuid())
        .execute(&mut *tx)
        .await
        .map_err(|err| AppError::from_sqlx(&err, "failed deleting voice_profiles row"))?;

    if deleted.rows_affected() == 0 {
        return Err(AppError::NotFound(format!(
            "voice profile '{slug}' was not found"
        )));
    }

    tx.commit()
        .await
        .map_err(|err| AppError::from_sqlx(&err, "failed committing profile deletion"))?;

    let _ = tokio::fs::remove_file(&profile.reference_wav_path).await;
    for sample in &profile.samples {
        let _ = tokio::fs::remove_file(&sample.wav_path).await;
    }
    let profile_dir = ctx.config.storage_dir.join("voice_profiles").join(slug);
    let _ = tokio::fs::remove_dir_all(&profile_dir).await;

    for (ogg_path, wav_path) in job_audio_paths {
        let _ = tokio::fs::remove_file(&ogg_path).await;
        if let Some(wav) = wav_path {
            let _ = tokio::fs::remove_file(&wav).await;
        }
    }

    Ok(())
}
