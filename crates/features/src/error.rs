//! Transport-agnostic application workflow errors and domain/infrastructure conversions.

use domain::{
    audio::error::AudioDomainError, ports::ExternalPortError,
    translation::error::TranslationDomainError,
};
use framework::db::pg_errors::is_unique_violation;

/// Canonical workflow error taxonomy mapped by `crates/api` into `RFC 7807` Problem Details.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AppError {
    #[error("validation error: {0}")]
    Validation(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("forbidden: {0}")]
    Forbidden(String),
    #[error("internal error: {0}")]
    Internal(String),
}

impl AppError {
    /// Maps a `SQLx` error into [`AppError`] with a caller-supplied context message,
    /// translating `RowNotFound` into [`Self::NotFound`] and `SQLSTATE 23505` into [`Self::Conflict`].
    pub fn from_sqlx(err: &sqlx::Error, context: &str) -> Self {
        if matches!(err, sqlx::Error::RowNotFound) {
            return Self::NotFound(context.to_owned());
        }
        if is_unique_violation(err) {
            return Self::Conflict(context.to_owned());
        }
        Self::Internal(format!("{context}: {err}"))
    }
}

impl From<AudioDomainError> for AppError {
    fn from(err: AudioDomainError) -> Self {
        Self::Validation(err.to_string())
    }
}

impl From<TranslationDomainError> for AppError {
    fn from(err: TranslationDomainError) -> Self {
        Self::Validation(err.to_string())
    }
}

impl From<ExternalPortError> for AppError {
    fn from(err: ExternalPortError) -> Self {
        match err {
            ExternalPortError::AudioCodec(msg) => Self::Validation(msg),
            other => Self::Internal(other.to_string()),
        }
    }
}

impl From<sqlx::Error> for AppError {
    fn from(err: sqlx::Error) -> Self {
        if matches!(err, sqlx::Error::RowNotFound) {
            return Self::NotFound("requested record was not found".to_owned());
        }
        if is_unique_violation(&err) {
            return Self::Conflict(err.to_string());
        }
        Self::Internal(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::{borrow::Cow, error::Error as StdError, fmt};

    use domain::{
        audio::error::AudioDomainError, ports::ExternalPortError,
        translation::error::TranslationDomainError,
    };
    use sqlx::error::{DatabaseError, ErrorKind};

    use super::AppError;

    #[derive(Debug)]
    struct FakePgError {
        sqlstate: &'static str,
    }

    impl fmt::Display for FakePgError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "pg error {}", self.sqlstate)
        }
    }

    impl StdError for FakePgError {}

    impl DatabaseError for FakePgError {
        fn message(&self) -> &'static str {
            "pg error"
        }

        fn code(&self) -> Option<Cow<'_, str>> {
            Some(Cow::Borrowed(self.sqlstate))
        }

        fn as_error(&self) -> &(dyn StdError + Send + Sync + 'static) {
            self
        }

        fn as_error_mut(&mut self) -> &mut (dyn StdError + Send + Sync + 'static) {
            self
        }

        fn into_error(self: Box<Self>) -> Box<dyn StdError + Send + Sync + 'static> {
            self
        }

        fn kind(&self) -> ErrorKind {
            if self.sqlstate == "23505" {
                ErrorKind::UniqueViolation
            } else {
                ErrorKind::Other
            }
        }
    }

    #[test]
    fn maps_domain_and_external_port_errors_to_expected_variants() {
        let audio_err = AppError::from(AudioDomainError::EmptySamples);
        assert!(matches!(audio_err, AppError::Validation(_)));

        let trans_err = AppError::from(TranslationDomainError::EmptyScript);
        assert!(matches!(trans_err, AppError::Validation(_)));

        let codec_err = AppError::from(ExternalPortError::AudioCodec("corrupt riff".to_owned()));
        assert_eq!(codec_err, AppError::Validation("corrupt riff".to_owned()));

        let tts_err = AppError::from(ExternalPortError::Tts("sidecar 503".to_owned()));
        assert!(matches!(tts_err, AppError::Internal(_)));
    }

    #[test]
    fn maps_sqlx_row_not_found_and_unique_violation_accurately() {
        let missing = AppError::from_sqlx(&sqlx::Error::RowNotFound, "voice profile 'default'");
        assert_eq!(
            missing,
            AppError::NotFound("voice profile 'default'".to_owned())
        );

        let duplicate_raw = sqlx::Error::database(FakePgError { sqlstate: "23505" });
        let conflict = AppError::from_sqlx(&duplicate_raw, "slug 'default' already exists");
        assert_eq!(
            conflict,
            AppError::Conflict("slug 'default' already exists".to_owned())
        );

        let generic_raw = sqlx::Error::database(FakePgError { sqlstate: "08006" });
        let internal = AppError::from_sqlx(&generic_raw, "query failed");
        assert!(matches!(internal, AppError::Internal(_)));
    }
}
