use crate::{
    error::{AppError, Result},
    model::ProviderIdentity,
};
use glib::variant::ToVariant;
use std::{collections::HashSet, path::PathBuf};
pub const ALIASES: &[(&str, &str)] = &[
    ("KDE KWallet", "org.kde.secretservicecompat"),
    ("KDE KWallet", "org.kde.kwalletd6"),
    ("KDE KWallet", "org.kde.kwalletd5"),
    ("GNOME Keyring", "org.gnome.keyring"),
    ("System Secret Service", "org.freedesktop.secrets"),
];
#[derive(Clone)]
pub struct ProviderCandidate {
    pub label: String,
    pub identity: Option<ProviderIdentity>,
    pub recommended: bool,
}
pub(crate) fn bus() -> Result<gio::DBusConnection> {
    gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE).map_err(|_| {
        AppError::new(
            "WALLET_UNAVAILABLE",
            "No desktop session bus. Run setup from your desktop session.",
        )
    })
}
pub(crate) fn call(
    bus: &gio::DBusConnection,
    method: &str,
    params: Option<&glib::Variant>,
) -> Result<glib::Variant> {
    bus.call_sync(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        method,
        params,
        None,
        gio::DBusCallFlags::NO_AUTO_START,
        5000,
        gio::Cancellable::NONE,
    )
    .map_err(|_| {
        AppError::new(
            "WALLET_UNAVAILABLE",
            "The selected wallet is not running. Open its native manager and retry.",
        )
    })
}
pub(crate) fn owner_identity(bus: &gio::DBusConnection, name: &str) -> Result<(String, PathBuf)> {
    let owner = call(bus, "GetNameOwner", Some(&(name,).to_variant()))?
        .get::<(String,)>()
        .ok_or_else(AppError::input)?
        .0;
    let pid = call(
        bus,
        "GetConnectionUnixProcessID",
        Some(&(owner.as_str(),).to_variant()),
    )?
    .get::<(u32,)>()
    .ok_or_else(AppError::input)?
    .0;
    let exe = std::fs::read_link(format!("/proc/{pid}/exe")).map_err(|_| {
        AppError::new(
            "WALLET_UNAVAILABLE",
            "Cannot identify the selected wallet process.",
        )
    })?;
    Ok((owner, exe))
}
pub(crate) fn bind_selected(identity: &ProviderIdentity) -> Result<String> {
    let bus = bus()?;
    let (owner, exe) = owner_identity(&bus, &identity.bus_name)?;
    if exe != identity.executable {
        return Err(AppError::new(
            "WALLET_PROVIDER_CHANGED",
            "The wallet provider changed. Run setup to review its selection.",
        ));
    }
    Ok(owner)
}
fn supports_secret_service(bus: &gio::DBusConnection, owner: &str) -> bool {
    bus.call_sync(
        Some(owner),
        "/org/freedesktop/secrets",
        "org.freedesktop.DBus.Introspectable",
        "Introspect",
        None,
        None,
        gio::DBusCallFlags::NO_AUTO_START,
        5000,
        gio::Cancellable::NONE,
    )
    .ok()
    .and_then(|v| v.get::<(String,)>())
    .is_some_and(|(xml,)| {
        xml.len() <= 65536
            && (xml.contains("name=\"org.freedesktop.Secret.Service\"")
                || xml.contains("name='org.freedesktop.Secret.Service'"))
    })
}
pub fn discover_providers() -> Result<Vec<ProviderCandidate>> {
    let bus = bus()?;
    let names = call(&bus, "ListNames", None)?
        .get::<(Vec<String>,)>()
        .ok_or_else(AppError::input)?
        .0;
    let inactive = call(&bus, "ListActivatableNames", None)?
        .get::<(Vec<String>,)>()
        .ok_or_else(AppError::input)?
        .0;
    let desktop = std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_lowercase();
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    for (label, name) in ALIASES {
        if names.iter().any(|n| n == name) {
            if let Ok((owner, exe)) = owner_identity(&bus, name)
                && supports_secret_service(&bus, &owner)
                && seen.insert(owner)
            {
                let executable_name = exe
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                let kde_desktop = desktop.contains("kde") || desktop.contains("lxqt");
                let gnome_desktop = [
                    "gnome", "cinnamon", "mate", "xfce", "lxde", "unity", "pantheon", "budgie",
                ]
                .iter()
                .any(|d| desktop.contains(d));
                let recommended = (kde_desktop
                    && (executable_name.contains("kwallet")
                        || executable_name.contains("ksecret")))
                    || (gnome_desktop && executable_name.contains("gnome-keyring"));
                output.push(ProviderCandidate {
                    label: label.to_string(),
                    identity: Some(ProviderIdentity {
                        bus_name: name.to_string(),
                        executable: exe,
                    }),
                    recommended,
                });
            }
        } else if inactive.iter().any(|n| n == name) && !output.iter().any(|p| p.label == *label) {
            output.push(ProviderCandidate {
                label: format!("{label} (not running; open its manager)"),
                identity: None,
                recommended: false,
            })
        }
    }
    output.sort_by_key(|p| (!p.recommended, p.identity.is_none()));
    Ok(output)
}
