//! A single markdown note: its frontmatter, body, and the structure inside it.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::frontmatter;
use crate::links::{self, Scan};

/// Lightweight description of a note, safe to list in bulk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoteMeta {
    /// Vault-relative path with forward slashes, e.g. `Projects/Alpha.md`.
    pub path: String,
    /// Frontmatter `title`, else the first H1, else the file stem.
    pub title: String,
    /// Alternate names this note answers to, from frontmatter `aliases`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    /// Tags from frontmatter and body, deduplicated.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// File size in bytes.
    pub size: u64,
    /// Last modification time, milliseconds since the Unix epoch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<u64>,
}

/// A note with its contents loaded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Note {
    #[serde(flatten)]
    pub meta: NoteMeta,
    /// Parsed frontmatter, empty when the note has none.
    #[serde(default)]
    pub frontmatter: Map<String, Value>,
    /// Note text with the frontmatter block removed.
    pub body: String,
    /// Links, tags, headings and block ids found in the body.
    pub scan: Scan,
}

impl Note {
    /// Build a note from raw file contents.
    pub fn parse(path: &str, content: &str, size: u64, modified: Option<u64>) -> Self {
        let split = frontmatter::split(content);
        let fm = split.raw.map(frontmatter::parse).unwrap_or_default();
        let body = split.body.to_string();
        let scan = links::scan(&body);

        let title = title_from(path, &fm, &scan);
        let aliases = string_list(fm.get("aliases")).into_iter().chain(string_list(fm.get("alias"))).collect();
        let tags = merge_tags(&fm, &scan);

        Note {
            meta: NoteMeta { path: path.to_string(), title, aliases, tags, size, modified },
            frontmatter: fm,
            body,
            scan,
        }
    }

    /// Re-render the note to the text that should be written to disk.
    pub fn to_content(&self) -> String {
        frontmatter::compose(&self.frontmatter, &self.body)
    }

    /// Filename stem, used for wikilink resolution.
    pub fn stem(&self) -> &str {
        let name = self.meta.path.rsplit('/').next().unwrap_or(&self.meta.path);
        name.strip_suffix(".md").unwrap_or(name)
    }
}

/// A note's title, in the order a reader would expect it to win.
fn title_from(path: &str, fm: &Map<String, Value>, scan: &Scan) -> String {
    if let Some(Value::String(title)) = fm.get("title") {
        let title = title.trim();
        if !title.is_empty() {
            return title.to_string();
        }
    }
    if let Some(h1) = scan.headings.iter().find(|h| h.level == 1) {
        return h1.text.clone();
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    name.strip_suffix(".md").unwrap_or(name).to_string()
}

/// Read a frontmatter field that may be a single string or a list of them.
pub fn string_list(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::String(single)) => {
            // `tags: a, b` is common enough to be worth splitting.
            single.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).map(str::to_string).collect()
        }
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| match item {
                Value::String(s) => Some(s.trim().to_string()),
                Value::Number(n) => Some(n.to_string()),
                _ => None,
            })
            .filter(|s| !s.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

fn merge_tags(fm: &Map<String, Value>, scan: &Scan) -> Vec<String> {
    let mut tags: Vec<String> = string_list(fm.get("tags"))
        .into_iter()
        .chain(string_list(fm.get("tag")))
        .map(|t| t.trim_start_matches('#').to_string())
        .filter(|t| !t.is_empty())
        .collect();
    tags.extend(scan.tags.iter().map(|t| t.name.clone()));
    tags.sort();
    tags.dedup();
    tags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_frontmatter_title() {
        let note = Note::parse("a/b.md", "---\ntitle: Real Title\n---\n\n# Heading\n", 0, None);
        assert_eq!(note.meta.title, "Real Title");
    }

    #[test]
    fn falls_back_to_h1_then_filename() {
        let with_h1 = Note::parse("a/b.md", "# From Heading\n", 0, None);
        assert_eq!(with_h1.meta.title, "From Heading");
        let bare = Note::parse("a/My Note.md", "text only\n", 0, None);
        assert_eq!(bare.meta.title, "My Note");
    }

    #[test]
    fn merges_frontmatter_and_inline_tags() {
        let note = Note::parse("a.md", "---\ntags: [alpha, beta]\n---\n\n#beta #gamma\n", 0, None);
        assert_eq!(note.meta.tags, vec!["alpha", "beta", "gamma"]);
    }

    #[test]
    fn accepts_comma_separated_tag_strings() {
        let note = Note::parse("a.md", "---\ntags: alpha, beta\n---\n", 0, None);
        assert_eq!(note.meta.tags, vec!["alpha", "beta"]);
    }

    #[test]
    fn collects_aliases() {
        let note = Note::parse("a.md", "---\naliases:\n  - Alpha\n  - A1\n---\n", 0, None);
        assert_eq!(note.meta.aliases, vec!["Alpha", "A1"]);
    }

    #[test]
    fn round_trips_content() {
        let source = "---\ntitle: Hello\n---\n\n# Hello\n\nBody [[Link]].\n";
        let note = Note::parse("a.md", source, 0, None);
        assert_eq!(note.to_content(), source);
    }

    #[test]
    fn round_trips_content_without_frontmatter() {
        let source = "# Hello\n\nBody.\n";
        assert_eq!(Note::parse("a.md", source, 0, None).to_content(), source);
    }
}
