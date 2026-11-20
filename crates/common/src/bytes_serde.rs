//! Pure `RFC 4648` `Base64` or `[u8]` byte-array `serde` codec for binary audio fields.

use std::fmt;

use serde::{
    Deserializer, Serializer,
    de::{Error as DeError, SeqAccess, Visitor},
};

const BASE64_ALPHABET: [u8; 64] =
    *b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

const fn decode_sextet(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// Encodes raw bytes as standard `RFC 4648` `Base64` with `'='` padding.
#[must_use]
pub fn encode_base64(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    let (triples, remainder) = bytes.as_chunks::<3>();

    for &[b0, b1, b2] in triples {
        let i0 = usize::from(b0 >> 2);
        let i1 = usize::from(((b0 & 0x03) << 4) | (b1 >> 4));
        let i2 = usize::from(((b1 & 0x0F) << 2) | (b2 >> 6));
        let i3 = usize::from(b2 & 0x3F);
        out.push(char::from(BASE64_ALPHABET[i0]));
        out.push(char::from(BASE64_ALPHABET[i1]));
        out.push(char::from(BASE64_ALPHABET[i2]));
        out.push(char::from(BASE64_ALPHABET[i3]));
    }

    match *remainder {
        [b0] => {
            let i0 = usize::from(b0 >> 2);
            let i1 = usize::from((b0 & 0x03) << 4);
            out.push(char::from(BASE64_ALPHABET[i0]));
            out.push(char::from(BASE64_ALPHABET[i1]));
            out.push('=');
            out.push('=');
        }
        [b0, b1] => {
            let i0 = usize::from(b0 >> 2);
            let i1 = usize::from(((b0 & 0x03) << 4) | (b1 >> 4));
            let i2 = usize::from((b1 & 0x0F) << 2);
            out.push(char::from(BASE64_ALPHABET[i0]));
            out.push(char::from(BASE64_ALPHABET[i1]));
            out.push(char::from(BASE64_ALPHABET[i2]));
            out.push('=');
        }
        _ => {}
    }

    out
}

/// Decodes a standard `RFC 4648` `Base64` string, ignoring `ASCII` whitespace and newlines.
pub fn decode_base64(input: &str) -> Result<Vec<u8>, String> {
    let clean: Vec<u8> = input.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if clean.is_empty() {
        return Ok(Vec::new());
    }
    if !clean.len().is_multiple_of(4) {
        return Err(format!(
            "invalid base64 length {}: must be a multiple of 4",
            clean.len()
        ));
    }

    let (quads, _) = clean.as_chunks::<4>();
    let total_quads = quads.len();
    let mut out = Vec::with_capacity(total_quads * 3);

    for (idx, &quad) in quads.iter().enumerate() {
        let is_last = idx + 1 == total_quads;
        let s0 = decode_sextet(quad[0])
            .ok_or_else(|| format!("invalid base64 byte 0x{:02x}", quad[0]))?;
        let s1 = decode_sextet(quad[1])
            .ok_or_else(|| format!("invalid base64 byte 0x{:02x}", quad[1]))?;

        if quad[2] == b'=' {
            if !is_last || quad[3] != b'=' {
                return Err("invalid base64 '=' padding position".to_owned());
            }
            let triple = (u32::from(s0) << 18) | (u32::from(s1) << 12);
            let [_, b0, _, _] = triple.to_be_bytes();
            out.push(b0);
        } else if quad[3] == b'=' {
            if !is_last {
                return Err("invalid base64 '=' padding before final block".to_owned());
            }
            let s2 = decode_sextet(quad[2])
                .ok_or_else(|| format!("invalid base64 byte 0x{:02x}", quad[2]))?;
            let triple = (u32::from(s0) << 18) | (u32::from(s1) << 12) | (u32::from(s2) << 6);
            let [_, b0, b1, _] = triple.to_be_bytes();
            out.push(b0);
            out.push(b1);
        } else {
            let s2 = decode_sextet(quad[2])
                .ok_or_else(|| format!("invalid base64 byte 0x{:02x}", quad[2]))?;
            let s3 = decode_sextet(quad[3])
                .ok_or_else(|| format!("invalid base64 byte 0x{:02x}", quad[3]))?;
            let triple = (u32::from(s0) << 18)
                | (u32::from(s1) << 12)
                | (u32::from(s2) << 6)
                | u32::from(s3);
            let [_, b0, b1, b2] = triple.to_be_bytes();
            out.push(b0);
            out.push(b1);
            out.push(b2);
        }
    }

    Ok(out)
}

/// Serializes a byte slice as a compact `RFC 4648` `Base64` string.
pub fn serialize<S>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&encode_base64(bytes))
}

struct BytesOrBase64Visitor;

impl<'de> Visitor<'de> for BytesOrBase64Visitor {
    type Value = Vec<u8>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an RFC 4648 base64 string, a [u8] byte array, or null")
    }

    fn visit_none<E>(self) -> Result<Self::Value, E>
    where
        E: DeError,
    {
        Ok(Vec::new())
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E>
    where
        E: DeError,
    {
        Ok(Vec::new())
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: DeError,
    {
        decode_base64(v).map_err(E::custom)
    }

    fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
    where
        E: DeError,
    {
        self.visit_str(&v)
    }

    fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
    where
        E: DeError,
    {
        Ok(v.to_vec())
    }

    fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Self::Value, E>
    where
        E: DeError,
    {
        Ok(v)
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut out = Vec::with_capacity(seq.size_hint().unwrap_or(0).min(65_536));
        while let Some(byte) = seq.next_element::<u8>()? {
            out.push(byte);
        }
        Ok(out)
    }
}

/// Deserializes `Vec<u8>` from a `Base64` string, a `JSON` `[u8]` integer array, or `null`.
pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
where
    D: Deserializer<'de>,
{
    deserializer.deserialize_any(BytesOrBase64Visitor)
}

#[cfg(test)]
mod tests {
    use serde::de::{
        IntoDeserializer,
        value::{Error as ValueError, SeqDeserializer, StrDeserializer, UnitDeserializer},
    };

    use super::{decode_base64, deserialize, encode_base64};

    #[test]
    fn rfc4648_test_vectors_round_trip_exact_bytes() {
        let vectors: &[(&[u8], &str)] = &[
            (b"", ""),
            (b"f", "Zg=="),
            (b"fo", "Zm8="),
            (b"foo", "Zm9v"),
            (b"foob", "Zm9vYg=="),
            (b"fooba", "Zm9vYmE="),
            (b"foobar", "Zm9vYmFy"),
            (b"RIFF", "UklGRg=="),
        ];
        for &(raw, expected_b64) in vectors {
            assert_eq!(encode_base64(raw), expected_b64);
            assert_eq!(decode_base64(expected_b64).unwrap(), raw);
        }
    }

    #[test]
    fn serde_accepts_base64_with_whitespace_and_u8_arrays_and_unit() {
        let b64_de: StrDeserializer<'_, ValueError> = "  UklG\nRg== \r\n".into_deserializer();
        assert_eq!(deserialize(b64_de).unwrap(), vec![82, 73, 70, 70]);

        let seq_de: SeqDeserializer<std::vec::IntoIter<u8>, ValueError> =
            SeqDeserializer::new(vec![82_u8, 73, 70, 70].into_iter());
        assert_eq!(deserialize(seq_de).unwrap(), b"RIFF");

        let unit_de: UnitDeserializer<ValueError> = ().into_deserializer();
        assert_eq!(deserialize(unit_de).unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn rejects_corrupt_base64_length_padding_and_characters() {
        assert!(decode_base64("abc").is_err());
        assert!(decode_base64("ab=c").is_err());
        assert!(decode_base64("Ukl$").is_err());
    }
}
