use crate::error::{AppError, Result};
use serde_json::Value;
/// Validate `value` against the reviewed subset of JSON Schema used by the catalog.
/// Errors name the offending argument path so agents can correct just that value.
pub fn check(value: &Value, schema: &Value) -> Result<()> {
    check_at(value, schema, "arguments")
}
fn check_at(value: &Value, schema: &Value, path: &str) -> Result<()> {
    if value.is_null() && schema["nullable"] == true {
        return Ok(());
    }
    if let Some(en) = schema["enum"].as_array()
        && !en.contains(value)
    {
        let options = en.iter().map(Value::to_string).collect::<Vec<_>>();
        return Err(AppError::field(
            path,
            format!("must be one of {}", options.join(", ")),
        ));
    }
    let typ = schema["type"].as_str().unwrap_or("");
    match typ {
        "object" => {
            let obj = value
                .as_object()
                .ok_or_else(|| AppError::field(path, "must be an object"))?;
            let child = |k: &str| {
                if path == "arguments" {
                    k.to_string()
                } else {
                    format!("{path}.{k}")
                }
            };
            if let Some(req) = schema["required"].as_array() {
                for k in req.iter().filter_map(Value::as_str) {
                    if !obj.contains_key(k) {
                        return Err(AppError::field(&child(k), "is required"));
                    }
                }
            }
            if let Some(props) = schema["properties"].as_object() {
                for (k, v) in obj {
                    match props.get(k) {
                        Some(s) => check_at(v, s, &child(k))?,
                        None if schema["additionalProperties"] == false => {
                            let known = props.keys().take(40).cloned().collect::<Vec<_>>();
                            return Err(AppError::field(
                                &child(k),
                                format!("unknown field; allowed: {}", known.join(", ")),
                            ));
                        }
                        _ => (),
                    }
                }
            }
        }
        "array" => {
            let arr = value
                .as_array()
                .ok_or_else(|| AppError::field(path, "must be an array"))?;
            let max = schema["maxItems"].as_u64().unwrap_or(500) as usize;
            if arr.len() > max {
                return Err(AppError::field(path, format!("at most {max} items")));
            }
            for (i, item) in arr.iter().enumerate() {
                check_at(item, &schema["items"], &format!("{path}[{i}]"))?
            }
        }
        "integer" | "number" => {
            let kind = if typ == "integer" {
                "a whole number"
            } else {
                "a number"
            };
            if !value.is_number() && !(schema["allow_decimal_string"] == true && value.is_string())
            {
                return Err(AppError::field(path, format!("must be {kind}")));
            }
            let n = crate::tools::validation::decimal(value)
                .ok_or_else(|| AppError::field(path, format!("must be {kind}")))?;
            if typ == "integer" && !n.fract().is_zero() {
                return Err(AppError::field(path, "must be a whole number"));
            }
            if let Some(min) = schema["minimum"].as_i64()
                && n < min.into()
            {
                return Err(AppError::field(path, format!("must be >= {min}")));
            }
            if let Some(max) = schema["maximum"].as_i64()
                && n > max.into()
            {
                return Err(AppError::field(path, format!("must be <= {max}")));
            }
        }
        "string" => {
            let s = value
                .as_str()
                .ok_or_else(|| AppError::field(path, "must be a string"))?;
            let max = schema["maxLength"].as_u64().unwrap_or(16384) as usize;
            if s.len() > max {
                return Err(AppError::field(path, format!("at most {max} bytes")));
            }
            if s.contains('\0') {
                return Err(AppError::field(path, "must not contain NUL"));
            }
            match schema["format"].as_str() {
                Some("date") if chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_err() => {
                    return Err(AppError::field(path, "must be a real date YYYY-MM-DD"));
                }
                Some("date-time")
                    if chrono::DateTime::parse_from_rfc3339(s).is_err()
                        && chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
                            .is_err() =>
                {
                    return Err(AppError::field(
                        path,
                        "must be YYYY-MM-DD HH:MM:SS or RFC3339",
                    ));
                }
                _ => (),
            }
        }
        "boolean" if !value.is_boolean() => {
            return Err(AppError::field(path, "must be true or false"));
        }
        _ => (),
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn errors_name_the_field_and_accept_scientific_numbers() {
        let schema = json!({"type":"object","properties":{"amount":{"type":"number","minimum":0},"day":{"type":"string","format":"date"}},"required":["amount"],"additionalProperties":false});
        assert!(check(&json!({"amount":1e-7}), &schema).is_ok());
        assert!(check(&json!({"amount":2.5e3}), &schema).is_ok());
        let e = check(&json!({"amount":-1}), &schema).unwrap_err();
        assert_eq!(e.message, "amount: must be >= 0");
        let e = check(&json!({}), &schema).unwrap_err();
        assert_eq!(e.message, "amount: is required");
        let e = check(&json!({"amount":1,"day":"2026-02-30"}), &schema).unwrap_err();
        assert!(e.message.starts_with("day:"));
        let e = check(&json!({"amount":1,"bogus":1}), &schema).unwrap_err();
        assert!(e.message.starts_with("bogus: unknown field"));
    }
}
