use crate::{
    client::ApiBase,
    error::{AppError, Result},
    model::{AccessMode, PrivateProfile, ProfileSelection, TransportPolicy},
};
use gio::prelude::*;
use libsecret::prelude::*;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashMap, sync::mpsc, time::Duration};
use zeroize::Zeroizing;
pub const APP_ID: &str = "io.github.hermes-grocy-mcp";
fn wallet_error() -> AppError {
    AppError::new(
        "WALLET_UNAVAILABLE",
        "The selected wallet is unavailable or did not finish. Open its native manager and retry; no fallback was used.",
    )
}
pub(crate) struct WalletSession {
    pub collection: libsecret::Collection,
    pub cancel: gio::Cancellable,
    done: mpsc::Sender<()>,
}
impl WalletSession {
    pub(crate) fn open(owner: &str) -> Result<Self> {
        let cancel = gio::Cancellable::new();
        let timer = cancel.clone();
        let (done, rx) = mpsc::channel();
        std::thread::spawn(move || {
            if matches!(
                rx.recv_timeout(Duration::from_secs(30)),
                Err(mpsc::RecvTimeoutError::Timeout)
            ) {
                timer.cancel();
            }
        });
        let service = libsecret::Service::open_sync(
            libsecret::Service::static_type(),
            Some(owner),
            libsecret::ServiceFlags::OPEN_SESSION,
            Some(&cancel),
        )
        .map_err(|_| wallet_error())?;
        if service.session_algorithms().as_deref() != Some("dh-ietf1024-sha256-aes128-cbc-pkcs7") {
            return Err(AppError::new(
                "UNENCRYPTED_SECRET_SESSION",
                "This wallet cannot provide encrypted communication. Choose a compatible desktop wallet; plaintext communication is refused.",
            ));
        }
        let collection = libsecret::Collection::for_alias_sync(
            Some(&service),
            "default",
            libsecret::CollectionFlags::NONE,
            Some(&cancel),
        )
        .map_err(|_| wallet_error())?
        .ok_or_else(|| {
            AppError::new(
                "NO_DEFAULT_WALLET",
                "Create a protected default wallet using the native manager and retry.",
            )
        })?;
        if collection.is_locked() {
            return Err(AppError::new(
                "WALLET_LOCKED",
                "Unlock your default wallet in its native manager, then retry.",
            ));
        }
        Ok(Self {
            collection,
            cancel,
            done,
        })
    }
    fn items(&self, name: &str) -> Result<Vec<libsecret::Item>> {
        self.collection
            .search_sync(
                None,
                attributes(name),
                libsecret::SearchFlags::ALL,
                Some(&self.cancel),
            )
            .map_err(|_| wallet_error())
    }
}
impl Drop for WalletSession {
    fn drop(&mut self) {
        let _ = self.done.send(());
    }
}
fn attributes(name: &str) -> HashMap<&str, &str> {
    HashMap::from([("application", APP_ID), ("profile", name)])
}
#[derive(Serialize)]
struct ProfileRef<'a> {
    version: u8,
    api_base: &'a str,
    api_key: &'a str,
    access: AccessMode,
    timezone: &'a Option<String>,
    defaults: &'a Value,
    ca_path: &'a Option<std::path::PathBuf>,
    transport: TransportPolicy,
    unsafe_storage: bool,
}
#[derive(Deserialize)]
struct ProfileWire {
    version: u8,
    api_base: String,
    api_key: String,
    access: AccessMode,
    timezone: Option<String>,
    defaults: Value,
    ca_path: Option<std::path::PathBuf>,
    transport: TransportPolicy,
    unsafe_storage: bool,
}
fn encode(p: &PrivateProfile) -> Result<Zeroizing<Vec<u8>>> {
    serde_json::to_vec(&ProfileRef {
        version: 1,
        api_base: p.api_base.as_str(),
        api_key: p.api_key.expose_secret(),
        access: p.access,
        timezone: &p.timezone,
        defaults: &p.defaults,
        ca_path: &p.ca_path,
        transport: p.transport,
        unsafe_storage: p.unsafe_storage,
    })
    .map(Zeroizing::new)
    .map_err(|_| AppError::input())
}
fn decode(bytes: &[u8]) -> Result<PrivateProfile> {
    if bytes.len() > 256 * 1024 {
        return Err(AppError::new(
            "INVALID_PROFILE",
            "The wallet profile is invalid. Run setup again.",
        ));
    }
    let mut w: ProfileWire = serde_json::from_slice(bytes).map_err(|_| {
        AppError::new(
            "INVALID_PROFILE",
            "The wallet profile is invalid. Run setup again.",
        )
    })?;
    let key = SecretString::from(std::mem::take(&mut w.api_key));
    if w.version != 1 {
        return Err(AppError::new(
            "INVALID_PROFILE",
            "Run setup to refresh this profile.",
        ));
    }
    Ok(PrivateProfile {
        api_base: ApiBase::parse(&w.api_base, w.transport)?,
        api_key: key,
        access: w.access,
        timezone: w.timezone,
        defaults: w.defaults,
        ca_path: w.ca_path,
        transport: w.transport,
        unsafe_storage: w.unsafe_storage,
    })
}
pub(crate) fn load(s: ProfileSelection) -> Result<PrivateProfile> {
    super::preferences::validate_profile(&s.profile)?;
    let owner = super::providers::bind_selected(&s.provider)?;
    let session = WalletSession::open(&owner)?;
    let items = session.items(&s.profile)?;
    if items.len() != 1 {
        return Err(AppError::new(
            "PROFILE_NOT_FOUND",
            "The wallet profile is missing or ambiguous. Run setup to review it.",
        ));
    }
    let item = &items[0];
    if item.is_locked() {
        return Err(AppError::new(
            "WALLET_LOCKED",
            "Unlock the wallet item in its native manager and retry.",
        ));
    }
    item.load_secret_sync(Some(&session.cancel))
        .map_err(|_| wallet_error())?;
    let secret = item.secret().ok_or_else(wallet_error)?;
    let bytes = Zeroizing::new(secret.get());
    decode(&bytes)
}
pub(crate) fn put(s: ProfileSelection, p: PrivateProfile) -> Result<()> {
    super::preferences::validate_profile(&s.profile)?;
    let owner = super::providers::bind_selected(&s.provider)?;
    let session = WalletSession::open(&owner)?;
    let bytes = encode(&p)?;
    if bytes.len() > 256 * 1024 {
        return Err(AppError::input());
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| AppError::input())?;
    let value = libsecret::Value::new(text, "application/json");
    libsecret::Item::create_sync(
        &session.collection,
        None,
        attributes(&s.profile),
        &format!("Grocy MCP ({})", s.profile),
        &value,
        libsecret::ItemCreateFlags::REPLACE,
        Some(&session.cancel),
    )
    .map_err(|_| wallet_error())?;
    Ok(())
}
pub(crate) fn forget(s: ProfileSelection) -> Result<()> {
    super::preferences::validate_profile(&s.profile)?;
    let owner = super::providers::bind_selected(&s.provider)?;
    let session = WalletSession::open(&owner)?;
    for item in session.items(&s.profile)? {
        if item.is_locked() {
            return Err(AppError::new(
                "WALLET_LOCKED",
                "Unlock the wallet item and retry.",
            ));
        }
        item.delete_sync(Some(&session.cancel))
            .map_err(|_| wallet_error())?;
    }
    Ok(())
}
