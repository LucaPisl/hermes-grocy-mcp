use crate::model::ProviderIdentity;
use std::{
    fs::OpenOptions,
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtectionEvidence {
    Protected,
    Unprotected,
    Unverified,
}
pub fn inspect_header(path: &Path) -> ProtectionEvidence {
    let Ok(mut f) = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    else {
        return ProtectionEvidence::Unverified;
    };
    let Ok(meta) = f.metadata() else {
        return ProtectionEvidence::Unverified;
    };
    if !meta.is_file() || meta.uid() != unsafe { libc::geteuid() } {
        return ProtectionEvidence::Unverified;
    }
    let mut buf = [0u8; 20];
    let Ok(n) = f.read(&mut buf) else {
        return ProtectionEvidence::Unverified;
    };
    let h = &buf[..n];
    if h.starts_with(b"GnomeKeyring\n\r\0\n\0\0\0\0") {
        ProtectionEvidence::Protected
    } else if h.starts_with(b"[keyring]") {
        ProtectionEvidence::Unprotected
    } else {
        ProtectionEvidence::Unverified
    }
}
pub fn inspect_protection(identity: &ProviderIdentity) -> ProtectionEvidence {
    // Only inspect a recognized provider's actual default collection label; no unrelated secret reads.
    let Ok(owner) = super::providers::bind_selected(identity) else {
        return ProtectionEvidence::Unverified;
    };
    let Ok(session) = super::secret_service::WalletSession::open(&owner) else {
        return ProtectionEvidence::Unverified;
    };
    let label = session.collection.label();
    if label.is_empty() || label.contains(['/', '\\']) || label == "." || label == ".." {
        return ProtectionEvidence::Unverified;
    }
    let Some(home) = std::env::var_os("HOME") else {
        return ProtectionEvidence::Unverified;
    };
    let root = std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(home).join(".local/share"));
    let exe = identity.executable.to_string_lossy();
    if exe.contains("gnome-keyring") {
        inspect_header(&root.join("keyrings").join(format!("{label}.keyring")))
    } else {
        ProtectionEvidence::Unverified
    }
}
use libsecret::prelude::*;
