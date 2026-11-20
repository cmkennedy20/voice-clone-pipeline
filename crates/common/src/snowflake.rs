//! Lossless `64`-bit Discord snowflake `serde` codecs accepting either `u64` integers or decimal strings.

use std::fmt;

use serde::{
    Deserializer, Serializer,
    de::{Error as DeError, Visitor},
};

fn parse_snowflake_str<E: DeError>(raw: &str) -> Result<u64, E> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(E::custom("snowflake string cannot be empty"));
    }
    trimmed
        .parse::<u64>()
        .map_err(|err| E::custom(format!("invalid 64-bit snowflake `{trimmed}`: {err}")))
}

/// `serde` codec serializing as `u64` and deserializing from either a `JSON` `u64` or decimal string.
pub mod u64_or_string {
    use super::{DeError, Deserializer, Serializer, Visitor, fmt, parse_snowflake_str};

    struct U64OrStringVisitor;

    impl Visitor<'_> for U64OrStringVisitor {
        type Value = u64;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a non-negative 64-bit integer or decimal snowflake string")
        }

        fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
        where
            E: DeError,
        {
            Ok(v)
        }

        fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
        where
            E: DeError,
        {
            u64::try_from(v).map_err(|_| E::custom(format!("snowflake cannot be negative: {v}")))
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: DeError,
        {
            parse_snowflake_str(v)
        }

        fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
        where
            E: DeError,
        {
            parse_snowflake_str(&v)
        }
    }

    /// Serializes a `64`-bit snowflake as an unsigned `64`-bit integer.
    pub fn serialize<S>(value: &u64, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u64(*value)
    }

    /// Deserializes a `64`-bit snowflake from either an integer (`42`) or decimal string (`"42"`).
    pub fn deserialize<'de, D>(deserializer: D) -> Result<u64, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(U64OrStringVisitor)
    }
}

/// `serde` codec for `Option<u64>` accepting `null`, a `u64` integer, or a decimal string.
pub mod option_u64_or_string {
    use super::{
        DeError, Deserializer, Serializer, Visitor, fmt, parse_snowflake_str, u64_or_string,
    };

    struct OptionU64OrStringVisitor;

    impl<'de> Visitor<'de> for OptionU64OrStringVisitor {
        type Value = Option<u64>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("null, a non-negative 64-bit integer, or decimal snowflake string")
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: DeError,
        {
            Ok(None)
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: DeError,
        {
            Ok(None)
        }

        fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: Deserializer<'de>,
        {
            u64_or_string::deserialize(deserializer).map(Some)
        }

        fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
        where
            E: DeError,
        {
            Ok(Some(v))
        }

        fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
        where
            E: DeError,
        {
            u64::try_from(v)
                .map(Some)
                .map_err(|_| E::custom(format!("snowflake cannot be negative: {v}")))
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: DeError,
        {
            parse_snowflake_str(v).map(Some)
        }

        fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
        where
            E: DeError,
        {
            parse_snowflake_str(&v).map(Some)
        }
    }

    /// Serializes `Option<u64>` as either `Some(u64)` or `None`.
    pub fn serialize<S>(value: &Option<u64>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(id) => serializer.serialize_some(id),
            None => serializer.serialize_none(),
        }
    }

    /// Deserializes `Option<u64>` from `null`, an integer, or a decimal string.
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_option(OptionU64OrStringVisitor)
    }
}

#[cfg(test)]
mod tests {
    use serde::{
        Deserializer,
        de::{
            IntoDeserializer, Visitor,
            value::{
                Error as ValueError, I64Deserializer, StrDeserializer, U64Deserializer,
                UnitDeserializer,
            },
        },
        forward_to_deserialize_any,
    };

    use super::{option_u64_or_string, u64_or_string};

    struct SomeDeserializer<D>(D);

    impl<'de, D> Deserializer<'de> for SomeDeserializer<D>
    where
        D: Deserializer<'de>,
    {
        type Error = D::Error;

        fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            visitor.visit_some(self.0)
        }

        forward_to_deserialize_any! {
            bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
            bytes byte_buf option unit unit_struct newtype_struct seq tuple
            tuple_struct map struct enum identifier ignored_any
        }
    }

    #[test]
    fn u64_or_string_losslessly_deserializes_19_digit_string_and_u64_above_js_safe_integer() {
        const SNOWFLAKE_19_DIGIT: u64 = 1_234_567_890_123_456_789_u64;

        let from_str_de: StrDeserializer<'_, ValueError> =
            "1234567890123456789".into_deserializer();
        assert_eq!(
            u64_or_string::deserialize(from_str_de).unwrap(),
            SNOWFLAKE_19_DIGIT
        );

        let from_u64_de: U64Deserializer<ValueError> = SNOWFLAKE_19_DIGIT.into_deserializer();
        assert_eq!(
            u64_or_string::deserialize(from_u64_de).unwrap(),
            SNOWFLAKE_19_DIGIT
        );

        let small_de: U64Deserializer<ValueError> = 42_u64.into_deserializer();
        assert_eq!(u64_or_string::deserialize(small_de).unwrap(), 42);
    }

    #[test]
    fn option_u64_or_string_handles_null_u64_and_19_digit_decimal_string() {
        const SNOWFLAKE_19_DIGIT: u64 = 1_234_567_890_123_456_789_u64;

        let none_de: UnitDeserializer<ValueError> = ().into_deserializer();
        assert_eq!(option_u64_or_string::deserialize(none_de).unwrap(), None);

        let some_str_de = SomeDeserializer::<StrDeserializer<'_, ValueError>>(
            "1234567890123456789".into_deserializer(),
        );
        assert_eq!(
            option_u64_or_string::deserialize(some_str_de).unwrap(),
            Some(SNOWFLAKE_19_DIGIT)
        );

        let some_u64_de =
            SomeDeserializer::<U64Deserializer<ValueError>>(SNOWFLAKE_19_DIGIT.into_deserializer());
        assert_eq!(
            option_u64_or_string::deserialize(some_u64_de).unwrap(),
            Some(SNOWFLAKE_19_DIGIT)
        );
    }

    #[test]
    fn rejects_negative_empty_and_non_numeric_snowflakes() {
        let neg_de: I64Deserializer<ValueError> = (-5_i64).into_deserializer();
        assert!(u64_or_string::deserialize(neg_de).is_err());

        let empty_de: StrDeserializer<'_, ValueError> = "   ".into_deserializer();
        assert!(u64_or_string::deserialize(empty_de).is_err());

        let bad_de: StrDeserializer<'_, ValueError> = "not-a-snowflake".into_deserializer();
        assert!(u64_or_string::deserialize(bad_de).is_err());
    }
}
