//! The handshake between the desktop app and the MCP server.
//!
//! Both processes are independent — the server runs whether or not the app is
//! open — but when both are running they should be looking at the same vault.
//! The app records which vault is open in a small file under the user's home
//! directory, and the server reads it. A plain file is the right medium: no
//! ports, no daemon, no ordering requirement between the two processes, and the
//! user can inspect or delete it.
//!
//! Location is `~/.intentio/knowledge/state.json`, chosen over the platform
//! config directory so that both a Tauri app and a bare CLI binary can compute
//! the same path without agreeing on a path-resolution library.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Serialized form of the shared state file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppState {
    /// Absolute path of the vault currently open in the app, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_vault: Option<String>,
    /// Recently opened vaults, most recent first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_vaults: Vec<String>,
    /// When the app last wrote this, in milliseconds since the Unix epoch.
    #[serde(default)]
    pub updated_at: u64,
    /// Git sync preferences, keyed by absolute vault path.
    ///
    /// Kept here rather than inside the vault because it is a preference about
    /// a folder on this machine, not content to be shared with everyone who
    /// clones it — the other person may well want a different interval, or
    /// none at all.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub sync: std::collections::BTreeMap<String, VaultSync>,
}

/// How one vault should keep itself in step with its remote.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultSync {
    pub enabled: bool,
    /// How often to sync while the app is open.
    #[serde(default = "default_interval_seconds")]
    pub interval_seconds: u64,
}

fn default_interval_seconds() -> u64 {
    180
}

impl Default for VaultSync {
    fn default() -> Self {
        // Three minutes: often enough that the other person is not waiting,
        // rare enough that the history stays readable.
        VaultSync { enabled: false, interval_seconds: default_interval_seconds() }
    }
}

/// Directory holding the shared state.
pub fn state_dir() -> Option<PathBuf> {
    home().map(|home| home.join(".intentio").join("knowledge"))
}

/// Path of the shared state file.
pub fn state_path() -> Option<PathBuf> {
    state_dir().map(|dir| dir.join("state.json"))
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
}

/// Read the shared state, or `None` when the app has never written it.
pub fn read() -> Option<AppState> {
    let path = state_path()?;
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// The vault the app currently has open, if it still exists on disk.
///
/// A stale path — the folder was moved or deleted since the app wrote it — is
/// treated as absent rather than returned, so callers never try to open it.
pub fn active_vault() -> Option<PathBuf> {
    let state = read()?;
    let path = PathBuf::from(state.active_vault?);
    path.is_dir().then_some(path)
}

/// Record the vault the app has open. `None` clears it.
///
/// Writes are skipped when nothing changed, because the app re-reads its vault
/// summary on every save and would otherwise rewrite this file constantly.
pub fn write_active_vault(vault: Option<&Path>, now_millis: u64) -> std::io::Result<()> {
    let Some(dir) = state_dir() else {
        return Ok(());
    };
    let path = dir.join("state.json");

    let mut state = read().unwrap_or_default();
    let next = vault.map(|p| p.to_string_lossy().to_string());
    if state.active_vault == next {
        return Ok(());
    }

    // Keep a short history so a future launcher can offer it without the app.
    if let Some(current) = &next {
        state.recent_vaults.retain(|entry| entry != current);
        state.recent_vaults.insert(0, current.clone());
        state.recent_vaults.truncate(8);
    }
    state.active_vault = next;
    state.updated_at = now_millis;

    fs::create_dir_all(&dir)?;
    let json = serde_json::to_string_pretty(&state)?;
    fs::write(path, format!("{json}\n"))
}

/// Read one vault's sync preference, falling back to the default (off).
pub fn sync_settings(vault: &Path) -> VaultSync {
    let key = vault.to_string_lossy().to_string();
    read().and_then(|state| state.sync.get(&key).cloned()).unwrap_or_default()
}

/// Record how a vault should sync.
pub fn write_sync_settings(vault: &Path, settings: VaultSync, now_millis: u64) -> std::io::Result<()> {
    let Some(dir) = state_dir() else {
        return Ok(());
    };
    let mut state = read().unwrap_or_default();
    state.sync.insert(vault.to_string_lossy().to_string(), settings);
    state.updated_at = now_millis;

    fs::create_dir_all(&dir)?;
    let json = serde_json::to_string_pretty(&state)?;
    fs::write(dir.join("state.json"), format!("{json}\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Point HOME at a scratch directory for the duration of a test.
    ///
    /// These tests share process-global state, so they run under one test
    /// function rather than racing each other across threads.
    #[test]
    fn round_trips_the_active_vault() {
        let scratch = std::env::temp_dir().join(format!("int-vault-state-{}", std::process::id()));
        let _ = fs::remove_dir_all(&scratch);
        fs::create_dir_all(&scratch).unwrap();
        std::env::set_var("HOME", &scratch);

        // Nothing written yet.
        assert!(read().is_none());
        assert!(active_vault().is_none());

        let vault = scratch.join("Notes");
        fs::create_dir_all(&vault).unwrap();
        write_active_vault(Some(&vault), 1000).unwrap();

        let state = read().expect("state written");
        assert_eq!(state.active_vault.as_deref(), Some(vault.to_string_lossy().as_ref()));
        assert_eq!(state.recent_vaults.len(), 1);
        assert_eq!(state.updated_at, 1000);
        assert_eq!(active_vault().as_deref(), Some(vault.as_path()));

        // Writing the same value again must not bump the timestamp.
        write_active_vault(Some(&vault), 2000).unwrap();
        assert_eq!(read().unwrap().updated_at, 1000);

        // A second vault moves to the front of the history.
        let other = scratch.join("Team");
        fs::create_dir_all(&other).unwrap();
        write_active_vault(Some(&other), 3000).unwrap();
        let state = read().unwrap();
        assert_eq!(state.recent_vaults.len(), 2);
        assert_eq!(state.recent_vaults[0], other.to_string_lossy());
        assert_eq!(active_vault().as_deref(), Some(other.as_path()));

        // Clearing keeps the history but drops the active vault.
        write_active_vault(None, 4000).unwrap();
        assert!(read().unwrap().active_vault.is_none());
        assert_eq!(read().unwrap().recent_vaults.len(), 2);
        assert!(active_vault().is_none());

        // A vault that no longer exists reads as absent.
        let ghost = scratch.join("Gone");
        fs::create_dir_all(&ghost).unwrap();
        write_active_vault(Some(&ghost), 5000).unwrap();
        fs::remove_dir_all(&ghost).unwrap();
        assert!(active_vault().is_none(), "a deleted vault must not be reported as active");

        // Sync settings live alongside, keyed per vault, and must survive a
        // round trip without disturbing the active vault.
        assert!(!sync_settings(&vault).enabled, "off until asked for");
        assert_eq!(sync_settings(&vault).interval_seconds, 180, "three minutes by default");

        // Writing a preference must not disturb anything else in the file.
        let before = read().unwrap();
        write_sync_settings(&vault, VaultSync { enabled: true, interval_seconds: 900 }, 6000).unwrap();
        let saved = sync_settings(&vault);
        assert!(saved.enabled);
        assert_eq!(saved.interval_seconds, 900);

        let after = read().unwrap();
        assert_eq!(after.active_vault, before.active_vault, "active vault untouched");
        assert_eq!(after.recent_vaults, before.recent_vaults, "history untouched");

        // A second vault keeps its own preference.
        let other = scratch.join("Other");
        fs::create_dir_all(&other).unwrap();
        assert!(!sync_settings(&other).enabled, "settings do not leak between vaults");
    }
}
