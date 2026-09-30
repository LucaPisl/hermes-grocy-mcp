use crate::{
    error::{AppError, Result},
    model::ProfileSelection,
};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::PathBuf,
};
pub struct PreferenceStore {
    pub directory: PathBuf,
}
pub fn validate_profile(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(AppError::input());
    }
    Ok(())
}
impl PreferenceStore {
    pub fn default_location() -> Result<Self> {
        let root = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .ok_or_else(|| {
                AppError::new(
                    "CONFIG_UNAVAILABLE",
                    "Cannot locate your user configuration directory.",
                )
            })?;
        Ok(Self {
            directory: root.join("hermes-grocy-mcp"),
        })
    }
    fn ensure_directory(&self) -> Result<()> {
        if !self.directory.exists() {
            fs::create_dir_all(&self.directory)
                .map_err(|_| AppError::new("CONFIG_UNAVAILABLE", "Cannot create preferences."))?
        }
        let m = fs::symlink_metadata(&self.directory).map_err(|_| AppError::input())?;
        if !m.is_dir() || m.uid() != unsafe { libc::geteuid() } {
            return Err(AppError::new(
                "UNSAFE_PREFERENCES",
                "Preferences must be in your owned directory.",
            ));
        }
        fs::set_permissions(&self.directory, fs::Permissions::from_mode(0o700))
            .map_err(|_| AppError::input())?;
        Ok(())
    }
    pub fn save(&self, s: &ProfileSelection) -> Result<()> {
        validate_profile(&s.profile)?;
        self.ensure_directory()?;
        let tmp = self
            .directory
            .join(format!(".{}.{}.tmp", s.profile, std::process::id()));
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&tmp)
            .map_err(|_| {
                AppError::new("CONFIG_UNAVAILABLE", "Cannot atomically write preferences.")
            })?;
        let result = (|| {
            f.write_all(&serde_json::to_vec(s).map_err(|_| AppError::input())?)
                .map_err(|_| AppError::input())?;
            f.sync_all().map_err(|_| AppError::input())?;
            fs::rename(&tmp, self.directory.join(format!("{}.json", s.profile)))
                .map_err(|_| AppError::input())?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(tmp);
        }
        result
    }
    pub fn load(&self, name: &str) -> Result<ProfileSelection> {
        validate_profile(name)?;
        let mut f = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(self.directory.join(format!("{name}.json")))
            .map_err(|_| AppError::new("PROFILE_NOT_FOUND", "Run setup to enroll this profile."))?;
        let m = f.metadata().map_err(|_| AppError::input())?;
        if !m.is_file()
            || m.uid() != unsafe { libc::geteuid() }
            || m.permissions().mode() & 0o077 != 0
        {
            return Err(AppError::new(
                "UNSAFE_PREFERENCES",
                "Preferences must be owned by you and accessible only to you.",
            ));
        }
        let mut bytes = Vec::new();
        Read::by_ref(&mut f)
            .take(16385)
            .read_to_end(&mut bytes)
            .map_err(|_| AppError::input())?;
        if bytes.len() > 16384 {
            return Err(AppError::input());
        }
        let s: ProfileSelection = serde_json::from_slice(&bytes).map_err(|_| AppError::input())?;
        if s.profile != name {
            return Err(AppError::input());
        }
        Ok(s)
    }
    pub fn remove(&self, name: &str) -> Result<()> {
        self.load(name)?;
        fs::remove_file(self.directory.join(format!("{name}.json"))).map_err(|_| AppError::input())
    }
}
