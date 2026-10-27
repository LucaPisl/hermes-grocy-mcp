#![allow(dead_code)]
use futures_util::future::BoxFuture;
use hermes_grocy_mcp::{
    client::ApiBase,
    credentials::{CredentialStore, EnrollmentConsent},
    error::{AppError, Result},
    model::{AccessMode, PrivateProfile, ProfileSelection, ProviderIdentity, TransportPolicy},
};
use serde_json::json;
pub struct MemoryStore {
    pub url: String,
    pub access: AccessMode,
    pub key: String,
}
impl CredentialStore for MemoryStore {
    fn load(&self, _s: ProfileSelection) -> BoxFuture<'static, Result<PrivateProfile>> {
        let url = self.url.clone();
        let access = self.access;
        let key = self.key.clone();
        Box::pin(async move {
            Ok(PrivateProfile {
                api_base: ApiBase::parse(&url, TransportPolicy::LoopbackDevelopment)?,
                api_key: secrecy::SecretString::from(key),
                access,
                timezone: Some("UTC".into()),
                defaults: json!({}),
                ca_path: None,
                transport: TransportPolicy::LoopbackDevelopment,
                unsafe_storage: false,
            })
        })
    }
    fn put(
        &self,
        _s: ProfileSelection,
        _p: PrivateProfile,
        _c: EnrollmentConsent,
    ) -> BoxFuture<'static, Result<()>> {
        Box::pin(async { Err(AppError::input()) })
    }
    fn forget(&self, _s: ProfileSelection) -> BoxFuture<'static, Result<()>> {
        Box::pin(async { Err(AppError::input()) })
    }
}
pub fn selection() -> ProfileSelection {
    ProfileSelection {
        profile: "synthetic".into(),
        provider: ProviderIdentity {
            bus_name: "org.freedesktop.secrets".into(),
            executable: "/test/wallet".into(),
        },
    }
}
/// A recorded request to the fake Grocy API.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct Seen {
    pub method: String,
    pub path: String,
    pub query: String,
    pub body: serde_json::Value,
}
/// Fake Grocy API: `respond` maps each request to (status, JSON); every request is logged.
#[allow(dead_code)]
pub async fn fake_grocy(
    respond: impl Fn(&Seen) -> (u16, serde_json::Value) + Send + Sync + 'static,
) -> (String, std::sync::Arc<std::sync::Mutex<Vec<Seen>>>) {
    use std::sync::{Arc, Mutex};
    let log = Arc::new(Mutex::new(Vec::new()));
    let seen = log.clone();
    let respond = Arc::new(respond);
    let app = axum::Router::new().fallback(move |req: axum::extract::Request| {
        let seen = seen.clone();
        let respond = respond.clone();
        async move {
            let method = req.method().to_string();
            let path = req.uri().path().to_string();
            let query = req
                .uri()
                .query()
                .map(|q| {
                    url::form_urlencoded::parse(q.as_bytes())
                        .map(|(k, v)| format!("{k}={v}"))
                        .collect::<Vec<_>>()
                        .join("&")
                })
                .unwrap_or_default();
            let bytes = axum::body::to_bytes(req.into_body(), 1 << 20)
                .await
                .unwrap();
            let body = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
            let r = Seen {
                method,
                path,
                query,
                body,
            };
            let (status, value) = respond(&r);
            seen.lock().unwrap().push(r);
            (
                axum::http::StatusCode::from_u16(status).unwrap(),
                axum::Json(value),
            )
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://127.0.0.1:{port}"), log)
}
/// A loopback API client with a synthetic key.
#[allow(dead_code)]
pub fn client(url: &str) -> hermes_grocy_mcp::client::ApiClient {
    hermes_grocy_mcp::client::ApiClient::new(PrivateProfile {
        api_base: ApiBase::parse(url, TransportPolicy::LoopbackDevelopment).unwrap(),
        api_key: secrecy::SecretString::from("synthetic-key"),
        access: AccessMode::Full,
        timezone: None,
        defaults: json!(null),
        ca_path: None,
        transport: TransportPolicy::LoopbackDevelopment,
        unsafe_storage: false,
    })
    .unwrap()
}
/// MCP test client that supports elicitation and answers every confirmation with `self.0`.
pub struct Approver(pub bool);
impl rmcp::ClientHandler for Approver {
    fn get_info(&self) -> rmcp::model::ClientInfo {
        let mut info = rmcp::model::ClientInfo::default();
        info.capabilities = rmcp::model::ClientCapabilities::builder()
            .enable_elicitation()
            .build();
        info
    }
    async fn create_elicitation(
        &self,
        _request: rmcp::model::CreateElicitationRequestParams,
        _context: rmcp::service::RequestContext<rmcp::RoleClient>,
    ) -> std::result::Result<rmcp::model::CreateElicitationResult, rmcp::ErrorData> {
        let mut result = rmcp::model::CreateElicitationResult::new(if self.0 {
            rmcp::model::ElicitationAction::Accept
        } else {
            rmcp::model::ElicitationAction::Decline
        });
        result.content = Some(json!({"confirm":self.0}));
        Ok(result)
    }
}
