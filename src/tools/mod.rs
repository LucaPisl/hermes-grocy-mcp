mod actions;
pub mod exports;
pub mod files;
pub mod records;
pub mod validation;
use crate::{catalog::ValidatedCall, client::ApiClient, error::Result, model::ToolOutput};
pub async fn execute(client: &ApiClient, call: ValidatedCall) -> Result<ToolOutput> {
    if call.op.name.starts_with("records_") || call.op.name.starts_with("userfields_") {
        records::run(client, &call).await
    } else if call.op.name.starts_with("file_") {
        files::run(client, &call).await
    } else if ["calendar_export", "shopping_export", "assignment_users"].contains(&call.op.name) {
        exports::run(client, &call).await
    } else {
        actions::run(client, &call).await
    }
}
