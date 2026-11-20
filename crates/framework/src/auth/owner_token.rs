//! Constant-time HTTP bearer token verification and Discord owner user ID guard.

use common::secret::Secret;

/// Authorization failures for HTTP bearer credentials and Discord owner identity checks.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuthError {
    #[error("missing Authorization header")]
    MissingBearerHeader,
    #[error("Authorization header must use Bearer scheme")]
    MalformedBearerHeader,
    #[error("invalid bearer token")]
    InvalidBearerToken,
    #[error("discord user {actual} is not authorized owner {expected}")]
    UnauthorizedDiscordUser { actual: u64, expected: u64 },
}

fn constant_time_bytes_eq(provided: &[u8], expected: &[u8]) -> bool {
    if provided.is_empty() || expected.is_empty() {
        return false;
    }
    let max_len = provided.len().max(expected.len());
    let mut diff: usize = provided.len() ^ expected.len();
    for idx in 0..max_len {
        let lhs = provided.get(idx).copied().unwrap_or(0);
        let rhs = expected.get(idx).copied().unwrap_or(0);
        diff |= usize::from(lhs ^ rhs);
    }
    diff == 0
}

/// Validates an HTTP `Authorization: Bearer <token>` header in constant time.
pub fn verify_bearer_header(
    header_value: Option<&str>,
    expected_token: &Secret<String>,
) -> Result<(), AuthError> {
    let raw = header_value.map_or("", str::trim);
    if raw.is_empty() {
        return Err(AuthError::MissingBearerHeader);
    }

    let Some((scheme, credentials)) = raw.split_once(' ') else {
        return Err(AuthError::MalformedBearerHeader);
    };
    if !scheme.eq_ignore_ascii_case("Bearer") {
        return Err(AuthError::MalformedBearerHeader);
    }

    let provided_token = credentials.trim();
    if provided_token.is_empty() {
        return Err(AuthError::MalformedBearerHeader);
    }

    let expected_bytes = expected_token.expose_secret().as_bytes();
    if constant_time_bytes_eq(provided_token.as_bytes(), expected_bytes) {
        Ok(())
    } else {
        Err(AuthError::InvalidBearerToken)
    }
}

/// Verifies that the invoking Discord user ID matches the configured `DISCORD_OWNER_USER_ID`.
pub const fn verify_discord_owner(user_id: u64, expected_owner_id: u64) -> Result<(), AuthError> {
    if user_id == expected_owner_id {
        Ok(())
    } else {
        Err(AuthError::UnauthorizedDiscordUser {
            actual: user_id,
            expected: expected_owner_id,
        })
    }
}

#[cfg(test)]
mod tests {
    use common::secret::Secret;

    use super::{AuthError, verify_bearer_header, verify_discord_owner};

    fn secret_token() -> Secret<String> {
        Secret::new("owner-live-token-abcdef123456".to_owned())
    }

    #[test]
    fn rejects_missing_and_whitespace_only_headers() {
        let expected = secret_token();
        assert_eq!(
            verify_bearer_header(None, &expected),
            Err(AuthError::MissingBearerHeader)
        );
        assert_eq!(
            verify_bearer_header(Some("   "), &expected),
            Err(AuthError::MissingBearerHeader)
        );
    }

    #[test]
    fn rejects_non_bearer_or_empty_bearer_credentials() {
        let expected = secret_token();
        assert_eq!(
            verify_bearer_header(Some("Basic dXNlcjpwYXNz"), &expected),
            Err(AuthError::MalformedBearerHeader)
        );
        assert_eq!(
            verify_bearer_header(Some("Bearer"), &expected),
            Err(AuthError::MalformedBearerHeader)
        );
        assert_eq!(
            verify_bearer_header(Some("Bearer    "), &expected),
            Err(AuthError::MalformedBearerHeader)
        );
    }

    #[test]
    fn rejects_wrong_prefix_and_suffix_tokens_and_accepts_exact_match() {
        let expected = secret_token();
        assert_eq!(
            verify_bearer_header(Some("Bearer completely-wrong-token"), &expected),
            Err(AuthError::InvalidBearerToken)
        );
        assert_eq!(
            verify_bearer_header(Some("Bearer owner-live-token"), &expected),
            Err(AuthError::InvalidBearerToken)
        );
        assert_eq!(
            verify_bearer_header(
                Some("Bearer owner-live-token-abcdef123456-extra"),
                &expected
            ),
            Err(AuthError::InvalidBearerToken)
        );
        assert_eq!(
            verify_bearer_header(Some("Bearer owner-live-token-abcdef123456"), &expected),
            Ok(())
        );
        assert_eq!(
            verify_bearer_header(Some("bearer owner-live-token-abcdef123456"), &expected),
            Ok(())
        );
    }

    #[test]
    fn enforces_discord_owner_user_id_equality() {
        assert_eq!(verify_discord_owner(777_888_999, 777_888_999), Ok(()));
        assert_eq!(
            verify_discord_owner(123_456_789, 777_888_999),
            Err(AuthError::UnauthorizedDiscordUser {
                actual: 123_456_789,
                expected: 777_888_999,
            })
        );
    }
}
