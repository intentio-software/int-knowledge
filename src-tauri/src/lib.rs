//! Tauri commands backing the Intentio Knowledge desktop app.
//!
//! Every command goes through `int-vault` — the same crate the MCP server uses —
//! so the app and an AI agent see identical link resolution, search and note
//! parsing. The frontend never touches the filesystem directly; it names a vault
//! root and a note path, and everything is validated here.

pub mod git_sync;
mod menu;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tauri::{AppHandle, Emitter};

use int_vault::{
    now_millis, search, Backlink, Heading, NoteMeta, ResolvedLink, SearchHit, SearchOptions, Vault,
    VaultIndex,
};

/// How long to wait for a burst of filesystem events to go quiet.
///
/// A single save often produces several events (write, attrs, rename of a temp
/// file), and an agent writing a batch of notes produces dozens. Coalescing them
/// turns that into one refresh.
const WATCH_DEBOUNCE: Duration = Duration::from_millis(250);

/// Cached index per vault root, refreshed when the folder changes on disk.
#[derive(Default)]
struct IndexCache {
    entries: HashMap<PathBuf, (u64, VaultIndex)>,
}

#[derive(Default)]
pub struct AppState {
    cache: Mutex<IndexCache>,
    /// Kept alive for as long as a vault is being watched; dropping it stops
    /// the watch and lets its worker thread exit.
    watcher: Mutex<Option<RecommendedWatcher>>,
}

impl AppState {
    /// Run `action` against a fresh-enough index for `root`.
    ///
    /// The vault is a plain folder that an agent or another editor may be writing
    /// to at the same time, so freshness is re-checked on every call rather than
    /// assumed.
    fn with_index<T>(&self, root: &str, action: impl FnOnce(&Vault, &VaultIndex) -> T) -> Result<T, String> {
        let vault = open(root)?;
        let key = vault.root().to_path_buf();
        let fingerprint = vault.fingerprint();

        let mut cache = self.cache.lock().map_err(|_| "index cache is poisoned".to_string())?;
        let needs_rebuild = cache.entries.get(&key).map(|(seen, _)| *seen != fingerprint).unwrap_or(true);
        if needs_rebuild {
            cache.entries.insert(key.clone(), (fingerprint, VaultIndex::build(&vault)));
        }
        let (_, index) = cache.entries.get(&key).expect("index inserted above");
        Ok(action(&vault, index))
    }

    fn invalidate(&self, root: &str) {
        if let Ok(vault) = open(root) {
            if let Ok(mut cache) = self.cache.lock() {
                cache.entries.remove(vault.root());
            }
        }
    }
}

fn open(root: &str) -> Result<Vault, String> {
    Vault::open(root).map_err(|err| err.to_string())
}

// ---------------------------------------------------------------------------
// payloads
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultSummary {
    pub name: String,
    pub path: String,
    pub notes: usize,
    pub folders: Vec<String>,
    pub tags: Vec<TagCount>,
    pub unresolved: usize,
}

#[derive(Debug, Serialize)]
pub struct TagCount {
    pub tag: String,
    pub notes: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteDetail {
    #[serde(flatten)]
    pub meta: NoteMeta,
    /// Full file text, frontmatter included — what the editor shows.
    pub content: String,
    /// Body only, used for previews.
    pub body: String,
    pub frontmatter: Map<String, Value>,
    pub headings: Vec<Heading>,
    pub links: Vec<ResolvedLink>,
    pub backlinks: Vec<Backlink>,
}

/// One node in the vault graph. Ghost nodes (`exists: false`) stand for links
/// that point at notes nobody has written yet.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphNode {
    /// Vault path for real notes; the raw link target for ghosts.
    pub id: String,
    pub label: String,
    pub exists: bool,
    /// Total links in and out, used to size the node.
    pub degree: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphEdge {
    pub source: String,
    pub target: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphData {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchRequest {
    pub root: String,
    pub query: String,
    #[serde(default)]
    pub folder: Option<String>,
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

// ---------------------------------------------------------------------------
// commands
// ---------------------------------------------------------------------------

/// Open a folder as a vault and describe it.
#[tauri::command]
fn open_vault(state: tauri::State<'_, AppState>, root: String) -> Result<VaultSummary, String> {
    // Record it so the MCP server can follow whichever vault is open here.
    // A failure is not worth refusing to open the vault over.
    if let Ok(vault) = open(&root) {
        if let Err(err) = int_vault::app_state::write_active_vault(Some(vault.root()), now_millis()) {
            eprintln!("[knowledge] could not record active vault: {err}");
        }
    }

    state.with_index(&root, |vault, index| VaultSummary {
        name: vault.name(),
        path: vault.root().to_string_lossy().to_string(),
        notes: index.len(),
        folders: vault.list_folders(),
        tags: index.tags().into_iter().map(|(tag, notes)| TagCount { tag, notes }).collect(),
        unresolved: index.unresolved().len(),
    })
}

/// Create a folder and open it as a new vault, seeded with a welcome note.
#[tauri::command]
fn create_vault(state: tauri::State<'_, AppState>, root: String) -> Result<VaultSummary, String> {
    let vault = Vault::create(&root).map_err(|err| err.to_string())?;
    if vault.list_notes().is_empty() {
        vault.write_note("Welcome.md", WELCOME_NOTE).map_err(|err| err.to_string())?;
    }
    let path = vault.root().to_string_lossy().to_string();
    state.invalidate(&path);
    open_vault(state, path)
}

#[tauri::command]
fn list_notes(state: tauri::State<'_, AppState>, root: String) -> Result<Vec<NoteMeta>, String> {
    state.with_index(&root, |_, index| index.notes().iter().map(|note| note.meta.clone()).collect())
}

#[tauri::command]
fn read_note(state: tauri::State<'_, AppState>, root: String, path: String) -> Result<NoteDetail, String> {
    state.with_index(&root, |vault, index| {
        let note = vault.read_note(&path).map_err(|err| err.to_string())?;
        let content = vault.read_raw(&note.meta.path).map_err(|err| err.to_string())?;
        let links = index.outgoing(&note.meta.path);
        let backlinks = index.backlinks(&note.meta.path);
        Ok(NoteDetail {
            content,
            body: note.body,
            frontmatter: note.frontmatter,
            headings: note.scan.headings,
            links,
            backlinks,
            meta: note.meta,
        })
    })?
}

#[tauri::command]
fn save_note(
    state: tauri::State<'_, AppState>,
    root: String,
    path: String,
    content: String,
) -> Result<String, String> {
    let vault = open(&root)?;
    let written = vault.write_note(&path, &content).map_err(|err| err.to_string())?;
    state.invalidate(&root);
    Ok(written)
}

#[tauri::command]
fn create_note(
    state: tauri::State<'_, AppState>,
    root: String,
    path: String,
    content: Option<String>,
) -> Result<String, String> {
    let vault = open(&root)?;
    let created = vault
        .create_note(&path, &content.unwrap_or_default())
        .map_err(|err| err.to_string())?;
    state.invalidate(&root);
    Ok(created)
}

#[tauri::command]
fn delete_note(state: tauri::State<'_, AppState>, root: String, path: String) -> Result<String, String> {
    let vault = open(&root)?;
    let deleted = vault.delete_note(&path).map_err(|err| err.to_string())?;
    state.invalidate(&root);
    Ok(deleted)
}

/// Create an empty folder.
///
/// Folders are otherwise implied by the notes inside them, so a new one would
/// vanish on the next refresh if it did not exist on disk.
#[tauri::command]
fn create_folder(state: tauri::State<'_, AppState>, root: String, path: String) -> Result<String, String> {
    let vault = open(&root)?;
    let created = vault.create_folder(&path).map_err(|err| err.to_string())?;
    state.invalidate(&root);
    Ok(created)
}

/// Move or rename a folder, keeping wikilinks to the notes inside it working.
///
/// The notes keep their filenames, so most links — which are written as bare
/// names — need no change at all; the rewrite catches the ones written as full
/// paths, and any that become ambiguous at the new location.
#[tauri::command]
fn move_folder(
    state: tauri::State<'_, AppState>,
    root: String,
    from: String,
    to: String,
    update_links: Option<bool>,
) -> Result<String, String> {
    let vault = open(&root)?;
    let from_path = vault.normalize(&from).map_err(|err| err.to_string())?;
    let to_path = vault.normalize(&to).map_err(|err| err.to_string())?;

    // Worked out before the move, while the old paths still resolve.
    let moves: Vec<(String, String)> = if update_links.unwrap_or(true) {
        vault
            .notes_under(&from_path)
            .map_err(|err| err.to_string())?
            .into_iter()
            .map(|note| {
                let moved = format!("{to_path}{}", &note[from_path.len()..]);
                (note, moved)
            })
            .collect()
    } else {
        Vec::new()
    };
    let rewrites = if moves.is_empty() {
        Vec::new()
    } else {
        state.with_index(&root, |_, index| index.rewrite_links_for_moves(&moves))?
    };

    let (_, moved_to) = vault.move_folder(&from_path, &to_path).map_err(|err| err.to_string())?;
    for (path, content) in rewrites {
        if let Err(err) = vault.write_note(&path, &content) {
            eprintln!("[knowledge] link rewrite failed for {path}: {err}");
        }
    }
    state.invalidate(&root);
    Ok(moved_to)
}

/// Delete a folder and every note inside it.
#[tauri::command]
fn delete_folder(state: tauri::State<'_, AppState>, root: String, path: String) -> Result<String, String> {
    let vault = open(&root)?;
    let deleted = vault.delete_folder(&path).map_err(|err| err.to_string())?;
    state.invalidate(&root);
    Ok(deleted)
}

/// Paths of the notes inside a folder, so the UI can say what a delete removes.
#[tauri::command]
fn notes_in_folder(root: String, path: String) -> Result<Vec<String>, String> {
    let vault = open(&root)?;
    vault.notes_under(&path).map_err(|err| err.to_string())
}

/// Rename or move a note, keeping wikilinks elsewhere in the vault pointing at it.
#[tauri::command]
fn rename_note(
    state: tauri::State<'_, AppState>,
    root: String,
    from: String,
    to: String,
    update_links: Option<bool>,
) -> Result<String, String> {
    let vault = open(&root)?;
    let from_path = vault.normalize_note(&from).map_err(|err| err.to_string())?;
    let to_path = vault.normalize_note(&to).map_err(|err| err.to_string())?;

    // Rewrites are computed against the pre-move index, where old links resolve.
    let rewrites = if update_links.unwrap_or(true) {
        state.with_index(&root, |_, index| index.rewrite_links_for_move(&from_path, &to_path))?
    } else {
        Vec::new()
    };

    let (_, moved_to) = vault.move_note(&from_path, &to_path).map_err(|err| err.to_string())?;
    for (path, content) in rewrites {
        if let Err(err) = vault.write_note(&path, &content) {
            eprintln!("[knowledge] link rewrite failed for {path}: {err}");
        }
    }
    state.invalidate(&root);
    Ok(moved_to)
}

#[tauri::command]
fn search_notes(state: tauri::State<'_, AppState>, request: SearchRequest) -> Result<Vec<SearchHit>, String> {
    let options = SearchOptions {
        limit: request.limit.unwrap_or(50),
        folder: request.folder.clone(),
        tag: request.tag.clone(),
        ..SearchOptions::default()
    };
    state.with_index(&request.root, |_, index| search::search(index, &request.query, &options))
}

/// Resolve a `[[wikilink]]` clicked in the editor to a vault path.
#[tauri::command]
fn resolve_link(
    state: tauri::State<'_, AppState>,
    root: String,
    from: String,
    target: String,
) -> Result<Option<String>, String> {
    state.with_index(&root, |_, index| index.resolve(&from, &target))
}

/// The whole vault as a link graph.
///
/// Unresolved targets become their own "ghost" nodes rather than being dropped,
/// which is what makes the graph show where the vault wants to grow. Edges are
/// deduplicated, so linking to the same note five times is one line.
#[tauri::command]
fn graph(state: tauri::State<'_, AppState>, root: String, include_ghosts: Option<bool>) -> Result<GraphData, String> {
    let include_ghosts = include_ghosts.unwrap_or(true);

    state.with_index(&root, |_, index| {
        let mut degrees: HashMap<String, usize> = HashMap::new();
        let mut edges: Vec<GraphEdge> = Vec::new();
        let mut seen_edges: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
        let mut ghosts: HashMap<String, String> = HashMap::new();

        for note in index.notes() {
            let source = note.meta.path.clone();
            for link in &note.links {
                if link.target.is_empty() {
                    continue;
                }
                let target = match index.resolve(&source, &link.target) {
                    Some(path) => path,
                    None => {
                        if !include_ghosts {
                            continue;
                        }
                        // Ghosts key on the lowercased target so `[[Ideas]]` and
                        // `[[ideas]]` converge on one node, as they would once created.
                        let key = link.target.to_lowercase();
                        ghosts.entry(key.clone()).or_insert_with(|| link.target.clone());
                        key
                    }
                };
                if target == source {
                    continue;
                }
                if seen_edges.insert((source.clone(), target.clone())) {
                    *degrees.entry(source.clone()).or_insert(0) += 1;
                    *degrees.entry(target.clone()).or_insert(0) += 1;
                    edges.push(GraphEdge { source: source.clone(), target });
                }
            }
        }

        let mut nodes: Vec<GraphNode> = index
            .notes()
            .iter()
            .map(|note| GraphNode {
                degree: degrees.get(&note.meta.path).copied().unwrap_or(0),
                id: note.meta.path.clone(),
                label: note.meta.title.clone(),
                exists: true,
                tags: note.meta.tags.clone(),
            })
            .collect();

        nodes.extend(ghosts.into_iter().map(|(key, label)| GraphNode {
            degree: degrees.get(&key).copied().unwrap_or(0),
            id: key,
            label,
            exists: false,
            tags: Vec::new(),
        }));

        GraphData { nodes, edges }
    })
}

#[tauri::command]
fn unresolved_links(state: tauri::State<'_, AppState>, root: String) -> Result<Vec<Value>, String> {
    state.with_index(&root, |_, index| {
        index
            .unresolved()
            .into_iter()
            .filter_map(|link| serde_json::to_value(link).ok())
            .collect()
    })
}

/// Payload of the `vault-changed` event.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct VaultChanged {
    root: String,
    /// Vault-relative paths that changed, capped so a bulk import does not
    /// produce a megabyte-sized event.
    paths: Vec<String>,
    /// True when more changed than `paths` lists.
    truncated: bool,
}

/// Watch a vault for changes made outside the app.
///
/// The vault is a plain folder — an agent driving the MCP server, a `git pull`,
/// or another editor can all change it while the app is open. Without this the
/// UI would silently show stale notes and a stale graph.
///
/// Replaces any previous watch, so switching vaults never leaves one running.
#[tauri::command]
fn watch_vault(app: AppHandle, state: tauri::State<'_, AppState>, root: String) -> Result<(), String> {
    let vault = open(&root)?;
    let watched_root = vault.root().to_path_buf();
    let root_label = watched_root.to_string_lossy().to_string();

    let (tx, rx) = std::sync::mpsc::channel::<notify::Result<notify::Event>>();
    let mut watcher =
        notify::recommended_watcher(move |event| {
            // A send failure just means the receiver thread is gone.
            let _ = tx.send(event);
        })
        .map_err(|err| format!("cannot watch vault: {err}"))?;

    watcher
        .watch(&watched_root, RecursiveMode::Recursive)
        .map_err(|err| format!("cannot watch {}: {err}", watched_root.display()))?;

    let thread_root = watched_root.clone();
    std::thread::spawn(move || {
        loop {
            // Block while idle, then drain the burst that woke us.
            let first = match rx.recv() {
                Ok(event) => event,
                // Sender dropped: the watcher was replaced or the app is closing.
                Err(_) => break,
            };

            let mut changed: HashSet<String> = HashSet::new();
            collect_changes(&thread_root, first, &mut changed);
            while let Ok(next) = rx.recv_timeout(WATCH_DEBOUNCE) {
                collect_changes(&thread_root, next, &mut changed);
            }
            if changed.is_empty() {
                continue;
            }

            let total = changed.len();
            let mut paths: Vec<String> = changed.into_iter().collect();
            paths.sort();
            let truncated = total > 200;
            paths.truncate(200);

            if app
                .emit("vault-changed", VaultChanged { root: root_label.clone(), paths, truncated })
                .is_err()
            {
                // The window is gone; nothing left to notify.
                break;
            }
        }
    });

    *state.watcher.lock().map_err(|_| "watcher lock is poisoned".to_string())? = Some(watcher);
    Ok(())
}

/// Stop watching.
///
/// This only detaches the current watcher. It is also called defensively by
/// `startWatching` before it attaches a fresh one, so it must NOT touch the
/// shared active-vault record — doing so previously meant every vault-open
/// immediately cleared the record it had just written, since a watch restart
/// always follows an open. Closing the vault outright goes through
/// `close_vault` instead.
#[tauri::command]
fn unwatch_vault(state: tauri::State<'_, AppState>) -> Result<(), String> {
    *state.watcher.lock().map_err(|_| "watcher lock is poisoned".to_string())? = None;
    Ok(())
}

/// Record that no vault is open, e.g. when the user closes the vault.
///
/// Separate from `unwatch_vault` so an agent following the shared record is
/// only ever pointed away from a vault the user actually closed, not one that
/// merely had its file watcher restarted.
#[tauri::command]
fn close_vault() -> Result<(), String> {
    if let Err(err) = int_vault::app_state::write_active_vault(None, now_millis()) {
        eprintln!("[knowledge] could not clear active vault: {err}");
    }
    Ok(())
}

/// Reduce a raw filesystem event to the vault-relative note paths it touched.
fn collect_changes(root: &Path, event: notify::Result<notify::Event>, out: &mut HashSet<String>) {
    let Ok(event) = event else { return };
    for path in event.paths {
        let Ok(relative) = path.strip_prefix(root) else { continue };
        let text = relative.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/");
        if text.is_empty() {
            continue;
        }
        // Editors and syncing tools churn through dotfiles and temp files that
        // are not part of the vault's content.
        if text.split('/').any(|segment| segment.starts_with('.')) {
            continue;
        }
        let is_note = text
            .rsplit_once('.')
            .map(|(_, ext)| int_vault::vault::NOTE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
            .unwrap_or(false);
        if is_note {
            out.insert(text);
        }
    }
}

const WELCOME_NOTE: &str = "---\ntitle: Welcome\ntags:\n  - intentio\n---\n\n# Welcome to your vault\n\nThis folder is yours. Every note is a plain `.md` file on your own disk — open them in any\neditor, keep them in git, sync them however you like.\n\n## Linking\n\nType `[[` to link to another note. Links to notes that do not exist yet are fine; they show up\nas unresolved and become real the moment you create them. Try [[Ideas]].\n\n## Working with agents\n\nRun the bundled MCP server against this folder and an AI agent can read and write these notes\ndirectly:\n\n    int-knowledge-mcp \"<this folder>\"\n\nThe agent sees the same links, tags and search you do.\n";

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{CreateKind, EventKind};

    fn event(paths: &[&str]) -> notify::Result<notify::Event> {
        Ok(notify::Event {
            kind: EventKind::Create(CreateKind::File),
            paths: paths.iter().map(PathBuf::from).collect(),
            attrs: Default::default(),
        })
    }

    fn changes(paths: &[&str]) -> Vec<String> {
        let mut out = HashSet::new();
        collect_changes(Path::new("/vault"), event(paths), &mut out);
        let mut sorted: Vec<String> = out.into_iter().collect();
        sorted.sort();
        sorted
    }

    #[test]
    fn reports_notes_relative_to_the_vault() {
        assert_eq!(changes(&["/vault/Notes/Alpha.md"]), vec!["Notes/Alpha.md"]);
    }

    #[test]
    fn ignores_non_note_files() {
        assert!(changes(&["/vault/image.png", "/vault/data.json"]).is_empty());
    }

    #[test]
    fn ignores_dotfiles_and_dot_directories() {
        // Obsidian config, git internals and editor swap files all churn
        // constantly and would otherwise trigger a refresh loop.
        assert!(changes(&["/vault/.obsidian/workspace.md", "/vault/.git/COMMIT_EDITMSG", "/vault/.tmp.md"]).is_empty());
    }

    #[test]
    fn ignores_paths_outside_the_vault() {
        assert!(changes(&["/elsewhere/Alpha.md"]).is_empty());
    }

    #[test]
    fn accepts_every_note_extension() {
        let found = changes(&["/vault/a.md", "/vault/b.markdown", "/vault/c.MDX"]);
        assert_eq!(found, vec!["a.md", "b.markdown", "c.MDX"]);
    }

    #[test]
    fn deduplicates_a_burst_touching_one_file() {
        let mut out = HashSet::new();
        collect_changes(Path::new("/vault"), event(&["/vault/A.md"]), &mut out);
        collect_changes(Path::new("/vault"), event(&["/vault/A.md"]), &mut out);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn ignores_watcher_errors() {
        let mut out = HashSet::new();
        let err = Err(notify::Error::generic("watch failed"));
        collect_changes(Path::new("/vault"), err, &mut out);
        assert!(out.is_empty());
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// Whether the open vault is a Git repository, and how it stands with its remote.
#[tauri::command]
fn git_sync_status(vault: String) -> git_sync::SyncStatus {
    git_sync::status(&git_sync::vault_path(&vault))
}

/// Commit, pull and push the vault. Returns what happened, including the
/// reason when it deliberately stopped.
#[tauri::command]
async fn git_sync_now(vault: String) -> git_sync::SyncOutcome {
    // Git can block on the network, so it must not run on the UI thread.
    tauri::async_runtime::spawn_blocking(move || git_sync::sync(&git_sync::vault_path(&vault)))
        .await
        .unwrap_or_else(|err| git_sync::SyncOutcome {
            changed: false,
            message: format!("Sync did not run: {err}"),
            blocked: Some("Sync did not run.".into()),
        })
}

pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            // Installed here rather than via `Builder::menu` so the menu can be
            // rebuilt later, when the Open Recent list changes.
            menu::install(app.handle())?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            menu::set_recent_vaults,
            git_sync_status,
            git_sync_now,
            open_vault,
            create_vault,
            list_notes,
            read_note,
            save_note,
            create_note,
            delete_note,
            create_folder,
            move_folder,
            delete_folder,
            notes_in_folder,
            rename_note,
            search_notes,
            resolve_link,
            unresolved_links,
            graph,
            watch_vault,
            unwatch_vault,
            close_vault,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
