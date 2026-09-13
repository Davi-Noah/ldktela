//! The refresh token's only home (RF-03, CLAUDE.md §2.8).
//!
//! Web storage is out of the question: `localStorage` is readable by anything
//! that gets script execution in the WebView, and it survives on disk in
//! plaintext. The token lives in the Windows credential vault, reachable only
//! from here, and crosses into the WebView exactly once per session.

use keyring::Entry;

/// Service name under which the credential is filed.
const SERVICE: &str = "ldkcord";
/// There is one session per install, so the account name is fixed.
const ACCOUNT: &str = "refresh-token";

#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("credential vault unavailable: {0}")]
    Unavailable(String),
}

/// Tauri needs the error to serialise to reach the WebView. The message is
/// deliberately vague: the vault's own errors can name paths and user accounts.
impl serde::Serialize for VaultError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str("credential vault unavailable")
    }
}

fn entry() -> Result<Entry, VaultError> {
    Entry::new(SERVICE, ACCOUNT).map_err(|e| VaultError::Unavailable(e.to_string()))
}

/// `None` when nothing is stored, which is the ordinary first-run state and not
/// an error.
#[tauri::command]
pub fn vault_get_refresh_token() -> Result<Option<String>, VaultError> {
    match entry()?.get_password() {
        Ok(token) => Ok(Some(token)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(VaultError::Unavailable(e.to_string())),
    }
}

#[tauri::command]
pub fn vault_set_refresh_token(token: String) -> Result<(), VaultError> {
    entry()?
        .set_password(&token)
        .map_err(|e| VaultError::Unavailable(e.to_string()))
}

/// Deleting something that is not there is success: logout must be idempotent,
/// or a failed logout leaves a live token behind.
#[tauri::command]
pub fn vault_clear_refresh_token() -> Result<(), VaultError> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(VaultError::Unavailable(e.to_string())),
    }
}
