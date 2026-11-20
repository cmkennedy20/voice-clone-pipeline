//! `PostgreSQL` `SQLSTATE` error code inspection helpers.

/// `PostgreSQL` `unique_violation` `SQLSTATE` code (`23505`).
pub const SQLSTATE_UNIQUE_VIOLATION: &str = "23505";

/// `PostgreSQL` `foreign_key_violation` `SQLSTATE` code (`23503`).
pub const SQLSTATE_FOREIGN_KEY_VIOLATION: &str = "23503";

/// Returns `true` when `err` represents a `PostgreSQL` `23505` unique constraint violation.
#[must_use]
pub fn is_unique_violation(err: &sqlx::Error) -> bool {
    err.as_database_error()
        .and_then(sqlx::error::DatabaseError::code)
        .is_some_and(|code| code == SQLSTATE_UNIQUE_VIOLATION)
}

/// Returns `true` when `err` represents a `PostgreSQL` `23503` foreign-key constraint violation.
#[must_use]
pub fn is_foreign_key_violation(err: &sqlx::Error) -> bool {
    err.as_database_error()
        .and_then(sqlx::error::DatabaseError::code)
        .is_some_and(|code| code == SQLSTATE_FOREIGN_KEY_VIOLATION)
}

#[cfg(test)]
mod tests {
    use std::{borrow::Cow, error::Error as StdError, fmt};

    use sqlx::error::{DatabaseError, ErrorKind};

    use super::{is_foreign_key_violation, is_unique_violation};

    #[derive(Debug)]
    struct SyntheticPgError {
        sqlstate: &'static str,
    }

    impl fmt::Display for SyntheticPgError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "synthetic database error {}", self.sqlstate)
        }
    }

    impl StdError for SyntheticPgError {}

    impl DatabaseError for SyntheticPgError {
        fn message(&self) -> &'static str {
            "synthetic database error"
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
            match self.sqlstate {
                "23505" => ErrorKind::UniqueViolation,
                "23503" => ErrorKind::ForeignKeyViolation,
                _ => ErrorKind::Other,
            }
        }
    }

    #[test]
    fn classifies_unique_and_foreign_key_sqlstates_accurately() {
        let unique_err = sqlx::Error::database(SyntheticPgError { sqlstate: "23505" });
        let fk_err = sqlx::Error::database(SyntheticPgError { sqlstate: "23503" });
        let other_db_err = sqlx::Error::database(SyntheticPgError { sqlstate: "23514" });

        assert!(is_unique_violation(&unique_err));
        assert!(!is_foreign_key_violation(&unique_err));

        assert!(is_foreign_key_violation(&fk_err));
        assert!(!is_unique_violation(&fk_err));

        assert!(!is_unique_violation(&other_db_err));
        assert!(!is_foreign_key_violation(&other_db_err));
        assert!(!is_unique_violation(&sqlx::Error::RowNotFound));
    }
}
