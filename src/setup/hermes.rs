use crate::{
    error::{AppError, Result},
    model::ProfileSelection,
};
use serde_json::{Value, json};
use std::{path::Path, process::Stdio, time::Duration};
use tokio::io::AsyncReadExt;
fn entry(exe: &Path, s: &ProfileSelection) -> Result<Value> {
    crate::credentials::preferences::validate_profile(&s.profile)?;
    if !exe.is_absolute() {
        return Err(AppError::input());
    }
    Ok(
        json!({"command":exe,"args":["serve","--profile",s.profile,"--provider",s.provider.bus_name],"timeout":120}),
    )
}
pub fn render_config(exe: &Path, s: &ProfileSelection) -> Result<String> {
    serde_json::to_string_pretty(&json!({"grocy":entry(exe,s)?})).map_err(|_| AppError::input())
}
async fn run(hermes: &Path, args: &[String]) -> Result<std::process::Output> {
    let mut child = tokio::process::Command::new(hermes)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| {
            AppError::new(
                "HERMES_UNAVAILABLE",
                "Hermes is unavailable. Install it or use print-config.",
            )
        })?;
    let stdout = child.stdout.take().ok_or_else(AppError::input)?;
    let stderr = child.stderr.take().ok_or_else(AppError::input)?;
    let wait = async {
        let (status, stdout, stderr) =
            tokio::try_join!(child.wait(), bounded_output(stdout), bounded_output(stderr))?;
        Ok::<_, std::io::Error>(std::process::Output {
            status,
            stdout,
            stderr,
        })
    };
    let output = tokio::time::timeout(Duration::from_secs(45), wait)
        .await
        .map_err(|_| {
            AppError::new(
                "HERMES_TIMEOUT",
                "Hermes did not finish. Retry connect-hermes; the wallet profile is still saved.",
            )
        })?
        .map_err(|_| AppError::new("HERMES_UNAVAILABLE", "Hermes did not finish this command."))?;
    if output.stdout.len() > 1024 * 1024 || output.stderr.len() > 1024 * 1024 {
        return Err(AppError::new(
            "HERMES_CONFIG_UNSUPPORTED",
            "Hermes output is too large. Use print-config.",
        ));
    }
    Ok(output)
}
async fn bounded_output<R: tokio::io::AsyncRead + Unpin>(reader: R) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(1024 * 1024 + 1).read_to_end(&mut bytes).await?;
    Ok(bytes)
}
async fn config_path(hermes: &Path) -> Result<std::path::PathBuf> {
    let o = run(hermes, &["config".into(), "path".into()]).await?;
    let text = std::str::from_utf8(&o.stdout)
        .map_err(|_| AppError::input())?
        .trim();
    let p = std::path::PathBuf::from(text);
    if !o.status.success() || text.contains('\n') || !p.is_absolute() {
        return Err(AppError::new(
            "HERMES_CONFIG_UNSUPPORTED",
            "Hermes cannot report its configuration path. Update it or use print-config.",
        ));
    }
    if let Ok(m) = std::fs::symlink_metadata(&p)
        && !m.is_file()
    {
        return Err(AppError::new(
            "HERMES_CONFIG_UNSUPPORTED",
            "Hermes configuration must be a regular file. Review it manually.",
        ));
    }
    Ok(p)
}
async fn servers(hermes: &Path) -> Result<Value> {
    let o = run(
        hermes,
        &[
            "config".into(),
            "get".into(),
            "mcp_servers".into(),
            "--json".into(),
        ],
    )
    .await?;
    if !o.status.success() {
        if String::from_utf8_lossy(&o.stdout).contains("Config key not set: mcp_servers")
            || String::from_utf8_lossy(&o.stderr).contains("Config key not set: mcp_servers")
        {
            return Ok(json!({}));
        }
        return Err(AppError::new(
            "HERMES_CONFIG_UNSUPPORTED",
            "Cannot read Hermes MCP configuration. Update Hermes or use print-config.",
        ));
    }
    let v: Value = serde_json::from_slice(&o.stdout).map_err(|_| {
        AppError::new(
            "HERMES_CONFIG_UNSUPPORTED",
            "Hermes MCP configuration is not supported. Use print-config.",
        )
    })?;
    if !v.is_object() {
        return Err(AppError::input());
    }
    Ok(v)
}
pub async fn connect_using(hermes: &Path, exe: &Path, s: &ProfileSelection) -> Result<()> {
    let path = config_path(hermes).await?;
    let expected = entry(exe, s)?;
    let before = servers(hermes).await?;
    if let Some(existing) = before.get("grocy") {
        if existing != &expected {
            return Err(AppError::new(
                "HERMES_ENTRY_CONFLICT",
                "Hermes already has a different grocy entry. It was left untouched. Review it or use another Hermes profile.",
            ));
        }
    } else {
        if config_path(hermes).await? != path || servers(hermes).await? != before {
            return Err(AppError::new(
                "HERMES_CONFIG_CHANGED",
                "Hermes configuration changed during setup. Retry after reviewing the active profile.",
            ));
        }
        let o = run(
            hermes,
            &[
                "config".into(),
                "set".into(),
                "mcp_servers.grocy".into(),
                expected.to_string(),
            ],
        )
        .await?;
        if !o.status.success() {
            return Err(AppError::new(
                "HERMES_WRITE_FAILED",
                "Hermes could not save its entry. The wallet profile remains available; retry connect-hermes.",
            ));
        }
    }
    if config_path(hermes).await? != path || servers(hermes).await?.get("grocy") != Some(&expected)
    {
        return Err(AppError::new(
            "HERMES_WRITE_UNCONFIRMED",
            "The configuration write could not be confirmed. Review it before repeating setup.",
        ));
    }
    let o = run(hermes, &["mcp".into(), "test".into(), "grocy".into()]).await?;
    if !o.status.success() {
        return Err(AppError::new(
            "HERMES_DISCOVERY_FAILED",
            "The entry is saved but tool discovery failed. Open your wallet and retry connect-hermes; no need to enroll again.",
        ));
    }
    Ok(())
}
pub async fn connect(exe: &Path, s: &ProfileSelection) -> Result<()> {
    connect_using(Path::new("hermes"), exe, s).await
}
