//! Companion 1-on-1 profile management slash commands (`/voice_settings` and `/voice_profiles`).

use std::fmt::Write as _;

use common::pagination::Paged;
use features::profiles::{AvatarPatch, UpdateProfileInput, VoiceProfileRecord};
use framework::auth::owner_token::verify_discord_owner;
use reqwest::header::AUTHORIZATION;

use super::{
    AppCtx, DEFAULT_PROFILE_SLUG, EngineChoice, LanguageChoice, WorkerError, WorkerState,
    with_all_install_and_interaction_contexts,
};

/// Resolves and validates a profile slug so it is safe to interpolate into an `API` URL path.
pub fn resolve_valid_profile_slug(raw: Option<&str>) -> Result<String, WorkerError> {
    let slug = raw
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_PROFILE_SLUG);
    let bytes = slug.as_bytes();
    if !(2..=64).contains(&bytes.len()) {
        return Err(WorkerError::Relay(format!(
            "profile slug length {} must be within 2..=64 characters",
            bytes.len()
        )));
    }
    let Some((&first, rest)) = bytes.split_first() else {
        return Err(WorkerError::Relay(
            "profile slug cannot be empty".to_owned(),
        ));
    };
    if !first.is_ascii_lowercase()
        || !rest
            .iter()
            .all(|&b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-'))
    {
        return Err(WorkerError::Relay(
            "profile slug must start with 'a'..='z' and contain only lowercase ASCII, digits, '_', or '-'"
                .to_owned(),
        ));
    }
    Ok(slug.to_owned())
}

/// Builds a partial [`UpdateProfileInput`] `JSON` payload from optional dropdown selections.
pub fn build_profile_patch_input(
    default_language: Option<LanguageChoice>,
    preferred_engine: Option<EngineChoice>,
) -> UpdateProfileInput {
    UpdateProfileInput {
        display_name: None,
        avatar_url: AvatarPatch::Keep,
        preferred_engine: preferred_engine.map(EngineChoice::engine_kind),
        default_target_language: default_language.map(LanguageChoice::target_language),
        reference_transcript_en: None,
        ..Default::default()
    }
}

/// Formats an ephemeral confirmation message after updating a voice profile's settings.
pub fn format_updated_profile_reply(profile: &VoiceProfileRecord) -> String {
    let lang = profile.default_target_language;
    let resolved = lang.resolve_engine(profile.preferred_engine, None);
    let engine_note = if resolved == profile.preferred_engine {
        format!("`{}`", profile.preferred_engine)
    } else {
        format!(
            "`{}` (auto-routes to `{resolved}` when synthesizing `{}`)",
            profile.preferred_engine,
            lang.code()
        )
    };
    format!(
        "Updated profile **`{}`** ({}):\n- Default language: **{} (`{}`)**\n- Preferred engine: {}",
        profile.slug,
        profile.display_name,
        lang.english_name(),
        lang.code(),
        engine_note,
    )
}

/// Formats an ephemeral summary of enrolled speaker profiles or a friendly onboarding hint.
pub fn format_profiles_summary(profiles: &[VoiceProfileRecord], total: u64) -> String {
    if profiles.is_empty() {
        return format!(
            "No voice profiles are enrolled yet.\n\
             Enroll **`{DEFAULT_PROFILE_SLUG}`** in the Studio Web UI or via `voice-cli` (`POST /api/v1/profiles`) \
             with a clean `5-30s` English reference WAV, then run `/voice_clone` or DM me directly!"
        );
    }

    let mut out = format!("Enrolled voice profiles (**{total}** total):\n");
    for record in profiles {
        let lang = record.default_target_language;
        let _ = writeln!(
            out,
            "- **`{}`** ({}) — default: **{} (`{}`)**, engine: `{}`",
            record.slug,
            record.display_name,
            lang.english_name(),
            lang.code(),
            record.preferred_engine,
        );
    }
    out.trim_end().to_owned()
}

pub(crate) async fn patch_profile_settings(
    state: &WorkerState,
    caller_user_id: u64,
    slug: &str,
    patch: &UpdateProfileInput,
) -> Result<VoiceProfileRecord, WorkerError> {
    let response = state
        .http
        .patch(format!("{}/api/v1/profiles/{slug}", state.api_base_url))
        .header(
            AUTHORIZATION,
            format!("Bearer {}", state.bearer_token.expose_secret()),
        )
        .header("x-discord-user-id", caller_user_id.to_string())
        .json(patch)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(WorkerError::Relay(format!(
            "api-server returned HTTP {status}: {body}"
        )));
    }
    Ok(response.json().await?)
}

pub(crate) async fn fetch_enrolled_profiles(
    state: &WorkerState,
    caller_user_id: u64,
) -> Result<Paged<VoiceProfileRecord>, WorkerError> {
    let response = state
        .http
        .get(format!(
            "{}/api/v1/profiles?limit=20&offset=0",
            state.api_base_url
        ))
        .header(
            AUTHORIZATION,
            format!("Bearer {}", state.bearer_token.expose_secret()),
        )
        .header("x-discord-user-id", caller_user_id.to_string())
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(WorkerError::Relay(format!(
            "api-server returned HTTP {status}: {body}"
        )));
    }
    Ok(response.json().await?)
}

/// Update your enrolled profile's saved default target language or preferred `TTS` engine.
#[poise::command(
    slash_command,
    install_context = "Guild|User",
    interaction_context = "Guild|BotDm|PrivateChannel"
)]
pub async fn voice_settings(
    ctx: AppCtx<'_>,
    #[description = "New default target language saved on this profile"] default_language: Option<
        LanguageChoice,
    >,
    #[description = "Preferred TTS voice cloning engine for this profile"] preferred_engine: Option<
        EngineChoice,
    >,
    #[description = "Enrolled voice profile slug (default: default_voice)"] profile: Option<String>,
) -> Result<(), WorkerError> {
    let state = ctx.data();
    let caller_id = ctx.author().id.get();

    if verify_discord_owner(caller_id, state.owner_user_id).is_err() {
        ctx.send(
            poise::CreateReply::default()
                .content("Unauthorized: this personal voice app is restricted to its owner.")
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    }

    ctx.defer_ephemeral().await?;
    let outcome = async {
        let slug = resolve_valid_profile_slug(profile.as_deref())?;
        let patch = build_profile_patch_input(default_language, preferred_engine);
        patch_profile_settings(state, caller_id, &slug, &patch).await
    }
    .await;

    match outcome {
        Ok(updated) => {
            ctx.say(format_updated_profile_reply(&updated)).await?;
        }
        Err(err) => {
            tracing::warn!(error = %err, "Discord /voice_settings update failed");
            ctx.say(format!("Failed updating profile settings: {err}"))
                .await?;
        }
    }
    Ok(())
}

/// View your enrolled speaker voice profiles, default languages, and preferred engines.
#[poise::command(
    slash_command,
    install_context = "Guild|User",
    interaction_context = "Guild|BotDm|PrivateChannel"
)]
pub async fn voice_profiles(ctx: AppCtx<'_>) -> Result<(), WorkerError> {
    let state = ctx.data();
    let caller_id = ctx.author().id.get();

    if verify_discord_owner(caller_id, state.owner_user_id).is_err() {
        ctx.send(
            poise::CreateReply::default()
                .content("Unauthorized: this personal voice app is restricted to its owner.")
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    }

    ctx.defer_ephemeral().await?;
    match fetch_enrolled_profiles(state, caller_id).await {
        Ok(paged) => {
            ctx.say(format_profiles_summary(&paged.items, paged.total))
                .await?;
        }
        Err(err) => {
            tracing::warn!(error = %err, "Discord /voice_profiles fetch failed");
            ctx.say(format!("Failed loading voice profiles: {err}"))
                .await?;
        }
    }
    Ok(())
}

/// Returns `/voice_settings` configured for `USER_INSTALL` + `GUILD_INSTALL` across contexts `0..=2`.
pub fn configured_voice_settings_command() -> poise::Command<WorkerState, WorkerError> {
    with_all_install_and_interaction_contexts(voice_settings())
}

/// Returns `/voice_profiles` configured for `USER_INSTALL` + `GUILD_INSTALL` across contexts `0..=2`.
pub fn configured_voice_profiles_command() -> poise::Command<WorkerState, WorkerError> {
    with_all_install_and_interaction_contexts(voice_profiles())
}
