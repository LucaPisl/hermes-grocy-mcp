mod support;
use hermes_grocy_mcp::{
    catalog::{self, Tier},
    mcp::{McpServer, ServeOptions},
    model::AccessMode,
};
use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, CallToolResult},
    service::{RoleClient, RunningService},
};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use support::{Approver, MemoryStore, selection};
#[tokio::test]
async fn protocol_returns_data_and_policy() {
    let app = axum::Router::new().fallback(|| async { axum::Json(json!([{"id":7,"amount":"2"}])) });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let api = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    for mode in [AccessMode::Full, AccessMode::ReadOnly] {
        let store = Arc::new(MemoryStore {
            url: format!("http://127.0.0.1:{port}"),
            access: mode,
            key: "synthetic-key".into(),
        });
        let server = McpServer::new(
            store,
            selection(),
            mode,
            ServeOptions {
                tier: Tier::Full,
                allow_destructive: true,
            },
        );
        let (a, b) = tokio::io::duplex(65536);
        let serving =
            tokio::spawn(async move { server.serve(a).await.unwrap().waiting().await.unwrap() });
        let client = ().serve(b).await.unwrap();
        let tools = client.list_all_tools().await.unwrap();
        assert!(tools.len() > 20);
        if mode == AccessMode::ReadOnly {
            assert!(!tools.iter().any(|t| t.name == "records_delete"));
        } else {
            let delete = tools.iter().find(|t| t.name == "records_delete").unwrap();
            assert_eq!(
                delete.annotations.as_ref().unwrap().destructive_hint,
                Some(true)
            );
        }
        let result = client
            .call_tool(
                CallToolRequestParams::new("stock_list")
                    .with_arguments(json!({}).as_object().unwrap().clone()),
            )
            .await
            .unwrap();
        let text = result.content[0].as_text().unwrap();
        let data: serde_json::Value = serde_json::from_str(&text.text).unwrap();
        assert_eq!(Some(data.clone()), result.structured_content);
        assert_eq!(data["data"]["items"][0]["id"], 7);
        assert_ne!(result.is_error, Some(true));
        let invalid = client
            .call_tool(CallToolRequestParams::new("not_a_tool"))
            .await
            .unwrap();
        assert_eq!(invalid.is_error, Some(true));
        assert!(
            !serde_json::to_string(&invalid)
                .unwrap()
                .contains("synthetic-key")
        );
        client.cancel().await.unwrap();
        serving.await.unwrap();
    }
    api.abort();
}

/// Serve over an in-memory pipe against `url`, with the given tier options and client handler.
async fn connect<C: rmcp::ClientHandler>(
    url: &str,
    mode: AccessMode,
    options: ServeOptions,
    handler: C,
) -> RunningService<RoleClient, C> {
    let store = Arc::new(MemoryStore {
        url: url.into(),
        access: mode,
        key: "synthetic-key".into(),
    });
    let server = McpServer::new(store, selection(), mode, options);
    let (a, b) = tokio::io::duplex(1 << 20);
    tokio::spawn(async move { server.serve(a).await.unwrap().waiting().await.unwrap() });
    handler.serve(b).await.unwrap()
}
async fn invoke<C: rmcp::ClientHandler>(
    client: &RunningService<RoleClient, C>,
    name: &'static str,
    args: Value,
) -> CallToolResult {
    client
        .call_tool(
            CallToolRequestParams::new(name).with_arguments(args.as_object().unwrap().clone()),
        )
        .await
        .unwrap()
}
fn error_code(result: &CallToolResult) -> Value {
    assert_eq!(result.is_error, Some(true));
    result.structured_content.as_ref().unwrap()["error"]["code"].clone()
}
fn options(tier: Tier, allow_destructive: bool) -> ServeOptions {
    ServeOptions {
        tier,
        allow_destructive,
    }
}
#[tokio::test]
async fn tiers_list_safe_annotated_tools() {
    let (url, _) = support::fake_grocy(|_| (200, json!([]))).await;
    let lite = connect(&url, AccessMode::Full, ServeOptions::default(), ()).await;
    let tools = lite.list_all_tools().await.unwrap();
    let mut names = tools.iter().map(|t| t.name.to_string()).collect::<Vec<_>>();
    names.sort();
    let mut expected = hermes_grocy_mcp::tools::lite::NAMES
        .map(String::from)
        .to_vec();
    expected.sort();
    assert_eq!(names, expected);
    for t in &tools {
        let a = t.annotations.as_ref().unwrap();
        assert_eq!(a.destructive_hint, Some(false), "{}", t.name);
        let reads = [
            "find_product",
            "shopping_list",
            "doctor",
            "meal_plan",
            "todo_list",
        ];
        if reads.contains(&t.name.as_ref()) {
            assert_eq!(a.read_only_hint, Some(true), "{}", t.name);
        }
    }
    let add = tools.iter().find(|t| t.name == "add_stock").unwrap();
    assert_eq!(
        add.annotations.as_ref().unwrap().read_only_hint,
        Some(false)
    );
    assert_eq!(
        error_code(&invoke(&lite, "stock_add", json!({"product_id":1,"amount":1})).await),
        "UNKNOWN_TOOL"
    );
    lite.cancel().await.unwrap();

    let full = connect(&url, AccessMode::Full, options(Tier::Full, false), ()).await;
    let tools = full.list_all_tools().await.unwrap();
    assert!(tools.len() > 50);
    assert!(
        !tools
            .iter()
            .any(|t| catalog::DESTRUCTIVE_TOOLS.contains(&t.name.as_ref()))
    );
    let create = tools.iter().find(|t| t.name == "records_create").unwrap();
    let entities = create.input_schema["properties"]["entity"]["enum"]
        .as_array()
        .unwrap();
    assert!(
        !entities
            .iter()
            .any(|e| catalog::SCHEMA_ENTITIES.contains(&e.as_str().unwrap()))
    );
    assert_eq!(
        error_code(&invoke(&full, "records_delete", json!({"entity":"products","id":1})).await),
        "UNKNOWN_TOOL"
    );
    full.cancel().await.unwrap();

    let read_only = connect(&url, AccessMode::ReadOnly, ServeOptions::default(), ()).await;
    let tools = read_only.list_all_tools().await.unwrap();
    assert!(
        tools
            .iter()
            .all(|t| t.annotations.as_ref().unwrap().read_only_hint == Some(true))
    );
    assert!(tools.iter().any(|t| t.name == "doctor"));
    read_only.cancel().await.unwrap();
}
#[tokio::test]
async fn destructive_tools_require_a_human_confirmation() {
    let (url, log) = support::fake_grocy(|_| (200, json!([]))).await;
    let wrote = || log.lock().unwrap().iter().any(|r| r.method != "GET");
    let args = json!({"list_id":1});
    let silent = connect(&url, AccessMode::Full, options(Tier::Full, true), ()).await;
    assert_eq!(
        error_code(&invoke(&silent, "shopping_clear", args.clone()).await),
        "CONFIRMATION_UNAVAILABLE"
    );
    silent.cancel().await.unwrap();
    let no = connect(
        &url,
        AccessMode::Full,
        options(Tier::Full, true),
        Approver(false),
    )
    .await;
    assert_eq!(
        error_code(&invoke(&no, "shopping_clear", args.clone()).await),
        "DECLINED"
    );
    no.cancel().await.unwrap();
    assert!(!wrote());
    let yes = connect(
        &url,
        AccessMode::Full,
        options(Tier::Full, true),
        Approver(true),
    )
    .await;
    assert_ne!(
        invoke(&yes, "shopping_clear", args).await.is_error,
        Some(true)
    );
    yes.cancel().await.unwrap();
    assert!(wrote());
}
#[tokio::test]
async fn undo_reverts_the_last_write_after_a_key_rotation_retry() {
    let rejected_once = Arc::new(AtomicBool::new(false));
    let flag = rejected_once.clone();
    let (url, log) = support::fake_grocy(move |r| {
        if !flag.swap(true, Ordering::SeqCst) {
            return (401, json!({"error_message":"Unauthorized"}));
        }
        match r.path.as_str() {
            "/api/objects/products/1" => (200, json!({"id":1,"name":"Milk","qu_id_stock":2})),
            "/api/stock/products/1" => (200, json!({"product":{"id":1,"name":"Milk"},"stock_amount":2,"quantity_unit_stock":{"name":"Piece"}})),
            "/api/stock/products/1/add" => (200, json!([{"transaction_id":"t1"}])),
            "/api/stock/transactions/t1" => (200, json!([{"transaction_type":"purchase","stock_id":"s1"}])),
            "/api/objects/stock_log" => (200, json!([{"transaction_id":"t1","undone":0}])),
            _ => (200, json!([])),
        }
    })
    .await;
    let client = connect(&url, AccessMode::Full, ServeOptions::default(), ()).await;
    let added = invoke(&client, "add_stock", json!({"product":1,"amount":1})).await;
    assert_ne!(added.is_error, Some(true), "{:?}", added.structured_content);
    assert!(rejected_once.load(Ordering::SeqCst));
    assert_eq!(
        error_code(&invoke(&client, "undo", json!({"transaction_id":"t1","x":1})).await),
        "INVALID_INPUT"
    );
    let undone = invoke(&client, "undo", json!({})).await;
    assert_ne!(
        undone.is_error,
        Some(true),
        "{:?}",
        undone.structured_content
    );
    assert!(
        log.lock()
            .unwrap()
            .iter()
            .any(|r| r.method == "POST" && r.path == "/api/stock/transactions/t1/undo")
    );
    assert_eq!(
        error_code(&invoke(&client, "undo", json!({})).await),
        "NOTHING_TO_UNDO"
    );
    client.cancel().await.unwrap();
}
#[tokio::test]
async fn large_results_get_a_text_summary() {
    let rows = (0..500)
        .map(|i| json!({"product_id":i,"note":"x".repeat(64)}))
        .collect::<Vec<_>>();
    let (url, _) = support::fake_grocy(move |_| (200, json!(rows))).await;
    let client = connect(&url, AccessMode::Full, options(Tier::Full, false), ()).await;
    let result = invoke(&client, "stock_list", json!({"limit":500})).await;
    let text = &result.content[0].as_text().unwrap().text;
    assert!(
        text.starts_with("Large result") && text.contains("500 items"),
        "{text}"
    );
    assert!(text.len() < 400);
    assert_eq!(
        result.structured_content.unwrap()["data"]["items"]
            .as_array()
            .unwrap()
            .len(),
        500
    );
    let small = invoke(&client, "stock_list", json!({"limit":2})).await;
    let text: Value = serde_json::from_str(&small.content[0].as_text().unwrap().text).unwrap();
    assert_eq!(Some(text), small.structured_content);
    client.cancel().await.unwrap();
}
