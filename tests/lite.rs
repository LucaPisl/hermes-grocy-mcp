//! Lite tools against a fake Grocy: name resolution, units, guardrails and undo mapping.
mod support;
use hermes_grocy_mcp::{
    catalog,
    model::AccessMode,
    tools::{self, lite},
};
use serde_json::{Value, json};
use support::Seen;

const PRODUCTS: [(u64, &str); 3] = [(1, "Whole Milk"), (2, "Oat Milk"), (3, "milk")];
fn product(id: u64, name: &str) -> Value {
    json!({"id":id,"name":name,"qu_id_stock":2,"location_id":2})
}
fn query_value<'a>(r: &'a Seen, prefix: &str) -> Option<&'a str> {
    r.query.split('&').find_map(|p| p.strip_prefix(prefix))
}
/// A small synthetic household; consuming more than 3 is rejected like Grocy does.
fn grocy(r: &Seen) -> (u16, Value) {
    let path = r.path.as_str();
    if let Some(id) = path.strip_prefix("/api/objects/products/") {
        return match PRODUCTS.iter().find(|(i, _)| i.to_string() == id) {
            Some((i, n)) => (200, product(*i, n)),
            None => (404, json!({"error_message":"Object not found"})),
        };
    }
    match (r.method.as_str(), path) {
        ("GET", "/api/objects/products") => {
            let needle = query_value(r, "query[]=name~").unwrap_or("").to_lowercase();
            let rows = PRODUCTS
                .iter()
                .filter(|(_, n)| n.to_lowercase().contains(&needle))
                .map(|(i, n)| product(*i, n))
                .collect::<Vec<_>>();
            (200, json!(rows))
        }
        ("GET", "/api/objects/product_barcodes") => {
            let hit = query_value(r, "query[]=barcode=") == Some("4006");
            (
                200,
                if hit {
                    json!([{"product_id":1}])
                } else {
                    json!([])
                },
            )
        }
        ("GET", "/api/stock") => (200, json!([{"product_id":1,"amount":3}])),
        ("GET", "/api/objects/quantity_units") => (
            200,
            json!([{"id":2,"name":"Piece","name_plural":"Pieces"},{"id":3,"name":"Pack","name_plural":"Packs"}]),
        ),
        ("GET", "/api/objects/quantity_unit_conversions_resolved") => (
            200,
            json!([{"product_id":1,"from_qu_id":3,"to_qu_id":2,"factor":6}]),
        ),
        ("GET", "/api/stock/products/1") => (
            200,
            json!({"product":product(1,"Whole Milk"),"stock_amount":3,"quantity_unit_stock":{"id":2,"name":"Piece","name_plural":"Pieces"},"location":{"name":"Fridge"}}),
        ),
        ("POST", "/api/stock/products/1/add") => (200, json!([{"transaction_id":"t1"}])),
        ("POST", "/api/stock/products/1/consume") => (
            400,
            json!({"error_message":"Amount to be consumed cannot be > current stock amount"}),
        ),
        ("GET", "/api/objects/shopping_lists/1") => (200, json!({"id":1,"name":"Main"})),
        ("GET", "/api/objects/shopping_list") => {
            let rows = if r.query.contains("product_id=1") {
                json!([{"id":1,"product_id":1,"amount":2,"qu_id":null},{"id":2,"product_id":1,"amount":1}])
            } else {
                json!([])
            };
            (200, rows)
        }
        ("GET", "/api/user/settings") => (200, json!({"product_presets_location_id":-1})),
        ("GET", "/api/objects/locations") => (200, json!([{"id":2,"name":"Fridge"}])),
        ("POST", "/api/objects/products") => (200, json!({"created_object_id":"9"})),
        ("GET", "/api/stock/transactions/t1") => (
            200,
            json!([{"transaction_type":"purchase","stock_id":"s1","amount":2}]),
        ),
        ("GET", "/api/objects/stock_log") => (
            200,
            json!([{"transaction_id":"t0","undone":0},{"transaction_id":"t1","undone":0}]),
        ),
        _ => (200, json!([])),
    }
}
fn call(name: &str, args: Value) -> catalog::ValidatedCall {
    catalog::validate(name, args, AccessMode::Full).unwrap()
}
async fn household() -> (
    hermes_grocy_mcp::client::ApiClient,
    std::sync::Arc<std::sync::Mutex<Vec<Seen>>>,
) {
    let (url, log) = support::fake_grocy(grocy).await;
    (support::client(&url), log)
}
fn posts(log: &std::sync::Mutex<Vec<Seen>>) -> Vec<Seen> {
    log.lock()
        .unwrap()
        .iter()
        .filter(|r| r.method != "GET")
        .cloned()
        .collect()
}

#[tokio::test]
async fn names_resolve_by_id_barcode_exact_then_unique_substring() {
    let (client, _) = household().await;
    let find = |q: Value| {
        let client = &client;
        async move {
            tools::execute(client, call("find_product", json!({"query":q})))
                .await
                .unwrap()
                .data
        }
    };
    // "milk" is a substring of all three, but one name matches exactly (case-insensitive).
    let exact = find(json!("MILK")).await;
    assert_eq!(exact["unique"], true);
    assert_eq!(exact["matches"][0]["id"], 3);
    let whole = find(json!("whole")).await;
    assert_eq!(whole["matches"][0]["name"], "Whole Milk");
    assert_eq!(whole["matches"][0]["unit"], "Pieces");
    assert_eq!(whole["matches"][0]["stock"], 3);
    assert_eq!(find(json!(2)).await["matches"][0]["name"], "Oat Milk");
    assert_eq!(find(json!("4006")).await["matches"][0]["id"], 1);
    let several = find(json!("mil")).await;
    assert_eq!(several["unique"], false);
    assert_eq!(several["matches"].as_array().unwrap().len(), 3);
    assert_eq!(several["matches"][1]["stock"], 0);
    assert_eq!(find(json!("bread")).await["matches"], json!([]));
    // A write with an ambiguous name is refused before anything is sent.
    let e = tools::execute(
        &client,
        call("add_stock", json!({"product":"mil","amount":1})),
    )
    .await
    .err()
    .unwrap();
    assert_eq!(e.code, "AMBIGUOUS_RECORD");
    assert!(e.message.starts_with("product:"), "{}", e.message);
    assert_eq!(
        e.recovery.unwrap()["candidates"].as_array().unwrap().len(),
        3
    );
}

#[tokio::test]
async fn add_stock_converts_units_and_reports_the_result() {
    let (client, log) = household().await;
    let out = tools::execute(
        &client,
        call(
            "add_stock",
            json!({"product":"whole milk","amount":2,"unit":"packs"}),
        ),
    )
    .await
    .unwrap();
    let post = posts(&log).pop().unwrap();
    assert_eq!(post.path, "/api/stock/products/1/add");
    assert_eq!(post.body["amount"], "12");
    assert_eq!(out.data["stock_amount"], 3);
    assert_eq!(out.data["unit"], "Pieces");
    assert_eq!(out.data["transaction_id"], "t1");
    let write = out.write.unwrap();
    assert_eq!(write.target["tool"], "stock_add");
    assert_eq!(
        lite::undo_for("stock_add", &write.target["args"], &write.receipt).unwrap(),
        json!({"tool":"stock_undo_transaction","args":{"transaction_id":"t1"}})
    );
    let e = tools::execute(
        &client,
        call(
            "add_stock",
            json!({"product":"milk","amount":1,"unit":"box"}),
        ),
    )
    .await
    .err()
    .unwrap();
    assert_eq!(e.code, "NOT_FOUND");
    assert!(e.message.starts_with("unit:"));
    assert_eq!(e.recovery.unwrap()["candidates"][1]["name"], "Pack");
}

#[tokio::test]
async fn unknown_products_are_never_created_implicitly() {
    let (client, log) = household().await;
    let e = tools::execute(
        &client,
        call("add_stock", json!({"product":"bread","amount":1})),
    )
    .await
    .err()
    .unwrap();
    assert_eq!(e.code, "NOT_FOUND");
    assert!(e.message.contains("create_product"));
    assert_eq!(e.recovery.unwrap()["suggestion"]["tool"], "create_product");
    assert!(posts(&log).is_empty());
    let e = tools::execute(
        &client,
        call("create_product", json!({"name":"Whole MILK"})),
    )
    .await
    .err()
    .unwrap();
    assert_eq!(e.code, "ALREADY_EXISTS");
    let made = tools::execute(&client, call("create_product", json!({"name":"Bread"})))
        .await
        .unwrap();
    assert_eq!(made.data["product"]["id"], 9);
    let post = posts(&log).pop().unwrap();
    assert_eq!(post.body["location_id"], 2);
    assert_eq!(post.body["qu_id_stock"], 2);
    assert_eq!(post.body["qu_id_purchase"], 2);
}

#[tokio::test]
async fn consuming_too_much_names_the_current_stock() {
    let (client, _) = household().await;
    let e = tools::execute(
        &client,
        call("consume_stock", json!({"product":"whole milk","amount":5})),
    )
    .await
    .err()
    .unwrap();
    assert_eq!(e.code, "NOT_ENOUGH_STOCK");
    assert_eq!(
        e.message,
        "amount: 5 exceeds current stock of Whole Milk (3 Pieces); nothing was changed"
    );
}

#[tokio::test]
async fn shopping_remove_defaults_to_the_listed_amount() {
    let (client, log) = household().await;
    tools::execute(&client, call("shopping_remove", json!({"product":1})))
        .await
        .unwrap();
    let post = posts(&log).pop().unwrap();
    assert_eq!(post.path, "/api/stock/shoppinglist/remove-product");
    assert_eq!(post.body["product_amount"], "3");
    assert_eq!(post.body["list_id"], 1);
    let e = tools::execute(&client, call("shopping_remove", json!({"product":"oat"})))
        .await
        .err()
        .unwrap();
    assert_eq!(e.code, "NOT_FOUND");
    assert!(e.message.contains("not on the Main list"));
}

#[tokio::test]
async fn undo_refuses_purchases_merged_into_other_stock() {
    let (client, log) = household().await;
    let e = tools::execute(
        &client,
        call("stock_undo_transaction", json!({"transaction_id":"t1"})),
    )
    .await
    .err()
    .unwrap();
    assert_eq!(e.code, "UNDO_UNSAFE");
    assert!(posts(&log).is_empty());
    assert!(
        log.lock()
            .unwrap()
            .iter()
            .any(|r| r.query.contains("query[]=stock_id=s1"))
    );
}

#[tokio::test]
async fn meal_plan_defaults_to_this_week() {
    use chrono::Datelike;
    let (client, log) = household().await;
    let out = tools::execute(&client, call("meal_plan", json!({})))
        .await
        .unwrap();
    let today = chrono::Local::now().date_naive();
    let monday = today - chrono::Days::new(today.weekday().num_days_from_monday().into());
    assert_eq!(out.data["from"], monday.to_string());
    assert_eq!(out.data["to"], (monday + chrono::Days::new(6)).to_string());
    let q = log.lock().unwrap().last().unwrap().query.clone();
    assert!(q.contains(&format!("query[]=day>={monday}")), "{q}");
    assert!(
        catalog::validate(
            "meal_plan",
            json!({"from":"2030-01-08","to":"2030-01-01"}),
            AccessMode::Full
        )
        .is_ok()
    );
    let e = tools::execute(
        &client,
        call("meal_plan", json!({"from":"2030-01-08","to":"2030-01-01"})),
    )
    .await
    .err()
    .unwrap();
    assert!(e.message.starts_with("to:"));
}

#[test]
fn lite_schemas_and_undo_mapping() {
    let e = catalog::validate("mark_done", json!({"chore":true}), AccessMode::Full)
        .err()
        .unwrap();
    assert_eq!(e.message, "chore: must be a string or integer");
    for (name, args, receipt, undo) in [
        (
            "chore_execute",
            json!({"chore_id":4}),
            json!({"id":11,"chore_id":4}),
            Some(json!({"tool":"chore_undo_execution","args":{"execution_id":11}})),
        ),
        (
            "task_complete",
            json!({"task_id":5}),
            Value::Null,
            Some(json!({"tool":"task_undo","args":{"task_id":5}})),
        ),
        (
            "battery_charge",
            json!({"battery_id":1}),
            json!({"id":3}),
            Some(json!({"tool":"battery_undo_charge","args":{"charge_cycle_id":3}})),
        ),
        (
            "stock_undo_transaction",
            json!({"transaction_id":"t1"}),
            json!([{"transaction_id":"t1"}]),
            None,
        ),
        (
            "records_create",
            json!({}),
            json!({"created_object_id":1}),
            None,
        ),
    ] {
        assert_eq!(lite::undo_for(name, &args, &receipt), undo, "{name}");
    }
    let read_only = lite::operations(AccessMode::ReadOnly);
    assert!(read_only.iter().all(|op| !op.mutates));
    assert!(read_only.iter().any(|op| op.name == "find_product"));
    let all = lite::operations(AccessMode::Full);
    assert_eq!(all.len(), lite::NAMES.len());
    for op in &all {
        assert!(!op.description.contains('\n') && op.description.len() < 100);
        for (k, p) in op.schema["properties"].as_object().unwrap() {
            assert!(p["description"].is_string(), "{}.{k}", op.name);
        }
    }
}
