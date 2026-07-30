//! The vault: a plain folder of markdown on the host filesystem.
//!
//! Every operation is expressed in vault-relative paths with forward slashes,
//! and every one of them is checked against the root before it touches disk.
//! Nothing here writes outside the folder the user pointed at, which is the
//! whole safety story for handing these operations to an agent.

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use walkdir::WalkDir;

use crate::error::{Result, VaultError};
use crate::note::{Note, NoteMeta};

/// Extensions treated as notes rather than attachments.
pub const NOTE_EXTENSIONS: [&str; 3] = ["md", "markdown", "mdx"];

/// Directory names never worth indexing.
const SKIPPED_DIRS: [&str; 5] = ["node_modules", "target", "dist", ".git", ".obsidian"];

/// Where trashed notes go. Dot-prefixed, so the walker already ignores it.
pub const TRASH_DIR: &str = ".trash";

/// Files larger than this are listed but not loaded into the index.
const MAX_NOTE_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct Vault {
    root: PathBuf,
}

impl Vault {
    /// Open an existing directory as a vault.
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref();
        if !root.exists() {
            return Err(VaultError::MissingRoot(root.to_path_buf()));
        }
        if !root.is_dir() {
            return Err(VaultError::RootNotADirectory(root.to_path_buf()));
        }
        // Canonicalize once so containment checks compare like with like.
        let root = fs::canonicalize(root)?;
        Ok(Vault { root })
    }

    /// Create the directory if needed, then open it.
    pub fn create(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref();
        if !root.exists() {
            fs::create_dir_all(root)?;
        }
        Self::open(root)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn name(&self) -> String {
        self.root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| self.root.to_string_lossy().to_string())
    }

    // -----------------------------------------------------------------------
    // paths
    // -----------------------------------------------------------------------

    /// Turn caller input into a safe vault-relative path.
    ///
    /// Absolute paths inside the vault are accepted and rebased; anything that
    /// climbs past the root with `..` is rejected outright.
    pub fn normalize(&self, input: &str) -> Result<String> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(VaultError::InvalidPath("path is empty".into()));
        }

        let candidate = Path::new(trimmed);
        let relative: PathBuf = if candidate.is_absolute() {
            // Compare against the canonical root; fall back to the raw prefix
            // for paths that do not exist yet.
            match candidate.strip_prefix(&self.root) {
                Ok(rest) => rest.to_path_buf(),
                Err(_) => return Err(VaultError::OutsideVault(trimmed.to_string())),
            }
        } else {
            candidate.to_path_buf()
        };

        let mut parts: Vec<String> = Vec::new();
        for component in relative.components() {
            match component {
                Component::Normal(part) => parts.push(part.to_string_lossy().to_string()),
                Component::CurDir => {}
                Component::ParentDir => {
                    if parts.pop().is_none() {
                        return Err(VaultError::OutsideVault(trimmed.to_string()));
                    }
                }
                Component::RootDir | Component::Prefix(_) => {
                    return Err(VaultError::OutsideVault(trimmed.to_string()))
                }
            }
        }

        if parts.is_empty() {
            return Err(VaultError::InvalidPath(trimmed.to_string()));
        }
        Ok(parts.join("/"))
    }

    /// Normalize, and give the path a `.md` extension when it has none.
    pub fn normalize_note(&self, input: &str) -> Result<String> {
        let path = self.normalize(input)?;
        let name = path.rsplit('/').next().unwrap_or(&path);
        let has_note_ext = name
            .rsplit_once('.')
            .map(|(_, ext)| NOTE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
            .unwrap_or(false);
        Ok(if has_note_ext { path } else { format!("{path}.md") })
    }

    /// Absolute path for a vault-relative path.
    pub fn absolute(&self, relative: &str) -> Result<PathBuf> {
        let normalized = self.normalize(relative)?;
        Ok(self.root.join(normalized.replace('/', std::path::MAIN_SEPARATOR_STR)))
    }

    pub fn exists(&self, relative: &str) -> bool {
        self.absolute(relative).map(|p| p.exists()).unwrap_or(false)
    }

    // -----------------------------------------------------------------------
    // reading
    // -----------------------------------------------------------------------

    /// Every note in the vault, sorted by path.
    pub fn list_notes(&self) -> Vec<NoteMeta> {
        self.walk()
            .filter(|entry| is_note_path(entry))
            .filter_map(|path| {
                let relative = self.relative_of(&path)?;
                self.read_note(&relative).ok().map(|note| note.meta)
            })
            .collect()
    }

    /// Notes whose contents are not on this machine.
    ///
    /// These are skipped by listing, indexing and search, so surfacing them
    /// matters: otherwise a note the user can see in Finder is silently missing
    /// from the app with no explanation.
    pub fn list_unavailable(&self) -> Vec<String> {
        self.walk()
            .filter(|path| is_note_path(path) && is_dataless(path))
            .filter_map(|path| self.relative_of(&path))
            .collect()
    }

    /// Non-note files (images, PDFs, attachments), sorted by path.
    pub fn list_attachments(&self) -> Vec<String> {
        self.walk()
            .filter(|entry| !is_note_path(entry))
            .filter_map(|path| self.relative_of(&path))
            .collect()
    }

    /// A cheap signature of the vault's current state.
    ///
    /// Walks metadata only — no file contents — so callers can skip rebuilding
    /// an index when nothing has changed on disk, including changes made by the
    /// editor, another agent, or git.
    pub fn fingerprint(&self) -> u64 {
        let mut count = 0u64;
        let mut newest = 0u64;
        let mut bytes = 0u64;
        for path in self.walk() {
            if !is_note_path(&path) {
                continue;
            }
            let (size, modified) = stat(&path);
            count += 1;
            bytes = bytes.wrapping_add(size);
            newest = newest.max(modified.unwrap_or(0));
        }
        // Mixed with distinct multipliers so a file swap of equal size still moves it.
        count.wrapping_mul(0x9E37_79B9).wrapping_add(bytes.wrapping_mul(31)).wrapping_add(newest)
    }

    /// Read and parse a note.
    pub fn read_note(&self, relative: &str) -> Result<Note> {
        let path = self.normalize_note(relative)?;
        let absolute = self.root.join(path.replace('/', std::path::MAIN_SEPARATOR_STR));
        if !absolute.is_file() {
            return Err(VaultError::NoteNotFound(path));
        }
        ensure_materialized(&absolute, &path)?;
        let content = fs::read_to_string(&absolute)?;
        let (size, modified) = stat(&absolute);
        Ok(Note::parse(&path, &content, size, modified))
    }

    /// Read a note only if it is small enough to be worth indexing.
    pub fn read_note_for_index(&self, relative: &str) -> Result<Option<Note>> {
        let absolute = self.absolute(relative)?;
        let (size, _) = stat(&absolute);
        if size > MAX_NOTE_BYTES {
            return Ok(None);
        }
        self.read_note(relative).map(Some)
    }

    /// Raw file contents, frontmatter included.
    pub fn read_raw(&self, relative: &str) -> Result<String> {
        let path = self.normalize_note(relative)?;
        let absolute = self.root.join(path.replace('/', std::path::MAIN_SEPARATOR_STR));
        if !absolute.is_file() {
            return Err(VaultError::NoteNotFound(path));
        }
        ensure_materialized(&absolute, &path)?;
        Ok(fs::read_to_string(absolute)?)
    }

    // -----------------------------------------------------------------------
    // writing
    // -----------------------------------------------------------------------

    /// Create a note, refusing to clobber an existing one.
    pub fn create_note(&self, relative: &str, content: &str) -> Result<String> {
        let path = self.normalize_note(relative)?;
        let absolute = self.root.join(path.replace('/', std::path::MAIN_SEPARATOR_STR));
        if absolute.exists() {
            return Err(VaultError::NoteExists(path));
        }
        if let Some(parent) = absolute.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&absolute, ensure_trailing_newline(content))?;
        Ok(path)
    }

    /// Write a note, creating it and any parent folders if needed.
    pub fn write_note(&self, relative: &str, content: &str) -> Result<String> {
        let path = self.normalize_note(relative)?;
        let absolute = self.root.join(path.replace('/', std::path::MAIN_SEPARATOR_STR));
        if let Some(parent) = absolute.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&absolute, ensure_trailing_newline(content))?;
        Ok(path)
    }

    /// Append to the end of a note, creating it if absent.
    pub fn append_note(&self, relative: &str, text: &str) -> Result<String> {
        let path = self.normalize_note(relative)?;
        let existing = self.read_raw(&path).unwrap_or_default();
        let mut merged = existing;
        if !merged.is_empty() && !merged.ends_with('\n') {
            merged.push('\n');
        }
        merged.push_str(text);
        self.write_note(&path, &merged)
    }

    /// Append under a specific heading, keeping the rest of the note intact.
    pub fn append_under_heading(&self, relative: &str, heading: &str, text: &str) -> Result<String> {
        let note = self.read_note(relative)?;
        let target = note
            .scan
            .headings
            .iter()
            .find(|h| h.text.eq_ignore_ascii_case(heading.trim()))
            .ok_or_else(|| VaultError::HeadingNotFound(heading.to_string()))?;

        let lines: Vec<&str> = note.body.lines().collect();
        // The section runs until the next heading at the same level or higher.
        let end = note
            .scan
            .headings
            .iter()
            .find(|h| h.line > target.line && h.level <= target.level)
            .map(|h| h.line - 1)
            .unwrap_or(lines.len());

        let mut rebuilt: Vec<String> = lines[..end].iter().map(|s| s.to_string()).collect();
        while rebuilt.last().map(|l| l.trim().is_empty()).unwrap_or(false) {
            rebuilt.pop();
        }
        rebuilt.push(String::new());
        rebuilt.extend(text.lines().map(|s| s.to_string()));
        if end < lines.len() {
            rebuilt.push(String::new());
            rebuilt.extend(lines[end..].iter().map(|s| s.to_string()));
        }

        let mut updated = note.clone();
        updated.body = format!("{}\n", rebuilt.join("\n"));
        self.write_note(&note.meta.path, &updated.to_content())
    }

    /// Move a note into the vault's `.trash` folder rather than deleting it.
    ///
    /// `.trash` begins with a dot, so the walker already skips it: the note
    /// vanishes from listing, search and the link graph exactly as a delete
    /// would, but the file is still on disk. That difference matters when the
    /// caller is an agent acting on an instruction it may have misread.
    ///
    /// Returns the note's new vault-relative path.
    pub fn trash_note(&self, relative: &str) -> Result<String> {
        let path = self.normalize_note(relative)?;
        let absolute = self.root.join(path.replace('/', std::path::MAIN_SEPARATOR_STR));
        if !absolute.is_file() {
            return Err(VaultError::NoteNotFound(path));
        }

        let trash = self.root.join(TRASH_DIR);
        fs::create_dir_all(&trash)?;

        let name = path.rsplit('/').next().unwrap_or(&path);
        let (stem, extension) = match name.rsplit_once('.') {
            Some((stem, ext)) => (stem, format!(".{ext}")),
            None => (name, String::new()),
        };

        // Two notes with the same filename can be trashed from different
        // folders, so the second must not silently replace the first.
        let mut target = trash.join(name);
        let mut suffix = 2;
        while target.exists() {
            target = trash.join(format!("{stem} {suffix}{extension}"));
            suffix += 1;
            if suffix > 999 {
                return Err(VaultError::NoteExists(format!("{TRASH_DIR}/{name}")));
            }
        }

        fs::rename(&absolute, &target)?;
        let file = target.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        Ok(format!("{TRASH_DIR}/{file}"))
    }

    /// Delete a note outright. Prefer [`Vault::trash_note`] where recovery matters.
    pub fn delete_note(&self, relative: &str) -> Result<String> {
        let path = self.normalize_note(relative)?;
        let absolute = self.root.join(path.replace('/', std::path::MAIN_SEPARATOR_STR));
        if !absolute.is_file() {
            return Err(VaultError::NoteNotFound(path));
        }
        fs::remove_file(absolute)?;
        Ok(path)
    }

    /// Move or rename a note. Callers that care about link integrity should
    /// follow up with [`crate::index::VaultIndex::rewrite_links_for_move`].
    pub fn move_note(&self, from: &str, to: &str) -> Result<(String, String)> {
        let from_path = self.normalize_note(from)?;
        let to_path = self.normalize_note(to)?;
        let from_abs = self.root.join(from_path.replace('/', std::path::MAIN_SEPARATOR_STR));
        let to_abs = self.root.join(to_path.replace('/', std::path::MAIN_SEPARATOR_STR));
        if !from_abs.is_file() {
            return Err(VaultError::NoteNotFound(from_path));
        }
        if to_abs.exists() {
            return Err(VaultError::NoteExists(to_path));
        }
        if let Some(parent) = to_abs.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(&from_abs, &to_abs)?;
        Ok((from_path, to_path))
    }

    /// Create a folder inside the vault.
    pub fn create_folder(&self, relative: &str) -> Result<String> {
        let path = self.normalize(relative)?;
        let absolute = self.root.join(path.replace('/', std::path::MAIN_SEPARATOR_STR));
        fs::create_dir_all(&absolute)?;
        Ok(path)
    }

    /// Move or rename a folder and everything inside it.
    ///
    /// The move is a single rename, so the notes inside keep their filenames and
    /// only their folder prefix changes. Callers that care about link integrity
    /// should follow up with [`crate::index::VaultIndex::rewrite_links_for_moves`].
    pub fn move_folder(&self, from: &str, to: &str) -> Result<(String, String)> {
        let from_path = self.normalize(from)?;
        let to_path = self.normalize(to)?;
        if from_path.is_empty() {
            return Err(VaultError::InvalidPath("the vault root cannot be moved".into()));
        }
        if to_path.is_empty() {
            return Err(VaultError::InvalidPath("a folder cannot replace the vault root".into()));
        }
        // Moving a folder under itself would recurse forever, and renaming it
        // onto its own path is a no-op worth rejecting rather than performing.
        if to_path == from_path || to_path.starts_with(&format!("{from_path}/")) {
            return Err(VaultError::FolderIntoItself(from_path));
        }

        let from_abs = self.root.join(from_path.replace('/', std::path::MAIN_SEPARATOR_STR));
        let to_abs = self.root.join(to_path.replace('/', std::path::MAIN_SEPARATOR_STR));
        if !from_abs.is_dir() {
            return Err(VaultError::FolderNotFound(from_path));
        }
        if to_abs.exists() {
            return Err(VaultError::PathExists(to_path));
        }
        if let Some(parent) = to_abs.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(&from_abs, &to_abs)?;
        Ok((from_path, to_path))
    }

    /// Delete a folder and everything inside it.
    ///
    /// This removes files the user may not have been looking at, so callers are
    /// expected to have confirmed with a count of what is about to go.
    pub fn delete_folder(&self, relative: &str) -> Result<String> {
        let path = self.normalize(relative)?;
        if path.is_empty() {
            return Err(VaultError::InvalidPath("the vault root cannot be deleted".into()));
        }
        let absolute = self.root.join(path.replace('/', std::path::MAIN_SEPARATOR_STR));
        if !absolute.is_dir() {
            return Err(VaultError::FolderNotFound(path));
        }
        fs::remove_dir_all(&absolute)?;
        Ok(path)
    }

    /// Note paths inside a folder, at any depth.
    pub fn notes_under(&self, folder: &str) -> Result<Vec<String>> {
        let path = self.normalize(folder)?;
        let prefix = format!("{path}/");
        Ok(self
            .list_notes()
            .into_iter()
            .map(|note| note.path)
            .filter(|note| note.starts_with(&prefix))
            .collect())
    }

    /// Folder paths in the vault, sorted, excluding skipped and hidden ones.
    pub fn list_folders(&self) -> Vec<String> {
        let mut folders: Vec<String> = WalkDir::new(&self.root)
            .min_depth(1)
            .into_iter()
            .filter_entry(|entry| !is_skipped(entry))
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_type().is_dir())
            .filter_map(|entry| self.relative_of(entry.path()))
            .collect();
        folders.sort();
        folders
    }

    // -----------------------------------------------------------------------
    // internals
    // -----------------------------------------------------------------------

    fn walk(&self) -> impl Iterator<Item = PathBuf> + '_ {
        let mut files: Vec<PathBuf> = WalkDir::new(&self.root)
            .min_depth(1)
            .into_iter()
            .filter_entry(|entry| !is_skipped(entry))
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_type().is_file())
            .map(|entry| entry.into_path())
            .collect();
        files.sort();
        files.into_iter()
    }

    fn relative_of(&self, path: &Path) -> Option<String> {
        path.strip_prefix(&self.root)
            .ok()
            .map(|rest| rest.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/"))
    }
}

fn is_skipped(entry: &walkdir::DirEntry) -> bool {
    let name = entry.file_name().to_string_lossy();
    if entry.file_type().is_dir() {
        return name.starts_with('.') || SKIPPED_DIRS.contains(&name.as_ref());
    }
    name.starts_with('.')
}

fn is_note_path(path: &Path) -> bool {
    path.extension()
        .map(|ext| NOTE_EXTENSIONS.contains(&ext.to_string_lossy().to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// Whether a file's contents have been evicted to the cloud.
///
/// macOS marks iCloud-evicted files `SF_DATALESS`. Reading one asks the system to
/// download it first, and that read blocks — indefinitely if sync is stalled or
/// offline. A vault living under `~/Documents` with "Optimise Mac Storage" turned
/// on will contain these routinely, so every read has to check first rather than
/// discover it by hanging.
#[cfg(target_os = "macos")]
fn is_dataless(path: &Path) -> bool {
    use std::os::macos::fs::MetadataExt;

    /// `SF_DATALESS` from `<sys/stat.h>`.
    const SF_DATALESS: u32 = 0x4000_0000;

    // `symlink_metadata` avoids following a link into another dataless file.
    fs::symlink_metadata(path)
        .map(|meta| meta.st_flags() & SF_DATALESS != 0)
        .unwrap_or(false)
}

#[cfg(not(target_os = "macos"))]
fn is_dataless(_path: &Path) -> bool {
    // Other platforms surface cloud placeholders as ordinary files or as reparse
    // points that fail fast, so there is nothing to pre-empt here.
    false
}

/// Guard a read against blocking on an undownloaded file.
fn ensure_materialized(path: &Path, relative: &str) -> Result<()> {
    if is_dataless(path) {
        return Err(VaultError::NotMaterialized(relative.to_string()));
    }
    Ok(())
}

fn stat(path: &Path) -> (u64, Option<u64>) {
    match fs::metadata(path) {
        Ok(meta) => {
            let modified = meta
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64);
            (meta.len(), modified)
        }
        Err(_) => (0, None),
    }
}

/// Notes are line-oriented; a trailing newline keeps appends and diffs clean.
fn ensure_trailing_newline(content: &str) -> String {
    if content.is_empty() || content.ends_with('\n') {
        content.to_string()
    } else {
        format!("{content}\n")
    }
}

/// Milliseconds since the Unix epoch, for stamping created/modified fields.
pub fn now_millis() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_vault(name: &str) -> Vault {
        let dir = std::env::temp_dir().join(format!("int-vault-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        Vault::create(&dir).expect("vault")
    }

    #[test]
    fn normalizes_and_defaults_extension() {
        let vault = temp_vault("normalize");
        assert_eq!(vault.normalize_note("Notes/Alpha").unwrap(), "Notes/Alpha.md");
        assert_eq!(vault.normalize_note("./Notes/../Beta.md").unwrap(), "Beta.md");
        assert_eq!(vault.normalize_note("image.png").unwrap(), "image.png.md");
    }

    #[test]
    fn moves_a_folder_with_its_notes() {
        let vault = temp_vault("move-folder");
        vault.write_note("Projects/Alpha.md", "one").unwrap();
        vault.write_note("Projects/Deep/Beta.md", "two").unwrap();

        let (from, to) = vault.move_folder("Projects", "Archive/Projects").unwrap();
        assert_eq!((from.as_str(), to.as_str()), ("Projects", "Archive/Projects"));
        assert!(vault.exists("Archive/Projects/Alpha.md"));
        assert!(vault.exists("Archive/Projects/Deep/Beta.md"));
        assert!(!vault.exists("Projects/Alpha.md"));
    }

    #[test]
    fn refuses_to_move_a_folder_into_itself() {
        let vault = temp_vault("move-folder-cycle");
        vault.create_folder("Projects").unwrap();
        assert!(vault.move_folder("Projects", "Projects/Inner").is_err());
        assert!(vault.move_folder("Projects", "Projects").is_err());
        // A folder whose name merely starts the same is a legitimate target.
        assert!(vault.move_folder("Projects", "ProjectsArchive").is_ok());
    }

    #[test]
    fn refuses_to_move_onto_something_that_exists() {
        let vault = temp_vault("move-folder-clash");
        vault.create_folder("One").unwrap();
        vault.create_folder("Two").unwrap();
        assert!(vault.move_folder("One", "Two").is_err());
    }

    #[test]
    fn deletes_a_folder_and_its_contents() {
        let vault = temp_vault("delete-folder");
        vault.write_note("Scratch/Note.md", "gone").unwrap();
        assert_eq!(vault.delete_folder("Scratch").unwrap(), "Scratch");
        assert!(!vault.exists("Scratch/Note.md"));
        assert!(vault.delete_folder("Scratch").is_err());
    }

    #[test]
    fn refuses_to_delete_or_move_the_vault_root() {
        let vault = temp_vault("protect-root");
        assert!(vault.delete_folder("").is_err());
        assert!(vault.delete_folder(".").is_err());
        assert!(vault.move_folder("", "Elsewhere").is_err());
    }

    #[test]
    fn lists_notes_under_a_folder_at_any_depth() {
        let vault = temp_vault("notes-under");
        vault.write_note("Projects/Alpha.md", "a").unwrap();
        vault.write_note("Projects/Deep/Beta.md", "b").unwrap();
        vault.write_note("Elsewhere.md", "c").unwrap();
        // A sibling whose name shares the prefix must not be swept in.
        vault.write_note("ProjectsOld/Gamma.md", "d").unwrap();

        let mut under = vault.notes_under("Projects").unwrap();
        under.sort();
        assert_eq!(under, vec!["Projects/Alpha.md", "Projects/Deep/Beta.md"]);
    }

    #[test]
    fn rejects_escaping_paths() {
        let vault = temp_vault("escape");
        assert!(vault.normalize("../outside.md").is_err());
        assert!(vault.normalize("/etc/passwd").is_err());
        assert!(vault.normalize("a/../../b.md").is_err());
    }

    #[test]
    fn accepts_absolute_paths_inside_the_vault() {
        let vault = temp_vault("absolute");
        let inside = vault.root().join("Notes").join("Alpha.md");
        assert_eq!(vault.normalize(&inside.to_string_lossy()).unwrap(), "Notes/Alpha.md");
    }

    #[test]
    fn creates_reads_and_refuses_to_clobber() {
        let vault = temp_vault("create");
        let path = vault.create_note("Projects/Alpha", "# Alpha\n").unwrap();
        assert_eq!(path, "Projects/Alpha.md");
        assert_eq!(vault.read_note("Projects/Alpha").unwrap().body, "# Alpha\n");
        assert!(vault.create_note("Projects/Alpha.md", "x").is_err());
    }

    #[test]
    fn appends_with_a_newline_boundary() {
        let vault = temp_vault("append");
        vault.write_note("A.md", "line one").unwrap();
        vault.append_note("A.md", "line two\n").unwrap();
        assert_eq!(vault.read_raw("A.md").unwrap(), "line one\nline two\n");
    }

    #[test]
    fn appends_under_the_right_heading() {
        let vault = temp_vault("heading");
        vault.write_note("A.md", "# Title\n\n## Tasks\n\n- one\n\n## Notes\n\n- other\n").unwrap();
        vault.append_under_heading("A.md", "Tasks", "- two").unwrap();
        let raw = vault.read_raw("A.md").unwrap();
        let tasks = raw.find("- two").unwrap();
        let notes = raw.find("## Notes").unwrap();
        assert!(tasks < notes, "appended line landed outside its section: {raw}");
        assert!(raw.contains("- other"));
    }

    #[test]
    fn append_under_missing_heading_errors() {
        let vault = temp_vault("heading-missing");
        vault.write_note("A.md", "# Title\n").unwrap();
        assert!(vault.append_under_heading("A.md", "Nope", "x").is_err());
    }

    #[test]
    fn lists_notes_and_skips_dot_directories() {
        let vault = temp_vault("list");
        vault.write_note("A.md", "# A\n").unwrap();
        vault.write_note("sub/B.md", "# B\n").unwrap();
        fs::create_dir_all(vault.root().join(".obsidian")).unwrap();
        fs::write(vault.root().join(".obsidian/C.md"), "# C\n").unwrap();
        let paths: Vec<String> = vault.list_notes().into_iter().map(|n| n.path).collect();
        assert_eq!(paths, vec!["A.md", "sub/B.md"]);
    }

    #[test]
    fn ordinary_files_are_not_treated_as_dataless() {
        // A dataless file cannot be created in a test — only the OS sets
        // SF_DATALESS — so this pins the negative case: normal notes must never
        // be mistaken for evicted ones, which would make them silently vanish.
        let vault = temp_vault("dataless");
        vault.write_note("A.md", "# A\n").unwrap();
        assert!(vault.list_unavailable().is_empty());
        assert!(vault.read_note("A.md").is_ok());
        assert_eq!(vault.list_notes().len(), 1);
    }

    #[test]
    fn trashing_removes_a_note_from_the_vault_but_keeps_the_file() {
        let vault = temp_vault("trash");
        vault.write_note("Projects/Alpha.md", "# Alpha\n").unwrap();
        assert_eq!(vault.list_notes().len(), 1);

        let trashed = vault.trash_note("Projects/Alpha.md").unwrap();
        assert_eq!(trashed, ".trash/Alpha.md");
        // Gone from the vault's view...
        assert!(vault.list_notes().is_empty());
        assert!(vault.read_note("Projects/Alpha.md").is_err());
        // ...but still on disk.
        assert!(vault.root().join(".trash/Alpha.md").is_file());
    }

    #[test]
    fn trashing_the_same_filename_twice_does_not_overwrite() {
        let vault = temp_vault("trash-collide");
        vault.write_note("One/Note.md", "first\n").unwrap();
        vault.write_note("Two/Note.md", "second\n").unwrap();

        assert_eq!(vault.trash_note("One/Note.md").unwrap(), ".trash/Note.md");
        assert_eq!(vault.trash_note("Two/Note.md").unwrap(), ".trash/Note 2.md");
        assert_eq!(fs::read_to_string(vault.root().join(".trash/Note.md")).unwrap(), "first\n");
        assert_eq!(fs::read_to_string(vault.root().join(".trash/Note 2.md")).unwrap(), "second\n");
    }

    #[test]
    fn trashing_a_missing_note_errors() {
        let vault = temp_vault("trash-missing");
        assert!(vault.trash_note("Nope.md").is_err());
    }

    #[test]
    fn moves_notes_without_overwriting() {
        let vault = temp_vault("move");
        vault.write_note("A.md", "# A\n").unwrap();
        vault.write_note("B.md", "# B\n").unwrap();
        assert!(vault.move_note("A.md", "B.md").is_err());
        let (from, to) = vault.move_note("A.md", "sub/A2").unwrap();
        assert_eq!((from.as_str(), to.as_str()), ("A.md", "sub/A2.md"));
        assert!(!vault.exists("A.md"));
    }
}
