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
/// One `doctor` finding. Details never contain the API key or server address.
pub fn check(name: &str, status: &str, detail: impl Into<String>) -> Value {
    json!({"check":name,"status":status,"detail":detail.into()})
}
/// Grocy feature flags that decide whether tool groups work.
const FEATURES: [(&str, &str); 7] = [
    ("FEATURE_FLAG_STOCK", "stock tools"),
    ("FEATURE_FLAG_SHOPPINGLIST", "shopping tools"),
    ("FEATURE_FLAG_RECIPES", "recipe tools"),
    ("FEATURE_FLAG_RECIPES_MEALPLAN", "meal plan tools"),
    ("FEATURE_FLAG_CHORES", "chore tools"),
    ("FEATURE_FLAG_TASKS", "task tools"),
    ("FEATURE_FLAG_BATTERIES", "battery tools"),
];
/// Read-only API checks shared by the `doctor` CLI and MCP tool. Makes no household changes.
pub async fn api_checks(client: &ApiClient) -> Vec<Value> {
    let mut checks = vec![];
    match client
        .request(ApiRequest::get("/system/info"))
        .await
        .and_then(|r| r.json())
    {
        Ok(info) => {
            checks.push(check(
                "grocy reachable",
                "pass",
                "TLS verified and API key accepted",
            ));
            let version = info["grocy_version"]["Version"]
                .as_str()
                .filter(|s| {
                    s.len() <= 64
                        && s.chars()
                            .all(|c| c.is_ascii_alphanumeric() || ".-+".contains(c))
                })
                .unwrap_or("unknown");
            let tested = version.starts_with("4.");
            checks.push(check(
                "grocy version",
                if tested { "pass" } else { "warn" },
                if tested {
                    format!("Grocy {version}")
                } else {
                    format!("Grocy {version}; this server is tested with Grocy 4.x")
                },
            ));
        }
        Err(e) => {
            let name = if e.code == "ACCESS_DENIED" {
                "grocy authenticated"
            } else {
                "grocy reachable"
            };
            checks.push(check(name, "fail", e.message.to_string()));
            return checks;
        }
    }
    match client
        .request(ApiRequest::get("/system/config"))
        .await
        .and_then(|r| r.json())
    {
        Ok(config) => {
            for (flag, tools) in FEATURES {
                if config[flag] == false {
                    checks.push(check(
                        flag,
                        "warn",
                        format!("disabled in Grocy; {tools} will fail"),
                    ));
                }
            }
            if !checks.iter().any(|c| {
                c["status"] == "warn"
                    && c["check"]
                        .as_str()
                        .is_some_and(|n| n.starts_with("FEATURE_"))
            }) {
                checks.push(check(
                    "grocy features",
                    "pass",
                    "stock, shopping, recipes, meal plan, chores, tasks and batteries enabled",
                ));
            }
            let printer = config["FEATURE_FLAG_LABEL_PRINTER"] == true;
            checks.push(check(
                "label printer",
                "pass",
                if printer {
                    "enabled in Grocy; this server never requests labels unless stock_label_type is passed"
                } else {
                    "disabled; no labels are printed"
                },
            ));
        }
        Err(e) => checks.push(check("grocy config", "warn", e.message.to_string())),
    }
    checks.push(match client.access() {
        crate::model::AccessMode::ReadOnly => check(
            "access mode",
            "warn",
            "read-only profile: write tools are hidden and refused",
        ),
        crate::model::AccessMode::Full => {
            check("access mode", "pass", "full access (writes allowed)")
        }
    });
    checks
}
/// Full local diagnosis for the CLI: wallet, profile, key presence, then API checks.
pub async fn doctor_checks(
    selection: Result<ProfileSelection>,
    store: Arc<dyn CredentialStore>,
) -> Vec<Value> {
    let mut checks = vec![];
    match crate::credentials::providers::discover_providers() {
        Ok(found) if !found.is_empty() => checks.push(check(
            "secret service",
            "pass",
            format!("{} wallet provider(s) found", found.len()),
        )),
        Ok(_) => checks.push(check(
            "secret service",
            "fail",
            "no Secret Service wallet is running; start KWallet or GNOME Keyring",
        )),
        Err(e) => checks.push(check("secret service", "fail", e.message.to_string())),
    }
    let selection = match selection {
        Ok(s) => {
            checks.push(check(
                "profile",
                "pass",
                format!("profile '{}' enrolled", s.profile),
            ));
            s
        }
        Err(e) => {
            checks.push(check(
                "profile",
                "fail",
                format!("{} Run setup first.", e.message),
            ));
            return checks;
        }
    };
    let profile = match store.load(selection).await {
        Ok(p) => {
            checks.push(check("wallet unlocked", "pass", "profile item readable"));
            p
        }
        Err(e) => {
            checks.push(check("wallet unlocked", "fail", e.message.to_string()));
            return checks;
        }
    };
    use secrecy::ExposeSecret;
    if profile.api_key.expose_secret().is_empty() {
        checks.push(check(
            "api key",
            "fail",
            "the stored profile has no API key; run setup again",
        ));
        return checks;
    }
    checks.push(check("api key", "pass", "present (not shown)"));
    match ApiClient::new(profile) {
        Ok(client) => checks.extend(api_checks(&client).await),
        Err(e) => checks.push(check("tls client", "fail", e.message.to_string())),
    }
    checks
}
