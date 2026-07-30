//! One or more vaults served by a single MCP process.
//!
//! Serving several vaults at once is what makes "shared vaults" workable: a team
//! vault and a personal vault can be mounted side by side and addressed by name,
//! without running a server per folder.

use std::path::{Path, PathBuf};

use int_vault::{Vault, VaultIndex};

#[derive(Debug)]
pub struct VaultEntry {
    vault: Vault,
    index: Option<VaultIndex>,
    /// Fingerprint the cached index was built from.
    fingerprint: u64,
}

impl VaultEntry {
    pub fn vault(&self) -> &Vault {
        &self.vault
    }

    pub fn name(&self) -> String {
        self.vault.name()
    }

    /// The link index, rebuilt only when the vault has changed on disk.
    ///
    /// Files can change under us at any moment — the desktop app, another agent,
    /// or a `git pull` — so freshness is checked on every access rather than
    /// trusting a cache that has no way to know.
    pub fn index(&mut self) -> &VaultIndex {
        let current = self.vault.fingerprint();
        if self.index.is_none() || self.fingerprint != current {
            self.index = Some(VaultIndex::build(&self.vault));
            self.fingerprint = current;
        }
        self.index.as_ref().expect("index built above")
    }

    /// Drop the cached index after a write, so the next read reflects it.
    pub fn invalidate(&mut self) {
        self.index = None;
        self.fingerprint = 0;
    }
}

pub struct Workspace {
    vaults: Vec<VaultEntry>,
    /// When true, the vault is whichever one the desktop app has open, resolved
    /// on every call rather than fixed at launch.
    follow_app: bool,
}

impl Workspace {
    /// Track whichever vault the desktop app currently has open.
    ///
    /// This is what makes the agent and the app agree without the user keeping
    /// the path in two places. Resolution happens per call, so switching vaults
    /// in the app switches the agent too, with no restart.
    pub fn follow_app() -> Self {
        Workspace { vaults: Vec::new(), follow_app: true }
    }

    /// Open every configured root. Fails if any of them is unusable, so a typo
    /// in the client config surfaces at startup instead of mid-conversation.
    pub fn open(roots: &[PathBuf]) -> Result<Self, String> {
        if roots.is_empty() {
            return Err("no vault configured: pass a vault path or set INT_KNOWLEDGE_VAULT".into());
        }
        let mut vaults = Vec::new();
        for root in roots {
            let vault = Vault::open(root).map_err(|err| format!("{}: {err}", root.display()))?;
            vaults.push(VaultEntry { vault, index: None, fingerprint: 0 });
        }

        let mut names: Vec<String> = vaults.iter().map(|entry| entry.name().to_lowercase()).collect();
        names.sort();
        let unique = names.len();
        names.dedup();
        if names.len() != unique {
            return Err("vault folder names must be unique so tools can address them by name".into());
        }

        Ok(Workspace { vaults, follow_app: false })
    }

    pub fn follows_app(&self) -> bool {
        self.follow_app
    }

    pub fn is_single(&self) -> bool {
        // In follow mode there is only ever one vault, so the `vault` argument
        // stays optional even before the app has reported one.
        self.follow_app || self.vaults.len() == 1
    }

    pub fn names(&self) -> Vec<String> {
        self.vaults.iter().map(|entry| entry.name()).collect()
    }

    pub fn entries(&self) -> &[VaultEntry] {
        &self.vaults
    }

    /// Pick the vault a tool call refers to.
    ///
    /// With a single vault configured the argument is optional; with several it
    /// is required, and the error names the valid choices.
    pub fn select(&mut self, requested: Option<&str>) -> Result<&mut VaultEntry, String> {
        if self.follow_app {
            self.sync_with_app()?;
        }
        match requested {
            None => {
                if self.vaults.len() == 1 {
                    return Ok(&mut self.vaults[0]);
                }
                Err(format!(
                    "`vault` is required when several vaults are open. Available: {}",
                    self.names().join(", ")
                ))
            }
            Some(name) => {
                let needle = name.trim();
                let position = self
                    .vaults
                    .iter()
                    .position(|entry| entry.name().eq_ignore_ascii_case(needle))
                    .or_else(|| {
                        // Also accept a full path, which is what a client config holds.
                        let as_path = Path::new(needle);
                        self.vaults.iter().position(|entry| entry.vault().root() == as_path)
                    });
                match position {
                    Some(index) => Ok(&mut self.vaults[index]),
                    None => Err(format!("unknown vault `{needle}`. Available: {}", self.names().join(", "))),
                }
            }
        }
    }

    /// Point at whatever vault the app has open, opening or swapping as needed.
    ///
    /// The error text is written for a model to act on: if no vault is open
    /// there is nothing the agent can do except tell the user to open one.
    fn sync_with_app(&mut self) -> Result<(), String> {
        let Some(active) = int_vault::app_state::active_vault() else {
            self.vaults.clear();
            return Err(concat!(
                "No vault is open. This server follows whichever vault Intentio Knowledge has open, ",
                "and the app either is not running or has no vault open. Ask the user to open one, ",
                "or configure the server with an explicit vault path."
            )
            .into());
        };

        // Already pointed at it — keep the entry so its cached index survives.
        if self.vaults.first().map(|entry| entry.vault().root() == active.as_path()).unwrap_or(false) {
            return Ok(());
        }

        let vault = Vault::open(&active).map_err(|err| format!("{}: {err}", active.display()))?;
        self.vaults = vec![VaultEntry { vault, index: None, fingerprint: 0 }];
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("int-knowledge-ws-{}-{}", std::process::id(), name))
            .join(name);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn single_vault_needs_no_name() {
        let root = temp_root("solo");
        let mut workspace = Workspace::open(&[root]).unwrap();
        assert!(workspace.select(None).is_ok());
    }

    #[test]
    fn multiple_vaults_require_a_name() {
        let mut workspace = Workspace::open(&[temp_root("team"), temp_root("personal")]).unwrap();
        let err = workspace.select(None).unwrap_err();
        assert!(err.contains("team") && err.contains("personal"));
        assert_eq!(workspace.select(Some("TEAM")).unwrap().name(), "team");
        assert!(workspace.select(Some("nope")).is_err());
    }

    #[test]
    fn rejects_duplicate_vault_names() {
        let a = temp_root("dupe");
        let b = std::env::temp_dir().join(format!("int-knowledge-ws-other-{}", std::process::id())).join("dupe");
        let _ = fs::remove_dir_all(&b);
        fs::create_dir_all(&b).unwrap();
        assert!(Workspace::open(&[a, b]).is_err());
    }

    #[test]
    fn missing_root_fails_at_startup() {
        assert!(Workspace::open(&[PathBuf::from("/definitely/not/here")]).is_err());
    }

    /// Follow mode is the default configuration, so its behaviour is pinned here:
    /// no vault open is a clear error, and switching vaults in the app switches
    /// the server without a restart.
    #[test]
    fn follows_whichever_vault_the_app_reports() {
        let _guard = crate::HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let scratch = std::env::temp_dir().join(format!("int-ws-follow-{}", std::process::id()));
        let _ = fs::remove_dir_all(&scratch);
        fs::create_dir_all(&scratch).unwrap();
        std::env::set_var("HOME", &scratch);

        let first = scratch.join("First");
        let second = scratch.join("Second");
        fs::create_dir_all(&first).unwrap();
        fs::create_dir_all(&second).unwrap();
        fs::write(first.join("A.md"), "# A\n").unwrap();
        fs::write(second.join("B.md"), "# B\n").unwrap();
        fs::write(second.join("C.md"), "# C\n").unwrap();

        let mut workspace = Workspace::follow_app();
        assert!(workspace.follows_app());
        // The `vault` argument stays optional even before a vault is reported.
        assert!(workspace.is_single());

        // Nothing open yet: the error has to tell the model what to do.
        let err = workspace.select(None).unwrap_err();
        assert!(err.contains("No vault is open"), "{err}");

        int_vault::app_state::write_active_vault(Some(&first), 1).unwrap();
        assert_eq!(workspace.select(None).unwrap().name(), "First");
        assert_eq!(workspace.select(None).unwrap().index().len(), 1);

        // The app switches vault — the server must follow, with no restart.
        int_vault::app_state::write_active_vault(Some(&second), 2).unwrap();
        assert_eq!(workspace.select(None).unwrap().name(), "Second");
        assert_eq!(workspace.select(None).unwrap().index().len(), 2);

        // The app closes its vault.
        int_vault::app_state::write_active_vault(None, 3).unwrap();
        assert!(workspace.select(None).is_err());

        // A vault the app reported but which has since been deleted.
        int_vault::app_state::write_active_vault(Some(&first), 4).unwrap();
        fs::remove_dir_all(&first).unwrap();
        assert!(workspace.select(None).is_err(), "a deleted vault must not resolve");
    }

    #[test]
    fn index_rebuilds_after_external_change() {
        let root = temp_root("fresh");
        let mut workspace = Workspace::open(&[root.clone()]).unwrap();
        assert_eq!(workspace.select(None).unwrap().index().len(), 0);
        fs::write(root.join("A.md"), "# A\n").unwrap();
        assert_eq!(workspace.select(None).unwrap().index().len(), 1);
    }
}
