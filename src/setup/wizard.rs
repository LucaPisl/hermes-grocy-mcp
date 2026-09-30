use crate::{
    client::{ApiBase, ApiClient},
    credentials::{
        CredentialStore, EnrollmentConsent, preferences::PreferenceStore,
        protection::ProtectionEvidence, providers::ProviderCandidate,
    },
    error::{AppError, Result},
    model::{AccessMode, PrivateProfile, ProfileSelection, TransportPolicy},
};
use secrecy::{ExposeSecret, SecretString};
use std::{path::PathBuf, sync::Arc};
pub struct SetupOptions {
    pub profile: String,
    pub preferences: PreferenceStore,
    pub ca_path: Option<PathBuf>,
    pub transport: TransportPolicy,
    pub access: Option<AccessMode>,
    pub connect_hermes: bool,
}
pub trait SetupUi {
    fn message(&mut self, text: &str);
    fn input(&mut self, prompt: &str, default: Option<&str>) -> Result<String>;
    fn secret(&mut self, prompt: &str, allow_empty: bool) -> Result<SecretString>;
    fn choose(&mut self, prompt: &str, options: &[String], default: usize) -> Result<usize>;
    fn confirm(&mut self, prompt: &str, default: bool) -> Result<bool>;
    fn providers(&mut self) -> Result<Vec<ProviderCandidate>>;
    fn protection(&mut self, s: &ProfileSelection) -> ProtectionEvidence;
}
pub struct SetupOutcome {
    pub selection: ProfileSelection,
    pub connected: bool,
}
fn cancelled() -> AppError {
    AppError::new("CANCELLED", "Setup cancelled before saving a new profile.")
}
pub async fn run(
    options: SetupOptions,
    ui: &mut dyn SetupUi,
    store: Arc<dyn CredentialStore>,
) -> Result<SetupOutcome> {
    crate::credentials::preferences::validate_profile(&options.profile)?;
    ui.message(super::help::ACCOUNT_HELP);
    let previous = match options.preferences.load(&options.profile) {
        Ok(s) => Some(s),
        Err(e) if e.code == "PROFILE_NOT_FOUND" => None,
        Err(e) => return Err(e),
    };
    let selection = loop {
        let candidates = ui.providers()?;
        let default = candidates
            .iter()
            .position(|p| {
                previous
                    .as_ref()
                    .is_some_and(|s| p.identity.as_ref() == Some(&s.provider))
            })
            .or_else(|| candidates.iter().position(|p| p.recommended))
            .unwrap_or(0);
        if candidates.is_empty() {
            ui.message(super::help::WALLET_HELP);
            if ui.confirm(
                "No wallet is running. Open its manager, then check again?",
                true,
            )? {
                continue;
            } else {
                return Err(cancelled());
            }
        }
        let index = if candidates.len() == 1 && candidates[0].identity.is_some() {
            0
        } else {
            ui.choose(
                "Choose the wallet",
                &candidates
                    .iter()
                    .map(|p| {
                        format!(
                            "{}{}",
                            p.label,
                            if p.recommended {
                                " (recommended for this desktop)"
                            } else {
                                ""
                            }
                        )
                    })
                    .collect::<Vec<_>>(),
                default,
            )?
        };
        let Some(provider) = candidates.get(index).and_then(|p| p.identity.clone()) else {
            ui.message(super::help::WALLET_HELP);
            if ui.confirm("Open the wallet manager, then check again?", true)? {
                continue;
            } else {
                return Err(cancelled());
            }
        };
        ui.message(&format!("Using {}.", candidates[index].label));
        break ProfileSelection {
            profile: options.profile.clone(),
            provider,
        };
    };
    let consent = match ui.protection(&selection) {
        ProtectionEvidence::Protected => {
            ui.message("Password-protected wallet storage detected.");
            EnrollmentConsent::Verified
        }
        e => {
            ui.message(if e==ProtectionEvidence::Unprotected{"This wallet currently stores secrets without password protection."}else{"This wallet does not expose proof of password protection. Check its password using the steps below."});
            loop {
                let choice = ui.choose(
                    "Wallet protection",
                    &[
                        "Show how to protect the wallet".into(),
                        "I have set a non-empty password in its manager".into(),
                        "Proceed with potentially unprotected storage (unsafe)".into(),
                        "Cancel".into(),
                    ],
                    0,
                )?;
                match choice{0=>ui.message(super::help::WALLET_HELP),1 if e!=ProtectionEvidence::Unprotected=>break EnrollmentConsent::ProtectedByUser,1=>{if ui.protection(&selection)==ProtectionEvidence::Protected{break EnrollmentConsent::Verified}ui.message("Protection is still unconfirmed. Reopen setup after protecting this wallet, or explicitly choose the unsafe option.")},2 if ui.confirm("Accept that this wallet may store your private profile and API key in plaintext?",false)?=>break EnrollmentConsent::UnsafeBackendAccepted,3=>return Err(cancelled()),_=>()}
            }
        }
    };
    let mut old = None;
    if previous.as_ref() == Some(&selection) {
        loop {
            match store.load(selection.clone()).await {
                Ok(p) => {
                    old = Some(p);
                    break;
                }
                Err(e) if e.code == "PROFILE_NOT_FOUND" => break,
                Err(e) => {
                    ui.message(&e.to_string());
                    ui.message(super::help::WALLET_HELP);
                    if !ui.confirm("Open/unlock the selected wallet, then retry?", true)? {
                        return Err(cancelled());
                    }
                }
            }
        }
    }
    let mut url = ui.input(
        "Grocy address (homepage or /api URL)",
        old.as_ref().map(|p| p.api_base.as_str()),
    )?;
    let entered = ui.secret(
        if old.is_some() {
            "API key (Enter keeps the saved key)"
        } else {
            "API key from Manage API keys"
        },
        old.is_some(),
    )?;
    let mut key = if entered.expose_secret().is_empty() {
        SecretString::from(
            old.as_ref()
                .ok_or_else(AppError::input)?
                .api_key
                .expose_secret()
                .to_owned(),
        )
    } else {
        entered
    };
    let access = options
        .access
        .or_else(|| old.as_ref().map(|p| p.access))
        .unwrap_or(AccessMode::Full);
    let mut ca_path = options
        .ca_path
        .or_else(|| old.as_ref().and_then(|p| p.ca_path.clone()));
    let transport = old
        .as_ref()
        .map(|p| p.transport)
        .unwrap_or(options.transport);
    let (base, report) = loop {
        let attempt = async {
            let base = ApiBase::parse(&url, transport)?;
            let p = PrivateProfile {
                api_base: base.clone(),
                api_key: SecretString::from(key.expose_secret().to_owned()),
                access,
                timezone: None,
                defaults: serde_json::Value::Null,
                ca_path: ca_path.clone(),
                transport,
                unsafe_storage: false,
            };
            let c = ApiClient::new(p)?;
            let report = super::probe(&c).await?;
            Ok::<_, AppError>((base, report))
        }
        .await;
        match attempt {
            Ok(result) => break result,
            Err(e) => {
                ui.message(&e.to_string());
                let retry = ui.choose(
                    "Connection recovery (your answers are kept)",
                    &[
                        "Retry after checking Grocy / API bypass".into(),
                        "Change the address".into(),
                        "Replace the API key".into(),
                        "Use a trusted PEM CA file".into(),
                        "Cancel".into(),
                    ],
                    0,
                )?;
                match retry {
                    0 => (),
                    1 => {
                        url = ui.input(
                            "Grocy address (remove UI page names such as /stockoverview)",
                            Some(&url),
                        )?
                    }
                    2 => key = ui.secret("New API key", false)?,
                    3 => ca_path = Some(ui.input("Trusted PEM CA file path", None)?.into()),
                    _ => return Err(cancelled()),
                }
            }
        }
    };
    let timezone = super::help::detected_timezone();
    ui.message(&format!("Ready to save profile '{}'.\nGrocy version: {}\nAccess: {}\nDesktop time zone: {} (timestamps use Grocy's server-local time or an explicit RFC3339 offset)\nWallet storage: {}\nCredentials stay in the selected wallet; the Hermes entry contains no API key or server URL.",options.profile,report.version.as_deref().unwrap_or("unknown"),if access==AccessMode::Full{"household reads, creates, edits, actions and deletes"}else{"household reads only"},timezone.as_deref().unwrap_or("server-local; no manual IANA search required"),if matches!(consent,EnrollmentConsent::UnsafeBackendAccepted){"UNSAFE override: the wallet may persist plaintext"}else{"protected or checked by you in its native manager"}));
    if !ui.confirm("Save this profile?", true)? {
        return Err(cancelled());
    }
    loop {
        let profile = PrivateProfile {
            api_base: base.clone(),
            api_key: SecretString::from(key.expose_secret().to_owned()),
            access,
            timezone: timezone.clone(),
            defaults: super::private_defaults(&report),
            ca_path: ca_path.clone(),
            transport,
            unsafe_storage: matches!(consent, EnrollmentConsent::UnsafeBackendAccepted),
        };
        match store.put(selection.clone(), profile, consent).await {
            Ok(()) => break,
            Err(e) => {
                ui.message(&e.to_string());
                ui.message(super::help::WALLET_HELP);
                if !ui.confirm(
                    "Open/unlock the selected wallet, then retry saving? Your answers are kept.",
                    true,
                )? {
                    return Err(cancelled());
                }
            }
        }
    }
    options.preferences.save(&selection)?;
    ui.message("Profile saved in the selected wallet.");
    let connected = if options.connect_hermes {
        let exe = std::env::current_exe().map_err(|_| AppError::input())?;
        match super::hermes::connect(&exe, &selection).await {
            Ok(()) => {
                ui.message("Hermes configuration and tool discovery checked. Run /reload-mcp in Hermes, or start a new session. Try: Use grocy to list my stock; do not change anything.");
                true
            }
            Err(e) => {
                ui.message(&e.to_string());
                ui.message("Enrollment is complete. Retry connect-hermes or use print-config; no need to enter the key again.");
                ui.message(&super::hermes::render_config(&exe, &selection)?);
                false
            }
        }
    } else {
        false
    };
    Ok(SetupOutcome {
        selection,
        connected,
    })
}
