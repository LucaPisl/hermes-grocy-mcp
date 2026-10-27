//! Lite tier: task-shaped tools that accept names and delegate every write to the reviewed
//! full-tier operation (same validation, unit conversion and read-back verification).
use super::{
    actions, records,
    validation::{convert_quantity, decimal, load_conversions, record_id},
};
use crate::{
    catalog::{self, OperationSpec, ValidatedCall},
    client::{ApiClient, ApiRequest, HttpMethod},
    error::{AppError, Result},
    model::{AccessMode, ToolOutput},
};
use chrono::{Datelike, Days, Local};
use rust_decimal::Decimal;
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub const NAMES: [&str; 17] = [
    "find_product",
    "whats_low_or_expiring",
    "add_stock",
    "consume_stock",
    "open_product",
    "inventory_set",
    "create_product",
    "shopping_list",
    "shopping_add",
    "shopping_remove",
    "recipes_suggest",
    "todo_list",
    "mark_done",
    "meal_plan",
    "meal_plan_add",
    "undo",
    "doctor",
];
/// How many candidates an ambiguous or unknown name returns.
const CANDIDATES: usize = 8;

fn name_or_id(what: &str) -> Value {
    json!({"type":["string","integer"],"maxLength":200,"description":format!("{what} name, or its id")})
}
fn amount(description: &str) -> Value {
    json!({"type":"number","minimum":0,"allow_decimal_string":true,"description":description})
}
fn date(description: &str) -> Value {
    json!({"type":"string","format":"date","maxLength":32,"description":description})
}
fn text(description: &str) -> Value {
    json!({"type":"string","maxLength":2000,"description":description})
}
pub fn operations(mode: AccessMode) -> Vec<OperationSpec> {
    let product = || name_or_id("Product (name, barcode)");
    let unit = || json!({"type":"string","maxLength":100,"description":"Unit the amount is in, e.g. \"Pack\"; omit for the product's stock unit."});
    let location = || name_or_id("Location");
    let list = || name_or_id("Shopping list (default: the main list)");
    let specs: [(&str, bool, &str, Value, &[&str]); 17] = [
        (
            "find_product",
            false,
            "Find products by name, barcode or id, with current stock and unit.",
            json!({"query":{"type":["string","integer"],"maxLength":200,"description":"Name, part of a name, barcode or id."}}),
            &["query"],
        ),
        (
            "whats_low_or_expiring",
            false,
            "Products expiring soon, overdue, expired, or below their minimum stock.",
            json!({"days":{"type":"integer","minimum":0,"maximum":365,"description":"Look this many days ahead (default 5)."}}),
            &[],
        ),
        (
            "add_stock",
            true,
            "Add purchased stock of an existing product.",
            json!({"product":product(),"amount":amount("How much was bought."),"unit":unit(),
                "best_before_date":date("Due date YYYY-MM-DD; omit for the product default."),
                "price":amount("Price per stock unit."),"location":location(),"note":text("Free-text note.")}),
            &["product", "amount"],
        ),
        (
            "consume_stock",
            true,
            "Use up (or throw away) stock of a product.",
            json!({"product":product(),"amount":amount("How much was used."),"unit":unit(),
                "spoiled":{"type":"boolean","description":"True if it was thrown away."},"location":location()}),
            &["product", "amount"],
        ),
        (
            "open_product",
            true,
            "Mark stock of a product as opened.",
            json!({"product":product(),"amount":amount("How much was opened (default 1)."),"unit":unit()}),
            &["product"],
        ),
        (
            "inventory_set",
            true,
            "Correct the total stock of a product after counting it.",
            json!({"product":product(),"new_amount":amount("The counted total."),"unit":unit(),"location":location(),
                "best_before_date":date("Due date YYYY-MM-DD for added stock.")}),
            &["product", "new_amount"],
        ),
        (
            "create_product",
            true,
            "Create a new product (only after the human confirms it is new).",
            json!({"name":text("Product name."),"location":location(),
                "stock_unit":{"type":"string","maxLength":100,"description":"Unit stock is counted in (default: Piece)."},
                "min_stock_amount":amount("Warn when stock falls below this."),
                "default_best_before_days":{"type":"integer","minimum":-1,"maximum":36500,"description":"Days until due after purchase; -1 never."}}),
            &["name"],
        ),
        (
            "shopping_list",
            false,
            "Show a shopping list with product names, amounts and units.",
            json!({"list":list()}),
            &[],
        ),
        (
            "shopping_add",
            true,
            "Put a product on the shopping list.",
            json!({"product":product(),"amount":amount("How much to buy (default 1)."),"unit":unit(),"list":list(),"note":text("Free-text note.")}),
            &["product"],
        ),
        (
            "shopping_remove",
            true,
            "Take a product off the shopping list (all of it unless amount is given).",
            json!({"product":product(),"amount":amount("How much to remove, in the stock unit (default: all)."),"list":list()}),
            &["product"],
        ),
        (
            "recipes_suggest",
            false,
            "Which recipes can be cooked with current stock, or what one recipe is missing.",
            json!({"recipe":name_or_id("Recipe; omit to rank all recipes")}),
            &[],
        ),
        (
            "todo_list",
            false,
            "Chores due and open tasks.",
            json!({"days":{"type":"integer","minimum":0,"maximum":3650,"description":"Include chores due within this many days (default 7)."}}),
            &[],
        ),
        (
            "mark_done",
            true,
            "Record a chore as done or complete a task (give exactly one).",
            json!({"chore":name_or_id("Chore"),"task":name_or_id("Task"),
                "tracked_time":{"type":"string","format":"date-time","maxLength":32,"description":"When it was done, YYYY-MM-DD HH:MM:SS; omit for now."}}),
            &[],
        ),
        (
            "meal_plan",
            false,
            "Show the meal plan (default: this week, Monday to Sunday).",
            json!({"from":date("First day YYYY-MM-DD."),"to":date("Last day YYYY-MM-DD.")}),
            &[],
        ),
        (
            "meal_plan_add",
            true,
            "Plan a recipe, a product or a note for a day.",
            json!({"day":date("Day YYYY-MM-DD."),"recipe":name_or_id("Recipe"),"product":product(),"note":text("Note, or the entry itself when no recipe/product."),
                "servings":amount("Recipe servings (default 1)."),"amount":amount("Product amount in its stock unit (default 1)."),
                "section":name_or_id("Meal plan section, e.g. Dinner")}),
            &["day"],
        ),
        (
            "undo",
            true,
            "Revert the last change made through this server, or one stock transaction_id.",
            json!({"transaction_id":{"type":"string","maxLength":128,"description":"A transaction_id from an earlier stock result; omit for the last change."}}),
            &[],
        ),
        (
            "doctor",
            false,
            "Check the Grocy connection, key, version and features (pass/warn/fail).",
            json!({}),
            &[],
        ),
    ];
    specs
        .into_iter()
        .filter(|(_, mutates, ..)| !mutates || mode == AccessMode::Full)
        .map(
            |(name, mutates, description, properties, required)| OperationSpec {
                name,
                route: "",
                method: if mutates {
                    HttpMethod::Post
                } else {
                    HttpMethod::Get
                },
                schema: json!({"type":"object","properties":properties,"required":required,"additionalProperties":false}),
                mutates,
                destructive: false,
                description: description.into(),
                path_params: BTreeMap::new(),
                body_keys: vec![],
            },
        )
        .collect()
}

/// The full-tier call that reverts a completed write, if Grocy offers one.
/// Lite writes report their delegated call as `target {tool, args}`.
pub fn undo_for(name: &str, args: &Value, receipt: &Value) -> Option<Value> {
    if name.contains("undo") {
        return None;
    }
    let tx = &receipt.get(0).unwrap_or(receipt)["transaction_id"];
    let (tool, args) = match name {
        _ if name.starts_with("stock_") && tx.is_string() => {
            ("stock_undo_transaction", json!({"transaction_id":tx}))
        }
        "chore_execute" if receipt["id"].is_u64() => (
            "chore_undo_execution",
            json!({"execution_id":receipt["id"]}),
        ),
        "battery_charge" if receipt["id"].is_u64() => (
            "battery_undo_charge",
            json!({"charge_cycle_id":receipt["id"]}),
        ),
        "task_complete" => ("task_undo", json!({"task_id":args["task_id"]})),
        _ => return None,
    };
    Some(json!({"tool":tool,"args":args}))
}

pub async fn run(client: &ApiClient, call: &ValidatedCall) -> Result<ToolOutput> {
    let a = &call.args;
    match call.op.name {
        "find_product" => find_product(client, a).await,
        "whats_low_or_expiring" => low_or_expiring(client, a).await,
        "add_stock" | "consume_stock" | "open_product" | "inventory_set" => {
            stock_write(client, call.op.name, a).await
        }
        "create_product" => create_product(client, a).await,
        "shopping_list" => {
            let list = shopping_list_row(client, a).await?;
            let items = shopping_items(client, &list["id"], None).await?;
            Ok(ToolOutput::data(
                json!({"list":{"id":list["id"],"name":list["name"]},"count":items.len(),"items":items}),
            ))
        }
        "shopping_add" | "shopping_remove" => shopping_write(client, call.op.name, a).await,
        "recipes_suggest" => recipes_suggest(client, a).await,
        "todo_list" => todo_list(client, a).await,
        "mark_done" => mark_done(client, a).await,
        "meal_plan" => meal_plan(client, a).await,
        "meal_plan_add" => meal_plan_add(client, a).await,
        _ => Err(AppError::input()),
    }
}

// ---- Grocy access helpers ----

async fn get(client: &ApiClient, route: &'static str, params: &[(&str, &Value)]) -> Result<Value> {
    let mut r = ApiRequest::get(route);
    for (k, v) in params {
        r.params.insert((*k).into(), plain(v));
    }
    client.request(r).await?.json()
}
async fn list(client: &ApiClient, entity: &str, filters: Vec<String>) -> Result<Vec<Value>> {
    let mut r = ApiRequest::get("/objects/{entity}");
    r.params.insert("entity".into(), entity.into());
    r.filters = filters;
    Ok(client
        .request(r)
        .await?
        .json()?
        .as_array()
        .cloned()
        .unwrap_or_default())
}
fn plain(v: &Value) -> String {
    v.as_str().map_or_else(|| v.to_string(), str::to_owned)
}
fn same_id(a: &Value, b: &Value) -> bool {
    matches!((record_id(a), record_id(b)), (Ok(x), Ok(y)) if x == y)
}
/// Drop null fields so small models see only what is set.
fn compact(mut v: Value) -> Value {
    if let Some(o) = v.as_object_mut() {
        o.retain(|_, x| !x.is_null());
    }
    v
}
/// Singular or plural unit name for an amount.
fn unit_label(unit: &Value, amount: &Value) -> Value {
    let plural = unit["name_plural"].as_str().filter(|p| !p.is_empty());
    match plural {
        Some(p) if decimal(amount) != Some(Decimal::ONE) => json!(p),
        _ => unit["name"].clone(),
    }
}
fn unit_of(units: &[Value], id: &Value, amount: &Value) -> Value {
    units
        .iter()
        .find(|u| same_id(&u["id"], id))
        .map_or(Value::Null, |u| unit_label(u, amount))
}
fn find<'a>(rows: &'a [Value], id: &Value) -> &'a Value {
    rows.iter()
        .find(|r| same_id(&r["id"], id))
        .unwrap_or(&Value::Null)
}

/// Resolve a name, barcode or id to one row: id, then barcode (products), then a unique
/// case-insensitive exact name, then a unique substring. Otherwise an error with candidates.
async fn resolve(client: &ApiClient, entity: &str, field: &str, value: &Value) -> Result<Value> {
    let text = plain(value).trim().to_string();
    if text.is_empty() {
        return Err(AppError::field(field, "must not be empty"));
    }
    if text.parse::<u64>().is_ok_and(|id| id > 0) {
        match records::get_record(client, entity, &json!(text)).await {
            Ok(row) => return Ok(row),
            Err(e) if e.code != "NOT_FOUND" => return Err(e),
            Err(_) => (),
        }
        if entity == "products"
            && let Some(hit) = list(client, "product_barcodes", vec![format!("barcode={text}")])
                .await?
                .first()
        {
            return records::get_record(client, "products", &hit["product_id"]).await;
        }
    }
    let mut filters = match entity {
        "recipes" => vec!["type=normal".to_string()],
        "tasks" => vec!["done=0".to_string()],
        _ => vec![],
    };
    // Units also match their plural, which a server-side name filter cannot express.
    if entity != "quantity_units" {
        filters.push(format!("name~{text}"));
    }
    let lower = text.to_lowercase();
    let names = |r: &Value| {
        ["name", "name_plural"]
            .into_iter()
            .filter_map(|k| r[k].as_str().map(str::to_lowercase))
            .collect::<Vec<_>>()
    };
    let rows = list(client, entity, filters).await?;
    let hits = rows
        .iter()
        .filter(|r| names(r).iter().any(|n| n.contains(&lower)))
        .collect::<Vec<_>>();
    let exact = hits
        .iter()
        .filter(|r| names(r).contains(&lower))
        .copied()
        .collect::<Vec<_>>();
    match (exact.as_slice(), hits.as_slice()) {
        ([one], _) | ([], [one]) => return Ok((*one).clone()),
        _ => (),
    }
    let label = entity.replace('_', " ");
    let (error, shown) = if exact.len() > 1 || hits.len() > 1 {
        let shown = if exact.len() > 1 { exact } else { hits };
        (
            AppError::new(
                "AMBIGUOUS_RECORD",
                format!(
                    "{field}: \"{text}\" matches several {label}; retry with the id of one candidate"
                ),
            ),
            shown,
        )
    } else if entity == "products" {
        (
            AppError::new(
                "NOT_FOUND",
                format!(
                    "{field}: no product matches \"{text}\"; try find_product with a shorter word"
                ),
            ),
            vec![],
        )
    } else {
        // Small lookup lists (units, locations, lists, sections): show what exists.
        (
            AppError::new(
                "NOT_FOUND",
                format!("{field}: no {label} matches \"{text}\"; use one of the candidates"),
            ),
            rows.iter().collect(),
        )
    };
    let shown = shown.into_iter().take(CANDIDATES).cloned().collect();
    let candidates = if entity == "products" {
        product_candidates(client, shown).await?
    } else {
        shown
            .iter()
            .map(|r| json!({"id":r["id"],"name":r["name"]}))
            .collect()
    };
    Err(error.with("candidates", json!(candidates)))
}
/// `{id, name, unit, stock}` for each product row.
async fn product_candidates(client: &ApiClient, rows: Vec<Value>) -> Result<Vec<Value>> {
    if rows.is_empty() {
        return Ok(vec![]);
    }
    let stock = get(client, "/stock", &[]).await?;
    let stock = stock.as_array().map_or(&[][..], Vec::as_slice);
    let units = list(client, "quantity_units", vec![]).await?;
    Ok(rows
        .iter()
        .map(|p| {
            let amount = stock
                .iter()
                .find(|s| same_id(&s["product_id"], &p["id"]))
                .map_or(json!(0), |s| s["amount"].clone());
            json!({"id":p["id"],"name":p["name"],"unit":unit_of(&units,&p["qu_id_stock"],&amount),"stock":amount})
        })
        .collect())
}
/// Compact view of `/stock/products/{id}` details.
fn product_state(details: &Value) -> Value {
    let amount = &details["stock_amount"];
    compact(json!({
        "product":{"id":details["product"]["id"],"name":details["product"]["name"]},
        "stock_amount":amount,
        "unit":unit_label(&details["quantity_unit_stock"],amount),
        "opened":details["stock_amount_opened"],
        "next_due_date":details["next_due_date"],
        "location":details["location"]["name"],
    }))
}
/// Run a full-tier operation with resolved IDs; the write target names that call for undo.
async fn delegate(client: &ApiClient, tool: &'static str, args: Value) -> Result<ToolOutput> {
    let call = catalog::validate(tool, args, client.access())?;
    let mut out = if tool.starts_with("records_") {
        records::run(client, &call).await?
    } else {
        actions::run(client, &call).await?
    };
    if let Some(w) = out.write.as_mut() {
        w.target = json!({"tool":tool,"args":call.args});
    }
    Ok(out)
}
/// The new record's id from a create receipt (Grocy sends it as a string).
fn created(out: &ToolOutput) -> Value {
    out.write
        .as_ref()
        .and_then(|w| record_id(&w.receipt["created_object_id"]).ok())
        .map_or(Value::Null, |id| json!(id.0))
}
fn copy(to: &mut Value, from: &Value, keys: &[&str]) {
    for k in keys {
        if let Some(v) = from.get(*k) {
            to[*k] = v.clone();
        }
    }
}

// ---- tools ----

async fn find_product(client: &ApiClient, a: &Value) -> Result<ToolOutput> {
    let (matches, unique) = match resolve(client, "products", "query", &a["query"]).await {
        Ok(row) => (json!(product_candidates(client, vec![row]).await?), true),
        Err(e) if matches!(e.code, "NOT_FOUND" | "AMBIGUOUS_RECORD") => {
            (e.recovery.unwrap_or_default()["candidates"].take(), false)
        }
        Err(e) => return Err(e),
    };
    let hint = if matches.as_array().is_some_and(Vec::is_empty) {
        "Nothing found. Try a shorter or different word. If it really is new, ask the human before create_product."
    } else if unique {
        "Use this product's id or exact name in other tools."
    } else {
        "Several matches; use the id of the right one."
    };
    Ok(ToolOutput::data(
        json!({"matches":matches,"unique":unique,"hint":hint}),
    ))
}

async fn low_or_expiring(client: &ApiClient, a: &Value) -> Result<ToolOutput> {
    let days = a["days"].as_u64().unwrap_or(5);
    let mut r = ApiRequest::get("/stock/volatile");
    r.query.insert("due_soon_days".into(), days.to_string());
    let v = client.request(r).await?.json()?;
    let units = list(client, "quantity_units", vec![]).await?;
    let group = |key: &str| {
        v[key]
            .as_array()
            .into_iter()
            .flatten()
            .map(|row| {
                let p = &row["product"];
                let (amount, extra) = if key == "missing_products" {
                    (&row["amount_missing"], json!({"missing":row["amount_missing"]}))
                } else {
                    (
                        &row["amount"],
                        json!({"amount":row["amount"],"best_before_date":row["best_before_date"]}),
                    )
                };
                let mut item = json!({"id":p["id"],"name":p["name"],"unit":unit_of(&units,&p["qu_id_stock"],amount)});
                copy(&mut item, &extra, &["missing", "amount", "best_before_date"]);
                item
            })
            .collect::<Vec<_>>()
    };
    Ok(ToolOutput::data(json!({
        "days":days,
        "expiring_soon":group("due_products"),
        "overdue":group("overdue_products"),
        "expired":group("expired_products"),
        "below_min_stock":group("missing_products"),
    })))
}

async fn stock_write(client: &ApiClient, tool: &str, a: &Value) -> Result<ToolOutput> {
    let product = resolve(client, "products", "product", &a["product"])
        .await
        .map_err(|mut e| {
            if tool == "add_stock" && e.code == "NOT_FOUND" {
                e.message = format!(
                    "{} If it is really new, ask the human, then call create_product.",
                    e.message
                )
                .into();
                e = e.with(
                    "suggestion",
                    json!({"tool":"create_product","args":{"name":a["product"]}}),
                );
            }
            e
        })?;
    let mut args = json!({"product_id":product["id"]});
    let (op, field) = match tool {
        "add_stock" => ("stock_add", "amount"),
        "consume_stock" => ("stock_consume", "amount"),
        "open_product" => ("stock_open", "amount"),
        _ => ("stock_inventory", "new_amount"),
    };
    args[field] = a.get(field).cloned().unwrap_or(json!(1));
    copy(
        &mut args,
        a,
        &["best_before_date", "price", "note", "spoiled"],
    );
    if a.get("location").is_some() {
        args["location_id"] =
            resolve(client, "locations", "location", &a["location"]).await?["id"].clone();
    }
    let unit = match a.get("unit") {
        Some(u) => Some(resolve(client, "quantity_units", "unit", u).await?),
        None => None,
    };
    if let Some(u) = &unit {
        args["quantity_unit_id"] = u["id"].clone();
    }
    let mut out = delegate(client, op, args).await.map_err(|e| match e.code {
        "UNIT_CONVERSION_REQUIRED" => AppError::field(
            "unit",
            format!(
                "{} has no conversion from {} to its stock unit; give the amount in the stock unit instead",
                plain(&product["name"]), unit.as_ref().map_or(String::new(), |u| plain(&u["name"]))
            ),
        ),
        _ => e,
    });
    if tool == "consume_stock"
        && let Err(e) = &out
        && e.recovery.as_ref().is_some_and(|r| {
            r["upstream_message"]
                .as_str()
                .is_some_and(|m| m.contains("cannot be >"))
        })
    {
        let state = product_state(
            &get(
                client,
                "/stock/products/{productId}",
                &[("productId", &product["id"])],
            )
            .await?,
        );
        let place = if a.get("location").is_some() {
            " at that location"
        } else {
            ""
        };
        out = Err(AppError::new(
            "NOT_ENOUGH_STOCK",
            format!(
                "amount: {} exceeds current stock of {} ({} {}){place}; nothing was changed",
                plain(&a["amount"]),
                product["name"].as_str().unwrap_or("this product"),
                state["stock_amount"],
                plain(&state["unit"])
            ),
        )
        .with("stock", state));
    }
    let mut out = out?;
    if !out.data.is_null() {
        let receipt = out.write.as_ref().map_or(&Value::Null, |w| &w.receipt);
        let mut state = product_state(&out.data);
        state["transaction_id"] = receipt.get(0).unwrap_or(receipt)["transaction_id"].clone();
        state["undo"] = json!("Call undo (no arguments) to revert this change.");
        out.data = state;
    }
    Ok(out)
}

async fn create_product(client: &ApiClient, a: &Value) -> Result<ToolOutput> {
    let name = a["name"].as_str().unwrap_or("").trim();
    if name.is_empty() {
        return Err(AppError::field("name", "must not be empty"));
    }
    let lower = name.to_lowercase();
    if let Some(existing) = list(client, "products", vec![format!("name~{name}")])
        .await?
        .into_iter()
        .find(|p| {
            p["name"]
                .as_str()
                .is_some_and(|n| n.to_lowercase() == lower)
        })
    {
        return Err(AppError::new(
            "ALREADY_EXISTS",
            format!(
                "name: a product called {} already exists (id {}); use it instead",
                plain(&existing["name"]),
                existing["id"]
            ),
        )
        .with(
            "candidates",
            json!([{"id":existing["id"],"name":existing["name"]}]),
        ));
    }
    let presets = get(client, "/user/settings", &[]).await?;
    let preset = |key: &str| {
        presets[key]
            .as_i64()
            .filter(|id| *id > 0)
            .map(|id| json!(id))
    };
    let location = match (a.get("location"), preset("product_presets_location_id")) {
        (Some(l), _) => resolve(client, "locations", "location", l).await?["id"].clone(),
        (None, Some(id)) => id,
        (None, None) => {
            let rows = list(client, "locations", vec![]).await?;
            match rows.as_slice() {
                [one] => one["id"].clone(),
                _ => {
                    return Err(
                        AppError::field("location", "is required: where is it stored?").with(
                            "candidates",
                            json!(
                                rows.iter()
                                    .take(CANDIDATES)
                                    .map(|r| json!({"id":r["id"],"name":r["name"]}))
                                    .collect::<Vec<_>>()
                            ),
                        ),
                    );
                }
            }
        }
    };
    let unit = match (a.get("stock_unit"), preset("product_presets_qu_id")) {
        (Some(u), _) => resolve(client, "quantity_units", "stock_unit", u).await?,
        (None, Some(id)) => records::get_record(client, "quantity_units", &id).await?,
        (None, None) => resolve(client, "quantity_units", "stock_unit", &json!("Piece")).await?,
    };
    let u = &unit["id"];
    let mut fields = json!({"name":name,"location_id":location,"qu_id_stock":u,"qu_id_purchase":u,"qu_id_consume":u,"qu_id_price":u});
    copy(
        &mut fields,
        a,
        &["min_stock_amount", "default_best_before_days"],
    );
    let mut out = delegate(
        client,
        "records_create",
        json!({"entity":"products","fields":fields}),
    )
    .await?;
    let id = created(&out);
    out.data = json!({"product":{"id":id,"name":name},"stock_unit":unit["name"],"location_id":location,
        "next":"Use add_stock to add stock. This cannot be undone here; ask the human to delete it in Grocy if it was a mistake."});
    Ok(out)
}

async fn shopping_list_row(client: &ApiClient, a: &Value) -> Result<Value> {
    match a.get("list") {
        Some(l) => resolve(client, "shopping_lists", "list", l).await,
        None => records::get_record(client, "shopping_lists", &json!(1)).await,
    }
}
async fn shopping_items(
    client: &ApiClient,
    list_id: &Value,
    product: Option<&Value>,
) -> Result<Vec<Value>> {
    let mut filters = vec![format!("shopping_list_id={}", plain(list_id))];
    if let Some(p) = product {
        filters.push(format!("product_id={}", plain(p)));
    }
    let rows = list(client, "shopping_list", filters).await?;
    let products = if rows.iter().any(|r| !r["product_id"].is_null()) {
        list(client, "products", vec![]).await?
    } else {
        vec![]
    };
    let units = list(client, "quantity_units", vec![]).await?;
    Ok(rows
        .iter()
        .map(|r| {
            let p = find(&products, &r["product_id"]);
            let unit = if r["qu_id"].is_null() { &p["qu_id_stock"] } else { &r["qu_id"] };
            compact(json!({"id":r["id"],"product_id":r["product_id"],"product":p["name"],"amount":r["amount"],
                "unit":unit_of(&units,unit,&r["amount"]),"note":r["note"],"done":r["done"]==1}))
        })
        .collect())
}
async fn shopping_write(client: &ApiClient, tool: &str, a: &Value) -> Result<ToolOutput> {
    let product = resolve(client, "products", "product", &a["product"]).await?;
    let list = shopping_list_row(client, a).await?;
    let mut args = json!({"product_id":product["id"],"list_id":list["id"]});
    let op = if tool == "shopping_add" {
        let mut amount = decimal(a.get("amount").unwrap_or(&json!(1)))
            .ok_or_else(|| AppError::field("amount", "must be a number"))?;
        if let Some(u) = a.get("unit") {
            let unit = resolve(client, "quantity_units", "unit", u).await?;
            let (from, to) = (record_id(&unit["id"])?, record_id(&product["qu_id_stock"])?);
            let conversions = if from == to {
                vec![]
            } else {
                load_conversions(client, record_id(&product["id"])?).await?
            };
            amount = convert_quantity(amount, from, to, &conversions).map_err(|_| {
                AppError::field("unit", format!("{} has no conversion from {} to its stock unit; give the amount without a unit", plain(&product["name"]), plain(&unit["name"])))
            })?;
        }
        args["product_amount"] = json!(amount.to_string());
        copy(&mut args, a, &["note"]);
        "shopping_add_product"
    } else {
        let listed = shopping_items(client, &list["id"], Some(&product["id"])).await?;
        if listed.is_empty() {
            return Err(AppError::new(
                "NOT_FOUND",
                format!(
                    "product: {} is not on the {} list",
                    plain(&product["name"]),
                    plain(&list["name"])
                ),
            ));
        }
        let total = listed
            .iter()
            .filter_map(|i| decimal(&i["amount"]))
            .sum::<Decimal>();
        args["product_amount"] = a.get("amount").cloned().unwrap_or(json!(total.to_string()));
        "shopping_remove_product"
    };
    let mut out = delegate(client, op, args).await?;
    let remaining = shopping_items(client, &list["id"], Some(&product["id"]))
        .await
        .ok();
    out.data = json!({"list":{"id":list["id"],"name":list["name"]},"product":{"id":product["id"],"name":product["name"]},"now_on_list":remaining});
    Ok(out)
}

async fn recipes_suggest(client: &ApiClient, a: &Value) -> Result<ToolOutput> {
    let units = list(client, "quantity_units", vec![]).await?;
    if let Some(r) = a.get("recipe") {
        let recipe = resolve(client, "recipes", "recipe", r).await?;
        let status = get(
            client,
            "/recipes/{recipeId}/fulfillment",
            &[("recipeId", &recipe["id"])],
        )
        .await?;
        let ingredients = list(
            client,
            "recipes_pos_resolved",
            vec![format!("recipe_id={}", plain(&recipe["id"]))],
        )
        .await?;
        let ingredients = ingredients
            .iter()
            .map(|i| compact(json!({"product":i["product_name"],"need":i["recipe_amount"],"have":i["stock_amount"],
                "missing":i["missing_amount"],"unit":unit_of(&units,&i["qu_id"],&i["recipe_amount"]),"enough":i["need_fulfilled"]==1})))
            .collect::<Vec<_>>();
        return Ok(ToolOutput::data(
            json!({"recipe":{"id":recipe["id"],"name":recipe["name"]},
            "can_make":status["need_fulfilled"]==1,"missing_count":status["missing_products_count"],"ingredients":ingredients}),
        ));
    }
    let names = list(client, "recipes", vec!["type=normal".into()]).await?;
    let all = get(client, "/recipes/fulfillment", &[]).await?;
    let mut rows = all
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| {
            let r = find(&names, &f["recipe_id"]);
            (!r.is_null()).then(|| json!({"id":r["id"],"name":r["name"],"can_make":f["need_fulfilled"]==1,
                "missing_count":f["missing_products_count"],"missing":f["product_names_comma_separated"]}))
        })
        .collect::<Vec<_>>();
    rows.sort_by_key(|r| {
        (
            !r["can_make"].as_bool().unwrap_or(false),
            r["missing_count"].as_u64().unwrap_or(u64::MAX),
        )
    });
    rows.truncate(25);
    Ok(ToolOutput::data(json!({"recipes":rows})))
}

async fn todo_list(client: &ApiClient, a: &Value) -> Result<ToolOutput> {
    let now = Local::now().naive_local();
    let until = now + chrono::Duration::days(a["days"].as_i64().unwrap_or(7));
    let (now, until) = (
        now.format("%Y-%m-%d %H:%M:%S").to_string(),
        until.format("%Y-%m-%d %H:%M:%S").to_string(),
    );
    let chores = get(client, "/chores", &[]).await?;
    let mut chores = chores
        .as_array()
        .into_iter()
        .flatten()
        .filter(|c| c["next_estimated_execution_time"].as_str().is_some_and(|t| *t <= *until))
        .map(|c| {
            let due = c["next_estimated_execution_time"].as_str().unwrap_or("");
            compact(json!({"id":c["chore_id"],"name":c["chore_name"],"due":due,"overdue":due < now.as_str(),
                "assigned_to":c["next_execution_assigned_user"]["display_name"]}))
        })
        .collect::<Vec<_>>();
    chores.sort_by(|x, y| x["due"].as_str().cmp(&y["due"].as_str()));
    let tasks = get(client, "/tasks", &[]).await?;
    let tasks = tasks
        .as_array()
        .into_iter()
        .flatten()
        .filter(|t| t["done"] != 1)
        .map(|t| compact(json!({"id":t["id"],"name":t["name"],"due_date":t["due_date"],"assigned_to":t["assigned_to_user"]["display_name"]})))
        .collect::<Vec<_>>();
    Ok(ToolOutput::data(
        json!({"until":until,"chores":chores,"tasks":tasks}),
    ))
}

async fn mark_done(client: &ApiClient, a: &Value) -> Result<ToolOutput> {
    let (out, kind, row) = match (a.get("chore"), a.get("task")) {
        (Some(c), None) => {
            let chore = resolve(client, "chores", "chore", c).await?;
            let mut args = json!({"chore_id":chore["id"]});
            copy(&mut args, a, &["tracked_time"]);
            (
                delegate(client, "chore_execute", args).await?,
                "chore",
                chore,
            )
        }
        (None, Some(t)) => {
            let task = resolve(client, "tasks", "task", t).await?;
            let mut args = json!({"task_id":task["id"]});
            if let Some(time) = a.get("tracked_time") {
                args["done_time"] = time.clone();
            }
            (delegate(client, "task_complete", args).await?, "task", task)
        }
        _ => {
            return Err(AppError::field(
                "chore",
                "give exactly one of chore or task",
            ));
        }
    };
    let next = out.data["next_estimated_execution_time"].clone();
    Ok(ToolOutput {
        data: compact(
            json!({"done":{"type":kind,"id":row["id"],"name":row["name"]},"next_due":next,
            "undo":"Call undo (no arguments) to revert this."}),
        ),
        ..out
    })
}

async fn meal_plan(client: &ApiClient, a: &Value) -> Result<ToolOutput> {
    let today = Local::now().date_naive();
    let monday = today - Days::new(today.weekday().num_days_from_monday().into());
    let from = a["from"]
        .as_str()
        .map_or_else(|| monday.to_string(), str::to_owned);
    let to = a["to"]
        .as_str()
        .map_or_else(|| (monday + Days::new(6)).to_string(), str::to_owned);
    if from > to {
        return Err(AppError::field("to", "must not be before from"));
    }
    let mut rows = list(
        client,
        "meal_plan",
        vec![format!("day>={from}"), format!("day<={to}")],
    )
    .await?;
    rows.sort_by(|x, y| x["day"].as_str().cmp(&y["day"].as_str()));
    let need = |k: &str| rows.iter().any(|r| r[k].as_u64().is_some_and(|id| id > 0));
    let recipes = if need("recipe_id") {
        list(client, "recipes", vec![]).await?
    } else {
        vec![]
    };
    let products = if need("product_id") {
        list(client, "products", vec![]).await?
    } else {
        vec![]
    };
    let sections = if need("section_id") {
        list(client, "meal_plan_sections", vec![]).await?
    } else {
        vec![]
    };
    let items = rows
        .iter()
        .map(|r| {
            let product = r["type"] == "product";
            compact(json!({"id":r["id"],"day":r["day"],"type":r["type"],
                "recipe":find(&recipes,&r["recipe_id"])["name"],"servings":if r["type"]=="recipe"{r["recipe_servings"].clone()}else{Value::Null},
                "product":find(&products,&r["product_id"])["name"],"amount":if product{r["product_amount"].clone()}else{Value::Null},
                "note":r["note"],"section":find(&sections,&r["section_id"])["name"],"done":r["done"]==1}))
        })
        .collect::<Vec<_>>();
    Ok(ToolOutput::data(json!({"from":from,"to":to,"items":items})))
}

async fn meal_plan_add(client: &ApiClient, a: &Value) -> Result<ToolOutput> {
    let mut fields = json!({"day":a["day"]});
    copy(&mut fields, a, &["note"]);
    let what = match (a.get("recipe"), a.get("product")) {
        (Some(_), Some(_)) => {
            return Err(AppError::field(
                "recipe",
                "give a recipe or a product, not both",
            ));
        }
        (Some(r), None) => {
            let recipe = resolve(client, "recipes", "recipe", r).await?;
            fields["type"] = json!("recipe");
            fields["recipe_id"] = recipe["id"].clone();
            fields["recipe_servings"] = a.get("servings").cloned().unwrap_or(json!(1));
            recipe["name"].clone()
        }
        (None, Some(p)) => {
            let product = resolve(client, "products", "product", p).await?;
            fields["type"] = json!("product");
            fields["product_id"] = product["id"].clone();
            fields["product_amount"] = a.get("amount").cloned().unwrap_or(json!(1));
            fields["product_qu_id"] = product["qu_id_stock"].clone();
            product["name"].clone()
        }
        (None, None) if a.get("note").is_some() => {
            fields["type"] = json!("note");
            a["note"].clone()
        }
        (None, None) => {
            return Err(AppError::field(
                "recipe",
                "give a recipe, a product or a note",
            ));
        }
    };
    if let Some(s) = a.get("section") {
        fields["section_id"] =
            resolve(client, "meal_plan_sections", "section", s).await?["id"].clone();
    }
    let mut out = delegate(
        client,
        "records_create",
        json!({"entity":"meal_plan","fields":fields}),
    )
    .await?;
    let id = created(&out);
    out.data = json!({"planned":{"id":id,"day":a["day"],"type":fields["type"],"what":what}});
    Ok(out)
}
