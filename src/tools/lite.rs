use crate::{catalog::OperationSpec, model::AccessMode};
pub const NAMES: [&str; 0] = [];
pub fn operations(_mode: AccessMode) -> Vec<OperationSpec> {
    vec![]
}
pub fn undo_for(
    _name: &str,
    _args: &serde_json::Value,
    _receipt: &serde_json::Value,
) -> Option<serde_json::Value> {
    None
}
