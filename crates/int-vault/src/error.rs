use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("vault root does not exist: {0}")]
    MissingRoot(PathBuf),

    #[error("vault root is not a directory: {0}")]
    RootNotADirectory(PathBuf),

    #[error("path escapes the vault: {0}")]
    OutsideVault(String),

    #[error("note not found: {0}")]
    NoteNotFound(String),

    #[error("a note already exists at: {0}")]
    NoteExists(String),

    #[error("invalid note path: {0}")]
    InvalidPath(String),

    #[error("folder not found: {0}")]
    FolderNotFound(String),

    #[error("something already exists at: {0}")]
    PathExists(String),

    #[error("a folder cannot be moved inside itself: {0}")]
    FolderIntoItself(String),

    #[error("heading not found: {0}")]
    HeadingNotFound(String),

    /// The file exists but its contents are not on this machine — evicted to
    /// iCloud or another cloud provider. Reading it would block until the
    /// download finished, which may be never.
    #[error("note is not downloaded to this machine: {0}")]
    NotMaterialized(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, VaultError>;
