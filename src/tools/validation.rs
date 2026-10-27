use crate::{
    error::{AppError, Result},
    model::RecordId,
};
use rust_decimal::Decimal;
use serde_json::{Value, json};
use std::str::FromStr;
pub enum RecordSelector {
    Id(RecordId),
    ExactName(String),
}
pub struct Conversion {
    pub from: RecordId,
    pub to: RecordId,
    pub factor: Decimal,
}
/// Parse a Grocy/agent quantity: JSON number (including `1e-7` notation) or decimal string.
pub fn decimal(value: &Value) -> Option<Decimal> {
    let text = match value {
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.trim().to_string(),
        _ => return None,
    };
    Decimal::from_str(&text)
        .or_else(|_| Decimal::from_scientific(&text))
        .ok()
        .or_else(|| value.as_f64().and_then(Decimal::from_f64_retain))
        .map(|d| d.normalize())
}
pub fn record_id(value: &Value) -> Result<RecordId> {
    let id = value
        .as_u64()
        .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
        .ok_or_else(AppError::input)?;
    if id == 0 {
        return Err(AppError::input());
    }
    Ok(RecordId(id))
}
pub fn resolve_record(rows: &[Value], selector: &RecordSelector) -> Result<RecordId> {
    let matches = rows
        .iter()
        .filter(|row| match selector {
            RecordSelector::Id(id) => record_id(&row["id"]).is_ok_and(|n| n == *id),
            RecordSelector::ExactName(name) => row["name"].as_str() == Some(name),
        })
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [row] => record_id(&row["id"]),
        [] => Err(AppError::new(
            "NOT_FOUND",
            "No exact record matches. List the relevant records first.",
        )),
        _ => Err(AppError::new(
            "AMBIGUOUS_RECORD",
            "Multiple records have this name. Choose a candidate ID explicitly.",
        )
        .with(
            "candidates",
            json!(
                matches
                    .iter()
                    .map(|r| json!({"id":r["id"],"name":r["name"]}))
                    .collect::<Vec<_>>()
            ),
        )),
    }
}
pub fn convert_quantity(
    amount: Decimal,
    from: RecordId,
    to: RecordId,
    conversions: &[Conversion],
) -> Result<Decimal> {
    if amount < Decimal::ZERO {
        return Err(AppError::input());
    }
    if from == to {
        return Ok(amount);
    }
    let matches = conversions
        .iter()
        .filter(|c| c.from == from && c.to == to && c.factor > Decimal::ZERO)
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [c] => amount.checked_mul(c.factor).ok_or_else(AppError::input),
        _ => Err(AppError::new(
            "UNIT_CONVERSION_REQUIRED",
            "Choose a valid unambiguous Grocy conversion to the product's stock unit.",
        )),
    }
}

pub async fn load_conversions(
    client: &crate::client::ApiClient,
    product_id: RecordId,
) -> Result<Vec<Conversion>> {
    let mut r = crate::client::ApiRequest::get("/objects/{entity}");
    r.params
        .insert("entity".into(), "quantity_unit_conversions_resolved".into());
    let data = client.request(r).await?.json()?;
    let mut output = Vec::new();
    for row in data.as_array().ok_or_else(AppError::input)? {
        if record_id(&row["product_id"]).is_ok_and(|id| id == product_id) {
            let factor = decimal(&row["factor"]).ok_or_else(|| {
                AppError::new(
                    "INVALID_RESPONSE",
                    "Grocy returned an unreadable unit factor.",
                )
            })?;
            output.push(Conversion {
                from: record_id(&row["from_qu_id"])?,
                to: record_id(&row["to_qu_id"])?,
                factor,
            });
        }
    }
    Ok(output)
}
