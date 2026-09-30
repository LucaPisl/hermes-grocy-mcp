use serde::Serialize;
use serde_json::{Value, json};
#[derive(Debug, Clone, Serialize)]
pub struct AppError {
    pub code: &'static str,
    pub message: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery: Option<Value>,
}
pub type Result<T> = std::result::Result<T, AppError>;
impl AppError {
    pub fn new(code: &'static str, message: &'static str) -> Self {
        Self {
            code,
            message,
            recovery: None,
        }
    }
    pub fn input() -> Self {
        Self::new(
            "INVALID_INPUT",
            "Check the tool schema and required fields.",
        )
    }
    pub fn unknown(target: Value) -> Self {
        Self {
            code: "UNKNOWN_WRITE_OUTCOME",
            message: "The write may have completed. Inspect current state before attempting another write; do not automatically repeat it.",
            recovery: Some(json!({"target":target})),
        }
    }
}
impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for AppError {}
