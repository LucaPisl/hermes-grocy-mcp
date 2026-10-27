//! Regression tests for request shapes and error reporting against a fake Grocy.
mod support;
use hermes_grocy_mcp::{catalog, model::AccessMode, tools};
use serde_json::json;
fn call(name: &str, args: serde_json::Value) -> catalog::ValidatedCall {
    catalog::validate(name, args, AccessMode::Full).unwrap()
}
#[tokio::test]
async fn stock_writes_never_request_labels() {
    let (url, log) = support::fake_grocy(|r| match r.path.as_str() {
        "/api/stock/products/7" | "/api/stock/products/by-barcode/123" => {
            (200, json!({"product":{"id":7,"qu_id_stock":1}}))
        }
        _ if r.method == "POST" => (200, json!([{"transaction_id":"t1"}])),
        _ => (200, json!([])),
    })
    .await;
    let client = support::client(&url);
    for (name, args) in [
        ("stock_add", json!({"product_id":7,"amount":1})),
        ("stock_inventory", json!({"product_id":7,"new_amount":2})),
        ("stock_add_by_barcode", json!({"barcode":"123","amount":1})),
        (
            "stock_inventory_by_barcode",
            json!({"barcode":"123","new_amount":1}),
        ),
    ] {
        tools::execute(&client, call(name, args)).await.unwrap();
    }
    let posts = log
        .lock()
        .unwrap()
        .iter()
        .filter(|r| r.method == "POST")
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(posts.len(), 4);
    assert!(
        posts
            .iter()
            .all(|r| r.body.get("stock_label_type").is_none())
    );
    // Explicit opt-in is still possible, and 0 (no label) is accepted.
    for v in [0, 1, 2] {
        assert!(
            catalog::validate(
                "stock_add",
                json!({"product_id":7,"amount":1,"stock_label_type":v}),
                AccessMode::Full
            )
            .is_ok()
        );
    }
    assert!(
        catalog::validate(
            "stock_add",
            json!({"product_id":7,"amount":1,"stock_label_type":3}),
            AccessMode::Full
        )
        .is_err()
    );
}
#[tokio::test]
async fn upstream_error_message_is_returned_without_details() {
    let long = "x".repeat(400);
    let (url, _) = support::fake_grocy(move |r| match r.path.as_str() {
        "/api/stock/products/7" => (200, json!({"product":{"id":7,"qu_id_stock":1}})),
        "/api/stock/products/7/consume" => (400, json!({"error_message":"Amount to be consumed cannot be > current stock amount (see https://secret.example/x)","error_details":{"stack_trace":"/app/www/secret.php"}})),
        "/api/objects/products" => (400, json!({"error_message":long})),
        _ => (200, json!([])),
    })
    .await;
    let client = support::client(&url);
    let e = tools::execute(
        &client,
        call("stock_consume", json!({"product_id":7,"amount":3})),
    )
    .await
    .err()
    .unwrap();
    assert_eq!(e.code, "API_REJECTED");
    let recovery = e.recovery.unwrap();
    let message = recovery["upstream_message"].as_str().unwrap();
    assert!(message.starts_with("Amount to be consumed"));
    assert!(!message.contains("secret.example"));
    assert!(!recovery.to_string().contains("stack_trace"));
    assert!(recovery["hint"].as_str().unwrap().contains("stock"));
    let e = tools::execute(&client, call("records_list", json!({"entity":"products"})))
        .await
        .err()
        .unwrap();
    assert!(
        e.recovery.unwrap()["upstream_message"]
            .as_str()
            .unwrap()
            .chars()
            .count()
            <= 301
    );
}
#[tokio::test]
async fn paging_only_for_array_endpoints() {
    let (url, log) = support::fake_grocy(|r| match r.path.as_str() {
        "/api/system/time" => (200, json!({"timezone":"UTC"})),
        "/api/stock" => (
            200,
            json!((0..30).map(|i| json!({"product_id":i})).collect::<Vec<_>>()),
        ),
        _ => (
            200,
            json!({"due_products":(0..30).collect::<Vec<_>>(),"missing_products":[1]}),
        ),
    })
    .await;
    let client = support::client(&url);
    assert!(catalog::validate("server_time", json!({"limit":5}), AccessMode::Full).is_err());
    assert!(
        catalog::validate(
            "stock_product",
            json!({"product_id":1,"offset":1}),
            AccessMode::Full
        )
        .is_err()
    );
    let time = tools::execute(&client, call("server_time", json!({})))
        .await
        .unwrap();
    assert_eq!(time.data["timezone"], "UTC");
    let stock = tools::execute(&client, call("stock_list", json!({})))
        .await
        .unwrap();
    assert_eq!(stock.data["items"].as_array().unwrap().len(), 25);
    assert_eq!(stock.data["next_offset"], 25);
    let volatile = tools::execute(&client, call("stock_volatile", json!({})))
        .await
        .unwrap();
    assert_eq!(volatile.data["due_products"].as_array().unwrap().len(), 30);
    assert!(
        log.lock()
            .unwrap()
            .iter()
            .all(|r| !r.query.contains("offset"))
    );
}
#[tokio::test]
async fn records_filter_order_projection_and_lookup() {
    let (url, log) = support::fake_grocy(|r| {
        if r.query.contains("name~milk") {
            (200, json!([{"id":2,"name":"Oat Milk","location_id":1},{"id":1,"name":"milk","location_id":1}]))
        } else {
            (200, json!([{"id":1,"name":"Milk","location_id":1,"description":"x"}]))
        }
    })
    .await;
    let client = support::client(&url);
    let listed = tools::execute(
        &client,
        call("records_list", json!({"entity":"products","filter":["name~Mi=lk","location_id=1"],"order":"name:desc","fields":["name"]})),
    )
    .await
    .unwrap();
    assert_eq!(listed.data["items"][0], json!({"id":1,"name":"Milk"}));
    let q = log.lock().unwrap().last().unwrap().query.clone();
    assert!(q.contains("query[]=name~Mi=lk"), "{q}");
    assert!(q.contains("query[]=location_id=1"));
    assert!(q.contains("order=name:desc"));
    assert!(q.contains("limit=26"));
    for bad in [
        json!({"entity":"products","filter":["bogus=1"]}),
        json!({"entity":"products","filter":["name"]}),
        json!({"entity":"products","order":"name;drop"}),
        json!({"entity":"products","fields":["password"]}),
    ] {
        let e = catalog::validate("records_list", bad, AccessMode::Full)
            .err()
            .unwrap();
        assert!(e.message.contains(':'), "{}", e.message);
    }
    let found = tools::execute(
        &client,
        call("records_lookup", json!({"entity":"products","name":"milk"})),
    )
    .await
    .unwrap();
    assert_eq!(found.data["candidates"][0]["name"], "milk");
    assert_eq!(found.data["exact_matches"], 1);
    assert_eq!(found.data["ambiguous"], false);
    assert!(
        log.lock()
            .unwrap()
            .last()
            .unwrap()
            .query
            .contains("query[]=name~milk")
    );
}
#[tokio::test]
async fn file_names_with_slash_in_base64_are_percent_encoded() {
    let (url, log) =
        support::fake_grocy(|_| (404, json!({"error_message":"Invalid filename"}))).await;
    let client = support::client(&url);
    // base64("aoé.pdf") = "YW/DqS5wZGY=" contains '/'.
    let e = tools::execute(
        &client,
        call(
            "file_get",
            json!({"group":"userfiles","filename":"aoé.pdf"}),
        ),
    )
    .await
    .err()
    .unwrap();
    assert!(
        e.recovery.unwrap()["hint"]
            .as_str()
            .unwrap()
            .contains("plain ASCII")
    );
    let seen = log.lock().unwrap();
    assert_eq!(seen[0].path, "/api/files/userfiles/YW%2FDqS5wZGY=");
}
#[test]
fn shopping_product_tools_require_a_product() {
    for name in ["shopping_add_product", "shopping_remove_product"] {
        let e = catalog::validate(name, json!({"list_id":1}), AccessMode::Full)
            .err()
            .unwrap();
        assert_eq!(e.message, "product_id: is required");
    }
}
