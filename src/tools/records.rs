use crate::{
    catalog::{ValidatedCall, entities},
    client::{ApiClient, ApiRequest, HttpMethod, RequestBody},
    error::{AppError, Result},
    model::{ToolOutput, Verification, WriteReport, WriteStatus},
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
fn allowed_definition(row: &Value) -> bool {
    row["entity"]
        .as_str()
        .is_some_and(crate::catalog::household_field_target)
}
fn check_definition(entity: &str, row: &Value) -> Result<()> {
    if entity == "userfields" && !allowed_definition(row) {
        return Err(AppError::new(
            "OUT_OF_SCOPE",
            "Account and other non-household custom-field definitions are unavailable.",
        ));
    }
    Ok(())
}
pub async fn get_record(client: &ApiClient, entity: &str, id: &Value) -> Result<Value> {
    let mut r = ApiRequest::get("/objects/{entity}/{id}");
    r.params = BTreeMap::from([
        ("entity".into(), entity.into()),
        (
            "id".into(),
            id.as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| id.to_string()),
        ),
    ]);
    let row = client.request(r).await?.json()?;
    check_definition(entity, &row)?;
    Ok(row)
}
pub async fn run(client: &ApiClient, call: &ValidatedCall) -> Result<ToolOutput> {
    let entity = call.args["entity"].as_str().ok_or_else(AppError::input)?;
    if call.op.name == "records_describe" {
        return Ok(ToolOutput::data(
            json!({"entity":entity,"definition":entities()[entity],"notes":"Create using required fields; updates contain only changed fields. IDs are explicit. Product units require valid conversions; stock-unit changes require a valid Grocy conversion and use its database cascade."}),
        ));
    }
    let mut field_object_id = call.args["id"].clone();
    if call.op.name.starts_with("userfields_") && entity == "stock" {
        let stock = get_record(client, entity, &call.args["id"]).await?;
        field_object_id = stock["stock_id"].clone();
        if field_object_id.as_str().is_none_or(|s| {
            s.is_empty()
                || s.len() > 128
                || s.contains(['/', '\\', '%'])
                || s.chars().any(char::is_control)
        }) {
            return Err(AppError::input());
        }
    }
    if call.op.name == "userfields_update" && entity == "stock" {
        let transaction_id = call.args["stock_transaction_id"]
            .as_str()
            .ok_or_else(AppError::input)?;
        let mut r = ApiRequest::get("/stock/transactions/{transactionId}");
        r.params
            .insert("transactionId".into(), transaction_id.into());
        let rows = client.request(r).await?.json()?;
        let bookings = rows
            .as_array()
            .ok_or_else(AppError::input)?
            .iter()
            .filter(|r| {
                ["purchase", "inventory-correction", "stock-edit-new"]
                    .contains(&r["transaction_type"].as_str().unwrap_or(""))
            })
            .collect::<Vec<_>>();
        if bookings.is_empty()
            || bookings.iter().any(|r| {
                r["stock_id"] != field_object_id || r["undone"].as_i64().is_some_and(|n| n != 0)
            })
        {
            return Err(AppError::new(
                "STOCK_TRANSACTION_MISMATCH",
                "Choose an active purchase/inventory/entry-edit transaction belonging only to this stock batch.",
            ));
        }
    }
    let target_entity = if call.op.name.starts_with("userfields_") && entity == "userobjects" {
        let object = get_record(client, entity, &call.args["id"]).await?;
        let definition = get_record(client, "userentities", &object["userentity_id"]).await?;
        let name = definition["name"].as_str().ok_or_else(AppError::input)?;
        if name.contains(['/', '\\', '%']) || name.chars().any(char::is_control) {
            return Err(AppError::input());
        }
        format!("userentity-{name}")
    } else {
        entity.to_owned()
    };
    if call.op.mutates {
        if entity == "userfields"
            && let Some(target) = call.args["fields"].get("entity").and_then(Value::as_str)
            && let Some(name) = target.strip_prefix("userentity-")
        {
            let mut definitions = ApiRequest::get("/objects/{entity}");
            definitions
                .params
                .insert("entity".into(), "userentities".into());
            let rows = client.request(definitions).await?.json()?;
            if !rows
                .as_array()
                .ok_or_else(AppError::input)?
                .iter()
                .any(|r| r["name"].as_str() == Some(name))
            {
                return Err(AppError::input());
            }
        }
        if call.op.method != HttpMethod::Post {
            get_record(client, entity, &call.args["id"]).await?;
        }
        if entity == "products" {
            for (key, target) in [
                ("location_id", "locations"),
                ("qu_id_stock", "quantity_units"),
                ("qu_id_purchase", "quantity_units"),
                ("qu_id_consume", "quantity_units"),
                ("qu_id_price", "quantity_units"),
            ] {
                if let Some(id) = call.args["fields"].get(key)
                    && id.as_u64().is_some_and(|n| n > 0)
                {
                    get_record(client, target, id).await?;
                }
            }
            if call.op.method == HttpMethod::Put && call.args["fields"].get("qu_id_stock").is_some()
            {
                let current = get_record(client, "products", &call.args["id"]).await?;
                let old = super::validation::record_id(&current["qu_id_stock"])?;
                let new = super::validation::record_id(&call.args["fields"]["qu_id_stock"])?;
                if old != new {
                    let product_id = super::validation::record_id(&call.args["id"])?;
                    let conversions =
                        super::validation::load_conversions(client, product_id).await?;
                    super::validation::convert_quantity(
                        rust_decimal::Decimal::ONE,
                        old,
                        new,
                        &conversions,
                    )?;
                }
            }
        }
        if call.op.name == "userfields_update" {
            let mut defs = ApiRequest::get("/objects/{entity}");
            defs.params.insert("entity".into(), "userfields".into());
            let rows = client.request(defs).await?.json()?;
            for key in call.args["fields"]
                .as_object()
                .ok_or_else(AppError::input)?
                .keys()
            {
                if !rows
                    .as_array()
                    .ok_or_else(AppError::input)?
                    .iter()
                    .any(|r| {
                        r["entity"].as_str() == Some(&target_entity)
                            && r["name"].as_str() == Some(key)
                    })
                {
                    return Err(AppError::input());
                }
            }
        }
    }
    let mut request = ApiRequest::get(call.op.route);
    request.method = call.op.method;
    for (key, param) in &call.op.path_params {
        let v = &call.args[key];
        request.params.insert(
            param.clone(),
            v.as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| v.to_string()),
        );
    }
    if call.op.name.starts_with("userfields_") {
        request
            .params
            .insert("entity".into(), target_entity.clone());
        request.params.insert(
            "id".into(),
            if entity == "stock" && call.op.name == "userfields_update" {
                call.args["stock_transaction_id"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            } else {
                field_object_id
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| field_object_id.to_string())
            },
        );
    }
    if let Some(fields) = call.args.get("fields") {
        request.body = RequestBody::Json(fields.clone());
    }
    if call.op.name == "records_list" {
        request
            .query
            .insert("limit".into(), (call.page.limit + 1).to_string());
        request
            .query
            .insert("offset".into(), call.page.offset.to_string());
        if entities()[entity]["writable"] == true
            || ["stock", "stock_log", "chores_log", "battery_charge_cycles"].contains(&entity)
        {
            request.query.insert("order".into(), "id".into());
        }
    }
    let receipt = client.request(request).await?.json()?;
    if call.op.name == "records_list" {
        let mut rows = receipt
            .as_array()
            .ok_or_else(|| {
                AppError::new("INVALID_RESPONSE", "Expected a list of household records.")
            })?
            .clone();
        let more = rows.len() > call.page.limit;
        rows.truncate(call.page.limit);
        if entity == "userfields" {
            rows.retain(allowed_definition);
        }
        return Ok(ToolOutput::data(
            json!({"items":rows,"has_more":more,"next_offset":if more{Some(call.page.offset+call.page.limit)}else{None}}),
        ));
    }
    if call.op.name == "records_lookup" {
        let name = call.args["name"].as_str().ok_or_else(AppError::input)?;
        let rows = receipt
            .as_array()
            .ok_or_else(AppError::input)?
            .iter()
            .filter(|r| {
                r["name"].as_str() == Some(name)
                    && (entity != "userfields" || allowed_definition(r))
            })
            .cloned()
            .collect::<Vec<_>>();
        return Ok(ToolOutput::data(
            json!({"candidates":rows,"ambiguous":rows.len()>1}),
        ));
    }
    if !call.op.mutates {
        if call.op.name == "records_get" {
            check_definition(entity, &receipt)?;
        }
        return Ok(ToolOutput::data(receipt));
    }
    let target = json!({"entity":entity,"id":call.args.get("id").or_else(||receipt.get("created_object_id")),"operation":call.op.name});
    let (data, verification) = if call.op.method == HttpMethod::Delete {
        let check = get_record(client, entity, &call.args["id"]).await;
        match check {
            Err(e) if e.code == "NOT_FOUND" => (json!({"deleted":true}), Verification::Confirmed),
            _ => (Value::Null, Verification::Incomplete),
        }
    } else if let Some(id) = call
        .args
        .get("id")
        .or_else(|| receipt.get("created_object_id"))
    {
        let result = if call.op.name == "userfields_update" {
            let mut r = ApiRequest::get("/userfields/{entity}/{id}");
            r.params = BTreeMap::from([
                ("entity".into(), target_entity.clone()),
                (
                    "id".into(),
                    field_object_id
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| field_object_id.to_string()),
                ),
            ]);
            match client.request(r).await {
                Ok(r) => r.json(),
                Err(e) => Err(e),
            }
        } else {
            get_record(client, entity, id).await
        };
        match result {
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
