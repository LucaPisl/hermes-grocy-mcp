mod support;
use hermes_grocy_mcp::{mcp::McpServer, model::AccessMode};
use rmcp::{ServiceExt, model::CallToolRequestParams};
use serde_json::json;
use std::sync::Arc;
use support::{MemoryStore, selection};
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
        let server = McpServer::new(store, selection(), mode);
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
