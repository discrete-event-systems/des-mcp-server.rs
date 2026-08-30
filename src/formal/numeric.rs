//! Compare parsed JSON numbers without rounding integer operands to f64.
use std::cmp::Ordering;

use serde_json::{Number, Value};

fn integer(number: &Number) -> Option<i128> {
    number
        .as_i64()
        .map(i128::from)
        .or_else(|| number.as_u64().map(i128::from))
}

pub(super) fn compare(left: &Number, right: &Number) -> Option<Ordering> {
    match (integer(left), integer(right)) {
        (Some(left), Some(right)) => Some(left.cmp(&right)),
        (Some(left), None) => integer_float(left, right.as_f64()?),
        (None, Some(right)) => integer_float(right, left.as_f64()?).map(Ordering::reverse),
        (None, None) => left.as_f64()?.partial_cmp(&right.as_f64()?),
    }
}

fn integer_float(integer: i128, float: f64) -> Option<Ordering> {
    if !float.is_finite() {
        return None;
    }
    // Both limits are exact powers of two. All accepted integer operands are
    // in [-2^63, 2^64-1], so out-of-range floats can be ordered without casting.
    if float < -9_223_372_036_854_775_808.0 {
        return Some(Ordering::Greater);
    }
    if float >= 18_446_744_073_709_551_616.0 {
        return Some(Ordering::Less);
    }
    // The range checks make this cast safe. Truncation is intentional; the
    // fractional part resolves the only case that truncation leaves tied.
    let truncated = float as i128;
    match integer.cmp(&truncated) {
        Ordering::Equal => 0.0f64.partial_cmp(&float.fract()),
        ordering => Some(ordering),
    }
}

pub(super) fn equal(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Number(left), Value::Number(right)) => {
            compare(left, right) == Some(Ordering::Equal)
        }
        (Value::Array(left), Value::Array(right)) => {
            left.len() == right.len() && left.iter().zip(right).all(|(a, b)| equal(a, b))
        }
        (Value::Object(left), Value::Object(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .all(|(key, value)| right.get(key).is_some_and(|other| equal(value, other)))
        }
        _ => left == right,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn comparison_is_symmetric_across_integer_and_float_boundaries() {
        let literals = [
            "-1e30", "-9223372036854775808", "-9007199254740993",
            "-9007199254740992.0", "-1.5", "-1", "-0.0", "0", "0.5", "1",
            "1.0", "9007199254740992.0", "9007199254740993",
            "18446744073709551615", "18446744073709551616.0", "1e30",
        ];
        for left in literals {
            for right in literals {
                let left: Number = serde_json::from_str(left).unwrap();
                let right: Number = serde_json::from_str(right).unwrap();
                assert_eq!(compare(&left, &right), compare(&right, &left).map(Ordering::reverse));
            }
        }
    }

    #[test]
    fn equality_uses_the_same_numeric_semantics_inside_arrays_and_objects() {
        assert!(equal(&json!({"a": [1, 0]}), &json!({"a": [1.0, -0.0]})));
        assert!(!equal(&json!([9007199254740993u64]), &json!([9007199254740992.0])));
    }
}
