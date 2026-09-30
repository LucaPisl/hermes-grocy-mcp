use hermes_grocy_mcp::{client::ApiBase, model::TransportPolicy};
#[test]
fn api_boundary() {
    for (input, path) in [
        ("https://grocy.example/household", "/household/api"),
        ("https://grocy.example/household/api/", "/household/api"),
        ("https://grocy.example", "/api"),
    ] {
        assert_eq!(
            ApiBase::parse(input, TransportPolicy::HttpsOnly)
                .unwrap()
                .path(),
            path
        );
    }
    for input in [
        "https://user:pass@grocy.example/api",
        "https://@grocy.example/api",
        "http://2130706433/api",
        "https://grocy.example/api?q=1",
        "https://grocy.example/api#x",
        "https://grocy.example/a/../api",
        "https://grocy.example/%2e/api",
        "https://grocy.example/a%2fb/api",
        "https://grocy.example/%252f/api",
        "http://192.168.1.2/api",
        "http://localhost/api",
    ] {
        assert!(
            ApiBase::parse(input, TransportPolicy::LoopbackDevelopment).is_err(),
            "accepted {input}"
        );
    }
    assert!(ApiBase::parse("http://127.0.0.1:8000", TransportPolicy::HttpsOnly).is_err());
    assert_eq!(
        ApiBase::parse(
            "http://127.0.0.1:8000",
            TransportPolicy::LoopbackDevelopment
        )
        .unwrap()
        .path(),
        "/api"
    );
}

use axum::{Router, http::StatusCode, response::IntoResponse};
use hermes_grocy_mcp::{
    client::{ApiClient, ApiRequest, HttpMethod, RequestBody, ResponseKind},
    model::{AccessMode, PrivateProfile},
};
use secrecy::SecretString;
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
fn profile(url: &str) -> PrivateProfile {
    PrivateProfile {
        api_base: ApiBase::parse(url, TransportPolicy::LoopbackDevelopment).unwrap(),
        api_key: SecretString::from("synthetic-key"),
        access: AccessMode::Full,
        timezone: None,
        defaults: serde_json::Value::Null,
        ca_path: None,
        transport: TransportPolicy::LoopbackDevelopment,
        unsafe_storage: false,
    }
}
#[tokio::test]
async fn write_dispatch_boundary() {
    let count = Arc::new(AtomicUsize::new(0));
    let counted = count.clone();
    let app = Router::new().fallback(move |req: axum::extract::Request| {
        let counted = counted.clone();
        async move {
            counted.fetch_add(1, Ordering::SeqCst);
            assert!(req.uri().path().starts_with("/household/api/"));
            assert_eq!(req.headers().get("GROCY-API-KEY").unwrap(), "synthetic-key");
            match req.uri().path() {
                "/household/api/redirect" => (
                    StatusCode::FOUND,
                    [("location", "http://127.0.0.1:1/secret")],
                    "",
                )
                    .into_response(),
                "/household/api/compressed" => {
                    (StatusCode::OK, [("content-encoding", "gzip")], "data").into_response()
                }
                "/household/api/large-json" => (
                    StatusCode::OK,
                    [("content-type", "application/json")],
                    "x".repeat(2 * 1024 * 1024 + 1),
                )
                    .into_response(),
                "/household/api/large-file" => {
                    (StatusCode::OK, "x".repeat(8 * 1024 * 1024 + 1)).into_response()
                }
                "/household/api/error" => {
                    (StatusCode::BAD_REQUEST, "synthetic-private-marker").into_response()
                }
                "/household/api/empty" => StatusCode::NO_CONTENT.into_response(),
                _ => (
                    StatusCode::OK,
                    [("content-type", "application/json")],
                    "{\"id\":7}",
                )
                    .into_response(),
            }
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = ApiClient::new(profile(&format!("http://127.0.0.1:{port}/household"))).unwrap();
    for (route, kind, code) in [
        ("/redirect", ResponseKind::Json, "API_REDIRECT"),
        ("/compressed", ResponseKind::Json, "COMPRESSION_REFUSED"),
        ("/large-json", ResponseKind::Json, "RESPONSE_TOO_LARGE"),
        ("/large-file", ResponseKind::Binary, "RESPONSE_TOO_LARGE"),
        ("/error", ResponseKind::Json, "API_REJECTED"),
    ] {
        let mut r = ApiRequest::get(route);
        r.kind = kind;
        let err = client.request(r).await.err().unwrap();
        assert_eq!(err.code, code);
        assert!(!err.to_string().contains("synthetic-private-marker"));
    }
    assert_eq!(count.load(Ordering::SeqCst), 5);
    let success = client
        .request(ApiRequest::get("/data"))
        .await
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(success["id"], 7);
    let mut empty = ApiRequest::get("/empty");
    empty.method = HttpMethod::Put;
    assert!(
        client
            .request(empty)
            .await
            .unwrap()
            .json()
            .unwrap()
            .is_null()
    );
    let dead = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = dead.local_addr().unwrap().port();
    let attempts = Arc::new(AtomicUsize::new(0));
    let accepted = attempts.clone();
    let dropper = tokio::spawn(async move {
        let (s, _) = dead.accept().await.unwrap();
        accepted.fetch_add(1, Ordering::SeqCst);
        drop(s);
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    });
    let c = ApiClient::new(profile(&format!("http://127.0.0.1:{port}"))).unwrap();
    let mut r = ApiRequest::get("/stock");
    r.method = HttpMethod::Post;
    r.body = RequestBody::Json(serde_json::json!({"amount":1}));
    assert_eq!(
        c.request(r).await.err().unwrap().code,
        "UNKNOWN_WRITE_OUTCOME"
    );
    dropper.await.unwrap();
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    let tls = ApiClient::new(profile(&format!("https://127.0.0.1:{port}"))).unwrap();
    assert_eq!(
        tls.request(ApiRequest::get("/data"))
            .await
            .err()
            .unwrap()
            .code,
        "CONNECTION_FAILED"
    );
    let mut path = BTreeMap::new();
    path.insert("id".into(), "a/b ?#%".into());
    assert_eq!(
        ApiBase::parse("https://grocy.example/prefix", TransportPolicy::HttpsOnly)
            .unwrap()
            .route("/objects/{id}", &path)
            .unwrap()
            .path(),
        "/prefix/api/objects/a%2Fb%20%3F%23%25"
    );
    server.abort();
}

#[tokio::test]
async fn untrusted_tls_certificate_is_rejected() {
    use std::process::{Command, Stdio};
    let dir = tempfile::tempdir().unwrap();
    let cert = dir.path().join("cert.pem");
    let key = dir.path().join("key.pem");
    assert!(
        Command::new("openssl")
            .args([
                "req",
                "-x509",
                "-newkey",
                "rsa:2048",
                "-nodes",
                "-days",
                "1",
                "-subj",
                "/CN=localhost",
                "-keyout"
            ])
            .arg(&key)
            .arg("-out")
            .arg(&cert)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success()
    );
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let mut server = Command::new("openssl")
        .args([
            "s_server",
            "-accept",
            &format!("127.0.0.1:{port}"),
            "-www",
            "-quiet",
            "-cert",
        ])
        .arg(cert)
        .arg("-key")
        .arg(key)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut ready = false;
    for _ in 0..50 {
        if tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_ok()
        {
            ready = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(ready);
    let result = ApiClient::new(profile(&format!("https://127.0.0.1:{port}")))
        .unwrap()
        .request(ApiRequest::get("/data"))
        .await;
    let _ = server.kill();
    let _ = server.wait();
    assert_eq!(result.err().unwrap().code, "CONNECTION_FAILED");
}
