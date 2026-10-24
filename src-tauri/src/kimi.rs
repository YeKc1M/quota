use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const DATA_DIR: &str = ".quota";
const ACCOUNTS_DIR: &str = "kimi_accounts";
const ACCOUNTS_INDEX_FILE: &str = "kimi_accounts.json";
// Kimi Code has a single local login session, so Quota keeps exactly one
// account that mirrors it. Re-importing reconnects the same account.
const LOCAL_ACCOUNT_ID: &str = "kimi_local";
const LOCAL_ACCOUNT_LABEL: &str = "Kimi Code";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KimiUsageWindow {
    pub label: String,
    pub remaining_percent: Option<i32>,
    /// Unix seconds, matching the other providers' reset fields.
    pub reset_at: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KimiUsage {
    pub windows: Vec<KimiUsageWindow>,
    /// What to show when the account is connected but has no usage numbers.
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredKimiAccount {
    id: String,
    label: String,
    usage: KimiUsage,
    quota_query_last_error: Option<String>,
    quota_query_last_error_at: Option<i64>,
    usage_updated_at: Option<i64>,
    created_at: i64,
    last_used: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KimiAccountSummary {
    pub id: String,
    pub label: String,
    pub usage: KimiUsage,
    pub quota_query_last_error: Option<String>,
    pub quota_query_last_error_at: Option<i64>,
    pub usage_updated_at: Option<i64>,
    pub created_at: i64,
    pub last_used: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KimiAccountIndex {
    version: String,
    account_ids: Vec<String>,
}

impl KimiAccountIndex {
    fn new() -> Self {
        Self {
            version: "1.0".to_string(),
            account_ids: Vec::new(),
        }
    }
}

impl StoredKimiAccount {
    fn to_summary(&self) -> KimiAccountSummary {
        KimiAccountSummary {
            id: self.id.clone(),
            label: self.label.clone(),
            usage: self.usage.clone(),
            quota_query_last_error: self.quota_query_last_error.clone(),
            quota_query_last_error_at: self.quota_query_last_error_at,
            usage_updated_at: self.usage_updated_at,
            created_at: self.created_at,
            last_used: self.last_used,
        }
    }
}

// ── Tauri commands ──────────────────────────────────────────────────────────

#[tauri::command]
pub fn list_kimi_accounts() -> Result<Vec<KimiAccountSummary>, String> {
    list_accounts_in(&quota_storage_dir()?)
}

#[tauri::command]
pub async fn import_kimi_from_local() -> Result<KimiAccountSummary, String> {
    // Fetch first so a missing or expired local session never becomes an account.
    let usage = fetch_local_usage().await?;
    upsert_account_in(&quota_storage_dir()?, build_local_account(usage))
}

#[tauri::command]
pub async fn refresh_kimi_account(account_id: String) -> Result<KimiAccountSummary, String> {
    refresh_account_in(&quota_storage_dir()?, &account_id).await
}

#[tauri::command]
pub async fn refresh_all_kimi_accounts() -> Result<Vec<KimiAccountSummary>, String> {
    let storage_dir = quota_storage_dir()?;
    for account_id in load_index_in(&storage_dir)?.account_ids {
        let _ = refresh_account_in(&storage_dir, &account_id).await;
    }
    list_accounts_in(&storage_dir)
}

#[tauri::command]
pub fn delete_kimi_account(account_id: String) -> Result<(), String> {
    // Only Quota's account summary goes away; Kimi Code's own credentials are
    // never read write or deleted here.
    delete_account_in(&quota_storage_dir()?, &account_id)
}

// ── Usage fetch (delegates to quota-core) ───────────────────────────────────

async fn fetch_local_usage() -> Result<KimiUsage, String> {
    let usage = quota_core::kimi::local_usage::fetch_local_usage()
        .await
        .map_err(|error| error.to_string())?;
    Ok(KimiUsage {
        windows: usage
            .windows
            .into_iter()
            .map(|window| KimiUsageWindow {
                label: window.label,
                remaining_percent: window.remaining_percent,
                reset_at: window.reset_at,
            })
            .collect(),
        note: usage.note,
    })
}

fn build_local_account(usage: KimiUsage) -> StoredKimiAccount {
    let now = now_timestamp();
    StoredKimiAccount {
        id: LOCAL_ACCOUNT_ID.to_string(),
        label: LOCAL_ACCOUNT_LABEL.to_string(),
        usage,
        quota_query_last_error: None,
        quota_query_last_error_at: None,
        usage_updated_at: Some(now_timestamp_ms()),
        created_at: now,
        last_used: now,
    }
}

// ── Storage helpers (mirrors opencode_go's private atomic layout) ───────────

fn now_timestamp() -> i64 {
    chrono::Utc::now().timestamp()
}

fn now_timestamp_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn quota_storage_dir() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or_else(|| "Could not locate home directory".to_string())?;
    let dir = home.join(DATA_DIR);
    fs::create_dir_all(&dir).map_err(|e| format!("Could not create Quota data directory: {}", e))?;
    Ok(dir)
}

fn index_path_in(storage_dir: &Path) -> PathBuf {
    storage_dir.join(ACCOUNTS_INDEX_FILE)
}

fn account_path_in(storage_dir: &Path, account_id: &str) -> PathBuf {
    storage_dir
        .join(ACCOUNTS_DIR)
        .join(format!("{}.json", account_id))
}

/// Write through a temp file, readable only by the owner on Unix.
fn write_private_atomic(path: &Path, content: &str) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "No parent directory".to_string())?;
    fs::create_dir_all(parent).map_err(|e| format!("Could not create directory: {}", e))?;
    let tmp = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("kimi"),
        std::process::id()
    ));
    fs::write(&tmp, content).map_err(|e| format!("Could not write temp file: {}", e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("Could not set file permissions: {}", e))?;
    }
    fs::rename(&tmp, path).map_err(|e| format!("Could not replace file: {}", e))?;
    Ok(())
}

fn load_index_in(storage_dir: &Path) -> Result<KimiAccountIndex, String> {
    let path = index_path_in(storage_dir);
    if !path.exists() {
        return Ok(KimiAccountIndex::new());
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Could not read Kimi account index: {}", e))?;
    if content.trim().is_empty() {
        return Ok(KimiAccountIndex::new());
    }
    serde_json::from_str(&content)
        .map_err(|e| format!("Could not parse Kimi account index: {}", e))
}

fn save_index_in(storage_dir: &Path, index: &KimiAccountIndex) -> Result<(), String> {
    let content = serde_json::to_string_pretty(index)
        .map_err(|e| format!("Could not encode Kimi account index: {}", e))?;
    write_private_atomic(&index_path_in(storage_dir), &content)
}

fn load_account_in(storage_dir: &Path, account_id: &str) -> Result<StoredKimiAccount, String> {
    let content = fs::read_to_string(account_path_in(storage_dir, account_id))
        .map_err(|e| format!("Could not read Kimi account: {}", e))?;
    serde_json::from_str(&content)
        .map_err(|e| format!("Could not parse Kimi account (line {}, column {}).", e.line(), e.column()))
}

fn save_account_in(storage_dir: &Path, account: &StoredKimiAccount) -> Result<(), String> {
    let content = serde_json::to_string_pretty(account)
        .map_err(|e| format!("Could not encode Kimi account: {}", e))?;
    write_private_atomic(&account_path_in(storage_dir, &account.id), &content)
}

fn list_accounts_in(storage_dir: &Path) -> Result<Vec<KimiAccountSummary>, String> {
    let index = load_index_in(storage_dir)?;
    Ok(index
        .account_ids
        .iter()
        .filter_map(|id| load_account_in(storage_dir, id).ok())
        .map(|account| account.to_summary())
        .collect())
}

fn upsert_account_in(
    storage_dir: &Path,
    mut account: StoredKimiAccount,
) -> Result<KimiAccountSummary, String> {
    let mut index = load_index_in(storage_dir)?;
    if let Some(existing) = load_account_in(storage_dir, &account.id).ok() {
        account.created_at = existing.created_at;
    }
    account.last_used = now_timestamp();
    save_account_in(storage_dir, &account)?;
    if !index.account_ids.iter().any(|id| id == &account.id) {
        index.account_ids.insert(0, account.id.clone());
    }
    save_index_in(storage_dir, &index)?;
    Ok(account.to_summary())
}

fn delete_account_in(storage_dir: &Path, account_id: &str) -> Result<(), String> {
    let mut index = load_index_in(storage_dir)?;
    index.account_ids.retain(|id| id != account_id);
    save_index_in(storage_dir, &index)?;
    let path = account_path_in(storage_dir, account_id);
    if path.exists() {
        fs::remove_file(&path).map_err(|e| format!("Could not delete Kimi account: {}", e))?;
    }
    Ok(())
}

async fn refresh_account_in(
    storage_dir: &Path,
    account_id: &str,
) -> Result<KimiAccountSummary, String> {
    let mut account = load_account_in(storage_dir, account_id)?;
    match fetch_local_usage().await {
        Ok(usage) => {
            account.usage = usage;
            account.quota_query_last_error = None;
            account.quota_query_last_error_at = None;
            account.usage_updated_at = Some(now_timestamp_ms());
            account.last_used = now_timestamp();
        }
        Err(message) => {
            // Keep the last successful usage and only record what went wrong.
            account.quota_query_last_error = Some(message);
            account.quota_query_last_error_at = Some(now_timestamp_ms());
        }
    }
    upsert_account_in(storage_dir, account)
}

#[cfg(test)]
#[path = "kimi_tests.rs"]
mod tests;
