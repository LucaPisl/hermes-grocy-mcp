use futures_util::future::BoxFuture;
use hermes_grocy_mcp::{
    client::ApiBase,
    credentials::{CredentialStore, EnrollmentConsent},
    error::{AppError, Result},
    model::{AccessMode, PrivateProfile, ProfileSelection, ProviderIdentity, TransportPolicy},
};
use serde_json::json;
pub struct MemoryStore {
    pub url: String,
    pub access: AccessMode,
    pub key: String,
}
impl CredentialStore for MemoryStore {
    fn load(&self, _s: ProfileSelection) -> BoxFuture<'static, Result<PrivateProfile>> {
        let url = self.url.clone();
        let access = self.access;
        let key = self.key.clone();
        Box::pin(async move {
            Ok(PrivateProfile {
                api_base: ApiBase::parse(&url, TransportPolicy::LoopbackDevelopment)?,
                api_key: secrecy::SecretString::from(key),
                access,
                timezone: Some("UTC".into()),
                defaults: json!({}),
                ca_path: None,
                transport: TransportPolicy::LoopbackDevelopment,
                unsafe_storage: false,
            })
        })
    }
    fn put(
        &self,
        _s: ProfileSelection,
        _p: PrivateProfile,
        _c: EnrollmentConsent,
    ) -> BoxFuture<'static, Result<()>> {
        Box::pin(async { Err(AppError::input()) })
    }
    fn forget(&self, _s: ProfileSelection) -> BoxFuture<'static, Result<()>> {
        Box::pin(async { Err(AppError::input()) })
    }
}
pub fn selection() -> ProfileSelection {
    ProfileSelection {
        profile: "synthetic".into(),
        provider: ProviderIdentity {
            bus_name: "org.freedesktop.secrets".into(),
            executable: "/test/wallet".into(),
        },
    }
}
