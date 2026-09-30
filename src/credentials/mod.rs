use crate::{
    error::{AppError, Result},
    model::{PrivateProfile, ProfileSelection},
};
use futures_util::future::BoxFuture;
pub mod preferences;
pub mod protection;
pub mod providers;
mod secret_service;
pub struct DesktopStore;
#[derive(Clone, Copy)]
pub enum EnrollmentConsent {
    Verified,
    ProtectedByUser,
    UnsafeBackendAccepted,
}
pub trait CredentialStore: Send + Sync {
    fn load(&self, selection: ProfileSelection) -> BoxFuture<'static, Result<PrivateProfile>>;
    fn put(
        &self,
        selection: ProfileSelection,
        profile: PrivateProfile,
        consent: EnrollmentConsent,
    ) -> BoxFuture<'static, Result<()>>;
    fn forget(&self, selection: ProfileSelection) -> BoxFuture<'static, Result<()>>;
}
impl CredentialStore for DesktopStore {
    fn load(&self, s: ProfileSelection) -> BoxFuture<'static, Result<PrivateProfile>> {
        Box::pin(async move {
            tokio::task::spawn_blocking(move || secret_service::load(s))
                .await
                .map_err(|_| {
                    AppError::new("WALLET_UNAVAILABLE", "Wallet operation did not complete.")
                })?
        })
    }
    fn put(
        &self,
        s: ProfileSelection,
        mut p: PrivateProfile,
        consent: EnrollmentConsent,
    ) -> BoxFuture<'static, Result<()>> {
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                let protection = protection::inspect_protection(&s.provider);
                if protection == protection::ProtectionEvidence::Unprotected
                    && !matches!(consent, EnrollmentConsent::UnsafeBackendAccepted)
                {
                    return Err(AppError::new(
                        "UNPROTECTED_WALLET",
                        "Protect the wallet in its native manager first.",
                    ));
                }
                if matches!(consent, EnrollmentConsent::Verified)
                    && protection != protection::ProtectionEvidence::Protected
                {
                    return Err(AppError::new(
                        "UNVERIFIED_WALLET",
                        "Review wallet protection in setup.",
                    ));
                }
                p.unsafe_storage = matches!(consent, EnrollmentConsent::UnsafeBackendAccepted);
                secret_service::put(s, p)
            })
            .await
            .map_err(|_| AppError::new("WALLET_UNAVAILABLE", "Wallet storage did not complete."))?
        })
    }
    fn forget(&self, s: ProfileSelection) -> BoxFuture<'static, Result<()>> {
        Box::pin(async move {
            tokio::task::spawn_blocking(move || secret_service::forget(s))
                .await
                .map_err(|_| {
                    AppError::new("WALLET_UNAVAILABLE", "Wallet removal did not complete.")
                })?
        })
    }
}
