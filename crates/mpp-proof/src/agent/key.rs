//! The API key: kept in Windows Credential Manager (through `keyring`), never in a file,
//! a log, an error message or the repository.

/// An API key. Its `Debug` output is redacted and it has no `Display`; the only way to
/// read it is [`ApiKey::expose`], which the transport calls to set the `x-api-key` header.
#[derive(Clone, PartialEq, Eq)]
pub struct ApiKey(String);

impl ApiKey {
    /// Wraps a key (surrounding whitespace removed).
    pub fn new(key: impl Into<String>) -> Self {
        ApiKey(key.into().trim().to_string())
    }

    /// The key itself, for the request header only.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Removes every copy of the key from `text` (for messages that echo a request).
    pub fn scrub(&self, text: &str) -> String {
        if self.0.len() < 8 {
            return text.to_string();
        }
        text.replace(&self.0, "[API key removed]")
    }
}

impl std::fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ApiKey([redacted])")
    }
}

/// Where the key lives.
pub trait KeyStore {
    /// The stored key, or `None` when none is set (the feature is then off).
    fn get(&self) -> Result<Option<ApiKey>, String>;
    fn set(&self, key: &ApiKey) -> Result<(), String>;
    fn delete(&self) -> Result<(), String>;
}

/// The service and account names under which the key is stored.
pub const SERVICE: &str = "Math++";
pub const ACCOUNT: &str = "anthropic-api-key";

/// Windows Credential Manager, or the macOS Keychain. On other systems there is no
/// supported secure store, so every call fails with a clear message rather than
/// pretending to save the key.
#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
pub struct CredentialStore {
    service: String,
    account: String,
}

impl Default for CredentialStore {
    fn default() -> Self {
        CredentialStore {
            service: SERVICE.into(),
            account: ACCOUNT.into(),
        }
    }
}

impl CredentialStore {
    /// A store under another name (for tests that must not touch the real entry).
    pub fn named(service: &str, account: &str) -> Self {
        CredentialStore {
            service: service.into(),
            account: account.into(),
        }
    }

    #[cfg(any(windows, target_os = "macos"))]
    fn entry(&self) -> Result<keyring::Entry, String> {
        keyring::Entry::new(&self.service, &self.account)
            .map_err(|e| format!("the credential store is not available: {e}"))
    }
}

#[cfg(any(windows, target_os = "macos"))]
impl KeyStore for CredentialStore {
    fn get(&self) -> Result<Option<ApiKey>, String> {
        match self.entry()?.get_password() {
            Ok(k) if !k.trim().is_empty() => Ok(Some(ApiKey::new(k))),
            Ok(_) | Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(format!(
                "the key could not be read from the credential store: {e}"
            )),
        }
    }

    fn set(&self, key: &ApiKey) -> Result<(), String> {
        self.entry()?
            .set_password(key.expose())
            .map_err(|e| format!("the key could not be saved in the credential store: {e}"))
    }

    fn delete(&self) -> Result<(), String> {
        match self.entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(format!("the key could not be removed: {e}")),
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
impl KeyStore for CredentialStore {
    fn get(&self) -> Result<Option<ApiKey>, String> {
        // No stored key: the agent stays off, as when none has been set.
        Ok(None)
    }

    fn set(&self, _key: &ApiKey) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }

    fn delete(&self) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
const UNSUPPORTED: &str =
    "secure key storage is only available on Windows and macOS, so the key was not saved";

/// A store in memory, for tests.
#[derive(Default)]
pub struct MemoryStore(std::sync::Mutex<Option<ApiKey>>);

impl KeyStore for MemoryStore {
    fn get(&self) -> Result<Option<ApiKey>, String> {
        self.0
            .lock()
            .map(|k| k.clone())
            .map_err(|_| "the store is poisoned".to_string())
    }

    fn set(&self, key: &ApiKey) -> Result<(), String> {
        let mut k = self
            .0
            .lock()
            .map_err(|_| "the store is poisoned".to_string())?;
        *k = Some(key.clone());
        Ok(())
    }

    fn delete(&self) -> Result<(), String> {
        let mut k = self
            .0
            .lock()
            .map_err(|_| "the store is poisoned".to_string())?;
        *k = None;
        Ok(())
    }
}
