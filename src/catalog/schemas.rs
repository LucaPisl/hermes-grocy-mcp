use crate::error::{AppError, Result};
use rust_decimal::Decimal;
use serde_json::Value;
use std::str::FromStr;
pub fn check(value: &Value, schema: &Value) -> Result<()> {
    if value.is_null() && schema["nullable"] == true {
        return Ok(());
    }
    if let Some(en) = schema["enum"].as_array() {
        if !en.contains(value) {
            return Err(AppError::input());
        }
    }
    let typ = schema["type"].as_str().unwrap_or("");
    match typ {
        "object" => {
            let obj = value.as_object().ok_or_else(AppError::input)?;
            if let Some(req) = schema["required"].as_array() {
                for k in req {
                    if !obj.contains_key(k.as_str().ok_or_else(AppError::input)?) {
                        return Err(AppError::input());
                    }
                }
            }
            if let Some(props) = schema["properties"].as_object() {
                for (k, v) in obj {
                    match props.get(k) {
                        Some(s) => check(v, s)?,
                        None if schema["additionalProperties"] == false => {
                            return Err(AppError::input());
                        }
                        _ => (),
                    }
                }
            }
        }
        "array" => {
            let arr = value.as_array().ok_or_else(AppError::input)?;
            if arr.len() > schema["maxItems"].as_u64().unwrap_or(500) as usize {
                return Err(AppError::input());
            }
            for item in arr {
                check(item, &schema["items"])?
            }
        }
        "integer" | "number" => {
            let s = if value.is_number() {
                value.to_string()
            } else if schema["allow_decimal_string"] == true {
                value.as_str().ok_or_else(AppError::input)?.to_string()
            } else {
                return Err(AppError::input());
            };
            let n = Decimal::from_str(&s).map_err(|_| AppError::input())?;
            if typ == "integer" && n.fract() != Decimal::ZERO {
                return Err(AppError::input());
            }
            if let Some(min) = schema["minimum"].as_i64() {
                if n < Decimal::from(min) {
                    return Err(AppError::input());
                }
            }
            if let Some(max) = schema["maximum"].as_i64() {
                if n > Decimal::from(max) {
                    return Err(AppError::input());
                }
            }
        }
        "string" => {
            let s = value.as_str().ok_or_else(AppError::input)?;
            if s.len() > schema["maxLength"].as_u64().unwrap_or(16384) as usize || s.contains('\0')
            {
                return Err(AppError::input());
            }
            match schema["format"].as_str() {
                Some("date") => {
                    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
                        .map_err(|_| AppError::input())?;
                }
                Some("date-time") => {
                    if chrono::DateTime::parse_from_rfc3339(s).is_err()
                        && chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").is_err()
                    {
                        return Err(AppError::input());
                    }
                }
                _ => (),
            }
        }
        "boolean" if !value.is_boolean() => return Err(AppError::input()),
        _ => (),
    }
    Ok(())
}
