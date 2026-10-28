use serde::{Deserialize, Deserializer};
use serde_json::Value;

pub fn null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

fn value_to_i64(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f.round() as i64)),
        Value::String(s) => {
            let s = s.trim();
            s.parse::<i64>()
                .ok()
                .or_else(|| s.parse::<f64>().ok().map(|f| f.round() as i64))
        }
        _ => None,
    }
}

fn parse_value<'de, D>(deserializer: D) -> Result<Option<Value>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<Value>::deserialize(deserializer)
}

pub fn tolerant_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(match parse_value(deserializer)? {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s,
        Some(other) => other.to_string(),
    })
}

pub fn tolerant_i64<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(parse_value(deserializer)?
        .as_ref()
        .and_then(value_to_i64)
        .unwrap_or(0))
}

pub fn tolerant_u8<'de, D>(deserializer: D) -> Result<u8, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(parse_value(deserializer)?
        .as_ref()
        .and_then(value_to_i64)
        .unwrap_or(0)
        .clamp(0, u8::MAX as i64) as u8)
}

pub fn tolerant_opt_i64<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(parse_value(deserializer)?.as_ref().and_then(value_to_i64))
}

pub fn tolerant_opt_i32<'de, D>(deserializer: D) -> Result<Option<i32>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(parse_value(deserializer)?
        .as_ref()
        .and_then(value_to_i64)
        .map(|v| v.clamp(i32::MIN as i64, i32::MAX as i64) as i32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Deserialize)]
    struct Case {
        #[serde(default, deserialize_with = "tolerant_i64")]
        a: i64,
        #[serde(default, deserialize_with = "tolerant_u8")]
        c: u8,
        #[serde(default, deserialize_with = "tolerant_opt_i64")]
        d: Option<i64>,
        #[serde(default, deserialize_with = "tolerant_opt_i32")]
        e: Option<i32>,
    }

    fn parse(v: serde_json::Value) -> Case {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn tolerant_numbers_accept_null_float_string() {
        let c = parse(serde_json::json!({"a": null, "c": 1.0, "d": "123", "e": 60.6}));
        assert_eq!(c.a, 0);
        assert_eq!(c.c, 1);
        assert_eq!(c.d, Some(123));
        assert_eq!(c.e, Some(61));

        let c = parse(serde_json::json!({}));
        assert_eq!((c.a, c.c, c.d, c.e), (0, 0, None, None));

        let c = parse(serde_json::json!({"a": 120.5, "c": null, "d": null, "e": "0.75"}));
        assert_eq!(c.a, 121);
        assert_eq!(c.c, 0);
        assert_eq!(c.d, None);
        assert_eq!(c.e, Some(1));

        let c = parse(serde_json::json!({"a": -3.6, "c": 300}));
        assert_eq!(c.a, -4);
        assert_eq!(c.c, 255);
    }

    #[test]
    fn null_default_helper() {
        #[derive(serde::Deserialize)]
        struct S {
            #[serde(default, deserialize_with = "null_default")]
            s: String,
            #[serde(default, deserialize_with = "null_default")]
            b: bool,
        }
        let v: S = serde_json::from_value(serde_json::json!({"s": null, "b": null})).unwrap();
        assert!(v.s.is_empty() && !v.b);
        let v: S = serde_json::from_value(serde_json::json!({})).unwrap();
        assert!(v.s.is_empty());
    }
}
