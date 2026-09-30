use crate::{
    client::HttpMethod,
    error::{AppError, Result},
    model::{AccessMode, Page},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::OnceLock};
pub mod schemas;
fn raw() -> &'static Value {
    static RAW: OnceLock<Value> = OnceLock::new();
    RAW.get_or_init(|| {
        serde_json::from_str(include_str!("data.json")).expect("reviewed catalog JSON")
    })
}
pub fn entities() -> &'static serde_json::Map<String, Value> {
    raw()["entities"].as_object().expect("entity catalog")
}
#[derive(Clone)]
pub struct OperationSpec {
    pub name: &'static str,
    pub route: &'static str,
    pub method: HttpMethod,
    pub schema: Value,
    pub mutates: bool,
    pub destructive: bool,
    pub description: String,
    pub path_params: BTreeMap<String, String>,
    pub body_keys: Vec<String>,
}
pub struct ValidatedCall {
    pub op: OperationSpec,
    pub args: Value,
    pub page: Page,
}
fn schema(properties: Value, required: Value) -> Value {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}
pub fn operations(mode: AccessMode) -> Vec<OperationSpec> {
    let mut ops = Vec::new();
    let enum_entities = entities().keys().collect::<Vec<_>>();
    let writable = entities()
        .iter()
        .filter(|(_, v)| v["writable"] == true)
        .map(|(n, _)| n)
        .collect::<Vec<_>>();
    for (name, method, route, description) in [
        (
            "records_describe",
            HttpMethod::Get,
            "/objects/{entity}",
            "Describe allowed household fields and required creation fields before changing records.",
        ),
        (
            "records_list",
            HttpMethod::Get,
            "/objects/{entity}",
            "List household records with actual data and pagination. Use entity and optional offset/limit.",
        ),
        (
            "records_lookup",
            HttpMethod::Get,
            "/objects/{entity}",
            "Find exact record names. Returns every matching candidate; never silently picks duplicate names.",
        ),
        (
            "records_get",
            HttpMethod::Get,
            "/objects/{entity}/{id}",
            "Read one household record by positive numeric ID.",
        ),
        (
            "records_create",
            HttpMethod::Post,
            "/objects/{entity}",
            "Create a household record using fields from records_describe.",
        ),
        (
            "records_update",
            HttpMethod::Put,
            "/objects/{entity}/{id}",
            "Edit only supplied fields. Stock-unit changes must use verified supported conversion procedures.",
        ),
        (
            "records_delete",
            HttpMethod::Delete,
            "/objects/{entity}/{id}",
            "Permanently delete the precise household entity and ID. Grocy constraints may reject dependent records.",
        ),
        (
            "userfields_get",
            HttpMethod::Get,
            "/userfields/{entity}/{id}",
            "Read custom-field values for a household record.",
        ),
        (
            "userfields_update",
            HttpMethod::Put,
            "/userfields/{entity}/{id}",
            "Update named custom-field values for a household record; discover their definitions using records_list userfields.",
        ),
    ] {
        let writes = method.mutates();
        if writes && mode == AccessMode::ReadOnly {
            continue;
        }
        let mut properties =
            json!({"entity":{"type":"string","enum":if writes{&writable}else{&enum_entities}}});
        let mut required = vec!["entity"];
        if route.contains("{id}") {
            properties["id"] = json!({"type":"integer","minimum":1});
            required.push("id");
        }
        if name == "records_lookup" {
            properties["name"] = json!({"type":"string","maxLength":256});
            required.push("name");
        }
        if name == "records_list" {
            properties["offset"] = json!({"type":"integer","minimum":0});
            properties["limit"] = json!({"type":"integer","minimum":1,"maximum":500});
        }
        if writes && method != HttpMethod::Delete {
            properties["fields"] = json!({"type":"object"});
            required.push("fields");
        }
        let mut params = BTreeMap::from([("entity".into(), "entity".into())]);
        if route.contains("{id}") {
            params.insert("id".into(), "id".into());
        }
        ops.push(OperationSpec {
            name,
            route,
            method,
            schema: schema(properties, json!(required)),
            mutates: writes,
            destructive: method == HttpMethod::Delete,
            description: description.into(),
            path_params: params,
            body_keys: vec![],
        });
    }
    for v in raw()["operations"].as_array().expect("operation catalog") {
        let method = match v["method"].as_str().unwrap() {
            "get" => HttpMethod::Get,
            "post" => HttpMethod::Post,
            "put" => HttpMethod::Put,
            "delete" => HttpMethod::Delete,
            _ => unreachable!(),
        };
        if method.mutates() && mode == AccessMode::ReadOnly {
            continue;
        }
        let name = v["name"].as_str().unwrap();
        let mut properties = v["properties"].clone();
        if name.starts_with("stock_")
            && ["amount", "new_amount"]
                .iter()
                .any(|k| properties.get(k).is_some())
        {
            properties["quantity_unit_id"] = json!({"type":"integer","minimum":1,"description":"Optional quantity unit ID. Omit to use this product's stock unit. A valid Grocy conversion is required."});
        }
        let path_params =
            serde_json::from_value(v["path_params"].clone()).expect("path parameters");
        let body_keys = serde_json::from_value(v["body_keys"].clone()).expect("body keys");
        let description = format!(
            "Grocy {}. Read IDs using record tools. Quantities use the product's stock unit unless quantity_unit_id is explicitly supplied with a valid conversion. Dates: YYYY-MM-DD; timestamps: RFC3339 or server-local YYYY-MM-DD HH:MM:SS. Paging defaults to 100, maximum 500. Writes return receipts and one readback where available. UNKNOWN_WRITE_OUTCOME means inspect current state before any repeat; never replay automatically. Recipe consumption can consume only available ingredients.",
            name.replace('_', " ")
        );
        ops.push(OperationSpec {
            name,
            route: v["route"].as_str().unwrap(),
            method,
            schema: schema(properties, v["required"].clone()),
            mutates: method.mutates(),
            destructive: name.contains("clear")
                || name.contains("merge")
                || name.contains("consume"),
            description,
            path_params,
            body_keys,
        });
    }
    ops
}
pub fn validate(name: &str, args: Value, mode: AccessMode) -> Result<ValidatedCall> {
    if serde_json::to_vec(&args)
        .map_err(|_| AppError::input())?
        .len()
        > crate::client::JSON_LIMIT
    {
        return Err(AppError::input());
    }
    let op = operations(mode)
        .into_iter()
        .find(|o| o.name == name)
        .ok_or_else(|| {
            AppError::new(
                "UNKNOWN_TOOL",
                "This tool is unavailable for the selected access mode.",
            )
        })?;
    schemas::check(&args, &op.schema)?;
    if let Some(amount) = args.get("amount") {
        if amount
            .as_number()
            .is_some_and(|n| n.as_f64().is_some_and(|n| n <= 0.0))
            || amount.as_str().is_some_and(|n| {
                n.parse::<rust_decimal::Decimal>()
                    .is_ok_and(|n| n <= rust_decimal::Decimal::ZERO)
            })
        {
            return Err(AppError::input());
        }
    }
    if name.starts_with("records_") || name.starts_with("userfields_") {
        let entity = args["entity"].as_str().ok_or_else(AppError::input)?;
        let spec = entities().get(entity).ok_or_else(AppError::input)?;
        if let Some(fields) = args.get("fields") {
            if name == "userfields_update" {
                if fields.as_object().is_none_or(|f| {
                    f.is_empty()
                        || f.keys()
                            .any(|k| k.is_empty() || k.len() > 128 || k.contains(['/', '\\', '\0']))
                }) {
                    return Err(AppError::input());
                }
            } else {
                if spec["writable"] != true {
                    return Err(AppError::input());
                }
                let required = if name == "records_create" {
                    spec["required"].clone()
                } else {
                    json!([])
                };
                schemas::check(fields, &schema(spec["fields"].clone(), required))?;
                if fields.as_object().is_none_or(|f| f.is_empty()) {
                    return Err(AppError::input());
                }
            }
        }
    }
    let offset = args["offset"].as_u64().unwrap_or(0);
    let limit = args["limit"].as_u64().unwrap_or(100);
    let page = Page::new(
        usize::try_from(offset).map_err(|_| AppError::input())?,
        usize::try_from(limit).map_err(|_| AppError::input())?,
    )?;
    Ok(ValidatedCall { op, args, page })
}
