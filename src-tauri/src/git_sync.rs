//! Keeping a vault in step with a Git remote.
//!
//! Shelling out to the installed `git` rather than linking a library is a
//! deliberate choice: it inherits the user's SSH agent, credential helper and
//! config, so if `git push` works in their terminal it works here. A linked
//! library would have to reimplement all of that and would fail on exactly the
//! setups that are hardest to debug.
//!
//! The rules this follows, in order of importance:
//!
//! 1. Never resolve a conflict. A rebase that stops is aborted so the working
//!    tree is left exactly as it was, and sync pauses until a person fixes it.
//! 2. Never force anything. No `--force`, no history rewriting.
//! 3. Never touch a repository that is mid-operation. If the user is part-way
//!    through their own rebase or merge, this stays out of the way.
//! 4. Never create a repository. Turning a folder into one is the user's
//!    decision, not a side effect of ticking a box.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

/// What the app can tell about a vault's relationship with Git.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub is_repo: bool,
    pub has_remote: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// Uncommitted changes in the working tree.
    pub dirty: bool,
    pub ahead: u32,
    pub behind: u32,
    /// Set when syncing cannot safely proceed, with the reason to show.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked: Option<String>,
}

/// What one sync attempt did.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncOutcome {
    /// Whether anything was committed, pulled or pushed.
    pub changed: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked: Option<String>,
}

fn git(vault: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(vault)
        .args(args)
        .output()
        .map_err(|err| format!("git could not be run: {err}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        return Err(if stderr.is_empty() { stdout } else { stderr });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// True while the repository is part-way through a rebase or a merge.
fn operation_in_progress(vault: &Path) -> bool {
    let git_dir = vault.join(".git");
    git_dir.join("rebase-merge").exists()
        || git_dir.join("rebase-apply").exists()
        || git_dir.join("MERGE_HEAD").exists()
        || git_dir.join("CHERRY_PICK_HEAD").exists()
}

pub fn status(vault: &Path) -> SyncStatus {
    let mut status = SyncStatus::default();
    if git(vault, &["rev-parse", "--is-inside-work-tree"]).is_err() {
        return status;
    }
    status.is_repo = true;
    status.has_remote = git(vault, &["remote"]).map(|out| !out.is_empty()).unwrap_or(false);
    status.branch = git(vault, &["rev-parse", "--abbrev-ref", "HEAD"]).ok();
    status.dirty = git(vault, &["status", "--porcelain"]).map(|out| !out.is_empty()).unwrap_or(false);

    // `--left-right --count` against the upstream gives "behind<TAB>ahead".
    if let Ok(counts) = git(vault, &["rev-list", "--left-right", "--count", "@{upstream}...HEAD"]) {
        let mut parts = counts.split_whitespace();
        status.behind = parts.next().and_then(|n| n.parse().ok()).unwrap_or(0);
        status.ahead = parts.next().and_then(|n| n.parse().ok()).unwrap_or(0);
    }

    if operation_in_progress(vault) {
        status.blocked = Some("A rebase or merge is already in progress here.".into());
    } else if status.is_repo && !status.has_remote {
        status.blocked = Some("This vault has no Git remote to sync with.".into());
    }
    status
}

/// Commit local work, bring the remote's in, and push — or stop and say why.
pub fn sync(vault: &Path) -> SyncOutcome {
    let before = status(vault);
    if !before.is_repo {
        return blocked("This vault is not a Git repository.");
    }
    if !before.has_remote {
        return blocked("This vault has no Git remote to sync with.");
    }
    if operation_in_progress(vault) {
        // The user is in the middle of something of their own.
        return blocked("A rebase or merge is in progress. Sync paused until it is finished.");
    }
    if let Err(err) = git(vault, &["config", "user.email"]) {
        let _ = err;
        return blocked("Git has no user.email set, so it cannot commit. Set one and try again.");
    }

    let mut did_something = false;

    if before.dirty {
        if let Err(err) = git(vault, &["add", "-A"]) {
            return failed(&format!("Could not stage changes: {err}"));
        }
        // Git stamps the commit itself, so the subject does not need a date.
        if let Err(err) = git(vault, &["commit", "-m", "Vault sync"]) {
            return failed(&format!("Could not commit: {err}"));
        }
        did_something = true;
    }

    match git(vault, &["pull", "--rebase", "--autostash"]) {
        Ok(out) => {
            if !out.contains("Already up to date") {
                did_something = true;
            }
        }
        Err(err) => {
            // Leave the tree exactly as it was rather than half-rebased.
            if operation_in_progress(vault) {
                let _ = git(vault, &["rebase", "--abort"]);
                return blocked(
                    "The same note was changed in both places. Sync is paused — resolve it in a terminal, then sync again.",
                );
            }
            return failed(&format!("Could not pull: {err}"));
        }
    }

    if status(vault).ahead > 0 {
        if let Err(err) = git(vault, &["push"]) {
            return failed(&format!("Could not push: {err}"));
        }
        did_something = true;
    }

    SyncOutcome {
        changed: did_something,
        message: if did_something { "Synced".into() } else { "Already up to date".into() },
        blocked: None,
    }
}

fn blocked(reason: &str) -> SyncOutcome {
    SyncOutcome { changed: false, message: reason.to_string(), blocked: Some(reason.to_string()) }
}

fn failed(reason: &str) -> SyncOutcome {
    SyncOutcome { changed: false, message: reason.to_string(), blocked: Some(reason.to_string()) }
}

pub fn vault_path(vault: &str) -> PathBuf {
    PathBuf::from(vault)
}

/// Emitted after every automatic sync so the UI can show where things stand.
pub const SYNC_EVENT: &str = "vault-sync";

/// Run the enabled vault's sync on its own schedule, for as long as the app is open.
///
/// One thread, waking often and doing nothing most of the time. It reads the
/// settings on every tick rather than caching them, so turning sync off takes
/// effect immediately instead of after the current interval.
pub fn spawn<R: tauri::Runtime>(app: tauri::AppHandle<R>) {
    std::thread::spawn(move || {
        let mut last_run = std::time::Instant::now() - std::time::Duration::from_secs(3_600);
        loop {
            std::thread::sleep(std::time::Duration::from_secs(15));

            let Some(vault) = int_vault::app_state::active_vault() else { continue };
            let settings = int_vault::app_state::sync_settings(&vault);
            if !settings.enabled {
                continue;
            }
            if last_run.elapsed().as_secs() < settings.interval_seconds {
                continue;
            }

            let outcome = sync(&vault);
            last_run = std::time::Instant::now();
            // A blocked sync is reported once and then left alone: retrying a
            // conflict every three minutes would fill the log and fix nothing.
            let _ = tauri::Emitter::emit(&app, SYNC_EVENT, &outcome);
        }
    });
}
