use hermes_grocy_mcp::{
    client::ApiBase,
    credentials::{
        CredentialStore, DesktopStore, EnrollmentConsent, providers::discover_providers,
    },
    model::{AccessMode, PrivateProfile, ProfileSelection, TransportPolicy},
};
use secrecy::{ExposeSecret, SecretString};
use std::{
    io::Write,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct WalletDaemon(Child);
impl Drop for WalletDaemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn profile(key: &str) -> PrivateProfile {
    PrivateProfile {
        api_base: ApiBase::parse("https://grocy.example/api", TransportPolicy::HttpsOnly).unwrap(),
        api_key: SecretString::from(key.to_owned()),
        access: AccessMode::Full,
        timezone: Some("Etc/UTC".into()),
        defaults: serde_json::json!({}),
        ca_path: None,
        transport: TransportPolicy::HttpsOnly,
        unsafe_storage: false,
    }
}

#[test]
fn encrypted_wallet_roundtrip() {
    if std::env::var_os("GROCY_ENCRYPTED_WALLET_TEST_CHILD").is_none() {
        // A private bus and data directory ensure this cannot reach the user's wallet.
        let dir = tempfile::tempdir().unwrap();
        let result = Command::new("dbus-run-session")
            .arg("--")
            .arg(std::env::current_exe().unwrap())
            .args(["--exact", "encrypted_wallet_roundtrip", "--nocapture"])
            .env("GROCY_ENCRYPTED_WALLET_TEST_CHILD", "1")
            .env("XDG_DATA_HOME", dir.path())
            .env("XDG_RUNTIME_DIR", dir.path())
            .env("G_DEBUG", "fatal-criticals")
            .output()
            .expect("Install dbus and gnome-keyring to run the isolated wallet test");
        assert!(
            result.status.success(),
            "Isolated wallet roundtrip failed:\n{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        return;
    }

    let mut daemon = WalletDaemon(
        Command::new("gnome-keyring-daemon")
            .args(["--foreground", "--unlock", "--components=secrets"])
            .arg("--control-directory")
            .arg(std::env::var_os("XDG_RUNTIME_DIR").unwrap())
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("Install gnome-keyring to run the isolated wallet test"),
    );
    daemon
        .0
        .stdin
        .take()
        .unwrap()
        .write_all(b"synthetic-isolated-wallet-password")
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let provider = loop {
        if let Ok(providers) = discover_providers()
            && let Some(identity) = providers.into_iter().find_map(|p| p.identity)
        {
            break identity;
        }
        assert!(Instant::now() < deadline, "Isolated wallet did not start");
        std::thread::sleep(Duration::from_millis(50));
    };
    let selection = ProfileSelection {
        profile: "synthetic-roundtrip".into(),
        provider,
    };
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        assert!(matches!(
            DesktopStore.load(selection.clone()).await,
            Err(e) if e.code == "PROFILE_NOT_FOUND"
        ));
        for key in ["synthetic-first-key", "synthetic-replacement-key"] {
            DesktopStore
                .put(
                    selection.clone(),
                    profile(key),
                    EnrollmentConsent::ProtectedByUser,
                )
                .await
                .unwrap();
            let loaded = DesktopStore.load(selection.clone()).await.unwrap();
            assert!(loaded.api_key.expose_secret() == key);
            assert!(loaded.api_base.as_str() == profile(key).api_base.as_str());
            assert_eq!(loaded.access, AccessMode::Full);
            assert!(!loaded.unsafe_storage);
        }
        DesktopStore.forget(selection.clone()).await.unwrap();
        assert!(matches!(
            DesktopStore.load(selection).await,
            Err(e) if e.code == "PROFILE_NOT_FOUND"
        ));
    });
}
