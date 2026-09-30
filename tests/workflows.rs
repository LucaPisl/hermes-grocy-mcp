use axum::{Router, http::StatusCode, response::IntoResponse};
use hermes_grocy_mcp::{
    catalog,
    client::{ApiBase, ApiClient},
    model::{AccessMode, PrivateProfile, TransportPolicy, Verification, WriteStatus},
    tools,
};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
fn profile(url: &str) -> PrivateProfile {
    PrivateProfile {
        api_base: ApiBase::parse(url, TransportPolicy::LoopbackDevelopment).unwrap(),
        api_key: secrecy::SecretString::from("synthetic-key"),
        access: AccessMode::Full,
        timezone: None,
        defaults: json!(null),
        ca_path: None,
        transport: TransportPolicy::LoopbackDevelopment,
        unsafe_storage: false,
    }
}
#[tokio::test]
async fn workflow_results_and_files() {
    let writes = Arc::new(AtomicUsize::new(0));
    let accepted = writes.clone();
    let uploaded = Arc::new(std::sync::Mutex::new(Vec::new()));
    let captured = uploaded.clone();
    let app = Router::new().fallback(move |req: axum::extract::Request| {
        let accepted = accepted.clone();
        let captured = captured.clone();
        async move {
            let path = req.uri().path().to_string();
            let method = req.method().to_string();
            if path.starts_with("/api/files/") {
                if method == "PUT" {
                    *captured.lock().unwrap() =
                        axum::body::to_bytes(req.into_body(), 8 * 1024 * 1024)
                            .await
                            .unwrap()
                            .to_vec();
                    return StatusCode::NO_CONTENT.into_response();
                }
                return (
                    [("content-type", "application/pdf")],
                    b"%PDF-synthetic".to_vec(),
                )
                    .into_response();
            }
            if path == "/api/calendar/ical" {
                return (
                    [("content-type", "text/calendar")],
                    "BEGIN:VCALENDAR\r\nEND:VCALENDAR",
                )
                    .into_response();
            }
            if method == "POST" {
                accepted.fetch_add(1, Ordering::SeqCst);
                return (
                    [("content-type", "application/json")],
                    "[{\"transaction_id\":\"synthetic-transaction\"}]",
                )
                    .into_response();
            }
            if path == "/api/stock/products/7" {
                if accepted.load(Ordering::SeqCst) > 0 {
                    return StatusCode::SERVICE_UNAVAILABLE.into_response();
                }
                return (
                    [("content-type", "application/json")],
                    "{\"product\":{\"id\":7,\"qu_id_stock\":1}}",
                )
                    .into_response();
            }
            if path == "/api/stock/volatile" {
                return axum::Json(json!({"due_products":[{"id":7}],"overdue_products":[]}))
                    .into_response();
            }
            axum::Json(json!([])).into_response()
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = ApiClient::new(profile(&format!("http://127.0.0.1:{port}"))).unwrap();
    let call = |name, args| catalog::validate(name, args, AccessMode::Full).unwrap();
    let volatile = tools::execute(&client, call("stock_volatile", json!({})))
        .await
        .unwrap();
    assert_eq!(volatile.data["due_products"]["items"][0]["id"], 7);
    let written = tools::execute(
        &client,
        call("stock_add", json!({"product_id":7,"amount":1})),
    )
    .await
    .unwrap();
    let report = written.write.unwrap();
    assert_eq!(report.status, WriteStatus::Completed);
    assert_eq!(report.verification, Verification::Incomplete);
    assert_eq!(report.receipt[0]["transaction_id"], "synthetic-transaction");
    assert_eq!(writes.load(Ordering::SeqCst), 1);
    let upload=tools::execute(&client,call("file_upload",json!({"group":"userfiles","filename":"sample.pdf","content_base64":"JVBERi1zeW50aGV0aWM="}))).await.unwrap();
    assert_eq!(*uploaded.lock().unwrap(), b"%PDF-synthetic");
    assert_eq!(upload.write.unwrap().status, WriteStatus::Completed);
    let file = tools::execute(
        &client,
        call(
            "file_get",
            json!({"group":"userfiles","filename":"sample.pdf"}),
        ),
    )
    .await
    .unwrap();
    assert_eq!(file.attachment.unwrap().bytes, b"%PDF-synthetic");
    assert!(
        catalog::validate(
            "file_get",
            json!({"group":"userfiles","filename":"../unsafe"}),
            AccessMode::Full
        )
        .is_err()
    );
    let ical = tools::execute(&client, call("calendar_export", json!({})))
        .await
        .unwrap();
    assert!(
        ical.data["content"]
            .as_str()
            .unwrap()
            .starts_with("BEGIN:VCALENDAR")
    );
    server.abort();
}
