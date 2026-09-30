use hermes_grocy_mcp::{
    model::{ProfileSelection, ProviderIdentity},
    setup::hermes,
};
use std::os::unix::fs::PermissionsExt;
fn selection() -> ProfileSelection {
    ProfileSelection {
        profile: "default".into(),
        provider: ProviderIdentity {
            bus_name: "org.kde.secretservicecompat".into(),
            executable: "/usr/bin/wallet".into(),
        },
    }
}
#[tokio::test]
async fn guided_setup_and_hermes_preservation() {
    let report = hermes_grocy_mcp::setup::DoctorReport {
        version: Some("4.7.1".into()),
        server_time: serde_json::json!("2030-01-01"),
        household_access: true,
        household_defaults: serde_json::json!({"locations":[{"id":1,"name":"x".repeat(16000),"description":"unnecessary".repeat(16000)}]}),
    };
    let defaults = hermes_grocy_mcp::setup::private_defaults(&report);
    assert!(!defaults.to_string().contains("unnecessary"));
    assert!(defaults.to_string().len() < 256 * 1024);
    let block = hermes::render_config(
        std::path::Path::new("/opt/bin/hermes-grocy-mcp"),
        &selection(),
    )
    .unwrap();
    let data: serde_json::Value = serde_json::from_str(&block).unwrap();
    assert_eq!(data["grocy"]["command"], "/opt/bin/hermes-grocy-mcp");
    assert!(!block.contains("api_key"));
    let temp = tempfile::tempdir().unwrap();
    let fake = temp.path().join("hermes");
    let state = temp.path().join("servers.json");
    std::fs::write(&state, "{\"other\":{\"command\":\"untouched\"}}").unwrap();
    let script = format!(
        "#!/usr/bin/env python3\nimport json,sys,pathlib\np=pathlib.Path({:?})\na=sys.argv[1:]\nif a==['config','path']: print(str(p))\nelif a==['config','get','mcp_servers','--json']:print(p.read_text())\nelif a[:3]==['config','set','mcp_servers.grocy']:\n d=json.loads(p.read_text());d['grocy']=json.loads(a[3]);p.write_text(json.dumps(d))\nelif a==['mcp','test','grocy']:pass\nelse:sys.exit(2)\n",
        state.to_string_lossy().to_string()
    );
    std::fs::write(&fake, script).unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o700)).unwrap();
    hermes::connect_using(
        &fake,
        std::path::Path::new("/opt/bin/hermes-grocy-mcp"),
        &selection(),
    )
    .await
    .unwrap();
    let data: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&state).unwrap()).unwrap();
    assert_eq!(data["other"]["command"], "untouched");
    assert_eq!(data["grocy"]["command"], "/opt/bin/hermes-grocy-mcp");
    let before = std::fs::read(&state).unwrap();
    let mut conflict = selection();
    conflict.profile = "other-profile".into();
    assert_eq!(
        hermes::connect_using(
            &fake,
            std::path::Path::new("/opt/bin/hermes-grocy-mcp"),
            &conflict
        )
        .await
        .err()
        .unwrap()
        .code,
        "HERMES_ENTRY_CONFLICT"
    );
    assert_eq!(std::fs::read(&state).unwrap(), before);
}

struct RetryStore {
    attempts: std::sync::atomic::AtomicUsize,
}
impl hermes_grocy_mcp::credentials::CredentialStore for RetryStore {
    fn load(
        &self,
        _s: ProfileSelection,
    ) -> futures_util::future::BoxFuture<
        'static,
        hermes_grocy_mcp::error::Result<hermes_grocy_mcp::model::PrivateProfile>,
    > {
        Box::pin(async {
            Err(hermes_grocy_mcp::error::AppError::new(
                "PROFILE_NOT_FOUND",
                "Missing",
            ))
        })
    }
    fn put(
        &self,
        _s: ProfileSelection,
        p: hermes_grocy_mcp::model::PrivateProfile,
        _c: hermes_grocy_mcp::credentials::EnrollmentConsent,
    ) -> futures_util::future::BoxFuture<'static, hermes_grocy_mcp::error::Result<()>> {
        use secrecy::ExposeSecret;
        assert_eq!(p.api_key.expose_secret(), "synthetic-key");
        let attempt = self
            .attempts
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Box::pin(async move {
            if attempt == 0 {
                Err(hermes_grocy_mcp::error::AppError::new(
                    "WALLET_LOCKED",
                    "Locked",
                ))
            } else {
                Ok(())
            }
        })
    }
    fn forget(
        &self,
        _s: ProfileSelection,
    ) -> futures_util::future::BoxFuture<'static, hermes_grocy_mcp::error::Result<()>> {
        Box::pin(async { Ok(()) })
    }
}
struct ScriptUi {
    url: String,
    inputs: usize,
    secrets: usize,
    final_confirm: bool,
}
impl hermes_grocy_mcp::setup::SetupUi for ScriptUi {
    fn message(&mut self, _s: &str) {}
    fn input(&mut self, _p: &str, _d: Option<&str>) -> hermes_grocy_mcp::error::Result<String> {
        self.inputs += 1;
        Ok(self.url.clone())
    }
    fn secret(
        &mut self,
        _p: &str,
        _empty: bool,
    ) -> hermes_grocy_mcp::error::Result<secrecy::SecretString> {
        self.secrets += 1;
        Ok(secrecy::SecretString::from("synthetic-key"))
    }
    fn choose(
        &mut self,
        _p: &str,
        _o: &[String],
        _d: usize,
    ) -> hermes_grocy_mcp::error::Result<usize> {
        Ok(0)
    }
    fn confirm(&mut self, p: &str, _d: bool) -> hermes_grocy_mcp::error::Result<bool> {
        Ok(p != "Save this profile?" || self.final_confirm)
    }
    fn providers(
        &mut self,
    ) -> hermes_grocy_mcp::error::Result<
        Vec<hermes_grocy_mcp::credentials::providers::ProviderCandidate>,
    > {
        Ok(vec![
            hermes_grocy_mcp::credentials::providers::ProviderCandidate {
                label: "Synthetic wallet".into(),
                identity: Some(selection().provider),
                recommended: true,
            },
        ])
    }
    fn protection(
        &mut self,
        _s: &ProfileSelection,
    ) -> hermes_grocy_mcp::credentials::protection::ProtectionEvidence {
        hermes_grocy_mcp::credentials::protection::ProtectionEvidence::Protected
    }
}
#[tokio::test]
async fn wallet_retry_preserves_setup_answers() {
    use hermes_grocy_mcp::{
        credentials::preferences::PreferenceStore, model::TransportPolicy, setup,
    };
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let app = axum::Router::new().fallback(|req: axum::extract::Request| async move {
        assert!(req.uri().path().starts_with("/api/"));
        axum::Json(if req.uri().path().ends_with("info") {
            serde_json::json!({"grocy_version":{"Version":"4.7.1"}})
        } else {
            serde_json::json!([])
        })
    });
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    let server = tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    for approve in [false, true] {
        let d = tempfile::tempdir().unwrap();
        let store = Arc::new(RetryStore {
            attempts: AtomicUsize::new(0),
        });
        let mut ui = ScriptUi {
            url: format!("http://127.0.0.1:{port}"),
            inputs: 0,
            secrets: 0,
            final_confirm: approve,
        };
        let opts = setup::SetupOptions {
            profile: "default".into(),
            preferences: PreferenceStore {
                directory: d.path().join("prefs"),
            },
            ca_path: None,
            transport: TransportPolicy::LoopbackDevelopment,
            access: None,
            connect_hermes: false,
        };
        let result = setup::run(opts, &mut ui, store.clone()).await;
        assert_eq!(ui.inputs, 1);
        assert_eq!(ui.secrets, 1);
        if approve {
            assert!(result.is_ok());
            assert_eq!(store.attempts.load(Ordering::SeqCst), 2);
            assert!(d.path().join("prefs/default.json").exists());
        } else {
            assert_eq!(result.err().unwrap().code, "CANCELLED");
            assert_eq!(store.attempts.load(Ordering::SeqCst), 0);
            assert!(!d.path().join("prefs/default.json").exists());
        }
    }
    server.abort();
}
