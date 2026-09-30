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
pub fn household_field_target(target: &str) -> bool {
    entities().contains_key(target)
        || (target.starts_with("userentity-")
            && target.len() > 11
            && !target.contains(['/', '\\', '%'])
            && !target.chars().any(char::is_control))
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
            "Update defined household custom fields. For userobjects, the custom entity is resolved automatically. For stock, id is the numeric entry row ID, and stock_transaction_id must come from the exact batch's purchase/inventory-correction/stock-edit-new receipt or history; Grocy writes batch fields via that transaction. No account fields.",
        ),
    ] {
        let writes = method.mutates();
        if writes && mode == AccessMode::ReadOnly {
            continue;
        }
        let mut properties = json!({"entity":{"type":"string","enum":if writes && !name.starts_with("userfields_"){&writable}else{&enum_entities}}});
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
        if name == "userfields_update" {
            properties["stock_transaction_id"] = json!({"type":"string","maxLength":128,"description":"Required only for entity stock. Exact purchase/inventory-correction/stock-edit-new transaction ID for the selected entry's batch. Read receipts/history; the transaction must refer only to that batch."});
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
        let description = if name == "recipe_consume" {
            format!(
                "{description} Grocy returns no booking IDs for this action. Inspect stock_log records filtered by recipe_id and time to identify transaction IDs before requesting undo; never guess under concurrent activity."
            )
        } else if name == "stock_entry_put" {
            format!(
                "{description} Omitted entry fields are preserved using a preflight read; concurrent external edits may race this update."
            )
        } else {
            description
        };
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
    ops.extend(crate::tools::files::operations(mode));
    ops
}
pub fn validate(name: &str, args: Value, mode: AccessMode) -> Result<ValidatedCall> {
    if serde_json::to_vec(&args)
        .map_err(|_| AppError::input())?
        .len()
        > if name == "file_upload" {
            12 * 1024 * 1024
        } else {
            crate::client::JSON_LIMIT
        }
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
    if name == "userfields_update"
        && args["entity"] == "stock"
        && args["stock_transaction_id"]
            .as_str()
            .is_none_or(|s| s.is_empty() || !s.chars().all(|c| c.is_ascii_hexdigit()))
    {
        return Err(AppError::new(
            "STOCK_TRANSACTION_REQUIRED",
            "Stock batch fields require the exact purchase, inventory-correction or entry-edit transaction ID from receipts/history.",
        ));
    }
    if name == "stock_merge" || name == "chore_merge" {
        let prefix = if name == "stock_merge" {
            "product"
        } else {
            "chore"
        };
        if args[format!("{prefix}_id_to_keep")] == args[format!("{prefix}_id_to_remove")] {
            return Err(AppError::input());
        }
    }
    if let Some(amount) = args.get("amount")
        && (amount
            .as_number()
            .is_some_and(|n| n.as_f64().is_some_and(|n| n <= 0.0))
            || amount.as_str().is_some_and(|n| {
                n.parse::<rust_decimal::Decimal>()
                    .is_ok_and(|n| n <= rust_decimal::Decimal::ZERO)
            }))
    {
        return Err(AppError::input());
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
                if entity == "userfields"
                    && let Some(target) = fields.get("entity")
                {
                    let target = target.as_str().ok_or_else(AppError::input)?;
                    if !household_field_target(target) {
                        return Err(AppError::input());
                    }
                }
                if entity == "quantity_unit_conversions"
                    && fields
                        .get("factor")
                        .is_some_and(|v| v.as_f64().is_none_or(|n| n <= 0.0))
                {
                    return Err(AppError::input());
                }
                if fields.as_object().is_none_or(|f| f.is_empty()) {
                    return Err(AppError::input());
                }
            }
        }
    }
    if name.starts_with("file_") {
        crate::tools::files::validate_filename(
            args["filename"].as_str().ok_or_else(AppError::input)?,
        )?;
    }
    for key in op.path_params.keys() {
        if let Some(s) = args[key].as_str()
            && (s.contains(['/', '\\', '%']) || matches!(s, "." | ".."))
        {
            return Err(AppError::input());
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
