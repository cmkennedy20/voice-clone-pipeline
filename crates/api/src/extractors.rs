//! Request extractors enforcing constant-time owner `Bearer` auth and `Discord` identity guards.

use std::future::{Future, ready};

use axum::{
    extract::FromRequestParts,
    http::{header, request::Parts},
};
use framework::{
    auth::owner_token::{verify_bearer_header, verify_discord_owner},
    context::AppContext,
};

use crate::error::ApiError;

/// Optional `HTTP` header overriding or asserting the caller's `Discord` snowflake user ID.
pub const DISCORD_USER_ID_HEADER: &str = "x-discord-user-id";

/// Verified owner identity extracted before any controller body or `GPU` workflow executes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthenticatedOwner {
    pub discord_user_id: u64,
}

fn authenticate_parts(parts: &Parts, state: &AppContext) -> Result<AuthenticatedOwner, ApiError> {
    let auth_header = parts
        .headers
        .get(header::AUTHORIZATION)
        .map(|raw| {
            raw.to_str().map_err(|_| {
                ApiError::Unauthorized("Authorization header contains non-ASCII bytes".to_owned())
            })
        })
        .transpose()?;

    verify_bearer_header(auth_header, &state.config.owner_api_bearer_token)
        .map_err(|err| ApiError::Unauthorized(err.to_string()))?;

    let discord_user_id = if let Some(raw_user_id) = parts.headers.get(DISCORD_USER_ID_HEADER) {
        let text = raw_user_id
            .to_str()
            .map_err(|_| {
                ApiError::BadRequest("x-discord-user-id header contains non-ASCII bytes".to_owned())
            })?
            .trim();

        if text.is_empty() {
            return Err(ApiError::BadRequest(
                "x-discord-user-id header cannot be empty".to_owned(),
            ));
        }

        let parsed_id = text.parse::<u64>().map_err(|_| {
            ApiError::BadRequest(format!(
                "malformed x-discord-user-id header `{text}`: expected unsigned 64-bit snowflake"
            ))
        })?;

        verify_discord_owner(parsed_id, state.config.discord_owner_user_id)
            .map_err(|err| ApiError::Forbidden(err.to_string()))?;

        parsed_id
    } else {
        state.config.discord_owner_user_id
    };

    Ok(AuthenticatedOwner { discord_user_id })
}

impl FromRequestParts<AppContext> for AuthenticatedOwner {
    type Rejection = ApiError;

    fn from_request_parts(
        parts: &mut Parts,
        state: &AppContext,
    ) -> impl Future<Output = Result<Self, Self::Rejection>> + Send {
        ready(authenticate_parts(parts, state))
    }
}
