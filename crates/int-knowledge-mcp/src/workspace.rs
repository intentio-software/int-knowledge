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
}

impl Workspace {
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

        Ok(Workspace { vaults })
    }

    pub fn is_single(&self) -> bool {
        self.vaults.len() == 1
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

    #[test]
    fn index_rebuilds_after_external_change() {
        let root = temp_root("fresh");
        let mut workspace = Workspace::open(&[root.clone()]).unwrap();
        assert_eq!(workspace.select(None).unwrap().index().len(), 0);
        fs::write(root.join("A.md"), "# A\n").unwrap();
        assert_eq!(workspace.select(None).unwrap().index().len(), 1);
    }
}
