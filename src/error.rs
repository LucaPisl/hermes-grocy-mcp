use serde::Serialize;
use serde_json::{Value, json};
use std::borrow::Cow;
#[derive(Debug, Clone, Serialize)]
pub struct AppError {
    pub code: &'static str,
    pub message: Cow<'static, str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery: Option<Value>,
}
pub type Result<T> = std::result::Result<T, AppError>;
impl AppError {
    pub fn new(code: &'static str, message: impl Into<Cow<'static, str>>) -> Self {
        Self {
            code,
            message: message.into(),
            recovery: None,
        }
    }
    pub fn input() -> Self {
        Self::new(
            "INVALID_INPUT",
            "Check the tool schema and required fields.",
        )
    }
    /// Invalid input for one argument: `field: problem` so agents can correct exactly that value.
    pub fn field(path: &str, problem: impl std::fmt::Display) -> Self {
        Self::new("INVALID_INPUT", format!("{path}: {problem}"))
    }
    pub fn unknown(target: Value) -> Self {
        Self {
            code: "UNKNOWN_WRITE_OUTCOME",
            message: "The write may have completed. Inspect current state before attempting another write; do not automatically repeat it.".into(),
            recovery: Some(json!({"target":target})),
        }
    }
    /// Attach machine-readable recovery data (candidates, hints, upstream messages).
    pub fn with(mut self, key: &str, value: Value) -> Self {
        let recovery = self.recovery.get_or_insert_with(|| json!({}));
        recovery[key] = value;
        self
    }
}
impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for AppError {}
