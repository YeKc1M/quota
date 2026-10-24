use super::*;
use serde_json::json;

fn temp_storage_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("quota-kimi-{}-{}", name, std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn usage(remaining: i32) -> KimiUsage {
    KimiUsage {
        windows: vec![KimiUsageWindow {
            label: "5h".to_string(),
            remaining_percent: Some(remaining),
            reset_at: Some(1_790_957_937),
        }],
        note: None,
    }
}

fn stored() -> StoredKimiAccount {
    build_local_account(usage(82))
}

#[test]
fn single_account_upsert_reuses_the_same_identity() {
    let dir = temp_storage_dir("upsert");
    let first = upsert_account_in(&dir, stored()).unwrap();
    assert_eq!(first.id, LOCAL_ACCOUNT_ID);
    assert_eq!(first.label, LOCAL_ACCOUNT_LABEL);

    let mut reimport = stored();
    reimport.usage = usage(40);
    let second = upsert_account_in(&dir, reimport).unwrap();
    assert_eq!(second.id, first.id);
    assert_eq!(second.created_at, first.created_at);
    assert_eq!(second.usage.windows[0].remaining_percent, Some(40));

    let listed = list_accounts_in(&dir).unwrap();
    assert_eq!(listed.len(), 1);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn stored_account_never_contains_a_token() {
    let dir = temp_storage_dir("no-token");
    let account = upsert_account_in(&dir, stored()).unwrap();
    let raw = fs::read_to_string(account_path_in(&dir, &account.id)).unwrap();
    assert!(!raw.contains("access_token"));
    assert!(!raw.contains("accessToken"));
    assert!(!raw.contains("refresh_token"));
    assert!(!raw.contains("token"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn serializes_usage_cache_and_refresh_errors() {
    let dir = temp_storage_dir("errors");
    let account = upsert_account_in(&dir, stored()).unwrap();
    assert_eq!(account.usage.windows[0].label, "5h");
    assert_eq!(account.usage.windows[0].reset_at, Some(1_790_957_937));

    let mut errored = load_account_in(&dir, &account.id).unwrap();
    errored.usage = usage(12);
    errored.quota_query_last_error = Some("token-secret leaked?".to_string());
    errored.quota_query_last_error_at = Some(42);
    let summary = upsert_account_in(&dir, errored).unwrap();
    assert_eq!(summary.quota_query_last_error.as_deref(), Some("token-secret leaked?"));
    assert_eq!(summary.quota_query_last_error_at, Some(42));
    // A recorded error keeps the last successful usage cache intact.
    assert_eq!(summary.usage.windows[0].remaining_percent, Some(12));

    let raw = fs::read_to_string(account_path_in(&dir, &account.id)).unwrap();
    let reparsed: KimiAccountSummary = serde_json::from_str(&raw).map(|account: StoredKimiAccount| account.to_summary()).unwrap();
    assert_eq!(reparsed.usage.windows[0].remaining_percent, Some(12));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn parse_errors_report_position_without_quoting_content() {
    let dir = temp_storage_dir("parse");
    fs::create_dir_all(account_path_in(&dir, LOCAL_ACCOUNT_ID).parent().unwrap()).unwrap();
    fs::write(account_path_in(&dir, LOCAL_ACCOUNT_ID), json!({"access_token": "secret-value"}).to_string()).unwrap();
    let error = load_account_in(&dir, LOCAL_ACCOUNT_ID).unwrap_err();
    assert!(error.contains("line"));
    assert!(!error.contains("secret-value"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn lists_and_deletes_accounts() {
    let dir = temp_storage_dir("delete");
    upsert_account_in(&dir, stored()).unwrap();
    assert_eq!(list_accounts_in(&dir).unwrap().len(), 1);

    delete_account_in(&dir, LOCAL_ACCOUNT_ID).unwrap();
    assert!(list_accounts_in(&dir).unwrap().is_empty());
    assert!(!account_path_in(&dir, LOCAL_ACCOUNT_ID).exists());

    // Deleting an unknown account is a no-op, not an error.
    delete_account_in(&dir, "missing").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn stored_files_are_private_on_unix() {
    let dir = temp_storage_dir("private");
    let account = upsert_account_in(&dir, stored()).unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(account_path_in(&dir, &account.id))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        let index_mode = fs::metadata(index_path_in(&dir))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(index_mode & 0o777, 0o600);
    }
    let _ = fs::remove_dir_all(&dir);
}
