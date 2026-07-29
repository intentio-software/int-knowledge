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

    #[error("heading not found: {0}")]
    HeadingNotFound(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, VaultError>;
