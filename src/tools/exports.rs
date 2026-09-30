use crate::{
    catalog::ValidatedCall,
    client::{ApiClient, ApiRequest, ResponseBody, ResponseKind},
    error::{AppError, Result},
    model::ToolOutput,
};
use serde_json::{Value, json};
pub async fn run(client: &ApiClient, call: &ValidatedCall) -> Result<ToolOutput> {
    let mut r = ApiRequest::get(call.op.route);
    if call.op.name == "calendar_export" {
        r.kind = ResponseKind::Text;
        let response = client.request(r).await?;
        let ResponseBody::Text(content) = response.body else {
            return Err(AppError::input());
        };
        if !content.trim_start().starts_with("BEGIN:VCALENDAR") {
            return Err(AppError::new(
                "INVALID_RESPONSE",
                "Expected iCalendar data from the API.",
            ));
        }
        return Ok(ToolOutput::data(
            json!({"mime":"text/calendar","content":content}),
        ));
    }
    if call.op.name == "shopping_export" {
        r.params.insert("entity".into(), "shopping_list".into());
        let rows = client.request(r).await?.json()?;
        let id = call.args["list_id"].as_u64().ok_or_else(AppError::input)?;
        let items = rows
            .as_array()
            .ok_or_else(AppError::input)?
            .iter()
            .filter(|r| {
                super::validation::record_id(&r["shopping_list_id"]).is_ok_and(|n| n.0 == id)
            })
            .cloned()
            .collect::<Vec<_>>();
        return Ok(ToolOutput::data(
            json!({"shopping_list_id":id,"items":items}),
        ));
    }
    let rows = client.request(r).await?.json()?;
    let users=rows.as_array().ok_or_else(AppError::input)?.iter().map(|u|json!({"id":u["id"],"name":u.get("display_name").or_else(||u.get("username")).unwrap_or(&Value::Null)})).collect::<Vec<_>>();
    Ok(ToolOutput::data(json!({"users":users})))
}
