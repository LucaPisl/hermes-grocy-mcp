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
/// Entities whose definitions change Grocy's schema (custom fields/entities, units).
/// Creating or editing them is gated behind `serve --allow-destructive`.
pub const SCHEMA_ENTITIES: [&str; 3] = ["userfields", "userentities", "quantity_units"];
/// Tools that delete or irreversibly merge data; gated behind `serve --allow-destructive`.
pub const DESTRUCTIVE_TOOLS: [&str; 5] = [
    "records_delete",
    "file_delete",
    "shopping_clear",
    "stock_merge",
    "chore_merge",
];
fn records_operations(mode: AccessMode) -> Vec<OperationSpec> {
    let mut ops = Vec::new();
    let enum_entities = entities().keys().collect::<Vec<_>>();
    let named = entities()
        .iter()
        .filter(|(_, v)| v["fields"].get("name").is_some())
        .map(|(n, _)| n)
        .collect::<Vec<_>>();
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
            "Show an entity's fields and which are required to create it.",
        ),
        (
            "records_list",
            HttpMethod::Get,
            "/objects/{entity}",
            "List records of an entity, 25 per page, with optional filter, order and field selection.",
        ),
        (
            "records_lookup",
            HttpMethod::Get,
            "/objects/{entity}",
            "Find records whose name contains the text (case-insensitive); exact matches first.",
        ),
        (
            "records_get",
            HttpMethod::Get,
            "/objects/{entity}/{id}",
            "Read one record by numeric ID.",
        ),
        (
            "records_create",
            HttpMethod::Post,
            "/objects/{entity}",
            "Create a record; see records_describe for fields.",
        ),
        (
            "records_update",
            HttpMethod::Put,
            "/objects/{entity}/{id}",
            "Change only the given fields of a record.",
        ),
        (
            "records_delete",
            HttpMethod::Delete,
            "/objects/{entity}/{id}",
            "Permanently delete one record (asks the human to confirm).",
        ),
        (
            "userfields_get",
            HttpMethod::Get,
            "/userfields/{entity}/{id}",
            "Read custom-field values of a record.",
        ),
        (
            "userfields_update",
            HttpMethod::Put,
            "/userfields/{entity}/{id}",
            "Set custom-field values of a record (stock entries need stock_transaction_id).",
        ),
    ] {
        let writes = method.mutates();
        if writes && mode == AccessMode::ReadOnly {
            continue;
        }
        let entity_enum = if name == "records_lookup" {
            &named
        } else if writes && !name.starts_with("userfields_") {
            &writable
        } else {
            &enum_entities
        };
        let mut properties = json!({"entity":{"type":"string","enum":entity_enum,"description":"Grocy entity (table) name, e.g. products, locations, shopping_list."}});
        let mut required = vec!["entity"];
        if route.contains("{id}") {
            properties["id"] = json!({"type":"integer","minimum":1,"description":"Record ID."});
            required.push("id");
        }
        if name == "records_lookup" {
            properties["name"] = json!({"type":"string","minLength":1,"maxLength":256,"description":"Text the name contains (case-insensitive)."});
            required.push("name");
        }
        if name == "records_list" {
            properties["offset"] = json!({"type":"integer","minimum":0,"description":"Skip this many records (paging)."});
            properties["limit"] = json!({"type":"integer","minimum":1,"maximum":500,"description":"Maximum records (default 25)."});
            properties["filter"] = json!({"type":"array","maxItems":10,"items":{"type":"string","maxLength":300},"description":"Conditions like \"name~milk\" (contains), \"location_id=2\", \"amount>1\". Operators: = != ~ !~ < > <= >=. All must match."});
            properties["order"] = json!({"type":"string","maxLength":80,"description":"Sort field, optionally with :asc or :desc, e.g. \"name\" or \"id:desc\"."});
        }
        if matches!(name, "records_list" | "records_lookup") {
            properties["fields"] = json!({"type":"array","maxItems":40,"items":{"type":"string","maxLength":64},"description":"Only return these fields (id is always included)."});
        }
        if name == "userfields_update" {
            properties["stock_transaction_id"] = json!({"type":"string","maxLength":128,"description":"Required only for entity stock: the purchase/inventory/entry-edit transaction ID of that exact batch."});
        }
        if writes && method != HttpMethod::Delete {
            properties["fields"] = json!({"type":"object","description":"Field values to write; see records_describe."});
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
            destructive: DESTRUCTIVE_TOOLS.contains(&name),
            description: description.into(),
            path_params: params,
            body_keys: vec![],
        });
    }
    ops
}
/// Every tool this access mode permits (full tier with destructive tools).
/// `exposed` narrows this to what a running server lists and accepts.
pub fn operations(mode: AccessMode) -> Vec<OperationSpec> {
    let mut ops = records_operations(mode);
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
            properties["quantity_unit_id"] = json!({"type":"integer","minimum":1,"description":"Unit ID the amount is given in; omit for the product's stock unit."});
        }
        let path_params =
            serde_json::from_value(v["path_params"].clone()).expect("path parameters");
        let body_keys = serde_json::from_value(v["body_keys"].clone()).expect("body keys");
        ops.push(OperationSpec {
            name,
            route: v["route"].as_str().unwrap(),
            method,
            schema: schema(properties, v["required"].clone()),
            mutates: method.mutates(),
            destructive: DESTRUCTIVE_TOOLS.contains(&name),
            description: v["description"].as_str().expect("description").into(),
            path_params,
            body_keys,
        });
    }
    ops.extend(crate::tools::files::operations(mode));
    ops.extend(crate::tools::lite::operations(mode));
    ops
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Tier {
    /// 17 task-shaped tools that accept names; best for small models.
    Lite,
    /// Every reviewed Grocy tool plus the lite tools.
    Full,
}
/// The tools a running server lists and accepts, given its tier and destructive opt-in.
pub fn exposed(mode: AccessMode, tier: Tier, allow_destructive: bool) -> Vec<OperationSpec> {
    let lite = crate::tools::lite::NAMES;
    operations(mode)
        .into_iter()
        .filter(|op| tier == Tier::Full || lite.contains(&op.name))
        .filter(|op| allow_destructive || !op.destructive)
        .map(|mut op| {
            if !allow_destructive
                && matches!(op.name, "records_create" | "records_update")
                && let Some(list) = op.schema["properties"]["entity"]["enum"].as_array_mut()
            {
                list.retain(|e| !SCHEMA_ENTITIES.contains(&e.as_str().unwrap_or("")));
            }
            op
        })
        .collect()
}
/// Whether `field` may be used in filters/order/projection for `entity`.
fn entity_field(entity: &str, field: &str) -> bool {
    let known = entities()
        .get(entity)
        .map_or(&Value::Null, |e| &e["fields"]);
    let plausible = !field.is_empty()
        && field.len() <= 64
        && field
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    plausible
        && (matches!(field, "id" | "row_created_timestamp")
            || known.get(field).is_some()
            || known.as_object().is_some_and(|f| f.is_empty()))
}
/// Split `field<op>value` into Grocy `query[]` syntax parts, validating the field name.
pub fn parse_filter(entity: &str, filter: &str) -> Result<(String, &'static str, String)> {
    let end = filter
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(filter.len());
    let (field, rest) = filter.split_at(end);
    let op = ["!=", "!~", "<=", ">=", "=", "~", "<", ">"]
        .into_iter()
        .find(|op| rest.starts_with(op))
        .ok_or_else(|| {
            AppError::field(
                "filter",
                format!("\"{filter}\" needs field, operator and value like \"name~milk\""),
            )
        })?;
    if !entity_field(entity, field) {
        return Err(AppError::field(
            "filter",
            format!("unknown field \"{field}\" for {entity}; see records_describe"),
        ));
    }
    Ok((field.into(), op, rest[op.len()..].into()))
}
pub fn validate(name: &str, args: Value, mode: AccessMode) -> Result<ValidatedCall> {
    validate_in(&operations(mode), name, args)
}
/// Validate a call against an explicit tool list (the server's exposed tools).
pub fn validate_in(ops: &[OperationSpec], name: &str, args: Value) -> Result<ValidatedCall> {
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
    let op = ops
        .iter()
        .find(|o| o.name == name)
        .cloned()
        .ok_or_else(|| {
            AppError::new(
                "UNKNOWN_TOOL",
                "This tool is not offered by this server (access mode, tool tier or destructive opt-in). Use a listed tool.",
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
        && crate::tools::validation::decimal(amount)
            .is_some_and(|n| n <= rust_decimal::Decimal::ZERO)
    {
        return Err(AppError::field("amount", "must be greater than 0"));
    }
    if name.starts_with("records_") || name.starts_with("userfields_") {
        let entity = args["entity"].as_str().ok_or_else(AppError::input)?;
        let spec = entities().get(entity).ok_or_else(AppError::input)?;
        if let Some(fields) = args.get("fields").filter(|_| op.mutates) {
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
    if matches!(name, "records_list" | "records_lookup") {
        let entity = args["entity"].as_str().unwrap_or("");
        for f in args["filter"].as_array().into_iter().flatten() {
            parse_filter(entity, f.as_str().unwrap_or(""))?;
        }
        if let Some(order) = args["order"].as_str() {
            let (field, dir) = order.split_once(':').unwrap_or((order, "asc"));
            if !entity_field(entity, field) || !matches!(dir, "asc" | "desc") {
                return Err(AppError::field(
                    "order",
                    format!("use a field of {entity} with optional :asc/:desc, e.g. \"name\""),
                ));
            }
        }
        for f in args["fields"].as_array().into_iter().flatten() {
            if !entity_field(entity, f.as_str().unwrap_or("")) {
                return Err(AppError::field(
                    "fields",
                    format!("unknown field {f} for {entity}; see records_describe"),
                ));
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
    let limit = args["limit"]
        .as_u64()
        .unwrap_or(Page::default().limit as u64);
    let page = Page::new(
        usize::try_from(offset).map_err(|_| AppError::input())?,
        usize::try_from(limit).map_err(|_| AppError::input())?,
    )?;
    Ok(ValidatedCall { op, args, page })
}
