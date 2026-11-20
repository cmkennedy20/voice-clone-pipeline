//! Paginated read projection (`list_profiles`) for owner-enrolled speaker profiles.

use common::pagination::{PageParams, Paged};
use framework::context::AppContext;
use uuid::Uuid;

use crate::{
    error::AppError,
    shared_queries::{
        profile_loader::{
            VoiceProfileRecord, VoiceProfileRow, discord_id_to_i64, ensure_caller_is_owner,
        },
        sample_queries::{hydrate_profile_samples, load_samples_for_profiles},
    },
};

/// Lists enrolled voice profiles belonging to `caller_discord_user_id` ordered newest-first,
/// hydrating multi-sample counts and durations in a single batched query.
pub async fn list_profiles(
    ctx: &AppContext,
    caller_discord_user_id: u64,
    page: PageParams,
) -> Result<Paged<VoiceProfileRecord>, AppError> {
    ensure_caller_is_owner(ctx, caller_discord_user_id)?;
    let owner_i64 = discord_id_to_i64(caller_discord_user_id, "caller_discord_user_id")?;
    let clamped_page = PageParams::new(page.limit, page.offset);
    let limit_i64 = i64::from(clamped_page.limit);
    let offset_i64 = i64::from(clamped_page.offset);

    let (total_i64,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM voice_profiles WHERE owner_discord_user_id = $1")
            .bind(owner_i64)
            .fetch_one(&ctx.db)
            .await
            .map_err(|err| AppError::from_sqlx(&err, "failed counting voice_profiles"))?;

    let total = u64::try_from(total_i64).unwrap_or(0);
    let rows = sqlx::query_as::<_, VoiceProfileRow>(
        "SELECT id, slug, display_name, avatar_url, reference_wav_path, reference_transcript_en,
                preferred_engine, default_target_language, owner_discord_user_id,
                tuning_json::text AS tuning_json, finetune_status, finetuned_checkpoint_dir,
                finetune_error, finetuned_at, created_at
           FROM voice_profiles
          WHERE owner_discord_user_id = $1
          ORDER BY created_at DESC, slug ASC
          LIMIT $2 OFFSET $3",
    )
    .bind(owner_i64)
    .bind(limit_i64)
    .bind(offset_i64)
    .fetch_all(&ctx.db)
    .await
    .map_err(|err| AppError::from_sqlx(&err, "failed listing voice_profiles"))?;

    let profile_ids: Vec<Uuid> = rows.iter().map(|row| row.id).collect();
    let mut samples_by_profile = load_samples_for_profiles(&ctx.db, &profile_ids).await?;

    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        let mut record = VoiceProfileRecord::try_from(row)?;
        let samples = samples_by_profile
            .remove(&record.id.as_uuid())
            .unwrap_or_default();
        hydrate_profile_samples(&mut record, samples);
        items.push(record);
    }

    Ok(Paged::new(items, total, clamped_page))
}
