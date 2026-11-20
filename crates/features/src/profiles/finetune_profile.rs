//! `RTX 3090` `XTTS-v2` speaker fine-tuning workflows (`start_profile_finetune`, `get_profile_finetune_status`).

use chrono::{DateTime, Utc};
use domain::ports::{FinetuneJobProgress, FinetuneStartRequest};
use framework::context::AppContext;
use uuid::Uuid;

use crate::{
    error::AppError,
    shared_queries::{
        profile_loader::{
            VoiceProfileId, ensure_caller_is_owner, load_owned_profile_by_slug, now_utc_micros,
        },
        sample_queries::VoiceFinetuneJobId,
    },
};

const DEFAULT_FINETUNE_EPOCHS: u16 = 6;
const DEFAULT_LEARNING_RATE: f64 = 5e-6;

/// Persisted `RTX 3090` `XTTS-v2` fine-tuning job receipt and live training progress.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VoiceFinetuneJobRecord {
    pub id: VoiceFinetuneJobId,
    pub profile_id: VoiceProfileId,
    pub profile_slug: String,
    pub status: String,
    pub epochs: u16,
    pub learning_rate: f64,
    pub sample_count: u16,
    pub current_epoch: u16,
    pub progress_bps: u32,
    pub last_loss: Option<f64>,
    pub checkpoint_dir: Option<String>,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Optional training hyperparameters for `POST /v1/profiles/{slug}/finetune`.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct StartProfileFinetuneInput {
    #[serde(default)]
    pub epochs: Option<u16>,
    #[serde(default)]
    pub learning_rate: Option<f64>,
}

#[derive(Debug, sqlx::FromRow)]
struct FinetuneJobRow {
    id: Uuid,
    profile_id: Uuid,
    status: String,
    epochs: i16,
    learning_rate: f64,
    sample_count: i16,
    current_epoch: i16,
    progress_bps: i32,
    last_loss: Option<f64>,
    checkpoint_dir: Option<String>,
    error_message: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

fn validate_finetune_hyperparams(
    input: &StartProfileFinetuneInput,
) -> Result<(u16, f64), AppError> {
    let epochs = input.epochs.unwrap_or(DEFAULT_FINETUNE_EPOCHS);
    if !(1..=50).contains(&epochs) {
        return Err(AppError::Validation(format!(
            "finetune epochs {epochs} must be within 1..=50"
        )));
    }

    let lr = input.learning_rate.unwrap_or(DEFAULT_LEARNING_RATE);
    if !lr.is_finite() || !(1e-7..=1e-3).contains(&lr) {
        return Err(AppError::Validation(format!(
            "finetune learning_rate {lr} must be finite and within 1e-7..=1e-3"
        )));
    }
    Ok((epochs, lr))
}

fn normalize_job_status(raw: &str) -> &'static str {
    match raw {
        "queued" => "queued",
        "succeeded" => "succeeded",
        "failed" => "failed",
        _ => "running",
    }
}

fn row_to_record(
    row: FinetuneJobRow,
    profile_slug: String,
) -> Result<VoiceFinetuneJobRecord, AppError> {
    let epochs = u16::try_from(row.epochs)
        .map_err(|_| AppError::Internal(format!("invalid epochs stored: {}", row.epochs)))?;
    let sample_count = u16::try_from(row.sample_count).map_err(|_| {
        AppError::Internal(format!("invalid sample_count stored: {}", row.sample_count))
    })?;
    let current_epoch = u16::try_from(row.current_epoch).unwrap_or(0);
    let progress_bps = u32::try_from(row.progress_bps).unwrap_or(0);

    Ok(VoiceFinetuneJobRecord {
        id: VoiceFinetuneJobId::from_uuid(row.id),
        profile_id: VoiceProfileId::from_uuid(row.profile_id),
        profile_slug,
        status: row.status,
        epochs,
        learning_rate: row.learning_rate,
        sample_count,
        current_epoch,
        progress_bps,
        last_loss: row.last_loss,
        checkpoint_dir: row.checkpoint_dir,
        error_message: row.error_message,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

async fn sync_finetune_progress(
    ctx: &AppContext,
    job_id: VoiceFinetuneJobId,
    profile_id: VoiceProfileId,
    profile_slug: String,
    progress: FinetuneJobProgress,
) -> Result<VoiceFinetuneJobRecord, AppError> {
    let status = normalize_job_status(&progress.status);
    let current_epoch_i16 = i16::try_from(progress.current_epoch).unwrap_or(i16::MAX);
    let clamped_bps = progress.progress_bps.min(10_000);
    let progress_i32 = i32::try_from(clamped_bps).unwrap_or(10_000);
    let updated_at = now_utc_micros(ctx);

    let mut tx =
        ctx.db.begin().await.map_err(|err| {
            AppError::from_sqlx(&err, "failed starting finetune sync transaction")
        })?;

    let updated_row = sqlx::query_as::<_, FinetuneJobRow>(
        "UPDATE voice_finetune_jobs
            SET status = $1,
                current_epoch = $2,
                progress_bps = $3,
                last_loss = $4,
                checkpoint_dir = COALESCE($5, checkpoint_dir),
                error_message = $6,
                updated_at = $7
          WHERE id = $8
          RETURNING id, profile_id, status, epochs, learning_rate, sample_count,
                    current_epoch, progress_bps, last_loss, checkpoint_dir,
                    error_message, created_at, updated_at",
    )
    .bind(status)
    .bind(current_epoch_i16)
    .bind(progress_i32)
    .bind(progress.last_loss)
    .bind(&progress.checkpoint_dir)
    .bind(&progress.error_message)
    .bind(updated_at)
    .bind(job_id.as_uuid())
    .fetch_one(&mut *tx)
    .await
    .map_err(|err| AppError::from_sqlx(&err, "failed updating voice_finetune_jobs row"))?;

    let finetuned_at = (status == "succeeded").then_some(updated_at);
    sqlx::query(
        "UPDATE voice_profiles
            SET finetune_status = $1,
                finetuned_checkpoint_dir = CASE
                    WHEN $1 = 'succeeded' THEN COALESCE($2, finetuned_checkpoint_dir)
                    ELSE finetuned_checkpoint_dir
                END,
                preferred_engine = CASE
                    WHEN $1 = 'succeeded' THEN 'xtts_v2'
                    ELSE preferred_engine
                END,
                finetune_error = $3,
                finetuned_at = COALESCE($4, finetuned_at)
          WHERE id = $5",
    )
    .bind(status)
    .bind(&updated_row.checkpoint_dir)
    .bind(&updated_row.error_message)
    .bind(finetuned_at)
    .bind(profile_id.as_uuid())
    .execute(&mut *tx)
    .await
    .map_err(|err| AppError::from_sqlx(&err, "failed syncing voice_profiles finetune status"))?;

    tx.commit()
        .await
        .map_err(|err| AppError::from_sqlx(&err, "failed committing finetune sync"))?;

    row_to_record(updated_row, profile_slug)
}

/// Launches an asynchronous `XTTS-v2` fine-tuning job across all enrolled samples on `slug`.
pub async fn start_profile_finetune(
    ctx: &AppContext,
    caller_discord_user_id: u64,
    slug: &str,
    input: StartProfileFinetuneInput,
) -> Result<VoiceFinetuneJobRecord, AppError> {
    ensure_caller_is_owner(ctx, caller_discord_user_id)?;
    let (epochs, learning_rate) = validate_finetune_hyperparams(&input)?;
    let profile = load_owned_profile_by_slug(&ctx.db, slug, caller_discord_user_id).await?;
    let samples = profile.reference_sample_refs();
    let sample_count = u16::try_from(samples.len()).unwrap_or(u16::MAX).max(1);

    let output_dir_path = ctx
        .config
        .storage_dir
        .join("voice_profiles")
        .join(&profile.slug)
        .join("finetuned_xtts");
    let _ = tokio::fs::create_dir_all(&output_dir_path).await;
    let output_dir = output_dir_path.to_string_lossy().into_owned();

    let job_id = VoiceFinetuneJobId::new_v7();
    let now = now_utc_micros(ctx);
    let epochs_i16 = i16::try_from(epochs).unwrap_or(6);
    let sample_count_i16 = i16::try_from(sample_count).unwrap_or(1);

    let mut tx = ctx
        .db
        .begin()
        .await
        .map_err(|err| AppError::from_sqlx(&err, "failed starting finetune creation tx"))?;

    sqlx::query(
        "INSERT INTO voice_finetune_jobs (
            id, profile_id, status, epochs, learning_rate, sample_count,
            current_epoch, progress_bps, created_at, updated_at
         ) VALUES ($1, $2, 'queued', $3, $4, $5, 0, 0, $6, $6)",
    )
    .bind(job_id.as_uuid())
    .bind(profile.id.as_uuid())
    .bind(epochs_i16)
    .bind(learning_rate)
    .bind(sample_count_i16)
    .bind(now)
    .execute(&mut *tx)
    .await
    .map_err(|err| AppError::from_sqlx(&err, "failed inserting voice_finetune_jobs row"))?;

    sqlx::query(
        "UPDATE voice_profiles
            SET finetune_status = 'queued',
                finetune_error = NULL
          WHERE id = $1",
    )
    .bind(profile.id.as_uuid())
    .execute(&mut *tx)
    .await
    .map_err(|err| AppError::from_sqlx(&err, "failed marking profile finetune queued"))?;

    tx.commit()
        .await
        .map_err(|err| AppError::from_sqlx(&err, "failed committing initial finetune row"))?;

    let start_req = FinetuneStartRequest {
        job_id: job_id.to_string(),
        profile_slug: profile.slug.clone(),
        samples,
        output_dir,
        epochs,
        learning_rate,
    };

    match ctx.finetuner.start_finetune(&start_req).await {
        Ok(progress) => {
            sync_finetune_progress(ctx, job_id, profile.id, profile.slug, progress).await
        }
        Err(port_err) => {
            let failed = FinetuneJobProgress {
                job_id: job_id.to_string(),
                profile_slug: slug.to_owned(),
                status: "failed".to_owned(),
                epochs,
                current_epoch: 0,
                progress_bps: 0,
                last_loss: None,
                checkpoint_dir: None,
                error_message: Some(port_err.to_string()),
            };
            let _ = sync_finetune_progress(ctx, job_id, profile.id, profile.slug, failed).await;
            Err(AppError::from(port_err))
        }
    }
}

/// Loads the latest fine-tuning job for `slug`, polling the sidecar when non-terminal (or
/// unsynced) and persisting updated status/checkpoint metadata on `voice_profiles`.
pub async fn get_profile_finetune_status(
    ctx: &AppContext,
    caller_discord_user_id: u64,
    slug: &str,
) -> Result<Option<VoiceFinetuneJobRecord>, AppError> {
    ensure_caller_is_owner(ctx, caller_discord_user_id)?;
    let profile = load_owned_profile_by_slug(&ctx.db, slug, caller_discord_user_id).await?;

    let maybe_row = sqlx::query_as::<_, FinetuneJobRow>(
        "SELECT id, profile_id, status, epochs, learning_rate, sample_count,
                current_epoch, progress_bps, last_loss, checkpoint_dir,
                error_message, created_at, updated_at
           FROM voice_finetune_jobs
          WHERE profile_id = $1
          ORDER BY created_at DESC, id DESC
          LIMIT 1",
    )
    .bind(profile.id.as_uuid())
    .fetch_optional(&ctx.db)
    .await
    .map_err(|err| AppError::from_sqlx(&err, "failed querying latest voice_finetune_jobs row"))?;

    let Some(row) = maybe_row else {
        return Ok(None);
    };

    let needs_poll = matches!(row.status.as_str(), "queued" | "running")
        || (row.status == "succeeded"
            && (row.checkpoint_dir.is_none()
                || (profile.finetune_status != "none"
                    && profile.finetuned_checkpoint_dir.is_none())));

    if needs_poll {
        let job_id = VoiceFinetuneJobId::from_uuid(row.id);
        let progress = ctx.finetuner.poll_finetune(&job_id.to_string()).await?;
        let synced =
            sync_finetune_progress(ctx, job_id, profile.id, profile.slug, progress).await?;
        return Ok(Some(synced));
    }

    Ok(Some(row_to_record(row, profile.slug)?))
}

#[cfg(test)]
mod tests {
    use super::{StartProfileFinetuneInput, validate_finetune_hyperparams};
    use crate::error::AppError;

    #[test]
    fn finetune_hyperparams_apply_defaults_and_reject_out_of_bounds() {
        let (epochs, lr) =
            validate_finetune_hyperparams(&StartProfileFinetuneInput::default()).unwrap();
        assert_eq!(epochs, 6);
        assert!((lr - 5e-6).abs() < 1e-12);

        assert!(matches!(
            validate_finetune_hyperparams(&StartProfileFinetuneInput {
                epochs: Some(0),
                learning_rate: None,
            }),
            Err(AppError::Validation(_))
        ));
        assert!(matches!(
            validate_finetune_hyperparams(&StartProfileFinetuneInput {
                epochs: Some(51),
                learning_rate: None,
            }),
            Err(AppError::Validation(_))
        ));
        assert!(matches!(
            validate_finetune_hyperparams(&StartProfileFinetuneInput {
                epochs: Some(6),
                learning_rate: Some(1e-8),
            }),
            Err(AppError::Validation(_))
        ));
        assert!(matches!(
            validate_finetune_hyperparams(&StartProfileFinetuneInput {
                epochs: Some(6),
                learning_rate: Some(f64::NAN),
            }),
            Err(AppError::Validation(_))
        ));
    }
}
