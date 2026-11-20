//! Typed `UUIDv7` identifier wrapper macro and parse error.

#[doc(hidden)]
pub mod __private {
    pub use serde;
    pub use uuid;
}

/// Error returned when parsing a string into a typed `UUID` identifier fails.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid identifier UUID: {0}")]
pub struct IdParseError(#[from] pub uuid::Error);

/// Defines a strongly typed, `#[repr(transparent)]` `UUIDv7`-backed identifier newtype.
#[macro_export]
macro_rules! define_id {
    ($name:ident) => {
        #[repr(transparent)]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name($crate::id::__private::uuid::Uuid);

        impl $name {
            /// Generates a new time-ordered `UUIDv7` identifier.
            #[must_use]
            pub fn new_v7() -> Self {
                Self($crate::id::__private::uuid::Uuid::now_v7())
            }

            /// Wraps an existing [`uuid::Uuid`].
            #[must_use]
            pub const fn from_uuid(id: $crate::id::__private::uuid::Uuid) -> Self {
                Self(id)
            }

            /// Extracts the inner [`uuid::Uuid`].
            #[must_use]
            pub const fn as_uuid(self) -> $crate::id::__private::uuid::Uuid {
                self.0
            }
        }

        impl ::core::fmt::Display for $name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                ::core::fmt::Display::fmt(&self.0, f)
            }
        }

        impl ::core::str::FromStr for $name {
            type Err = $crate::id::IdParseError;

            fn from_str(s: &str) -> ::core::result::Result<Self, Self::Err> {
                <$crate::id::__private::uuid::Uuid as ::core::str::FromStr>::from_str(s)
                    .map(Self)
                    .map_err($crate::id::IdParseError::from)
            }
        }

        impl ::core::convert::From<$crate::id::__private::uuid::Uuid> for $name {
            fn from(value: $crate::id::__private::uuid::Uuid) -> Self {
                Self(value)
            }
        }

        impl ::core::convert::From<$name> for $crate::id::__private::uuid::Uuid {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl $crate::id::__private::serde::Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> ::core::result::Result<S::Ok, S::Error>
            where
                S: $crate::id::__private::serde::Serializer,
            {
                self.0.serialize(serializer)
            }
        }

        impl<'de> $crate::id::__private::serde::Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> ::core::result::Result<Self, D::Error>
            where
                D: $crate::id::__private::serde::Deserializer<'de>,
            {
                <$crate::id::__private::uuid::Uuid as $crate::id::__private::serde::Deserialize<
                    'de,
                >>::deserialize(deserializer)
                .map(Self)
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use uuid::Uuid;

    define_id!(SampleEntityId);

    #[test]
    fn new_v7_creates_version_7_uuid() {
        let id = SampleEntityId::new_v7();
        assert_eq!(id.as_uuid().get_version_num(), 7);
    }

    #[test]
    fn display_and_from_str_round_trip_canonical_uuid() {
        let raw = "018f1234-5678-7abc-8def-0123456789ab";
        let parsed = SampleEntityId::from_str(raw).unwrap();
        assert_eq!(parsed.to_string(), raw);
        assert_eq!(
            SampleEntityId::from_uuid(Uuid::parse_str(raw).unwrap()),
            parsed
        );
    }

    #[test]
    fn invalid_uuid_string_is_rejected_with_displayable_error() {
        let err = SampleEntityId::from_str("not-a-valid-uuid").unwrap_err();
        assert!(err.to_string().contains("invalid identifier UUID"));
    }
}
