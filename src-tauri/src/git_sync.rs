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
    sync_with(vault, true)
}

/// Sync without committing: pull the other side's work and push anything
/// already committed here, but leave the working tree alone.
///
/// This is what runs on the interval. Receiving is cheap and wants to be
/// frequent; committing is what fills the history and wants to wait until the
/// writing has stopped.
pub fn receive(vault: &Path) -> SyncOutcome {
    sync_with(vault, false)
}

fn sync_with(vault: &Path, commit_local: bool) -> SyncOutcome {
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

    if before.dirty && commit_local {
        if let Err(err) = git(vault, &["add", "-A"]) {
            return failed(&format!("Could not stage changes: {err}"));
        }
        let (subject, body) = commit_message(vault);
        if let Err(err) = git(vault, &["commit", "-m", &subject, "-m", &body]) {
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

/// The subject and body for one sync commit.
///
/// Git records its own timestamp, but `git log --oneline` — the view people
/// actually read — does not show it, so a run of automatic commits becomes a
/// wall of identical subjects. The time and the count go in the subject; the
/// files go in the body, where `git show` will find them.
fn commit_message(vault: &Path) -> (String, String) {
    let staged = git(vault, &["diff", "--cached", "--name-only"]).unwrap_or_default();
    let files: Vec<&str> = staged.lines().filter(|line| !line.trim().is_empty()).collect();
    let when = chrono::Local::now().format("%Y-%m-%d %H:%M");

    let subject = match files.len() {
        0 => format!("Vault sync {when}"),
        1 => format!("Vault sync {when} — {}", short_name(files[0])),
        n => format!("Vault sync {when} — {n} files"),
    };
    (subject, files.join("\n"))
}

/// A note's name without its folders or extension, for the subject line.
fn short_name(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).trim_end_matches(".md").to_string()
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

/// How still the vault must be before an automatic commit is made. A pause this
/// long usually means a thought was finished, which is the right unit for a
/// commit — and it collapses a whole writing session into one.
const QUIET_SECONDS: u64 = 120;

/// The longest the vault may stay uncommitted while being edited continuously.
/// Without this, someone writing all afternoon would sync nothing to anyone.
const MAX_UNCOMMITTED_SECONDS: u64 = 1_800;

/// Run the enabled vault's sync on its own schedule, for as long as the app is open.
///
/// One thread, waking often and doing nothing most of the time. It reads the
/// settings on every tick rather than caching them, so turning sync off takes
/// effect immediately instead of after the current interval.
pub fn spawn<R: tauri::Runtime>(app: tauri::AppHandle<R>) {
    std::thread::spawn(move || {
        let mut last_receive = std::time::Instant::now() - std::time::Duration::from_secs(3_600);
        let mut fingerprint = String::new();
        let mut unchanged_since: Option<std::time::Instant> = None;
        let mut dirty_since: Option<std::time::Instant> = None;

        loop {
            std::thread::sleep(std::time::Duration::from_secs(15));

            let Some(vault) = int_vault::app_state::active_vault() else { continue };
            let settings = int_vault::app_state::sync_settings(&vault);
            if !settings.enabled {
                continue;
            }

            // What the working tree looks like right now. Any difference means
            // the writing is still going on.
            let current = git(&vault, &["status", "--porcelain"]).unwrap_or_default();
            if current != fingerprint {
                fingerprint = current.clone();
                unchanged_since = Some(std::time::Instant::now());
            }
            if current.is_empty() {
                dirty_since = None;
            } else if dirty_since.is_none() {
                dirty_since = Some(std::time::Instant::now());
            }

            // Commit once the vault has been still for a while, so a long
            // writing session becomes one commit rather than twenty. The
            // backstop stops a continuously edited vault from never committing
            // at all, which would leave the other person seeing nothing.
            let settled = unchanged_since
                .map(|at| at.elapsed().as_secs() >= QUIET_SECONDS)
                .unwrap_or(false);
            let overdue = dirty_since
                .map(|at| at.elapsed().as_secs() >= MAX_UNCOMMITTED_SECONDS)
                .unwrap_or(false);
            let should_commit = !current.is_empty() && (settled || overdue);

            let due_to_receive = last_receive.elapsed().as_secs() >= settings.interval_seconds;
            if !should_commit && !due_to_receive {
                continue;
            }

            let outcome = sync_with(&vault, should_commit);
            last_receive = std::time::Instant::now();
            if should_commit {
                dirty_since = None;
            }
            // A blocked sync is reported once and then left alone: retrying a
            // conflict every few minutes would fill the log and fix nothing.
            let _ = tauri::Emitter::emit(&app, SYNC_EVENT, &outcome);
        }
    });
}
