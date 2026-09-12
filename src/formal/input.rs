//! Strict, side-effect-free input checks before model deserialization.
use std::collections::BTreeSet;
use std::fmt;

use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};

use super::MAX_MODEL_BYTES;

pub(super) fn validate_json(raw: &str) -> Result<(), String> {
    if raw.len() > MAX_MODEL_BYTES {
        return Err(format!("model exceeds {MAX_MODEL_BYTES} bytes"));
    }
    let mut deserializer = serde_json::Deserializer::from_str(raw);
    UniqueValue::deserialize(&mut deserializer)
        .map_err(|error| format!("invalid model JSON: {error}"))?;
    deserializer
        .end()
        .map_err(|error| format!("invalid model JSON: {error}"))?;
    validate_integer_tokens(raw)
}

// This visitor discards values but checks decoded object keys at every depth.
// Checking after deserializing into Value/BTreeMap would lose duplicate keys.
struct UniqueValue;

impl<'de> Deserialize<'de> for UniqueValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(UniqueVisitor)
    }
}

struct UniqueVisitor;

impl<'de> Visitor<'de> for UniqueVisitor {
    type Value = UniqueValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("JSON with unique object keys")
    }

    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
        let mut keys = BTreeSet::new();
        while let Some(key) = map.next_key::<String>()? {
            if !keys.insert(key) {
                return Err(de::Error::custom("duplicate JSON object key"));
            }
            map.next_value::<UniqueValue>()?;
        }
        Ok(UniqueValue)
    }

    fn visit_seq<S: SeqAccess<'de>>(self, mut sequence: S) -> Result<Self::Value, S::Error> {
        while sequence.next_element::<UniqueValue>()?.is_some() {}
        Ok(UniqueValue)
    }

    fn visit_bool<E: de::Error>(self, _: bool) -> Result<Self::Value, E> {
        Ok(UniqueValue)
    }

    fn visit_i64<E: de::Error>(self, _: i64) -> Result<Self::Value, E> {
        Ok(UniqueValue)
    }

    fn visit_u64<E: de::Error>(self, _: u64) -> Result<Self::Value, E> {
        Ok(UniqueValue)
    }

    fn visit_f64<E: de::Error>(self, _: f64) -> Result<Self::Value, E> {
        Ok(UniqueValue)
    }

    fn visit_str<E: de::Error>(self, _: &str) -> Result<Self::Value, E> {
        Ok(UniqueValue)
    }

    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(UniqueValue)
    }
}

// JSON syntax has already been checked above. This linear lexical pass keeps
// an overflowing integer token from silently becoming an approximate f64.
// Decimal/exponent tokens deliberately retain serde_json's f64 semantics.
fn validate_integer_tokens(raw: &str) -> Result<(), String> {
    let bytes = raw.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'"' => {
                index += 1;
                while index < bytes.len() {
                    match bytes[index] {
                        b'\\' => index += 2,
                        b'"' => {
                            index += 1;
                            break;
                        }
                        _ => index += 1,
                    }
                }
            }
            b'-' | b'0'..=b'9' => {
                let start = index;
                while index < bytes.len()
                    && matches!(bytes[index], b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E')
                {
                    index += 1;
                }
                let token = &raw[start..index];
                if !token.contains(['.', 'e', 'E']) {
                    let in_range = if token.starts_with('-') {
                        token.parse::<i64>().is_ok()
                    } else {
                        token.parse::<u64>().is_ok()
                    };
                    if !in_range {
                        return Err("integer literal is outside the i64/u64 domain".to_string());
                    }
                }
            }
            _ => index += 1,
        }
    }
    Ok(())
}
