//! Where refresh tokens are kept between runs. Access tokens never leave memory.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use abs_api::{SinkError, TokenSink};
use keyring_core::{CredentialStore, Entry};

const SERVICE: &str = "abs-desktop";
const PROBE_USER: &str = "availability-probe";

/// Credential errors.
#[derive(Debug, thiserror::Error)]
pub enum CredentialError {
    /// No system keyring, such as Linux without a Secret Service. Ask the user what to do.
    #[error("system keyring unavailable: {0}")]
    Unavailable(String),
    #[error("keyring error: {0}")]
    Keyring(#[from] keyring_core::Error),
    #[error("token file error: {0}")]
    Io(#[from] io::Error),
}

#[derive(Debug)]
enum Backend {
    Keyring(Arc<CredentialStore>),
    /// Files readable only by the user, chosen explicitly when there is no keyring.
    Files(PathBuf),
    /// "Don't remember me": tokens are gone when the app exits.
    Memory(Mutex<HashMap<String, String>>),
}

/// Stores one refresh token per account, keyed by account UUID.
#[derive(Debug, Clone)]
pub struct Credentials {
    backend: Arc<Backend>,
}

impl Credentials {
    /// The OS keyring: Windows Credential Manager or the Secret Service on Linux.
    pub fn system() -> Result<Self, CredentialError> {
        let store = system_store().map_err(|e| CredentialError::Unavailable(e.to_string()))?;
        probe(&store)?;
        Ok(Self::with(Backend::Keyring(store)))
    }

    /// One file per account in `dir`, mode 0600 on Unix. Only after the user agreed to it.
    pub fn files(dir: impl Into<PathBuf>) -> Self {
        Self::with(Backend::Files(dir.into()))
    }

    /// Keeps tokens in memory only.
    pub fn memory() -> Self {
        Self::with(Backend::Memory(Mutex::default()))
    }

    fn with(backend: Backend) -> Self {
        Self {
            backend: Arc::new(backend),
        }
    }

    /// True when tokens survive an app restart.
    pub fn is_persistent(&self) -> bool {
        !matches!(*self.backend, Backend::Memory(_))
    }

    pub fn save(&self, account_id: &str, token: &str) -> Result<(), CredentialError> {
        match &*self.backend {
            Backend::Keyring(store) => entry(store, account_id)?.set_password(token)?,
            Backend::Files(dir) => write_private(&token_path(dir, account_id), token)?,
            Backend::Memory(map) => {
                lock(map).insert(account_id.to_owned(), token.to_owned());
            }
        }
        Ok(())
    }

    /// The stored token, or `None` when there is none.
    pub fn load(&self, account_id: &str) -> Result<Option<String>, CredentialError> {
        match &*self.backend {
            Backend::Keyring(store) => match entry(store, account_id)?.get_password() {
                Ok(token) => Ok(Some(token)),
                Err(keyring_core::Error::NoEntry) => Ok(None),
                Err(e) => Err(e.into()),
            },
            Backend::Files(dir) => match std::fs::read_to_string(token_path(dir, account_id)) {
                Ok(token) => Ok(Some(token)),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(e.into()),
            },
            Backend::Memory(map) => Ok(lock(map).get(account_id).cloned()),
        }
    }

    /// Removes the token. Removing a missing token is not an error.
    pub fn delete(&self, account_id: &str) -> Result<(), CredentialError> {
        match &*self.backend {
            Backend::Keyring(store) => match entry(store, account_id)?.delete_credential() {
                Ok(()) | Err(keyring_core::Error::NoEntry) => {}
                Err(e) => return Err(e.into()),
            },
            Backend::Files(dir) => match std::fs::remove_file(token_path(dir, account_id)) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            },
            Backend::Memory(map) => {
                lock(map).remove(account_id);
            }
        }
        Ok(())
    }

    /// A [`TokenSink`] that saves every new refresh token of `account_id`.
    pub fn sink(&self, account_id: &str) -> impl TokenSink + 'static {
        let credentials = self.clone();
        let account_id = account_id.to_owned();
        move |token: &str| -> Result<(), SinkError> {
            credentials
                .save(&account_id, token)
                .map_err(|e| Box::new(e) as SinkError)
        }
    }
}

#[cfg(windows)]
fn system_store() -> keyring_core::Result<Arc<CredentialStore>> {
    Ok(windows_native_keyring_store::Store::new()?)
}

#[cfg(target_os = "linux")]
fn system_store() -> keyring_core::Result<Arc<CredentialStore>> {
    Ok(zbus_secret_service_keyring_store::Store::new()?)
}

#[cfg(not(any(windows, target_os = "linux")))]
fn system_store() -> keyring_core::Result<Arc<CredentialStore>> {
    Err(keyring_core::Error::NotSupportedByStore(
        "no keyring for this platform".into(),
    ))
}

/// Reads a missing entry to check the keyring answers at all.
fn probe(store: &Arc<CredentialStore>) -> Result<(), CredentialError> {
    match entry(store, PROBE_USER)?.get_password() {
        Ok(_) | Err(keyring_core::Error::NoEntry) => Ok(()),
        Err(e) => Err(CredentialError::Unavailable(e.to_string())),
    }
}

fn entry(store: &Arc<CredentialStore>, account_id: &str) -> keyring_core::Result<Entry> {
    store.build(SERVICE, account_id, None)
}

fn token_path(dir: &Path, account_id: &str) -> PathBuf {
    dir.join(format!("{account_id}.token"))
}

fn lock(
    map: &Mutex<HashMap<String, String>>,
) -> std::sync::MutexGuard<'_, HashMap<String, String>> {
    map.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Writes through a temporary file so a crash never leaves half a token.
fn write_private(path: &Path, contents: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    io::Write::write_all(&mut options.open(&tmp)?, contents.as_bytes())?;
    std::fs::rename(tmp, path)
}

#[cfg(test)]
mod tests;
