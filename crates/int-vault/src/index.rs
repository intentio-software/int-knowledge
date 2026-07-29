//! The link graph over a vault.
//!
//! Building the index reads every note once and keeps its body in memory, which
//! is what makes resolution, backlinks and search cheap afterwards. Notes are
//! text, so a large personal vault costs a few megabytes.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::links::{Heading, LinkRef};
use crate::note::{Note, NoteMeta};
use crate::vault::Vault;

#[derive(Debug, Clone)]
pub struct IndexedNote {
    pub meta: NoteMeta,
    /// Kept so rewrites can re-emit the note without losing its frontmatter.
    pub frontmatter: Map<String, Value>,
    pub body: String,
    pub links: Vec<LinkRef>,
    pub headings: Vec<Heading>,
}

/// An outgoing link with its resolution attempted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedLink {
    #[serde(flatten)]
    pub link: LinkRef,
    /// Vault path the link points at, or `None` when nothing matches.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_path: Option<String>,
}

/// A reference to a note from somewhere else in the vault.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Backlink {
    /// Path of the note containing the reference.
    pub source: String,
    pub source_title: String,
    /// 1-based line within the source note's body.
    pub line: usize,
    /// The line the link appears on, trimmed.
    pub context: String,
    pub embed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnresolvedLink {
    pub source: String,
    pub target: String,
    pub line: usize,
}

#[derive(Debug, Clone, Default)]
pub struct VaultIndex {
    notes: Vec<IndexedNote>,
    by_path: HashMap<String, usize>,
    /// Lowercased lookup keys (stem, path without extension, aliases).
    by_key: HashMap<String, Vec<usize>>,
}

impl VaultIndex {
    /// Read the whole vault and build the link graph.
    pub fn build(vault: &Vault) -> Self {
        let mut index = VaultIndex::default();
        for meta in vault.list_notes() {
            let Ok(Some(note)) = vault.read_note_for_index(&meta.path) else {
                continue;
            };
            index.insert(note);
        }
        index
    }

    fn insert(&mut self, note: Note) {
        let position = self.notes.len();
        let path = note.meta.path.clone();
        let stem = note.stem().to_string();
        let without_ext = strip_note_ext(&path).to_string();

        let mut keys = vec![stem.to_lowercase(), without_ext.to_lowercase(), path.to_lowercase()];
        keys.extend(note.meta.aliases.iter().map(|a| a.to_lowercase()));
        keys.sort();
        keys.dedup();
        for key in keys {
            self.by_key.entry(key).or_default().push(position);
        }

        self.by_path.insert(path, position);
        self.notes.push(IndexedNote {
            meta: note.meta,
            frontmatter: note.frontmatter,
            body: note.body,
            links: note.scan.links,
            headings: note.scan.headings,
        });
    }

    pub fn len(&self) -> usize {
        self.notes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }

    pub fn notes(&self) -> &[IndexedNote] {
        &self.notes
    }

    pub fn get(&self, path: &str) -> Option<&IndexedNote> {
        self.by_path.get(path).map(|i| &self.notes[*i])
    }

    // -----------------------------------------------------------------------
    // resolution
    // -----------------------------------------------------------------------

    /// Resolve a link target written inside `from` to a vault path.
    ///
    /// Path-like targets are tried relative to the linking note first, then the
    /// vault root. Bare names match on filename or alias, preferring a note in
    /// the same folder, then the shallowest path — the behaviour a vault user
    /// expects from `[[Alpha]]`.
    pub fn resolve(&self, from: &str, target: &str) -> Option<String> {
        let target = target.trim();
        if target.is_empty() {
            return self.by_path.contains_key(from).then(|| from.to_string());
        }

        if target.contains('/') || target.starts_with('.') {
            let from_dir = from.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");
            for base in [from_dir, ""] {
                if let Some(joined) = join_relative(base, target) {
                    if let Some(hit) = self.match_exact(&joined) {
                        return Some(hit);
                    }
                }
            }
        }

        let candidates = self.by_key.get(&target.to_lowercase())?;
        let from_dir = from.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");
        candidates
            .iter()
            .map(|i| &self.notes[*i].meta.path)
            .min_by_key(|path| {
                let dir = path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");
                let same_folder = if dir == from_dir { 0 } else { 1 };
                let depth = path.matches('/').count();
                (same_folder, depth, path.len(), (*path).clone())
            })
            .cloned()
    }

    /// Exact path match, tolerating a missing `.md`.
    fn match_exact(&self, path: &str) -> Option<String> {
        if self.by_path.contains_key(path) {
            return Some(path.to_string());
        }
        let with_ext = format!("{path}.md");
        self.by_path.contains_key(&with_ext).then_some(with_ext)
    }

    /// Every outgoing link of a note, resolved where possible.
    pub fn outgoing(&self, path: &str) -> Vec<ResolvedLink> {
        let Some(note) = self.get(path) else { return Vec::new() };
        note.links
            .iter()
            .map(|link| ResolvedLink {
                resolved_path: if link.target.is_empty() {
                    Some(path.to_string())
                } else {
                    self.resolve(path, &link.target)
                },
                link: link.clone(),
            })
            .collect()
    }

    /// Every note that links to `path`.
    pub fn backlinks(&self, path: &str) -> Vec<Backlink> {
        let mut out = Vec::new();
        for note in &self.notes {
            if note.meta.path == path {
                continue;
            }
            for link in &note.links {
                if link.target.is_empty() {
                    continue;
                }
                if self.resolve(&note.meta.path, &link.target).as_deref() != Some(path) {
                    continue;
                }
                out.push(Backlink {
                    source: note.meta.path.clone(),
                    source_title: note.meta.title.clone(),
                    line: link.line,
                    context: note.body.lines().nth(link.line - 1).unwrap_or("").trim().to_string(),
                    embed: link.embed,
                });
            }
        }
        out
    }

    /// Links that point at nothing — the vault's to-write list.
    pub fn unresolved(&self) -> Vec<UnresolvedLink> {
        let mut out = Vec::new();
        for note in &self.notes {
            for link in &note.links {
                if link.target.is_empty() {
                    continue;
                }
                if self.resolve(&note.meta.path, &link.target).is_none() {
                    out.push(UnresolvedLink {
                        source: note.meta.path.clone(),
                        target: link.target.clone(),
                        line: link.line,
                    });
                }
            }
        }
        out
    }

    /// Notes nothing links to and that link nowhere.
    pub fn orphans(&self) -> Vec<String> {
        let mut linked: HashMap<&str, bool> = HashMap::new();
        for note in &self.notes {
            for link in note.links.iter().filter(|link| !link.target.is_empty()) {
                if let Some(target) = self.resolve(&note.meta.path, &link.target) {
                    if let Some(entry) = self.by_path.get(&target) {
                        linked.insert(self.notes[*entry].meta.path.as_str(), true);
                    }
                }
            }
        }
        self.notes
            .iter()
            .filter(|note| !linked.contains_key(note.meta.path.as_str()) && note.links.is_empty())
            .map(|note| note.meta.path.clone())
            .collect()
    }

    /// Tag counts across the vault, sorted by name.
    pub fn tags(&self) -> BTreeMap<String, usize> {
        let mut counts = BTreeMap::new();
        for note in &self.notes {
            for tag in &note.meta.tags {
                *counts.entry(tag.clone()).or_insert(0) += 1;
            }
        }
        counts
    }

    /// Notes carrying a tag, including nested children (`x` matches `x/y`).
    pub fn notes_with_tag(&self, tag: &str) -> Vec<&NoteMeta> {
        let needle = tag.trim_start_matches('#').to_lowercase();
        self.notes
            .iter()
            .filter(|note| {
                note.meta.tags.iter().any(|t| {
                    let t = t.to_lowercase();
                    t == needle || t.starts_with(&format!("{needle}/"))
                })
            })
            .map(|note| &note.meta)
            .collect()
    }

    // -----------------------------------------------------------------------
    // maintenance
    // -----------------------------------------------------------------------

    /// Rewrite links across the vault after a note moved.
    ///
    /// Returns the notes whose text changed along with their new content, so
    /// the caller decides when to write. Only links that actually resolved to
    /// `from` are touched.
    pub fn rewrite_links_for_move(&self, from: &str, to: &str) -> Vec<(String, String)> {
        let new_stem = strip_note_ext(to.rsplit('/').next().unwrap_or(to)).to_string();
        // Use the bare name when it is unambiguous, otherwise the full path.
        let ambiguous = self
            .by_key
            .get(&new_stem.to_lowercase())
            .map(|hits| hits.len() > 1)
            .unwrap_or(false);
        let replacement = if ambiguous { strip_note_ext(to).to_string() } else { new_stem };

        let mut changed = Vec::new();
        for note in &self.notes {
            if note.meta.path == from {
                continue;
            }
            // Distinct spellings that all pointed at the moved note.
            let mut stale: Vec<&str> = note
                .links
                .iter()
                .filter(|link| self.resolve(&note.meta.path, &link.target).as_deref() == Some(from))
                .map(|link| link.target.as_str())
                .collect();
            stale.sort();
            stale.dedup();
            if stale.is_empty() {
                continue;
            }

            let mut body = note.body.clone();
            let mut touched = false;
            for target in stale {
                let (next, hit) = replace_wikilink_target(&body, target, &replacement);
                if hit {
                    body = next;
                    touched = true;
                }
            }
            if touched {
                changed.push((note.meta.path.clone(), crate::frontmatter::compose(&note.frontmatter, &body)));
            }
        }
        changed
    }
}

/// Swap the target of `[[old]]`, `[[old|alias]]` and `[[old#heading]]`.
///
/// Works on whole wikilinks rather than raw substrings so that renaming `Alpha`
/// never mangles `[[Alpha Beta]]`.
fn replace_wikilink_target(body: &str, old_target: &str, new_target: &str) -> (String, bool) {
    let chars: Vec<char> = body.chars().collect();
    let mut out = String::with_capacity(body.len());
    let mut i = 0usize;
    let mut hit = false;

    while i < chars.len() {
        if i + 1 < chars.len() && chars[i] == '[' && chars[i + 1] == '[' {
            if let Some(end) = find_wikilink_end(&chars, i + 2) {
                let inner: String = chars[i + 2..end].iter().collect();
                let (target, rest) = match inner.find(['|', '#']) {
                    Some(split) => (inner[..split].to_string(), inner[split..].to_string()),
                    None => (inner.clone(), String::new()),
                };
                if target.trim() == old_target.trim() {
                    out.push_str(&format!("[[{new_target}{rest}]]"));
                    i = end + 2;
                    hit = true;
                    continue;
                }
                out.push_str(&format!("[[{inner}]]"));
                i = end + 2;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }

    (out, hit)
}

fn find_wikilink_end(chars: &[char], from: usize) -> Option<usize> {
    let mut i = from;
    while i + 1 < chars.len() {
        if chars[i] == '\n' {
            return None;
        }
        if chars[i] == ']' && chars[i + 1] == ']' {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Join a possibly-dotted relative target onto a directory, refusing escapes.
fn join_relative(base_dir: &str, target: &str) -> Option<String> {
    let mut parts: Vec<&str> = if base_dir.is_empty() { Vec::new() } else { base_dir.split('/').collect() };
    for segment in target.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    if parts.is_empty() {
        return None;
    }
    Some(parts.join("/"))
}

fn strip_note_ext(path: &str) -> &str {
    for ext in crate::vault::NOTE_EXTENSIONS {
        if let Some(stripped) = path.strip_suffix(&format!(".{ext}")) {
            return stripped;
        }
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn vault_with(name: &str, files: &[(&str, &str)]) -> Vault {
        let dir = std::env::temp_dir().join(format!("int-index-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let vault = Vault::create(&dir).expect("vault");
        for (path, content) in files {
            vault.write_note(path, content).expect("write");
        }
        vault
    }

    #[test]
    fn resolves_bare_names_and_paths() {
        let vault = vault_with("resolve", &[
            ("Alpha.md", "# Alpha\n"),
            ("Projects/Beta.md", "See [[Alpha]] and [[Projects/Beta]].\n"),
        ]);
        let index = VaultIndex::build(&vault);
        assert_eq!(index.resolve("Projects/Beta.md", "Alpha").as_deref(), Some("Alpha.md"));
        assert_eq!(index.resolve("Projects/Beta.md", "Projects/Beta").as_deref(), Some("Projects/Beta.md"));
        assert_eq!(index.resolve("Alpha.md", "Nothing"), None);
    }

    #[test]
    fn prefers_a_note_in_the_same_folder() {
        let vault = vault_with("same-folder", &[
            ("Alpha.md", "# root\n"),
            ("Deep/Nested/Alpha.md", "# nested\n"),
            ("Deep/Nested/Ref.md", "[[Alpha]]\n"),
        ]);
        let index = VaultIndex::build(&vault);
        assert_eq!(index.resolve("Deep/Nested/Ref.md", "Alpha").as_deref(), Some("Deep/Nested/Alpha.md"));
        assert_eq!(index.resolve("Other.md", "Alpha").as_deref(), Some("Alpha.md"));
    }

    #[test]
    fn resolves_relative_markdown_links() {
        let vault = vault_with("relative", &[
            ("Notes/Alpha.md", "# Alpha\n"),
            ("Notes/Sub/Beta.md", "[a](../Alpha.md)\n"),
        ]);
        let index = VaultIndex::build(&vault);
        assert_eq!(index.resolve("Notes/Sub/Beta.md", "../Alpha.md").as_deref(), Some("Notes/Alpha.md"));
    }

    #[test]
    fn resolves_aliases() {
        let vault = vault_with("alias", &[
            ("Alpha.md", "---\naliases:\n  - The First One\n---\n"),
            ("Ref.md", "[[The First One]]\n"),
        ]);
        let index = VaultIndex::build(&vault);
        assert_eq!(index.resolve("Ref.md", "The First One").as_deref(), Some("Alpha.md"));
    }

    #[test]
    fn collects_backlinks_with_context() {
        let vault = vault_with("backlinks", &[
            ("Alpha.md", "# Alpha\n"),
            ("Ref.md", "intro\n\nlinks to [[Alpha]] here\n"),
        ]);
        let index = VaultIndex::build(&vault);
        let backlinks = index.backlinks("Alpha.md");
        assert_eq!(backlinks.len(), 1);
        assert_eq!(backlinks[0].source, "Ref.md");
        assert_eq!(backlinks[0].context, "links to [[Alpha]] here");
    }

    #[test]
    fn reports_unresolved_links() {
        let vault = vault_with("unresolved", &[("Ref.md", "[[Ghost]]\n")]);
        let index = VaultIndex::build(&vault);
        let unresolved = index.unresolved();
        assert_eq!(unresolved.len(), 1);
        assert_eq!(unresolved[0].target, "Ghost");
    }

    #[test]
    fn counts_tags_including_nesting() {
        let vault = vault_with("tags", &[
            ("A.md", "---\ntags: [project/alpha]\n---\n"),
            ("B.md", "#project/beta\n"),
        ]);
        let index = VaultIndex::build(&vault);
        assert_eq!(index.tags().len(), 2);
        assert_eq!(index.notes_with_tag("project").len(), 2);
        assert_eq!(index.notes_with_tag("project/alpha").len(), 1);
    }

    #[test]
    fn rewrites_wikilinks_when_a_note_moves() {
        let vault = vault_with("rewrite", &[
            ("Alpha.md", "# Alpha\n"),
            ("Ref.md", "see [[Alpha]] and [[Alpha|the first]]\n"),
        ]);
        let index = VaultIndex::build(&vault);
        let changed = index.rewrite_links_for_move("Alpha.md", "Archive/Alpha One.md");
        assert_eq!(changed.len(), 1);
        assert!(changed[0].1.contains("[[Alpha One]]"));
        assert!(changed[0].1.contains("[[Alpha One|the first]]"));
    }
}
