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
        defaults.insert(entity.into(), client.request(r).await?.json()?);
    }
    Ok(DoctorReport {
        version: info["grocy_version"]["Version"].as_str().map(str::to_owned),
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
    json!({"grocy_version":report.version,"server_time":report.server_time,"household_defaults":report.household_defaults})
}
