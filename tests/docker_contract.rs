//! Explicit opt-in contract against an owned, disposable fixture. No production credential path.
mod support;
use axum::{
    body::Body,
    extract::{Request, State},
    http::StatusCode,
    response::Response,
};
use hermes_grocy_mcp::{mcp::McpServer, model::AccessMode, setup};
use rmcp::{ServiceExt, model::CallToolRequestParams};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use support::{MemoryStore, selection};
#[derive(Clone)]
struct Proxy {
    target: String,
    http: reqwest::Client,
    outside: Arc<AtomicUsize>,
}
async fn forward(State(state): State<Proxy>, req: Request) -> Response {
    let path = req.uri().path();
    let route = if path.starts_with("/api/") {
        path
    } else if let Some(s) = path.strip_prefix("/household") {
        if s.starts_with("/api/") { s } else { "" }
    } else {
        ""
    };
    if route.is_empty() {
        state.outside.fetch_add(1, Ordering::SeqCst);
        return Response::builder()
            .status(StatusCode::FORBIDDEN)
            .body(Body::empty())
            .unwrap();
    }
    let url = format!(
        "{}{}{}",
        state.target,
        route,
        req.uri()
            .query()
            .map(|q| format!("?{q}"))
            .unwrap_or_default()
    );
    let mut r = state.http.request(req.method().clone(), url);
    for name in ["GROCY-API-KEY", "content-type", "accept-encoding"] {
        if let Some(v) = req.headers().get(name) {
            r = r.header(name, v)
        }
    }
    let bytes = axum::body::to_bytes(req.into_body(), 9 * 1024 * 1024)
        .await
        .unwrap();
    let res = r.body(bytes).send().await.unwrap();
    let status = res.status();
    let headers = res.headers().clone();
    let bytes = res.bytes().await.unwrap();
    if !status.is_success() && status != StatusCode::NOT_FOUND {
        eprintln!(
            "Fixture API {}: {}",
            status,
            String::from_utf8_lossy(&bytes[..bytes.len().min(600)])
        )
    }
    let mut response = Response::builder().status(status);
    if let Some(v) = headers.get("content-type") {
        response = response.header("content-type", v)
    }
    response.body(Body::from(bytes)).unwrap()
}
fn id(v: &Value) -> u64 {
    let v = &v["write"]["receipt"]["created_object_id"];
    v.as_u64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        .expect("created ID in receipt")
}
fn numeric(v: &Value) -> f64 {
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        .unwrap()
}
#[tokio::test]
#[ignore = "requires scripts/docker_fixture.py with a disposable synthetic instance"]
async fn docker_household_contract() {
    let path = std::env::var("GROCY_CONTRACT_MANIFEST").expect("fixture manifest");
    let fixture: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let outside = Arc::new(AtomicUsize::new(0));
    let proxy = Proxy {
        target: fixture["url"].as_str().unwrap().into(),
        http: reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap(),
        outside: outside.clone(),
    };
    let app = axum::Router::new().fallback(forward).with_state(proxy);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let api = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let store = Arc::new(MemoryStore {
        url: format!("http://127.0.0.1:{port}/household"),
        access: AccessMode::Full,
        key: fixture["api_key"].as_str().unwrap().into(),
    });
    let report = setup::doctor(selection(), store.clone()).await.unwrap();
    assert_eq!(report.version.as_deref(), Some("4.7.1"));
    assert!(report.household_access);
    let server = McpServer::new(store.clone(), selection(), AccessMode::Full);
    let (a, b) = tokio::io::duplex(65536);
    let serving =
        tokio::spawn(async move { server.serve(a).await.unwrap().waiting().await.unwrap() });
    let client = ().serve(b).await.unwrap();
    assert!(client.list_all_tools().await.unwrap().len() > 50);
    macro_rules! call {
        ($name:expr,$args:expr) => {{
            let result = client
                .call_tool(
                    CallToolRequestParams::new($name)
                        .with_arguments($args.as_object().unwrap().clone()),
                )
                .await
                .unwrap();
            assert_ne!(
                result.is_error,
                Some(true),
                "{}: {:?}",
                $name,
                result.structured_content
            );
            let text: Value =
                serde_json::from_str(&result.content[0].as_text().unwrap().text).unwrap();
            assert_eq!(Some(text.clone()), result.structured_content);
            text
        }};
    }
    macro_rules! create {($entity:expr,$fields:expr)=>{id(&call!("records_create",json!({"entity":$entity,"fields":$fields})))}}
    let fixture_http = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let account:Value=fixture_http.post(format!("http://127.0.0.1:{port}/api/objects/userfields")).header("GROCY-API-KEY",&store.key).json(&json!({"entity":"users","name":format!("account{}",std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()),"caption":"Account-only definition","type":"text-single-line"})).send().await.unwrap().json().await.unwrap();
    let account_id = numeric(&account["created_object_id"]) as u64;
    let definitions = call!("records_list", json!({"entity":"userfields","limit":500}));
    assert!(
        !definitions["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["entity"] == "users")
    );
    for (tool, args) in [
        (
            "records_get",
            json!({"entity":"userfields","id":account_id}),
        ),
        (
            "records_update",
            json!({"entity":"userfields","id":account_id,"fields":{"caption":"Changed"}}),
        ),
        (
            "records_delete",
            json!({"entity":"userfields","id":account_id}),
        ),
    ] {
        let denied = client
            .call_tool(
                CallToolRequestParams::new(tool).with_arguments(args.as_object().unwrap().clone()),
            )
            .await
            .unwrap();
        assert_eq!(denied.is_error, Some(true));
    }
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
        .to_string();
    let name = |s: &str| format!("Contract {s} {suffix}");
    let unit = create!("quantity_units", json!({"name":name("unit")}));
    let kilo = create!("quantity_units", json!({"name":name("kilounit")}));
    let loc = create!("locations", json!({"name":name("location")}));
    let loc2 = create!("locations", json!({"name":name("location2")}));
    let product = create!(
        "products",
        json!({"name":name("product"),"location_id":loc,"qu_id_purchase":unit,"qu_id_stock":unit,"qu_id_consume":unit,"qu_id_price":unit})
    );
    call!(
        "records_update",
        json!({"entity":"products","id":product,"fields":{"description":"Synthetic contract product"}})
    );
    let added = call!(
        "stock_add",
        json!({"product_id":product,"amount":"10","location_id":loc,"best_before_date":"2999-01-01"})
    );
    assert!(added["write"]["receipt"].is_array());
    let stock = call!("stock_list", json!({}));
    assert!(
        stock["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| numeric(&r["product_id"]) == product as f64)
    );
    let entries = call!("stock_entries", json!({"product_id":product}));
    let entry = numeric(&entries["data"]["items"][0]["id"]) as u64;
    let entry_edit = call!(
        "stock_entry_put",
        json!({"entry_id":entry,"amount":"10","price":"1.25"})
    );
    let stock_note = format!("stocknote{suffix}");
    create!(
        "userfields",
        json!({"entity":"stock","name":stock_note,"caption":"Stock note","type":"text-single-line"})
    );
    call!(
        "userfields_update",
        json!({"entity":"stock","id":entry,"stock_transaction_id":entry_edit["write"]["receipt"][0]["transaction_id"],"fields":{stock_note.clone():"Stock value"}})
    );
    let stock_fields = call!("userfields_get", json!({"entity":"stock","id":entry}));
    assert_eq!(stock_fields["data"][&stock_note], "Stock value");
    call!("stock_open", json!({"product_id":product,"amount":"1"}));
    call!(
        "stock_transfer",
        json!({"product_id":product,"amount":"1","location_id_from":loc,"location_id_to":loc2})
    );
    let consumed = call!(
        "stock_consume",
        json!({"product_id":product,"amount":"1","location_id":loc})
    );
    let booking = &consumed["write"]["receipt"][0];
    let transaction = booking["transaction_id"].clone();
    call!("stock_transaction", json!({"transaction_id":transaction}));
    call!(
        "stock_undo_transaction",
        json!({"transaction_id":transaction})
    );
    call!(
        "stock_inventory",
        json!({"product_id":product,"new_amount":"9","location_id":loc})
    );
    create!(
        "quantity_unit_conversions",
        json!({"from_qu_id":unit,"to_qu_id":kilo,"factor":0.001,"product_id":product})
    );
    call!(
        "records_update",
        json!({"entity":"products","id":product,"fields":{"qu_id_stock":kilo}})
    );
    let details = call!("stock_product", json!({"product_id":product}));
    assert_eq!(
        numeric(&details["data"]["product"]["qu_id_stock"]),
        kilo as f64
    );
    call!(
        "stock_add",
        json!({"product_id":product,"amount":"1000","quantity_unit_id":unit,"best_before_date":"2999-01-01"})
    );
    call!("stock_volatile", json!({}));
    let list = create!("shopping_lists", json!({"name":name("shopping")}));
    call!(
        "shopping_add_product",
        json!({"product_id":product,"list_id":list,"product_amount":2,"qu_id":kilo})
    );
    let exported = call!("shopping_export", json!({"list_id":list}));
    assert!(!exported["data"]["items"].as_array().unwrap().is_empty());
    call!(
        "shopping_remove_product",
        json!({"product_id":product,"list_id":list,"product_amount":1})
    );
    call!("shopping_clear", json!({"list_id":list}));
    let recipe = create!(
        "recipes",
        json!({"name":name("recipe"),"base_servings":1,"desired_servings":1})
    );
    create!(
        "recipes_pos",
        json!({"recipe_id":recipe,"product_id":product,"amount":0.001,"qu_id":kilo})
    );
    let nested = create!(
        "recipes",
        json!({"name":name("nested"),"base_servings":1,"desired_servings":1})
    );
    create!(
        "recipes_nestings",
        json!({"recipe_id":nested,"includes_recipe_id":recipe,"servings":1})
    );
    call!("recipe_fulfillment", json!({"recipe_id":recipe}));
    call!("recipe_copy", json!({"recipe_id":recipe}));
    let used = call!("recipe_consume", json!({"recipe_id":recipe}));
    assert!(used["write"]["receipt"].is_null()); // Grocy returns 204 for this action.
    let log = call!("records_list", json!({"entity":"stock_log","limit":500}));
    let row = log["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|r| {
            r["recipe_id"]
                .as_str()
                .is_some_and(|s| s == recipe.to_string())
                || r["recipe_id"].as_u64() == Some(recipe)
        })
        .expect("recipe consume booking");
    let tx = row["transaction_id"].clone();
    call!("stock_undo_transaction", json!({"transaction_id":tx}));
    let section = create!("meal_plan_sections", json!({"name":name("section")}));
    let meal = create!(
        "meal_plan",
        json!({"day":"2030-01-01","type":"recipe","recipe_id":recipe,"recipe_servings":1,"section_id":section})
    );
    call!(
        "records_update",
        json!({"entity":"meal_plan","id":meal,"fields":{"note":"Synthetic meal"}})
    );
    let chore = create!(
        "chores",
        json!({"name":name("chore"),"period_type":"daily","period_interval":1})
    );
    let executed = call!(
        "chore_execute",
        json!({"chore_id":chore,"tracked_time":"2030-01-01 10:00:00"})
    );
    call!(
        "chore_undo_execution",
        json!({"execution_id":executed["write"]["receipt"]["id"]})
    );
    let task = create!(
        "tasks",
        json!({"name":name("task"),"due_date":"2030-01-01 10:00:00"})
    );
    call!("task_complete", json!({"task_id":task}));
    call!("task_undo", json!({"task_id":task}));
    call!(
        "records_update",
        json!({"entity":"tasks","id":task,"fields":{"due_date":"2030-01-02 10:00:00"}})
    );
    let battery = create!(
        "batteries",
        json!({"name":name("battery"),"charge_interval_days":7})
    );
    let charged = call!(
        "battery_charge",
        json!({"battery_id":battery,"tracked_time":"2030-01-01 10:00:00"})
    );
    call!(
        "battery_undo_charge",
        json!({"charge_cycle_id":charged["write"]["receipt"]["id"]})
    );
    let equipment = create!("equipment", json!({"name":name("equipment")}));
    let note_name = format!("contractnote{suffix}");
    create!(
        "userfields",
        json!({"entity":"equipment","name":note_name,"caption":"Contract note","type":"text-single-line"})
    );
    call!(
        "userfields_update",
        json!({"entity":"equipment","id":equipment,"fields":{note_name.clone():"Synthetic only"}})
    );
    let fields = call!(
        "userfields_get",
        json!({"entity":"equipment","id":equipment})
    );
    assert_eq!(fields["data"][&note_name], "Synthetic only");
    let entity = create!(
        "userentities",
        json!({"name":name("entity"),"caption":"Synthetic"})
    );
    let custom = create!("userobjects", json!({"userentity_id":entity}));
    create!(
        "userfields",
        json!({"entity":format!("userentity-{}",name("entity")),"name":"customnote","caption":"Custom note","type":"text-single-line"})
    );
    call!(
        "userfields_update",
        json!({"entity":"userobjects","id":custom,"fields":{"customnote":"Custom value"}})
    );
    let custom_fields = call!(
        "userfields_get",
        json!({"entity":"userobjects","id":custom})
    );
    assert_eq!(custom_fields["data"]["customnote"], "Custom value");
    call!("assignment_users", json!({}));
    use base64::{Engine, engine::general_purpose::STANDARD};
    let file = format!("manual-{suffix}.pdf");
    let bytes = b"%PDF-1.4\nSynthetic test\n%%EOF";
    call!(
        "file_upload",
        json!({"group":"equipmentmanuals","filename":file,"content_base64":STANDARD.encode(bytes)})
    );
    let downloaded = client
        .call_tool(
            CallToolRequestParams::new("file_get").with_arguments(
                json!({"group":"equipmentmanuals","filename":file})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .unwrap();
    assert_ne!(downloaded.is_error, Some(true));
    let wire = serde_json::to_value(&downloaded.content).unwrap();
    assert!(wire.to_string().contains(&STANDARD.encode(bytes)));
    call!(
        "file_delete",
        json!({"group":"equipmentmanuals","filename":file})
    );
    call!("calendar_export", json!({}));
    call!(
        "records_delete",
        json!({"entity":"equipment","id":equipment})
    );
    client.cancel().await.unwrap();
    serving.await.unwrap();
    // Root API enrollment and read-only rejection are independent of deployment prefix.
    let readonly = Arc::new(MemoryStore {
        url: format!("http://127.0.0.1:{port}/api"),
        access: AccessMode::ReadOnly,
        key: store.key.clone(),
    });
    setup::doctor(selection(), readonly.clone()).await.unwrap();
    let server = McpServer::new(readonly, selection(), AccessMode::ReadOnly);
    let (a, b) = tokio::io::duplex(65536);
    let serving =
        tokio::spawn(async move { server.serve(a).await.unwrap().waiting().await.unwrap() });
    let client = ().serve(b).await.unwrap();
    assert!(
        !client
            .list_all_tools()
            .await
            .unwrap()
            .iter()
            .any(|t| t.name == "stock_add")
    );
    let blocked = client
        .call_tool(
            CallToolRequestParams::new("stock_add").with_arguments(
                json!({"product_id":product,"amount":1})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .unwrap();
    assert_eq!(blocked.is_error, Some(true));
    client.cancel().await.unwrap();
    serving.await.unwrap();
    assert_eq!(outside.load(Ordering::SeqCst), 0);
    api.abort();
}
