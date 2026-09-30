use crate::{
    catalog::{ValidatedCall, entities},
    client::{ApiClient, ApiRequest, HttpMethod, RequestBody},
    error::{AppError, Result},
    model::{ToolOutput, Verification, WriteReport, WriteStatus},
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
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
    client.request(r).await?.json()
}
pub async fn run(client: &ApiClient, call: &ValidatedCall) -> Result<ToolOutput> {
    let entity = call.args["entity"].as_str().ok_or_else(AppError::input)?;
    if call.op.name == "records_describe" {
        return Ok(ToolOutput::data(
            json!({"entity":entity,"definition":entities()[entity],"notes":"Create using required fields; updates contain only changed fields. IDs are explicit. Product units require valid conversions; stock-unit changes require a valid Grocy conversion and use its database cascade."}),
        ));
    }
    if call.op.mutates {
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
                if let Some(id) = call.args["fields"].get(key) {
                    if id.as_u64().is_some_and(|n| n > 0) {
                        get_record(client, target, id).await?;
                    }
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
                        r["entity"].as_str() == Some(entity) && r["name"].as_str() == Some(key)
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
            .filter(|r| r["name"].as_str() == Some(name))
            .cloned()
            .collect::<Vec<_>>();
        return Ok(ToolOutput::data(
            json!({"candidates":rows,"ambiguous":rows.len()>1}),
        ));
    }
    if !call.op.mutates {
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
                ("entity".into(), entity.into()),
                ("id".into(), id.to_string()),
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
