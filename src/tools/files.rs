use crate::{
    catalog::OperationSpec,
    client::{
        ApiClient, ApiRequest, BINARY_LIMIT, HttpMethod, RequestBody, ResponseBody, ResponseKind,
    },
    error::{AppError, Result},
    model::{AccessMode, Attachment, ToolOutput, Verification, WriteReport, WriteStatus},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
pub fn validate_filename(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 255
        || matches!(name, "." | "..")
        || name.contains(['/', '\\', '%'])
        || name.chars().any(char::is_control)
    {
        return Err(AppError::input());
    }
    Ok(())
}
pub fn operations(mode: AccessMode) -> Vec<OperationSpec> {
    let mut ops = vec![];
    for (name, method, route) in [
        ("file_get", HttpMethod::Get, "/files/{group}/{fileName}"),
        ("file_upload", HttpMethod::Put, "/files/{group}/{fileName}"),
        (
            "file_delete",
            HttpMethod::Delete,
            "/files/{group}/{fileName}",
        ),
        ("calendar_export", HttpMethod::Get, "/calendar/ical"),
        ("shopping_export", HttpMethod::Get, "/objects/{entity}"),
        ("assignment_users", HttpMethod::Get, "/users"),
    ] {
        if method.mutates() && mode == AccessMode::ReadOnly {
            continue;
        }
        let mut properties = json!({});
        let mut required = vec![];
        if name.starts_with("file_") {
            properties = json!({"group":{"type":"string","enum":["equipmentmanuals","recipepictures","productpictures","userfiles"]},"filename":{"type":"string","maxLength":255}});
            required.extend(["group", "filename"]);
            if name == "file_upload" {
                properties["content_base64"] = json!({"type":"string","maxLength":BINARY_LIMIT.div_ceil(3)*4,"description":"Standard base64 of actual file bytes, at most 8 MiB decoded. No local path or external URL."});
                required.push("content_base64")
            }
        }
        if name == "shopping_export" {
            properties["list_id"] = json!({"type":"integer","minimum":1});
            required.push("list_id")
        }
        let description = match name {
            "file_get" => {
                "Read a known API file by group and filename. Returns real bytes as an image or embedded resource, at most 8 MiB. Does not fetch URLs or host files. Grocy has no orphan-file listing API."
            }
            "file_upload" => {
                "Upload bounded base64 content as binary through the API, replacing this exact named household file. Returns a write receipt and readback verification."
            }
            "file_delete" => {
                "Permanently delete the exact API file. Specify the group and filename; no host filesystem access."
            }
            "calendar_export" => {
                "Export household dates as iCalendar data through the API; no public sharing link."
            }
            "shopping_export" => {
                "Export actual shopping-list items for an explicit list ID as JSON; no printer action."
            }
            _ => {
                "List assignment user IDs and display names only. No passwords, keys or account fields."
            }
        };
        ops.push(OperationSpec{name,route,method,schema:json!({"type":"object","properties":properties,"required":required,"additionalProperties":false}),mutates:method.mutates(),destructive:method.mutates(),description:description.into(),path_params:Default::default(),body_keys:vec![]});
    }
    ops
}
fn file_request(args: &Value, method: HttpMethod) -> Result<ApiRequest> {
    let filename = args["filename"].as_str().ok_or_else(AppError::input)?;
    validate_filename(filename)?;
    let mut r = ApiRequest::get("/files/{group}/{fileName}");
    r.params.insert(
        "group".into(),
        args["group"].as_str().ok_or_else(AppError::input)?.into(),
    );
    r.params
        .insert("fileName".into(), STANDARD.encode(filename));
    r.method = method;
    r.kind = ResponseKind::Binary;
    Ok(r)
}
pub async fn run(client: &ApiClient, call: &crate::catalog::ValidatedCall) -> Result<ToolOutput> {
    let mut r = file_request(&call.args, call.op.method)?;
    let target = json!({"group":call.args["group"],"filename":call.args["filename"]});
    let mut expected = None;
    if call.op.method == HttpMethod::Put {
        let encoded = call.args["content_base64"]
            .as_str()
            .ok_or_else(AppError::input)?;
        if encoded.len() > BINARY_LIMIT.div_ceil(3) * 4 {
            return Err(AppError::input());
        }
        let bytes = STANDARD.decode(encoded).map_err(|_| AppError::input())?;
        if bytes.len() > BINARY_LIMIT {
            return Err(AppError::input());
        }
        expected = Some(bytes.clone());
        r.body = RequestBody::Binary(bytes);
    }
    let response = client.request(r).await?;
    if call.op.method == HttpMethod::Get {
        let ResponseBody::Binary(bytes) = response.body else {
            return Err(AppError::input());
        };
        let mime = validated_mime(&bytes, &response.content_type);
        return Ok(ToolOutput {
            data: json!({"file":target,"bytes":bytes.len(),"mime":mime}),
            write: None,
            attachment: Some(Attachment {
                bytes,
                mime,
                name: call.args["filename"].as_str().unwrap().into(),
            }),
        });
    }
    let check = client
        .request(file_request(&call.args, HttpMethod::Get)?)
        .await;
    let verification = match (call.op.method, check) {
        (HttpMethod::Delete, Err(e)) if e.code == "NOT_FOUND" => Verification::Confirmed,
        (HttpMethod::Put, Ok(r)) => match r.body {
            ResponseBody::Binary(b) if Some(&b) == expected.as_ref() => Verification::Confirmed,
            _ => Verification::Incomplete,
        },
        _ => Verification::Incomplete,
    };
    Ok(ToolOutput {
        data: json!({"file":target,"bytes":expected.as_ref().map(Vec::len)}),
        write: Some(WriteReport {
            status: WriteStatus::Completed,
            verification,
            target,
            receipt: json!({"http_status":response.status}),
        }),
        attachment: None,
    })
}
fn validated_mime(bytes: &[u8], reported: &str) -> String {
    let image = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[255, 216, 255]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some("image/webp")
    } else {
        None
    };
    if let Some(mime) = image {
        return mime.into();
    }
    if reported == "application/pdf" && bytes.starts_with(b"%PDF-") {
        "application/pdf".into()
    } else {
        "application/octet-stream".into()
    }
}
