use crate::{
    catalog::ValidatedCall,
    client::{ApiClient, ApiRequest, RequestBody},
    error::{AppError, Result},
    model::{ToolOutput, Verification, WriteReport, WriteStatus},
};
use rust_decimal::Decimal;
use serde_json::{Value, json};
fn build(call: &ValidatedCall) -> ApiRequest {
    let mut r = ApiRequest::get(call.op.route);
    r.method = call.op.method;
    for (key, param) in &call.op.path_params {
        let value = &call.args[key];
        r.params.insert(
            param.clone(),
            value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string()),
        );
    }
    if !call.op.body_keys.is_empty() {
        let body = call
            .op
            .body_keys
            .iter()
            .filter_map(|k| call.args.get(k).map(|v| (k.clone(), v.clone())))
            .collect::<serde_json::Map<_, _>>();
        r.body = RequestBody::Json(Value::Object(body));
    }
    r
}
fn readback(route: &'static str, param: &str, id: &Value) -> ApiRequest {
    let mut r = ApiRequest::get(route);
    r.params.insert(
        param.into(),
        id.as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| id.to_string()),
    );
    r
}
pub async fn run(client: &ApiClient, call: &ValidatedCall) -> Result<ToolOutput> {
    let mut r = build(call);
    let target = r.target();
    let mut verify = None;
    if call.op.mutates {
        if let Some(id) = call
            .args
            .get("product_id")
            .or_else(|| call.args.get("product_id_to_keep"))
        {
            let read = readback("/stock/products/{productId}", "productId", id);
            let details = client.request(read).await?.json()?;
            if let Some(unit) = call.args.get("quantity_unit_id") {
                apply_conversion(client, call, &details, &mut r, unit).await?;
            }
            verify = Some(readback("/stock/products/{productId}", "productId", id));
        } else if let Some(barcode) = call.args.get("barcode") {
            let read = readback("/stock/products/by-barcode/{barcode}", "barcode", barcode);
            let details = client.request(read).await?.json()?;
            if let Some(unit) = call.args.get("quantity_unit_id") {
                apply_conversion(client, call, &details, &mut r, unit).await?;
            }
            verify = Some(readback(
                "/stock/products/by-barcode/{barcode}",
                "barcode",
                barcode,
            ));
        } else if let Some(id) = call.args.get("entry_id") {
            let details = client
                .request(readback("/stock/entry/{entryId}", "entryId", id))
                .await?
                .json()?;
            if let Some(unit) = call.args.get("quantity_unit_id") {
                let details = json!({"product":super::records::get_record(client,"products",&details["product_id"]).await?});
                apply_conversion(client, call, &details, &mut r, unit).await?;
            }
            verify = Some(readback("/stock/entry/{entryId}", "entryId", id));
        } else if let Some(id) = call.args.get("recipe_id") {
            super::records::get_record(client, "recipes", id).await?;
            verify = Some(readback("/recipes/{recipeId}/fulfillment", "recipeId", id));
        } else if let Some(id) = call
            .args
            .get("chore_id")
            .or_else(|| call.args.get("chore_id_to_keep"))
        {
            super::records::get_record(client, "chores", id).await?;
            verify = Some(readback("/chores/{choreId}", "choreId", id));
        } else if let Some(id) = call.args.get("battery_id") {
            super::records::get_record(client, "batteries", id).await?;
            verify = Some(readback("/batteries/{batteryId}", "batteryId", id));
        } else if let Some(id) = call.args.get("task_id") {
            super::records::get_record(client, "tasks", id).await?;
            verify = Some(readback("/objects/{entity}/{id}", "id", id));
            verify
                .as_mut()
                .unwrap()
                .params
                .insert("entity".into(), "tasks".into());
        } else if let Some(id) = call.args.get("transaction_id") {
            client
                .request(readback(
                    "/stock/transactions/{transactionId}",
                    "transactionId",
                    id,
                ))
                .await?;
            verify = Some(readback(
                "/stock/transactions/{transactionId}",
                "transactionId",
                id,
            ));
        } else if let Some(id) = call.args.get("booking_id") {
            client
                .request(readback("/stock/bookings/{bookingId}", "bookingId", id))
                .await?;
            verify = Some(readback("/stock/bookings/{bookingId}", "bookingId", id));
        } else if let Some(id) = call.args.get("execution_id") {
            let log = super::records::get_record(client, "chores_log", id).await?;
            verify = Some(readback("/chores/{choreId}", "choreId", &log["chore_id"]));
        } else if let Some(id) = call.args.get("charge_cycle_id") {
            let log = super::records::get_record(client, "battery_charge_cycles", id).await?;
            verify = Some(readback(
                "/batteries/{batteryId}",
                "batteryId",
                &log["battery_id"],
            ));
        } else if call.op.name.starts_with("shopping_") {
            if let Some(id) = call.args.get("list_id") {
                super::records::get_record(client, "shopping_lists", id).await?;
            }
            let mut q = ApiRequest::get("/objects/{entity}");
            q.params.insert("entity".into(), "shopping_list".into());
            verify = Some(q);
        }
        for key in ["location_id", "location_id_from", "location_id_to"] {
            if let Some(id) = call.args.get(key) {
                super::records::get_record(client, "locations", id).await?;
            }
        }
        if call.op.name.starts_with("stock_add") || call.op.name.starts_with("stock_inventory") {
            if let RequestBody::Json(body) = &mut r.body {
                body["stock_label_type"] = json!(1);
            }
        }
    }
    if call.op.mutates && call.op.name.starts_with("shopping_") {
        let mut q = ApiRequest::get("/objects/{entity}");
        q.params.insert("entity".into(), "shopping_list".into());
        verify = Some(q);
    }
    if call.op.mutates && call.op.name == "chores_recalculate_assignments" {
        verify = Some(ApiRequest::get("/chores"));
    }
    let receipt = client.request(r).await?.json()?;
    if !call.op.mutates {
        let data = if let Some(arr) = receipt.as_array() {
            call.page.slice(arr.clone())
        } else if call.op.name == "stock_volatile" {
            let mut grouped = serde_json::Map::new();
            for (k, v) in receipt.as_object().ok_or_else(AppError::input)? {
                grouped.insert(
                    k.clone(),
                    if let Some(a) = v.as_array() {
                        call.page.slice(a.clone())
                    } else {
                        v.clone()
                    },
                );
            }
            Value::Object(grouped)
        } else {
            receipt
        };
        return Ok(ToolOutput::data(data));
    }
    if call.op.name == "stock_copy" {
        if let Some(id) = receipt.get("created_object_id") {
            verify = Some(readback("/stock/products/{productId}", "productId", id));
        }
    }
    if call.op.name == "recipe_copy" {
        if let Some(id) = receipt.get("created_object_id") {
            verify = Some(readback("/recipes/{recipeId}/fulfillment", "recipeId", id));
        }
    }
    let (data, verification) = if let Some(q) = verify {
        match client.request(q).await.and_then(|r| r.json()) {
            Ok(v) => (v, Verification::Confirmed),
            Err(_) => (Value::Null, Verification::Incomplete),
        }
    } else {
        (Value::Null, Verification::Unavailable)
    };
    Ok(ToolOutput {
        data,
        write: Some(WriteReport {
            status: WriteStatus::Completed,
            verification,
            target,
            receipt,
        }),
        attachment: None,
    })
}
async fn apply_conversion(
    client: &ApiClient,
    call: &ValidatedCall,
    details: &Value,
    r: &mut ApiRequest,
    unit: &Value,
) -> Result<()> {
    let product = details.get("product").unwrap_or(details);
    let id = super::validation::record_id(&product["id"])?;
    let stock = super::validation::record_id(&product["qu_id_stock"])?;
    let from = super::validation::record_id(unit)?;
    let conversions = if from != stock {
        super::validation::load_conversions(client, id).await?
    } else {
        vec![]
    };
    let field = if call.args.get("new_amount").is_some() {
        "new_amount"
    } else {
        "amount"
    };
    let value = &call.args[field];
    let amount = value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string())
        .parse::<Decimal>()
        .map_err(|_| AppError::input())?;
    let converted = super::validation::convert_quantity(amount, from, stock, &conversions)?;
    if let RequestBody::Json(body) = &mut r.body {
        body[field] = json!(converted.to_string());
    }
    Ok(())
}
