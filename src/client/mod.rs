mod destination;
use crate::{
    error::{AppError, Result},
    model::{AccessMode, PrivateProfile},
};
pub use destination::ApiBase;
use futures_util::StreamExt;
use reqwest::{
    Method,
    header::{ACCEPT_ENCODING, CONTENT_ENCODING, CONTENT_TYPE, HeaderValue},
};
use secrecy::ExposeSecret;
use serde_json::{Value, json};
use std::{collections::BTreeMap, time::Duration};
pub const JSON_LIMIT: usize = 2 * 1024 * 1024;
pub const BINARY_LIMIT: usize = 8 * 1024 * 1024;
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
}
impl HttpMethod {
    pub fn mutates(self) -> bool {
        self != Self::Get
    }
    fn reqwest(self) -> Method {
        match self {
            Self::Get => Method::GET,
            Self::Post => Method::POST,
            Self::Put => Method::PUT,
            Self::Delete => Method::DELETE,
        }
    }
}
#[derive(Clone, Copy)]
pub enum ResponseKind {
    Json,
    Text,
    Binary,
}
pub enum RequestBody {
    Empty,
    Json(Value),
    Binary(Vec<u8>),
}
pub struct ApiRequest {
    pub route: &'static str,
    pub method: HttpMethod,
    pub params: BTreeMap<String, String>,
    pub query: BTreeMap<String, String>,
    pub body: RequestBody,
    pub kind: ResponseKind,
}
impl ApiRequest {
    pub fn get(route: &'static str) -> Self {
        Self {
            route,
            method: HttpMethod::Get,
            params: BTreeMap::new(),
            query: BTreeMap::new(),
            body: RequestBody::Empty,
            kind: ResponseKind::Json,
        }
    }
    pub fn target(&self) -> Value {
        json!({"operation":self.route,"identifiers":self.params})
    }
}
pub enum ResponseBody {
    Json(Value),
    Text(String),
    Binary(Vec<u8>),
}
pub struct ApiResponse {
    pub status: u16,
    pub body: ResponseBody,
    pub content_type: String,
}
impl ApiResponse {
    pub fn json(self) -> Result<Value> {
        match self.body {
            ResponseBody::Json(v) => Ok(v),
            _ => Err(AppError::new(
                "INVALID_RESPONSE",
                "Grocy returned an unexpected response format.",
            )),
        }
    }
}
pub struct ApiClient {
    http: reqwest::Client,
    profile: PrivateProfile,
}
impl ApiClient {
    pub fn new(profile: PrivateProfile) -> Result<Self> {
        let mut builder = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd();
        if let Some(path) = &profile.ca_path {
            let cert = std::fs::read(path).map_err(|_| {
                AppError::new(
                    "CA_UNAVAILABLE",
                    "Cannot read the selected trusted CA bundle.",
                )
            })?;
            let certs = reqwest::Certificate::from_pem_bundle(&cert)
                .map_err(|_| AppError::new("INVALID_CA", "Choose a valid PEM CA bundle."))?;
            if certs.is_empty() {
                return Err(AppError::new("INVALID_CA", "Choose a valid PEM CA bundle."));
            }
            for cert in certs {
                builder = builder.add_root_certificate(cert)
            }
        }
        let http = builder.build().map_err(|_| {
            AppError::new(
                "HTTP_CLIENT_ERROR",
                "Could not initialize the verified HTTP client.",
            )
        })?;
        Ok(Self { http, profile })
    }
    pub fn access(&self) -> AccessMode {
        self.profile.access
    }
    pub async fn request(&self, r: ApiRequest) -> Result<ApiResponse> {
        if r.method.mutates() && self.profile.access == AccessMode::ReadOnly {
            return Err(AppError::new(
                "READ_ONLY",
                "This profile does not allow writes.",
            ));
        }
        let target = r.target();
        let write = r.method.mutates();
        let url = self.profile.api_base.route(r.route, &r.params)?;
        let mut key = HeaderValue::from_str(self.profile.api_key.expose_secret())
            .map_err(|_| AppError::new("INVALID_CREDENTIAL", "The stored API key is not valid."))?;
        key.set_sensitive(true);
        let mut request = self
            .http
            .request(r.method.reqwest(), url)
            .header("GROCY-API-KEY", key)
            .header(ACCEPT_ENCODING, "identity")
            .query(&r.query);
        request = match r.body {
            RequestBody::Empty => request,
            RequestBody::Json(v) => request.json(&v),
            RequestBody::Binary(b) => {
                if b.len() > BINARY_LIMIT {
                    return Err(AppError::new(
                        "RESPONSE_TOO_LARGE",
                        "The binary content exceeds 8 MiB.",
                    ));
                }
                request
                    .header(CONTENT_TYPE, "application/octet-stream")
                    .body(b)
            }
        };
        let response=request.send().await.map_err(|_|if write{AppError::unknown(target.clone())}else{AppError::new("CONNECTION_FAILED","Cannot reach the API using verified TLS. Check the URL, trusted CA and API proxy bypass.")})?;
        let status = response.status();
        if status.is_redirection() {
            return Err(AppError::new(
                "API_REDIRECT",
                "The API redirected the request. Check the API URL and reverse-proxy /api bypass; no redirect was followed.",
            ));
        }
        if !status.is_success() {
            return Err(match status.as_u16() {
                401 | 403 => AppError::new(
                    "ACCESS_DENIED",
                    "The API rejected access. Check the dedicated user's key and household permissions.",
                ),
                404 => AppError::new(
                    "NOT_FOUND",
                    "The requested record or API operation is unavailable.",
                ),
                400 | 409 | 422 => AppError::new(
                    "API_REJECTED",
                    "Grocy rejected the operation. Check the target, fields, units and current state.",
                ),
                _ if write => AppError::unknown(target),
                _ => AppError::new(
                    "API_UNAVAILABLE",
                    "The API is unavailable. Check Grocy health and whether an administrator must complete its upgrade.",
                ),
            });
        }
        let outcome_error = |code, message| {
            if write {
                AppError::unknown(target.clone())
            } else {
                AppError::new(code, message)
            }
        };
        if response
            .headers()
            .get(CONTENT_ENCODING)
            .is_some_and(|e| e != "identity")
        {
            return Err(outcome_error(
                "COMPRESSION_REFUSED",
                "Compressed API responses are not supported.",
            ));
        }
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/octet-stream")
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        let limit = match r.kind {
            ResponseKind::Binary => BINARY_LIMIT,
            _ => JSON_LIMIT,
        };
        if response.content_length().is_some_and(|n| n > limit as u64) {
            return Err(outcome_error(
                "RESPONSE_TOO_LARGE",
                "The API response exceeds the configured size limit.",
            ));
        }
        let mut bytes = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let c = chunk.map_err(|_| {
                outcome_error("INCOMPLETE_RESPONSE", "The response was interrupted.")
            })?;
            if c.len() > limit - bytes.len() {
                return Err(outcome_error(
                    "RESPONSE_TOO_LARGE",
                    "The API response exceeds the configured size limit.",
                ));
            }
            bytes.extend_from_slice(&c);
        }
        let body = match r.kind {
            ResponseKind::Json if bytes.is_empty() => ResponseBody::Json(Value::Null),
            ResponseKind::Json => {
                if content_type != "application/json" {
                    return Err(outcome_error(
                        "INVALID_RESPONSE",
                        "Expected JSON from the API. Check the proxy bypass.",
                    ));
                }
                ResponseBody::Json(serde_json::from_slice(&bytes).map_err(|_| {
                    outcome_error("INVALID_RESPONSE", "The API returned invalid JSON.")
                })?)
            }
            ResponseKind::Text => ResponseBody::Text(String::from_utf8(bytes).map_err(|_| {
                outcome_error("INVALID_RESPONSE", "The API returned invalid text.")
            })?),
            ResponseKind::Binary => ResponseBody::Binary(bytes),
        };
        Ok(ApiResponse {
            status: status.as_u16(),
            body,
            content_type,
        })
    }
}
