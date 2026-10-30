use super::*;
use std::cell::RefCell;

const VALID_BODY: &str = r#"{"access_token":"keyring-token","expires_at":4102444800}"#;
const SECRET: &str = "keyring-token";

struct FakeKeyring {
    payload: Result<String, KeyringReadError>,
    calls: RefCell<usize>,
}

impl FakeKeyring {
    fn returning(payload: Result<String, KeyringReadError>) -> Self {
        Self {
            payload,
            calls: RefCell::new(0),
        }
    }

    fn calls(&self) -> usize {
        *self.calls.borrow()
    }
}

impl KeyringSource for FakeKeyring {
    fn get_password(&self) -> Result<String, KeyringReadError> {
        *self.calls.borrow_mut() += 1;
        self.payload.clone()
    }
}

fn fixture(name: &str, body: Option<&str>) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "quota-kimi-keyring-{}-{}",
        std::process::id(),
        name
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("kimi-code.json");
    if let Some(body) = body {
        std::fs::write(&path, body).unwrap();
    } else {
        let _ = std::fs::remove_file(&path);
    }
    path
}

#[test]
fn valid_file_avoids_the_keyring_entirely() {
    let keyring = FakeKeyring::returning(Ok(SECRET.to_string()));
    let path = fixture(
        "valid-file",
        Some(r#"{"access_token":"file-token","expires_at":4102444800}"#),
    );
    let credentials = read_credentials_with(Some(path.clone()), &keyring).unwrap();
    assert_eq!(credentials.access_token, "file-token");
    assert_eq!(keyring.calls(), 0);
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn missing_file_falls_back_to_the_keyring() {
    let keyring = FakeKeyring::returning(Ok(VALID_BODY.to_string()));
    let path = fixture("missing-file", None);
    let credentials = read_credentials_with(Some(path.clone()), &keyring).unwrap();
    assert_eq!(credentials.access_token, SECRET);
    assert_eq!(keyring.calls(), 1);
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn missing_home_falls_back_to_the_keyring() {
    let keyring = FakeKeyring::returning(Ok(VALID_BODY.to_string()));
    let credentials = read_credentials_with(None, &keyring).unwrap();
    assert_eq!(credentials.access_token, SECRET);
    assert_eq!(keyring.calls(), 1);
}

#[test]
fn missing_file_with_no_keyring_entry_is_not_found() {
    let keyring = FakeKeyring::returning(Err(KeyringReadError::NoEntry));
    let path = fixture("no-entry", None);
    let error = read_credentials_with(Some(path.clone()), &keyring).unwrap_err();
    assert!(matches!(error, LocalCredentialError::NotFound));
    assert_eq!(keyring.calls(), 1);
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn unavailable_keyring_is_a_sanitized_unreadable_error() {
    let keyring = FakeKeyring::returning(Err(KeyringReadError::Unavailable));
    let path = fixture("unavailable", None);
    let error = read_credentials_with(Some(path.clone()), &keyring).unwrap_err();
    match &error {
        LocalCredentialError::Unreadable(detail) => {
            assert_eq!(detail, KEYRING_UNAVAILABLE_DETAIL)
        }
        other => panic!("expected Unreadable, got {:?}", other),
    }
    let rendered = format!("{} {:?}", error, error);
    assert!(!rendered.contains(SECRET));
    assert!(!rendered.contains('@'));
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn unreadable_malformed_and_expired_files_never_fall_back() {
    let future = chrono::Utc::now().timestamp() + 3600;
    for (name, body) in [
        ("malformed-file", r#"{"access_token":"#.to_string()),
        (
            "empty-token-file",
            format!(r#"{{"access_token":"  ","expires_at":{}}}"#, future),
        ),
        (
            "expired-file",
            r#"{"access_token":"expired-token","expires_at":1}"#.to_string(),
        ),
    ] {
        let keyring = FakeKeyring::returning(Ok(VALID_BODY.to_string()));
        let path = fixture(name, Some(&body));
        let error = read_credentials_with(Some(path.clone()), &keyring).unwrap_err();
        assert!(
            !matches!(error, LocalCredentialError::NotFound),
            "{} unexpectedly looked like NotFound",
            name
        );
        assert_eq!(keyring.calls(), 0, "{} consulted the keyring", name);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    // A directory at the file path reads as Unreadable, not NotFound.
    let keyring = FakeKeyring::returning(Ok(VALID_BODY.to_string()));
    let dir = fixture("unreadable-dir", None)
        .parent()
        .unwrap()
        .to_path_buf();
    let error = read_credentials_with(Some(dir.clone()), &keyring).unwrap_err();
    assert!(matches!(error, LocalCredentialError::Unreadable(_)));
    assert_eq!(keyring.calls(), 0);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn keyring_payloads_use_the_same_validation_as_the_file() {
    let future = chrono::Utc::now().timestamp() + 3600;
    for (name, payload, expected) in [
        (
            "malformed-payload",
            r#"{"access_token":"#.to_string(),
            "malformed",
        ),
        (
            "empty-token-payload",
            format!(r#"{{"access_token":" ","expires_at":{}}}"#, future),
            "malformed",
        ),
        (
            "expired-payload",
            r#"{"access_token":"expired-token","expires_at":1}"#.to_string(),
            "expired",
        ),
    ] {
        let keyring = FakeKeyring::returning(Ok(payload));
        let error = read_credentials_with(None, &keyring).unwrap_err();
        let rendered = format!("{} {:?}", error, error);
        assert!(!rendered.contains("expired-token"), "{}", name);
        assert!(!rendered.contains('@'), "{}", name);
        if expected == "expired" {
            assert!(matches!(error, LocalCredentialError::Expired), "{}", name);
        } else {
            assert!(
                matches!(error, LocalCredentialError::Malformed(_)),
                "{}",
                name
            );
        }
    }
}

#[test]
fn fallback_reads_leave_files_and_payloads_untouched() {
    let body = r#"{"access_token":"file-token","expires_at":1}"#;
    let path = fixture("untouched", Some(body));
    let keyring = FakeKeyring::returning(Ok(VALID_BODY.to_string()));
    let _ = read_credentials_with(Some(path.clone()), &keyring);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), body);
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn windows_helpers_match_python_keyring_layout() {
    assert_eq!(windows_compound_target(), "oauth/kimi-code@kimi-code");

    let text = r#"{"access_token":"x","expires_at":4102444800}"#;
    let utf16: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
    assert_eq!(decode_windows_keyring_blob(&utf16).unwrap(), text);

    let mut with_bom = vec![0xFF, 0xFE];
    with_bom.extend_from_slice(&utf16);
    assert_eq!(decode_windows_keyring_blob(&with_bom).unwrap(), text);

    assert!(decode_windows_keyring_blob(&utf16[..utf16.len() - 1]).is_err());
    assert!(decode_windows_keyring_blob(&[0x00, 0xDC]).is_err());
    assert!(decode_windows_keyring_blob(&[]).unwrap().is_empty());
}

/// Scripted per-target Windows keyring. Each response stands in for a
/// `read_windows_target` call in which username verification already
/// happened: `Matched` only after the stored username checked out.
struct FakeWindowsKeyring {
    primary: Result<WindowsTargetOutcome, KeyringReadError>,
    compound: Result<WindowsTargetOutcome, KeyringReadError>,
    read: RefCell<Vec<String>>,
}

impl FakeWindowsKeyring {
    fn new(
        primary: Result<WindowsTargetOutcome, KeyringReadError>,
        compound: Result<WindowsTargetOutcome, KeyringReadError>,
    ) -> Self {
        Self {
            primary,
            compound,
            read: RefCell::new(Vec::new()),
        }
    }

    fn read_targets(&self) -> Vec<String> {
        self.read.borrow().clone()
    }
}

impl WindowsKeyring for FakeWindowsKeyring {
    fn read_target(&self, target: &str) -> Result<WindowsTargetOutcome, KeyringReadError> {
        self.read.borrow_mut().push(target.to_string());
        if target == KEYRING_SERVICE {
            self.primary.clone()
        } else {
            assert_eq!(target, windows_compound_target());
            self.compound.clone()
        }
    }
}

#[test]
fn windows_primary_match_stops_without_reading_the_compound_target() {
    let keyring = FakeWindowsKeyring::new(
        Ok(WindowsTargetOutcome::Matched(VALID_BODY.to_string())),
        Ok(WindowsTargetOutcome::Matched("unreachable".to_string())),
    );
    let payload = read_windows_keyring(&keyring).unwrap();
    assert_eq!(payload, VALID_BODY);
    assert_eq!(keyring.read_targets(), vec![KEYRING_SERVICE.to_string()]);
}

#[test]
fn windows_primary_missing_reads_the_compound_target() {
    let keyring = FakeWindowsKeyring::new(
        Err(KeyringReadError::NoEntry),
        Ok(WindowsTargetOutcome::Matched(VALID_BODY.to_string())),
    );
    let payload = read_windows_keyring(&keyring).unwrap();
    assert_eq!(payload, VALID_BODY);
    assert_eq!(
        keyring.read_targets(),
        vec![KEYRING_SERVICE.to_string(), windows_compound_target()]
    );
}

#[test]
fn windows_primary_username_mismatch_reads_the_compound_target() {
    let keyring = FakeWindowsKeyring::new(
        Ok(WindowsTargetOutcome::NotMatched),
        Ok(WindowsTargetOutcome::Matched(VALID_BODY.to_string())),
    );
    let payload = read_windows_keyring(&keyring).unwrap();
    assert_eq!(payload, VALID_BODY);
    assert_eq!(keyring.read_targets().len(), 2);
}

#[test]
fn windows_no_matching_target_is_no_entry() {
    for (primary, compound) in [
        (
            Err(KeyringReadError::NoEntry),
            Err(KeyringReadError::NoEntry),
        ),
        (
            Ok(WindowsTargetOutcome::NotMatched),
            Err(KeyringReadError::NoEntry),
        ),
        (
            Ok(WindowsTargetOutcome::NotMatched),
            Ok(WindowsTargetOutcome::NotMatched),
        ),
    ] {
        let keyring = FakeWindowsKeyring::new(primary, compound);
        let error = read_windows_keyring(&keyring).unwrap_err();
        assert!(matches!(error, KeyringReadError::NoEntry));
        assert_eq!(keyring.read_targets().len(), 2);
    }
}

#[test]
fn windows_attributes_backend_failure_is_unavailable_and_stops_the_walk() {
    // A locked or unreadable attributes call must not read as "no entry",
    // and the compound target must not be tried afterwards.
    let keyring = FakeWindowsKeyring::new(
        Err(KeyringReadError::Unavailable),
        Ok(WindowsTargetOutcome::Matched("unreachable".to_string())),
    );
    let error = read_windows_keyring(&keyring).unwrap_err();
    assert!(matches!(error, KeyringReadError::Unavailable));
    assert_eq!(keyring.read_targets(), vec![KEYRING_SERVICE.to_string()]);
}
