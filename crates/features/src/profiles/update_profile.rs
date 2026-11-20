//! Partial metadata, synthesis tuning, and checkpoint toggle workflow (`update_profile`).

use domain::{
    ports::SynthesisTuning,
    translation::types::{TargetLanguage, TtsEngineKind},
};
use framework::context::AppContext;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{
    error::AppError,
    shared_queries::profile_loader::{
        VoiceProfileRecord, ensure_caller_is_owner, load_owned_profile_by_slug,
        require_trimmed_non_empty,
    },
};

/// Three-state `PATCH` field distinguishing omitted (`Keep`), explicit `null` (`Clear`), and new value (`Set`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum AvatarPatch {
    #[default]
    Keep,
    Clear,
    Set(String),
}

impl AvatarPatch {
    /// Returns `true` when `avatar_url` was omitted from the `PATCH` payload.
    #[must_use]
    pub const fn is_keep(&self) -> bool {
        matches!(self, Self::Keep)
    }
}

impl Serialize for AvatarPatch {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Keep | Self::Clear => serializer.serialize_none(),
            Self::Set(url) => serializer.serialize_some(url),
        }
    }
}

impl<'de> Deserialize<'de> for AvatarPatch {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(match Option::<String>::deserialize(deserializer)? {
            Some(url) => Self::Set(url),
            None => Self::Clear,
        })
    }
}

/// Partial `PATCH` payload for updating mutable voice profile metadata, tuning, and fine-tune toggle.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateProfileInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "AvatarPatch::is_keep")]
    pub avatar_url: AvatarPatch,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_engine: Option<TtsEngineKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_target_language: Option<TargetLanguage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_transcript_en: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tuning: Option<SynthesisTuning>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub use_finetuned_checkpoint: Option<bool>,
}

struct ResolvedProfileUpdate {
    display_name: String,
    avatar_url: Option<String>,
    preferred_engine: TtsEngineKind,
    default_target_language: TargetLanguage,
    reference_transcript_en: String,
    transcript_changed: bool,
    tuning_json: String,
    finetune_status: String,
    finetuned_checkpoint_dir: Option<String>,
    finetune_error: Option<String>,
    finetuned_at: Option<chrono::DateTime<chrono::Utc>>,
}

fn resolve_profile_update(
    current: VoiceProfileRecord,
    input: UpdateProfileInput,
) -> Result<
    (
        crate::shared_queries::profile_loader::VoiceProfileId,
        ResolvedProfileUpdate,
    ),
    AppError,
> {
    let display_name = if let Some(raw) = input.display_name {
        require_trimmed_non_empty(&raw, "display_name")?
    } else {
        current.display_name
    };
    let avatar_url = match input.avatar_url {
        AvatarPatch::Keep => current.avatar_url,
        AvatarPatch::Clear => None,
        AvatarPatch::Set(raw) => {
            let trimmed = raw.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_owned())
        }
    };
    let preferred_engine = input.preferred_engine.unwrap_or(current.preferred_engine);
    let default_target_language = input
        .default_target_language
        .unwrap_or(current.default_target_language);

    let transcript_changed = input.reference_transcript_en.is_some();
    let reference_transcript_en = if let Some(raw) = input.reference_transcript_en {
        require_trimmed_non_empty(&raw, "reference_transcript_en")?
    } else {
        current.reference_transcript_en
    };

    let tuning = input.tuning.unwrap_or(current.tuning);
    tuning.validate().map_err(AppError::Validation)?;
    let tuning_json = serde_json::to_string(&tuning)
        .map_err(|err| AppError::Internal(format!("failed serializing tuning JSON: {err}")))?;

    let (finetune_status, finetuned_checkpoint_dir, finetune_error, finetuned_at) =
        if input.use_finetuned_checkpoint == Some(false) {
            ("none".to_owned(), None, None, None)
        } else {
            (
                current.finetune_status,
                current.finetuned_checkpoint_dir,
                current.finetune_error,
                current.finetuned_at,
            )
        };

    Ok((
        current.id,
        ResolvedProfileUpdate {
            display_name,
            avatar_url,
            preferred_engine,
            default_target_language,
            reference_transcript_en,
            transcript_changed,
            tuning_json,
            finetune_status,
            finetuned_checkpoint_dir,
            finetune_error,
            finetuned_at,
        },
    ))
}

/// Updates mutable metadata, `tuning_json`, and fine-tuned checkpoint selection on `voice_profiles`.
pub async fn update_profile(
    ctx: &AppContext,
    caller_discord_user_id: u64,
    slug: &str,
    input: UpdateProfileInput,
) -> Result<VoiceProfileRecord, AppError> {
    ensure_caller_is_owner(ctx, caller_discord_user_id)?;
    let current = load_owned_profile_by_slug(&ctx.db, slug, caller_discord_user_id).await?;
    let (profile_id, upd) = resolve_profile_update(current, input)?;

    let mut tx =
        ctx.db.begin().await.map_err(|err| {
            AppError::from_sqlx(&err, "failed starting profile update transaction")
        })?;

    sqlx::query(
        "UPDATE voice_profiles
            SET display_name = $1,
                avatar_url = $2,
                preferred_engine = $3,
                default_target_language = $4,
                reference_transcript_en = $5,
                tuning_json = $6::jsonb,
                finetune_status = $7,
                finetuned_checkpoint_dir = $8,
                finetune_error = $9,
                finetuned_at = $10
          WHERE id = $11",
    )
    .bind(&upd.display_name)
    .bind(&upd.avatar_url)
    .bind(upd.preferred_engine.as_str())
    .bind(upd.default_target_language.code())
    .bind(&upd.reference_transcript_en)
    .bind(&upd.tuning_json)
    .bind(&upd.finetune_status)
    .bind(&upd.finetuned_checkpoint_dir)
    .bind(&upd.finetune_error)
    .bind(upd.finetuned_at)
    .bind(profile_id.as_uuid())
    .execute(&mut *tx)
    .await
    .map_err(|err| AppError::from_sqlx(&err, "failed updating voice_profiles row"))?;

    if upd.transcript_changed {
        sqlx::query(
            "UPDATE voice_profile_samples
                SET transcript_en = $1
              WHERE profile_id = $2 AND is_primary = TRUE",
        )
        .bind(&upd.reference_transcript_en)
        .bind(profile_id.as_uuid())
        .execute(&mut *tx)
        .await
        .map_err(|err| AppError::from_sqlx(&err, "failed syncing primary sample transcript"))?;
    }

    tx.commit()
        .await
        .map_err(|err| AppError::from_sqlx(&err, "failed committing profile update"))?;

    load_owned_profile_by_slug(&ctx.db, slug, caller_discord_user_id).await
}

#[cfg(test)]
mod tests {
    use super::{AvatarPatch, UpdateProfileInput};

    #[test]
    fn distinguishes_omitted_avatar_from_explicit_null_avatar_in_json() {
        let omitted: UpdateProfileInput =
            serde_json::from_str(r#"{"display_name":"New"}"#).unwrap();
        assert_eq!(omitted.avatar_url, AvatarPatch::Keep);
        assert!(omitted.tuning.is_none());
        assert!(omitted.use_finetuned_checkpoint.is_none());

        let cleared: UpdateProfileInput = serde_json::from_str(r#"{"avatar_url":null}"#).unwrap();
        assert_eq!(cleared.avatar_url, AvatarPatch::Clear);

        let updated: UpdateProfileInput =
            serde_json::from_str(r#"{"avatar_url":"https://cdn.example.com/a.png"}"#).unwrap();
        assert_eq!(
            updated.avatar_url,
            AvatarPatch::Set("https://cdn.example.com/a.png".to_owned())
        );
    }
}
