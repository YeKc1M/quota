//! Read-only access to Kimi Code's OAuth credentials. Token renewal belongs to
//! Kimi Code so Quota cannot invalidate the CLI's refresh token.

use serde::Deserialize;
use std::fmt;
use std::path::{Path, PathBuf};

pub fn local_credentials_path() -> Option<PathBuf> {
    crate::local_paths::provider_home("KIMI_CODE_HOME", ".kimi-code")
        .map(|home| home.join("credentials").join("kimi-code.json"))
}

#[derive(Clone)]
pub struct LocalKimiCredentials {
    pub access_token: String,
    pub expires_at: f64,
}

impl fmt::Debug for LocalKimiCredentials {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalKimiCredentials")
            .field("access_token", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

#[derive(Debug)]
pub enum LocalCredentialError {
    NotFound,
    Unreadable(String),
    Malformed(String),
    Expired,
}

impl fmt::Display for LocalCredentialError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => write!(
                formatter,
                "No Kimi Code OAuth credentials found. Run /login in Kimi Code first."
            ),
            Self::Unreadable(detail) => {
                write!(
                    formatter,
                    "Could not read Kimi Code credentials: {}",
                    detail
                )
            }
            Self::Malformed(detail) => write!(
                formatter,
                "Kimi Code credentials are not in the expected shape: {}. Run /login in Kimi Code.",
                detail
            ),
            Self::Expired => write!(
                formatter,
                "Kimi Code's stored token has expired. Run Kimi Code to refresh it, or use /login."
            ),
        }
    }
}

#[derive(Deserialize)]
struct AuthFile {
    access_token: String,
    expires_at: f64,
}

pub fn read_local_credentials_at(
    path: &Path,
) -> Result<LocalKimiCredentials, LocalCredentialError> {
    let raw = std::fs::read_to_string(path).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            LocalCredentialError::NotFound
        } else {
            LocalCredentialError::Unreadable(err.to_string())
        }
    })?;
    parse_credentials_json(&raw)
}

/// Parse a credential payload from the file or from the OS keyring. Only
/// position information from the serde error is kept, since its message can
/// quote a credential value.
fn parse_credentials_json(raw: &str) -> Result<LocalKimiCredentials, LocalCredentialError> {
    let parsed: AuthFile = serde_json::from_str(raw).map_err(|err| {
        LocalCredentialError::Malformed(format!(
            "JSON error at line {} column {}",
            err.line(),
            err.column()
        ))
    })?;
    let access_token = parsed.access_token.trim().to_string();
    if access_token.is_empty() {
        return Err(LocalCredentialError::Malformed(
            "no access token".to_string(),
        ));
    }
    if !parsed.expires_at.is_finite() || parsed.expires_at <= 0.0 {
        return Err(LocalCredentialError::Malformed(
            "invalid expiry timestamp".to_string(),
        ));
    }
    if parsed.expires_at <= chrono::Utc::now().timestamp() as f64 {
        return Err(LocalCredentialError::Expired);
    }

    Ok(LocalKimiCredentials {
        access_token,
        expires_at: parsed.expires_at,
    })
}

// ---------------------------------------------------------------------------
// Keyring fallback
// ---------------------------------------------------------------------------
//
// Kimi Code normally keeps its OAuth credentials in
// `~/.kimi-code/credentials/kimi-code.json`, but when that file is missing
// (or no home directory can be resolved) the credentials may instead live in
// the OS keyring, where Python's `keyring` package stores them under the
// service `kimi-code` and the user `oauth/kimi-code`. The file wins whenever
// it exists: an unreadable, malformed, or expired file reports its own error
// and the keyring is never consulted. The keyring is read-only here; nothing
// is ever stored, refreshed, or deleted.

const KEYRING_SERVICE: &str = "kimi-code";
const KEYRING_USER: &str = "oauth/kimi-code";

/// Fixed detail for keyring backend and encoding failures. Deliberately
/// carries nothing from the underlying error so no platform message can leak
/// secret material into diagnostics.
const KEYRING_UNAVAILABLE_DETAIL: &str = "OS keyring lookup failed";

/// Result of one keyring lookup, reduced to what the fallback policy needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyringReadError {
    NoEntry,
    Unavailable,
}

/// Injectable boundary over the OS keyring so the fallback policy can be
/// tested without a session bus, an unlock prompt, or the real store.
trait KeyringSource {
    fn get_password(&self) -> Result<String, KeyringReadError>;
}

fn map_keyring_error(err: keyring::Error) -> KeyringReadError {
    match err {
        keyring::Error::NoEntry => KeyringReadError::NoEntry,
        _ => KeyringReadError::Unavailable,
    }
}

struct OsKeyring;

#[cfg(not(windows))]
impl KeyringSource for OsKeyring {
    fn get_password(&self) -> Result<String, KeyringReadError> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
            .map_err(|_| KeyringReadError::Unavailable)?;
        entry.get_password().map_err(map_keyring_error)
    }
}

#[cfg(windows)]
impl KeyringSource for OsKeyring {
    fn get_password(&self) -> Result<String, KeyringReadError> {
        // The store must be initialized before `keyring-core` entries can be
        // built, matching the Antigravity reader.
        if keyring::Entry::store_status().is_err() {
            return Err(KeyringReadError::Unavailable);
        }
        read_windows_keyring(self)
    }
}

/// The result of reading one Credential Manager target. `NotMatched` covers
/// both an absent entry and an entry whose stored username is not the one
/// Kimi Code uses; either way the selection moves on to the next target.
#[cfg(any(windows, test))]
#[derive(Debug, Clone, PartialEq, Eq)]
enum WindowsTargetOutcome {
    NotMatched,
    /// The stored username was verified before the payload was read, so a
    /// matched payload is always safe to parse.
    Matched(String),
}

/// Per-target Windows keyring boundary. Implementations must verify the
/// stored username before consuming or returning any secret payload.
#[cfg(any(windows, test))]
trait WindowsKeyring {
    fn read_target(&self, target: &str) -> Result<WindowsTargetOutcome, KeyringReadError>;
}

/// Walk the Credential Manager targets Python's keyring writes: the service
/// name first, then the compound `{username}@{service}` target used after a
/// collision. Backend failures propagate; only a fully absent or mismatched
/// pair reads as `NoEntry`.
#[cfg(any(windows, test))]
fn read_windows_keyring(keyring: &dyn WindowsKeyring) -> Result<String, KeyringReadError> {
    for target in [KEYRING_SERVICE.to_string(), windows_compound_target()] {
        match keyring.read_target(&target) {
            Ok(WindowsTargetOutcome::Matched(payload)) => return Ok(payload),
            Ok(WindowsTargetOutcome::NotMatched) | Err(KeyringReadError::NoEntry) => continue,
            Err(err) => return Err(err),
        }
    }
    Err(KeyringReadError::NoEntry)
}

#[cfg(windows)]
impl WindowsKeyring for OsKeyring {
    fn read_target(&self, target: &str) -> Result<WindowsTargetOutcome, KeyringReadError> {
        read_windows_target(target)
    }
}

/// Read one Credential Manager target. The stored username is checked before
/// the secret blob is read or decoded, so a locked or unreadable attributes
/// call surfaces as `Unavailable` instead of masquerading as "no entry".
///
/// Validation at implementation time: this module passed an isolated Windows
/// MSVC cross-check, and the target-selection and UTF-16 helpers passed unit
/// tests on Linux. A full Windows build was blocked by C dependencies requiring
/// an unavailable MSVC/MinGW toolchain. Reading a real Windows Credential Manager
/// entry has not been validated; the mocked tests do not exercise that backend.
#[cfg(windows)]
fn read_windows_target(target: &str) -> Result<WindowsTargetOutcome, KeyringReadError> {
    let modifiers = std::collections::HashMap::from([("target", target)]);
    let entry = keyring_core::Entry::new_with_modifiers(KEYRING_SERVICE, KEYRING_USER, &modifiers)
        .map_err(|_| KeyringReadError::Unavailable)?;
    let username = entry
        .get_attributes()
        .map_err(map_keyring_error)?
        .remove("username");
    match username.as_deref() {
        Some(KEYRING_USER) => {
            let secret = entry.get_secret().map_err(map_keyring_error)?;
            Ok(WindowsTargetOutcome::Matched(decode_windows_keyring_blob(
                &secret,
            )?))
        }
        _ => Ok(WindowsTargetOutcome::NotMatched),
    }
}

/// Credential Manager target Python's keyring falls back to after a username
/// collision under the service: `{username}@{service}`.
#[cfg(any(windows, test))]
fn windows_compound_target() -> String {
    format!("{}@{}", KEYRING_USER, KEYRING_SERVICE)
}

/// Decode a Credential Manager blob written by Python's keyring, which stores
/// passwords as UTF-16. A byte-order mark is stripped when present; the blob
/// is otherwise little-endian, matching the platform. Never echoes the input.
#[cfg(any(windows, test))]
fn decode_windows_keyring_blob(blob: &[u8]) -> Result<String, KeyringReadError> {
    let units: &[u8] = match blob {
        [0xFF, 0xFE, rest @ ..] => rest,
        _ => blob,
    };
    if units.len() % 2 != 0 {
        return Err(KeyringReadError::Unavailable);
    }
    let utf16: Vec<u16> = units
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    String::from_utf16(&utf16).map_err(|_| KeyringReadError::Unavailable)
}

/// Resolve credentials, preferring the file and consulting the keyring only
/// when the file is missing or no home directory can be resolved. Any other
/// file error (unreadable, malformed, expired) is returned as-is.
fn read_credentials_with(
    path: Option<PathBuf>,
    keyring: &dyn KeyringSource,
) -> Result<LocalKimiCredentials, LocalCredentialError> {
    if let Some(path) = path {
        match read_local_credentials_at(&path) {
            Ok(credentials) => return Ok(credentials),
            Err(LocalCredentialError::NotFound) => {}
            Err(error) => return Err(error),
        }
    }
    read_keyring_fallback(keyring)
}

fn read_keyring_fallback(
    keyring: &dyn KeyringSource,
) -> Result<LocalKimiCredentials, LocalCredentialError> {
    let raw = keyring.get_password().map_err(|err| match err {
        KeyringReadError::NoEntry => LocalCredentialError::NotFound,
        KeyringReadError::Unavailable => {
            LocalCredentialError::Unreadable(KEYRING_UNAVAILABLE_DETAIL.to_string())
        }
    })?;
    parse_credentials_json(&raw)
}

pub fn read_local_credentials() -> Result<LocalKimiCredentials, LocalCredentialError> {
    read_credentials_with(local_credentials_path(), &OsKeyring)
}

#[cfg(test)]
#[path = "local/tests.rs"]
mod tests;
