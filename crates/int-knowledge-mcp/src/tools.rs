//! The tool surface an AI agent sees for a vault.
//!
//! Tool descriptions here are part of the product: they are the only thing the
//! model reads before deciding what to call, so they say what a tool does *and*
//! when to prefer it over a neighbour.

use serde_json::{json, Map, Value};

use int_vault::{frontmatter, now_millis, search, SearchOptions};

use crate::mcp::{opt_bool, opt_str, opt_str_list, opt_usize, require_str, ServerInfo, Tool, ToolOutput, ToolProvider};
use crate::workspace::Workspace;

pub struct VaultTools {
    workspace: Workspace,
}

impl VaultTools {
    pub fn new(workspace: Workspace) -> Self {
        VaultTools { workspace }
    }

    /// Schema fragment for the shared `vault` selector.
    fn vault_property(&self) -> Value {
        if self.workspace.follows_app() {
            return json!({
                "type": "string",
                "description": "Not needed: this server always acts on whichever vault Intentio Knowledge currently has open."
            });
        }
        let names = self.workspace.names().join(", ");
        json!({
            "type": "string",
            "description": format!(
                "Which vault to act on. Open vaults: {names}.{}",
                if self.workspace.is_single() { " Optional — only one vault is open." } else { " Required." }
            )
        })
    }

    /// Build an object schema with the `vault` selector already mixed in.
    fn schema(&self, properties: Value, required: &[&str]) -> Value {
        let mut props = properties.as_object().cloned().unwrap_or_default();
        props.insert("vault".into(), self.vault_property());
        let mut required: Vec<&str> = required.to_vec();
        if !self.workspace.is_single() {
            required.push("vault");
        }
        json!({ "type": "object", "properties": props, "required": required })
    }
}

impl ToolProvider for VaultTools {
    fn server_info(&self) -> ServerInfo {
        ServerInfo {
            name: "intentio-knowledge".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            instructions: concat!(
                "Read and write an Intentio Knowledge vault — a folder of markdown notes on the user's ",
                "filesystem, linked with [[wikilinks]].\n\n",
                "Guidance:\n",
                "- Unless configured with explicit paths, this server acts on whichever vault the ",
                "desktop app currently has open, re-checked on every call. If a call reports that no ",
                "vault is open, ask the user to open one in the app rather than guessing a path.\n",
                "- Paths are vault-relative and forward-slashed, e.g. `Projects/Alpha.md`. The `.md` ",
                "extension is added automatically when omitted.\n",
                "- Prefer `search_notes` to find a note before reading it. To change part of a note use ",
                "`edit_note`, and to add to the end use `append_note`; `write_note` replaces the entire ",
                "file and will discard anything changed since you last read it.\n",
                "- `delete_note` moves a note to `.trash` by default, so a mistake is recoverable. ",
                "`delete_folder` and `delete_note` with `permanent` are not — confirm those with the user.\n",
                "- Link notes together with `[[Note Name]]`. Links to notes that do not exist yet are fine ",
                "and show up in `unresolved_links` as suggested writing.\n",
                "- Vault files are the user's own documents. Deleting or overwriting is destructive and ",
                "not undoable from here; confirm before doing either.",
            )
            .into(),
        }
    }

    fn tools(&self) -> Vec<Tool> {
        vec![
            Tool::new(
                "list_vaults",
                "List the vaults this server has open, with their paths and note counts. Call this first when unsure which vault to use.",
                json!({ "type": "object", "properties": {} }),
            ),
            Tool::new(
                "vault_info",
                "Summarize a vault: path, note and folder counts, tag count, and how many links point at notes that do not exist yet.",
                self.schema(json!({}), &[]),
            ),
            Tool::new(
                "list_notes",
                "List notes with their titles, tags and modification times. Optionally restrict to a folder or a tag. Use search_notes instead when looking for content.",
                self.schema(
                    json!({
                        "folder": {"type": "string", "description": "Only notes under this folder, e.g. `Projects`."},
                        "tag": {"type": "string", "description": "Only notes with this tag. Nested tags are included: `project` matches `project/alpha`."},
                        "limit": {"type": "integer", "description": "Maximum notes to return. Default 200."}
                    }),
                    &[],
                ),
            ),
            Tool::new(
                "list_folders",
                "List the folders in a vault, so new notes can be filed somewhere that already exists.",
                self.schema(json!({}), &[]),
            ),
            Tool::new(
                "read_note",
                "Read one note: its frontmatter, body, headings, outgoing links and backlinks. This is the tool to use before editing a note.",
                self.schema(
                    json!({
                        "path": {"type": "string", "description": "Vault-relative path, e.g. `Projects/Alpha.md`."},
                        "include_backlinks": {"type": "boolean", "description": "Include notes that link here. Default true."}
                    }),
                    &["path"],
                ),
            ),
            Tool::new(
                "create_note",
                "Create a new note. Fails if one already exists at that path, so it can never overwrite the user's work. Frontmatter is generated from the title, tags and aliases given.",
                self.schema(
                    json!({
                        "path": {"type": "string", "description": "Vault-relative path. `.md` is added if missing."},
                        "content": {"type": "string", "description": "Markdown body. Use [[Note Name]] to link to other notes."},
                        "title": {"type": "string", "description": "Title for the frontmatter. Defaults to the filename."},
                        "tags": {"type": "array", "items": {"type": "string"}, "description": "Tags for the frontmatter."},
                        "aliases": {"type": "array", "items": {"type": "string"}, "description": "Other names this note should be findable by."}
                    }),
                    &["path"],
                ),
            ),
            Tool::new(
                "edit_note",
                "Replace an exact piece of text inside a note, leaving the rest untouched. Prefer this over write_note for changing part of a note: it does not require sending the whole file back, and it cannot silently discard edits made since you read it. Fails if the text is absent, or if it appears more than once and replace_all is not set.",
                self.schema(
                    json!({
                        "path": {"type": "string", "description": "Vault-relative path."},
                        "find": {"type": "string", "description": "Exact text to replace, including whitespace and newlines. Include enough surrounding context to make it unique."},
                        "replace": {"type": "string", "description": "Replacement text. Use an empty string to delete the matched text."},
                        "replace_all": {"type": "boolean", "description": "Replace every occurrence instead of requiring exactly one. Default false."}
                    }),
                    &["path", "find", "replace"],
                ),
            ),
            Tool::new(
                "write_note",
                "Replace a note's entire contents, creating it if absent. Destructive: prefer append_note or update_frontmatter for incremental changes.",
                self.schema(
                    json!({
                        "path": {"type": "string", "description": "Vault-relative path."},
                        "content": {"type": "string", "description": "Full file contents, including any frontmatter block."}
                    }),
                    &["path", "content"],
                ),
            ),
            Tool::new(
                "append_note",
                "Append text to a note, either at the end or under a specific heading. Creates the note if it does not exist. The safe way to add to existing notes.",
                self.schema(
                    json!({
                        "path": {"type": "string", "description": "Vault-relative path."},
                        "text": {"type": "string", "description": "Markdown to append."},
                        "heading": {"type": "string", "description": "Append at the end of this section instead of the end of the file. The heading must already exist."}
                    }),
                    &["path", "text"],
                ),
            ),
            Tool::new(
                "update_frontmatter",
                "Set or remove frontmatter fields on a note, leaving the body untouched.",
                self.schema(
                    json!({
                        "path": {"type": "string", "description": "Vault-relative path."},
                        "set": {"type": "object", "description": "Fields to set, e.g. {\"status\": \"active\", \"tags\": [\"alpha\"]}."},
                        "remove": {"type": "array", "items": {"type": "string"}, "description": "Field names to remove."}
                    }),
                    &["path"],
                ),
            ),
            Tool::new(
                "delete_note",
                "Move a note to the vault's .trash folder. It disappears from listing, search and links, but the file survives and can be restored by hand. Set permanent to erase it instead, which cannot be undone.",
                self.schema(
                    json!({
                        "path": {"type": "string", "description": "Vault-relative path."},
                        "permanent": {"type": "boolean", "description": "Erase the file rather than moving it to .trash. Default false."}
                    }),
                    &["path"],
                ),
            ),
            Tool::new(
                "move_folder",
                "Move or rename a folder and everything in it, rewriting [[wikilinks]] across the vault so nothing breaks.",
                self.schema(
                    json!({
                        "from": {"type": "string", "description": "Current folder path, e.g. `Projects`."},
                        "to": {"type": "string", "description": "New folder path. Parent folders are created as needed."},
                        "update_links": {"type": "boolean", "description": "Rewrite wikilinks pointing into the folder. Default true."}
                    }),
                    &["from", "to"],
                ),
            ),
            Tool::new(
                "delete_folder",
                "Delete a folder and every note inside it. Destructive and not undoable — list the notes with list_notes first and confirm with the user.",
                self.schema(json!({"path": {"type": "string", "description": "Vault-relative folder path."}}), &["path"]),
            ),
            Tool::new(
                "list_orphans",
                "List notes that nothing links to and that link nowhere themselves — the parts of the vault that have fallen out of the graph.",
                self.schema(json!({}), &[]),
            ),
            Tool::new(
                "move_note",
                "Move or rename a note, updating [[wikilinks]] in other notes so nothing breaks.",
                self.schema(
                    json!({
                        "from": {"type": "string", "description": "Current vault-relative path."},
                        "to": {"type": "string", "description": "New vault-relative path. Folders are created as needed."},
                        "update_links": {"type": "boolean", "description": "Rewrite wikilinks elsewhere in the vault. Default true."}
                    }),
                    &["from", "to"],
                ),
            ),
            Tool::new(
                "search_notes",
                "Full-text search across a vault, returning matching notes with the lines that matched. All terms must appear; wrap a phrase in double quotes to match it exactly.",
                self.schema(
                    json!({
                        "query": {"type": "string", "description": "Search terms, e.g. `roadmap q3` or `\"release train\"`."},
                        "folder": {"type": "string", "description": "Only search under this folder."},
                        "tag": {"type": "string", "description": "Only search notes carrying this tag."},
                        "limit": {"type": "integer", "description": "Maximum notes to return. Default 25."}
                    }),
                    &["query"],
                ),
            ),
            Tool::new(
                "get_backlinks",
                "List the notes that link to a given note, with the line each reference appears on. Use this to understand how a note is used before changing it.",
                self.schema(json!({"path": {"type": "string", "description": "Vault-relative path."}}), &["path"]),
            ),
            Tool::new(
                "get_links",
                "List a note's outgoing links, showing which resolve to real notes and which do not.",
                self.schema(json!({"path": {"type": "string", "description": "Vault-relative path."}}), &["path"]),
            ),
            Tool::new(
                "list_tags",
                "List every tag in a vault with how many notes carry it.",
                self.schema(json!({}), &[]),
            ),
            Tool::new(
                "unresolved_links",
                "List links that point at notes which do not exist yet — the vault's implicit to-write list.",
                self.schema(json!({"limit": {"type": "integer", "description": "Maximum results. Default 100."}}), &[]),
            ),
            Tool::new(
                "create_folder",
                "Create a folder in the vault. Rarely needed: writing a note creates its folders automatically.",
                self.schema(json!({"path": {"type": "string", "description": "Vault-relative folder path."}}), &["path"]),
            ),
        ]
    }

    fn call(&mut self, name: &str, args: &Value) -> Result<ToolOutput, String> {
        if name == "list_vaults" {
            let follows = self.workspace.follows_app();
            // In follow mode the list is empty until the app is consulted.
            if follows {
                if let Err(message) = self.workspace.select(None) {
                    return Ok(ToolOutput::json(&json!({
                        "followsApp": true,
                        "vaults": [],
                        "note": message,
                    })));
                }
            }
            let vaults: Vec<Value> = self
                .workspace
                .entries()
                .iter()
                .map(|entry| {
                    json!({
                        "name": entry.name(),
                        "path": entry.vault().root().to_string_lossy(),
                        "notes": entry.vault().list_notes().len(),
                    })
                })
                .collect();
            return Ok(ToolOutput::json(&json!({ "followsApp": follows, "vaults": vaults })));
        }

        let selector = opt_str(args, "vault");
        let entry = self.workspace.select(selector.as_deref())?;

        match name {
            "vault_info" => {
                let vault_name = entry.name();
                let root = entry.vault().root().to_string_lossy().to_string();
                let folders = entry.vault().list_folders().len();
                let attachments = entry.vault().list_attachments().len();
                let unavailable = entry.vault().list_unavailable();
                let index = entry.index();
                let mut info = json!({
                    "name": vault_name,
                    "path": root,
                    "notes": index.len(),
                    "folders": folders,
                    "attachments": attachments,
                    "tags": index.tags().len(),
                    "unresolved_links": index.unresolved().len(),
                });
                // Evicted notes are skipped everywhere else, so say so here
                // rather than letting them look like notes that never existed.
                if !unavailable.is_empty() {
                    info["not_downloaded"] = json!(unavailable);
                    info["note"] = json!(concat!(
                        "Some notes are not downloaded to this machine (evicted to iCloud) and are ",
                        "excluded from listing and search. Opening them in Finder or turning off ",
                        "\"Optimise Mac Storage\" will download them."
                    ));
                }
                Ok(ToolOutput::json(&info))
            }

            "list_notes" => {
                let folder = opt_str(args, "folder").map(|f| {
                    let trimmed = f.trim_matches('/').to_string();
                    if trimmed.is_empty() { trimmed } else { format!("{trimmed}/") }
                });
                let tag = opt_str(args, "tag");
                let limit = opt_usize(args, "limit", 200);

                let index = entry.index();
                let metas: Vec<_> = match &tag {
                    Some(tag) => index.notes_with_tag(tag).into_iter().cloned().collect(),
                    None => index.notes().iter().map(|note| note.meta.clone()).collect(),
                };
                let total = metas.len();
                let notes: Vec<Value> = metas
                    .into_iter()
                    .filter(|meta| match &folder {
                        Some(prefix) if !prefix.is_empty() => meta.path.starts_with(prefix.as_str()),
                        _ => true,
                    })
                    .take(limit)
                    .map(|meta| serde_json::to_value(meta).unwrap_or(Value::Null))
                    .collect();
                Ok(ToolOutput::json(&json!({ "count": notes.len(), "total": total, "notes": notes })))
            }

            "list_folders" => {
                Ok(ToolOutput::json(&json!({ "folders": entry.vault().list_folders() })))
            }

            "read_note" => {
                let path = require_str(args, "path")?;
                let note = entry.vault().read_note(&path).map_err(|err| err.to_string())?;
                let resolved_path = note.meta.path.clone();
                let include_backlinks = opt_bool(args, "include_backlinks", true);

                let index = entry.index();
                let links = index.outgoing(&resolved_path);
                let backlinks =
                    if include_backlinks { index.backlinks(&resolved_path) } else { Vec::new() };

                Ok(ToolOutput::json(&json!({
                    "path": note.meta.path,
                    "title": note.meta.title,
                    "tags": note.meta.tags,
                    "aliases": note.meta.aliases,
                    "modified": note.meta.modified,
                    "frontmatter": note.frontmatter,
                    "body": note.body,
                    "headings": note.scan.headings,
                    "links": links,
                    "backlinks": backlinks,
                })))
            }

            "create_note" => {
                let path = require_str(args, "path")?;
                let body = opt_str(args, "content").unwrap_or_default();
                let tags = opt_str_list(args, "tags");
                let aliases = opt_str_list(args, "aliases");

                let mut fm = Map::new();
                if let Some(title) = opt_str(args, "title") {
                    fm.insert("title".into(), Value::String(title));
                }
                if !tags.is_empty() {
                    fm.insert("tags".into(), json!(tags));
                }
                if !aliases.is_empty() {
                    fm.insert("aliases".into(), json!(aliases));
                }
                fm.insert("created".into(), Value::Number(now_millis().into()));

                let content = frontmatter::compose(&fm, &body);
                let written = entry.vault().create_note(&path, &content).map_err(|err| err.to_string())?;
                entry.invalidate();
                Ok(ToolOutput::json(&json!({ "created": written })))
            }

            "edit_note" => {
                let path = require_str(args, "path")?;
                let find = args.get("find").and_then(Value::as_str).unwrap_or_default();
                if find.is_empty() {
                    return Err("`find` must not be empty".into());
                }
                let replace = args.get("replace").and_then(Value::as_str).unwrap_or_default();
                let replace_all = opt_bool(args, "replace_all", false);

                let raw = entry.vault().read_raw(&path).map_err(|err| err.to_string())?;
                let occurrences = raw.matches(find).count();
                match occurrences {
                    0 => {
                        return Err(format!(
                            "`find` does not appear in {path}. Read the note first — whitespace and line breaks must match exactly."
                        ))
                    }
                    // Refusing an ambiguous edit is the whole point: picking one
                    // occurrence arbitrarily would change the wrong line.
                    n if n > 1 && !replace_all => {
                        return Err(format!(
                            "`find` appears {n} times in {path}. Add surrounding context to make it unique, or set replace_all."
                        ))
                    }
                    _ => {}
                }

                let updated = if replace_all { raw.replace(find, replace) } else { raw.replacen(find, replace, 1) };
                let written = entry.vault().write_note(&path, &updated).map_err(|err| err.to_string())?;
                entry.invalidate();
                Ok(ToolOutput::json(&json!({
                    "edited": written,
                    "replacements": if replace_all { occurrences } else { 1 },
                })))
            }

            "write_note" => {
                let path = require_str(args, "path")?;
                let content = args.get("content").and_then(Value::as_str).unwrap_or_default();
                let existed = entry.vault().exists(&entry.vault().normalize_note(&path).map_err(|e| e.to_string())?);
                let written = entry.vault().write_note(&path, content).map_err(|err| err.to_string())?;
                entry.invalidate();
                Ok(ToolOutput::json(&json!({ "written": written, "replaced_existing": existed })))
            }

            "append_note" => {
                let path = require_str(args, "path")?;
                let text = require_str(args, "text")?;
                let written = match opt_str(args, "heading") {
                    Some(heading) => entry
                        .vault()
                        .append_under_heading(&path, &heading, &text)
                        .map_err(|err| err.to_string())?,
                    None => {
                        let mut block = text.clone();
                        if !block.ends_with('\n') {
                            block.push('\n');
                        }
                        entry.vault().append_note(&path, &block).map_err(|err| err.to_string())?
                    }
                };
                entry.invalidate();
                Ok(ToolOutput::json(&json!({ "appended_to": written })))
            }

            "update_frontmatter" => {
                let path = require_str(args, "path")?;
                let mut note = entry.vault().read_note(&path).map_err(|err| err.to_string())?;
                if let Some(Value::Object(set)) = args.get("set") {
                    for (key, value) in set {
                        note.frontmatter.insert(key.clone(), value.clone());
                    }
                }
                for key in opt_str_list(args, "remove") {
                    note.frontmatter.remove(&key);
                }
                let content = note.to_content();
                let written = entry.vault().write_note(&note.meta.path, &content).map_err(|err| err.to_string())?;
                entry.invalidate();
                Ok(ToolOutput::json(&json!({ "updated": written, "frontmatter": note.frontmatter })))
            }

            "delete_note" => {
                let path = require_str(args, "path")?;
                let permanent = opt_bool(args, "permanent", false);
                let result = if permanent {
                    let deleted = entry.vault().delete_note(&path).map_err(|err| err.to_string())?;
                    json!({ "deleted": deleted, "recoverable": false })
                } else {
                    let trashed = entry.vault().trash_note(&path).map_err(|err| err.to_string())?;
                    json!({ "deleted": path, "movedTo": trashed, "recoverable": true })
                };
                entry.invalidate();
                Ok(ToolOutput::json(&result))
            }

            "move_folder" => {
                let from = require_str(args, "from")?;
                let to = require_str(args, "to")?;
                let update_links = opt_bool(args, "update_links", true);

                let from_path = entry.vault().normalize(&from).map_err(|err| err.to_string())?;
                let to_path = entry.vault().normalize(&to).map_err(|err| err.to_string())?;

                // Worked out before the move, while the old paths still resolve.
                let moves: Vec<(String, String)> = if update_links {
                    entry
                        .vault()
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
                    entry.index().rewrite_links_for_moves(&moves)
                };

                let (_, moved_to) =
                    entry.vault().move_folder(&from_path, &to_path).map_err(|err| err.to_string())?;
                let mut relinked = Vec::new();
                for (path, content) in rewrites {
                    match entry.vault().write_note(&path, &content) {
                        Ok(written) => relinked.push(written),
                        Err(err) => eprintln!("[knowledge] link rewrite failed for {path}: {err}"),
                    }
                }
                entry.invalidate();
                Ok(ToolOutput::json(&json!({
                    "from": from_path,
                    "to": moved_to,
                    "notes_moved": moves.len(),
                    "notes_relinked": relinked,
                })))
            }

            "delete_folder" => {
                let path = require_str(args, "path")?;
                let contained = entry.vault().notes_under(&path).unwrap_or_default();
                let deleted = entry.vault().delete_folder(&path).map_err(|err| err.to_string())?;
                entry.invalidate();
                Ok(ToolOutput::json(&json!({
                    "deleted": deleted,
                    "notes_removed": contained,
                    "recoverable": false,
                })))
            }

            "list_orphans" => {
                let orphans = entry.index().orphans();
                Ok(ToolOutput::json(&json!({ "count": orphans.len(), "orphans": orphans })))
            }

            "move_note" => {
                let from = require_str(args, "from")?;
                let to = require_str(args, "to")?;
                let update_links = opt_bool(args, "update_links", true);

                let from_path = entry.vault().normalize_note(&from).map_err(|err| err.to_string())?;
                let to_path = entry.vault().normalize_note(&to).map_err(|err| err.to_string())?;

                // Work out the rewrites against the pre-move index, where the old
                // links still resolve, then move and apply them.
                let rewrites = if update_links {
                    entry.index().rewrite_links_for_move(&from_path, &to_path)
                } else {
                    Vec::new()
                };

                let (moved_from, moved_to) =
                    entry.vault().move_note(&from_path, &to_path).map_err(|err| err.to_string())?;

                let mut updated = Vec::new();
                for (path, content) in rewrites {
                    match entry.vault().write_note(&path, &content) {
                        Ok(written) => updated.push(written),
                        Err(err) => eprintln!("[knowledge] link rewrite failed for {path}: {err}"),
                    }
                }
                entry.invalidate();
                Ok(ToolOutput::json(&json!({
                    "from": moved_from,
                    "to": moved_to,
                    "notes_relinked": updated,
                })))
            }

            "search_notes" => {
                let query = require_str(args, "query")?;
                let options = SearchOptions {
                    limit: opt_usize(args, "limit", 25),
                    folder: opt_str(args, "folder"),
                    tag: opt_str(args, "tag"),
                    ..SearchOptions::default()
                };
                let hits = search::search(entry.index(), &query, &options);
                Ok(ToolOutput::json(&json!({ "query": query, "count": hits.len(), "results": hits })))
            }

            "get_backlinks" => {
                let path = require_str(args, "path")?;
                let resolved = entry.vault().normalize_note(&path).map_err(|err| err.to_string())?;
                let backlinks = entry.index().backlinks(&resolved);
                Ok(ToolOutput::json(&json!({
                    "path": resolved,
                    "count": backlinks.len(),
                    "backlinks": backlinks,
                })))
            }

            "get_links" => {
                let path = require_str(args, "path")?;
                let resolved = entry.vault().normalize_note(&path).map_err(|err| err.to_string())?;
                let links = entry.index().outgoing(&resolved);
                Ok(ToolOutput::json(&json!({ "path": resolved, "count": links.len(), "links": links })))
            }

            "list_tags" => {
                let tags: Vec<Value> = entry
                    .index()
                    .tags()
                    .into_iter()
                    .map(|(tag, count)| json!({ "tag": tag, "notes": count }))
                    .collect();
                Ok(ToolOutput::json(&json!({ "count": tags.len(), "tags": tags })))
            }

            "unresolved_links" => {
                let limit = opt_usize(args, "limit", 100);
                let mut unresolved = entry.index().unresolved();
                let total = unresolved.len();
                unresolved.truncate(limit);
                Ok(ToolOutput::json(&json!({ "total": total, "links": unresolved })))
            }

            "create_folder" => {
                let path = require_str(args, "path")?;
                let created = entry.vault().create_folder(&path).map_err(|err| err.to_string())?;
                Ok(ToolOutput::json(&json!({ "created": created })))
            }

            other => Err(format!("unknown tool: {other}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn tools_with(name: &str, files: &[(&str, &str)]) -> VaultTools {
        let root: PathBuf = std::env::temp_dir()
            .join(format!("int-knowledge-tools-{}-{name}", std::process::id()))
            .join("vault");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        for (path, content) in files {
            let full = root.join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, content).unwrap();
        }
        VaultTools::new(Workspace::open(&[root]).unwrap())
    }

    fn call(tools: &mut VaultTools, name: &str, args: Value) -> Value {
        let output = tools.call(name, &args).expect("tool call succeeded");
        serde_json::from_str(&output.text).expect("tool returned json")
    }

    #[test]
    fn every_tool_has_a_description_and_object_schema() {
        let tools = tools_with("schemas", &[]);
        for tool in tools.tools() {
            assert!(!tool.description.is_empty(), "{} has no description", tool.name);
            assert_eq!(tool.input_schema["type"], "object", "{} schema is not an object", tool.name);
        }
    }

    #[test]
    fn single_vault_schemas_do_not_require_a_vault_argument() {
        let tools = tools_with("optional-vault", &[]);
        let read = tools.tools().into_iter().find(|t| t.name == "read_note").unwrap();
        let required: Vec<&str> = read.input_schema["required"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        assert_eq!(required, vec!["path"]);
    }

    #[test]
    fn creates_and_reads_a_note() {
        let mut tools = tools_with("create", &[]);
        let created = call(&mut tools, "create_note", json!({
            "path": "Projects/Alpha",
            "content": "# Alpha\n\nLinks to [[Beta]].\n",
            "tags": ["project"]
        }));
        assert_eq!(created["created"], "Projects/Alpha.md");

        let read = call(&mut tools, "read_note", json!({"path": "Projects/Alpha"}));
        assert_eq!(read["title"], "Alpha");
        assert_eq!(read["tags"], json!(["project"]));
        assert!(read["body"].as_str().unwrap().contains("[[Beta]]"));
        assert_eq!(read["links"][0]["resolved_path"], Value::Null);
    }

    #[test]
    fn create_refuses_to_overwrite() {
        let mut tools = tools_with("no-clobber", &[("A.md", "# A\n")]);
        assert!(tools.call("create_note", &json!({"path": "A.md"})).is_err());
    }

    #[test]
    fn appends_under_a_heading() {
        let mut tools = tools_with("append", &[("A.md", "# A\n\n## Tasks\n\n- one\n\n## Other\n")]);
        call(&mut tools, "append_note", json!({"path": "A.md", "text": "- two", "heading": "Tasks"}));
        let read = call(&mut tools, "read_note", json!({"path": "A.md"}));
        let body = read["body"].as_str().unwrap();
        assert!(body.find("- two").unwrap() < body.find("## Other").unwrap());
    }

    #[test]
    fn search_reflects_writes_immediately() {
        let mut tools = tools_with("fresh-search", &[]);
        call(&mut tools, "create_note", json!({"path": "A", "content": "unmistakable-token\n"}));
        let hits = call(&mut tools, "search_notes", json!({"query": "unmistakable-token"}));
        assert_eq!(hits["count"], 1);
        assert_eq!(hits["results"][0]["path"], "A.md");
    }

    #[test]
    fn backlinks_and_unresolved_links_line_up() {
        let mut tools = tools_with("graph", &[
            ("Alpha.md", "# Alpha\n"),
            ("Ref.md", "see [[Alpha]] and [[Ghost]]\n"),
        ]);
        let backlinks = call(&mut tools, "get_backlinks", json!({"path": "Alpha"}));
        assert_eq!(backlinks["count"], 1);
        assert_eq!(backlinks["backlinks"][0]["source"], "Ref.md");

        let unresolved = call(&mut tools, "unresolved_links", json!({}));
        assert_eq!(unresolved["total"], 1);
        assert_eq!(unresolved["links"][0]["target"], "Ghost");
    }

    #[test]
    fn moving_a_note_relinks_the_vault() {
        let mut tools = tools_with("move", &[
            ("Alpha.md", "# Alpha\n"),
            ("Ref.md", "see [[Alpha]]\n"),
        ]);
        let moved = call(&mut tools, "move_note", json!({"from": "Alpha.md", "to": "Archive/Alpha One"}));
        assert_eq!(moved["to"], "Archive/Alpha One.md");
        assert_eq!(moved["notes_relinked"], json!(["Ref.md"]));

        let ref_note = call(&mut tools, "read_note", json!({"path": "Ref.md"}));
        assert!(ref_note["body"].as_str().unwrap().contains("[[Alpha One]]"));
        assert_eq!(ref_note["links"][0]["resolved_path"], "Archive/Alpha One.md");
    }

    #[test]
    fn frontmatter_updates_leave_the_body_alone() {
        let mut tools = tools_with("fm", &[("A.md", "---\ntitle: A\nstatus: draft\n---\n\nBody text\n")]);
        call(&mut tools, "update_frontmatter", json!({
            "path": "A.md",
            "set": {"status": "live"},
            "remove": ["title"]
        }));
        let read = call(&mut tools, "read_note", json!({"path": "A.md"}));
        assert_eq!(read["frontmatter"]["status"], "live");
        assert!(read["frontmatter"].get("title").is_none());
        assert!(read["body"].as_str().unwrap().contains("Body text"));
    }

    #[test]
    fn edits_a_unique_fragment_in_place() {
        let mut tools = tools_with("edit", &[("A.md", "# A\n\nstatus: draft\n\nbody text\n")]);
        let result = call(&mut tools, "edit_note", json!({
            "path": "A.md", "find": "status: draft", "replace": "status: live"
        }));
        assert_eq!(result["replacements"], 1);
        let read = call(&mut tools, "read_note", json!({"path": "A.md"}));
        let body = read["body"].as_str().unwrap();
        assert!(body.contains("status: live"));
        assert!(body.contains("body text"), "the rest of the note must survive");
    }

    #[test]
    fn refuses_an_ambiguous_edit() {
        let mut tools = tools_with("edit-ambiguous", &[("A.md", "todo\ntodo\n")]);
        let err = tools
            .call("edit_note", &json!({"path": "A.md", "find": "todo", "replace": "done"}))
            .unwrap_err();
        assert!(err.contains("appears 2 times"), "{err}");

        // Explicit opt-in replaces every occurrence.
        let all = call(&mut tools, "edit_note", json!({
            "path": "A.md", "find": "todo", "replace": "done", "replace_all": true
        }));
        assert_eq!(all["replacements"], 2);
    }

    #[test]
    fn refuses_an_edit_that_does_not_match() {
        let mut tools = tools_with("edit-missing", &[("A.md", "# A\n")]);
        let err = tools
            .call("edit_note", &json!({"path": "A.md", "find": "nowhere", "replace": "x"}))
            .unwrap_err();
        assert!(err.contains("does not appear"), "{err}");
    }

    #[test]
    fn delete_is_recoverable_by_default() {
        let mut tools = tools_with("trash", &[("A.md", "# A\n")]);
        let result = call(&mut tools, "delete_note", json!({"path": "A.md"}));
        assert_eq!(result["recoverable"], true);
        assert_eq!(result["movedTo"], ".trash/A.md");
        // Out of the vault's view, but still on disk.
        assert_eq!(call(&mut tools, "list_notes", json!({}))["count"], 0);
    }

    #[test]
    fn permanent_delete_is_marked_unrecoverable() {
        let mut tools = tools_with("erase", &[("A.md", "# A\n")]);
        let result = call(&mut tools, "delete_note", json!({"path": "A.md", "permanent": true}));
        assert_eq!(result["recoverable"], false);
    }

    #[test]
    fn moving_a_folder_relinks_the_vault() {
        let mut tools = tools_with("folder-move", &[
            ("Projects/Alpha.md", "# Alpha\n"),
            ("Ref.md", "see [[Alpha]]\n"),
        ]);
        let moved = call(&mut tools, "move_folder", json!({"from": "Projects", "to": "Archive/Projects"}));
        assert_eq!(moved["notes_moved"], 1);

        let read = call(&mut tools, "read_note", json!({"path": "Ref.md"}));
        // The bare name still resolves, so the link need not have been rewritten —
        // what matters is that it still points at the note in its new home.
        assert_eq!(read["links"][0]["resolved_path"], "Archive/Projects/Alpha.md");
    }

    #[test]
    fn deleting_a_folder_reports_what_it_removed() {
        let mut tools = tools_with("folder-delete", &[
            ("Old/A.md", "# A\n"),
            ("Old/B.md", "# B\n"),
            ("Keep.md", "# Keep\n"),
        ]);
        let deleted = call(&mut tools, "delete_folder", json!({"path": "Old"}));
        assert_eq!(deleted["notes_removed"].as_array().unwrap().len(), 2);
        assert_eq!(call(&mut tools, "list_notes", json!({}))["count"], 1);
    }

    #[test]
    fn lists_orphaned_notes() {
        let mut tools = tools_with("orphans", &[
            ("Linked.md", "# Linked\n"),
            ("Ref.md", "see [[Linked]]\n"),
            ("Alone.md", "# Alone\n"),
        ]);
        let orphans = call(&mut tools, "list_orphans", json!({}));
        assert_eq!(orphans["orphans"], json!(["Alone.md"]));
    }

    #[test]
    fn paths_outside_the_vault_are_refused() {
        let mut tools = tools_with("escape", &[]);
        assert!(tools.call("read_note", &json!({"path": "../../etc/passwd"})).is_err());
        assert!(tools.call("write_note", &json!({"path": "/etc/x", "content": "x"})).is_err());
    }

    #[test]
    fn unknown_tools_are_reported() {
        let mut tools = tools_with("unknown", &[]);
        assert!(tools.call("nope", &json!({})).is_err());
    }
}
