//! Redacting wrapper preventing accidental secret leakage in logs or error displays.

use std::fmt;

/// Wrapper around sensitive credentials that always formats as `"[REDACTED]"` in
/// [`fmt::Debug`] and [`fmt::Display`] while preserving transparent `serde` transport.
#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Secret<T>(T);

impl<T> Secret<T> {
    /// Wraps a sensitive value inside [`Secret`].
    #[must_use]
    pub const fn new(inner: T) -> Self {
        Self(inner)
    }

    /// Explicitly borrows the underlying secret value.
    #[must_use]
    pub const fn expose_secret(&self) -> &T {
        &self.0
    }

    /// Consumes the wrapper and returns the inner secret value.
    #[must_use]
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> fmt::Debug for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

impl<T> fmt::Display for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

#[cfg(test)]
mod tests {
    use super::Secret;

    #[test]
    fn debug_and_display_never_leak_secret_payload() {
        let raw_token = "super-secret-bearer-token-998877";
        let secret = Secret::new(raw_token.to_owned());

        let debug_out = format!("{secret:?}");
        let display_out = format!("{secret}");

        assert_eq!(debug_out, "[REDACTED]");
        assert_eq!(display_out, "[REDACTED]");
        assert!(!debug_out.contains(raw_token));
        assert!(!display_out.contains(raw_token));
    }

    #[test]
    fn expose_secret_and_into_inner_recover_exact_value() {
        let secret = Secret::new("top-secret-value".to_owned());
        assert_eq!(secret.expose_secret(), "top-secret-value");
        assert_eq!(secret.into_inner(), "top-secret-value");
    }
}
