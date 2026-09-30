pub mod help;
pub mod hermes;
mod wizard;
use crate::{
    client::{ApiClient, ApiRequest},
    credentials::CredentialStore,
    error::Result,
    model::ProfileSelection,
};
use serde_json::{Value, json};
use std::sync::Arc;
pub use wizard::{SetupOptions, SetupOutcome, SetupUi, run};
pub struct DoctorReport {
    pub version: Option<String>,
    pub server_time: Value,
    pub household_access: bool,
    pub household_defaults: Value,
}
pub async fn probe(client: &ApiClient) -> Result<DoctorReport> {
    let info = client
        .request(ApiRequest::get("/system/info"))
        .await?
        .json()?;
    let time = client
        .request(ApiRequest::get("/system/time"))
        .await?
        .json()?;
    let mut defaults = serde_json::Map::new();
    for entity in ["locations", "quantity_units", "shopping_lists"] {
        let mut r = ApiRequest::get("/objects/{entity}");
        r.params.insert("entity".into(), entity.into());
        r.query.insert("limit".into(), "100".into());
        let rows = client.request(r).await?.json()?;
        if !rows.is_array() {
            return Err(crate::error::AppError::new(
                "INVALID_RESPONSE",
                "Expected household metadata lists from the API.",
            ));
        }
        defaults.insert(entity.into(), rows);
    }
    Ok(DoctorReport {
        version: info["grocy_version"]["Version"]
            .as_str()
            .filter(|s| {
                s.len() <= 64
                    && s.chars()
                        .all(|c| c.is_ascii_alphanumeric() || ".-+".contains(c))
            })
            .map(str::to_owned),
        server_time: time,
        household_access: true,
        household_defaults: Value::Object(defaults),
    })
}
pub async fn doctor(s: ProfileSelection, store: Arc<dyn CredentialStore>) -> Result<DoctorReport> {
    let p = store.load(s).await?;
    let client = ApiClient::new(p)?;
    probe(&client).await
}
pub fn private_defaults(report: &DoctorReport) -> Value {
    let mut samples = serde_json::Map::new();
    for entity in ["locations", "quantity_units", "shopping_lists"] {
        let rows = report.household_defaults[entity].as_array().map(|rows| rows.iter().take(100).map(|r|json!({"id":r["id"].as_u64().or_else(||r["id"].as_str().and_then(|s|s.parse().ok())),"name":r["name"].as_str().unwrap_or("").chars().take(128).collect::<String>()})).collect::<Vec<_>>()).unwrap_or_default();
        samples.insert(entity.into(), json!(rows));
    }
    let time = if report.server_time.to_string().len() <= 4096 {
        report.server_time.clone()
    } else {
        Value::Null
    };
    json!({"grocy_version":report.version,"server_time":time,"household_defaults":samples})
}
