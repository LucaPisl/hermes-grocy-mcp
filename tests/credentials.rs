use hermes_grocy_mcp::{
    credentials::{
        CredentialStore, DesktopStore,
        preferences::PreferenceStore,
        protection::{ProtectionEvidence, inspect_header},
    },
    model::{ProfileSelection, ProviderIdentity},
};
use std::{
    io::{BufRead, BufReader},
    os::unix::fs::PermissionsExt,
    process::{Command, Stdio},
};
#[test]
fn protected_preferences() {
    let dir = tempfile::tempdir().unwrap();
    let selection = ProfileSelection {
        profile: "default".into(),
        provider: ProviderIdentity {
            bus_name: "org.freedesktop.secrets".into(),
            executable: "/usr/bin/fixture-wallet".into(),
        },
    };
    let prefs = PreferenceStore {
        directory: dir.path().join("prefs"),
    };
    prefs.save(&selection).unwrap();
    assert_eq!(prefs.load("default").unwrap(), selection);
    assert_eq!(
        std::fs::metadata(prefs.directory.join("default.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    let header = dir.path().join("header");
    std::fs::write(&header, b"[keyring]\nplaintext").unwrap();
    assert_eq!(inspect_header(&header), ProtectionEvidence::Unprotected);
    std::fs::write(&header, b"GnomeKeyring\n\r\0\n\0\0\0\0").unwrap();
    assert_eq!(inspect_header(&header), ProtectionEvidence::Protected);
    std::fs::write(&header, b"KWALLET\n\r\0\r\n").unwrap();
    assert_eq!(inspect_header(&header), ProtectionEvidence::Unverified);
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&header, &link).unwrap();
    assert_eq!(inspect_header(&link), ProtectionEvidence::Unverified);
    assert!(prefs.load("../escape").is_err());
    assert_eq!(
        std::fs::metadata(&prefs.directory)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    std::os::unix::fs::symlink(
        prefs.directory.join("default.json"),
        prefs.directory.join("linked.json"),
    )
    .unwrap();
    assert_eq!(prefs.load("linked").unwrap_err().code, "UNSAFE_PREFERENCES");
    std::fs::set_permissions(
        prefs.directory.join("default.json"),
        std::fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    assert_eq!(
        prefs.load("default").unwrap_err().code,
        "UNSAFE_PREFERENCES"
    );
}
struct PlainService;
#[zbus::interface(name = "org.freedesktop.Secret.Service")]
impl PlainService {
    fn open_session(
        &self,
        algorithm: &str,
        _input: zbus::zvariant::OwnedValue,
    ) -> zbus::fdo::Result<(zbus::zvariant::OwnedValue, zbus::zvariant::OwnedObjectPath)> {
        if algorithm != "plain" {
            return Err(zbus::fdo::Error::NotSupported("Only plain".into()));
        }
        Ok((
            zbus::zvariant::Value::from("").try_into().unwrap(),
            "/org/freedesktop/secrets/session/plain".try_into().unwrap(),
        ))
    }
    #[zbus(property)]
    fn collections(&self) -> Vec<zbus::zvariant::OwnedObjectPath> {
        vec![]
    }
}
#[test]
fn selected_wallet_only() {
    if std::env::var_os("GROCY_WALLET_TEST_CHILD").is_some() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let _conn = zbus::connection::Builder::address(
                std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap().as_str(),
            )
            .unwrap()
            .name("org.freedesktop.secrets")
            .unwrap()
            .serve_at("/org/freedesktop/secrets", PlainService)
            .unwrap()
            .build()
            .await
            .unwrap();
            let selection = ProfileSelection {
                profile: "default".into(),
                provider: ProviderIdentity {
                    bus_name: "org.freedesktop.secrets".into(),
                    executable: std::fs::read_link("/proc/self/exe").unwrap(),
                },
            };
            assert_eq!(
                DesktopStore
                    .load(selection.clone())
                    .await
                    .err()
                    .unwrap()
                    .code,
                "UNENCRYPTED_SECRET_SESSION"
            );
            let mut wrong = selection;
            wrong.provider.executable = "/not-the-selected-provider".into();
            assert_eq!(
                DesktopStore.load(wrong).await.err().unwrap().code,
                "WALLET_PROVIDER_CHANGED"
            );
        });
        return;
    }
    let mut daemon = Command::new("dbus-daemon")
        .args(["--session", "--nofork", "--print-address"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut address = String::new();
    BufReader::new(daemon.stdout.take().unwrap())
        .read_line(&mut address)
        .unwrap();
    let result = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "selected_wallet_only",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("GROCY_WALLET_TEST_CHILD", "1")
        .env("DBUS_SESSION_BUS_ADDRESS", address.trim())
        .output()
        .unwrap();
    let _ = daemon.kill();
    let _ = daemon.wait();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stdout)
    );
}
