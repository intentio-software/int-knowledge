//! Full-text search over an indexed vault.
//!
//! Deliberately simple: case-insensitive term matching with a score that favours
//! titles, tags and paths over body text, plus a snippet per hit. No stemming,
//! no ranking model — a vault is small enough that predictability beats cleverness,
//! and an agent calling this needs to trust that quoting a phrase finds that phrase.

use serde::{Deserialize, Serialize};

use crate::index::VaultIndex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub path: String,
    pub title: String,
    pub score: u32,
    /// Matching lines with 1-based line numbers, capped per note.
    pub matches: Vec<SearchMatch>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchMatch {
    pub line: usize,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct SearchOptions {
    /// Maximum notes returned.
    pub limit: usize,
    /// Maximum matching lines shown per note.
    pub max_matches_per_note: usize,
    /// Restrict to notes whose path starts with this prefix.
    pub folder: Option<String>,
    /// Restrict to notes carrying this tag (nested tags included).
    pub tag: Option<String>,
}

impl Default for SearchOptions {
    fn default() -> Self {
        SearchOptions { limit: 25, max_matches_per_note: 3, folder: None, tag: None }
    }
}

/// Search the vault. All terms must appear somewhere in a note for it to match.
///
/// Quoted segments (`"exact phrase"`) are matched verbatim.
pub fn search(index: &VaultIndex, query: &str, options: &SearchOptions) -> Vec<SearchHit> {
    let terms = parse_terms(query);
    if terms.is_empty() {
        return Vec::new();
    }

    let folder = options.folder.as_ref().map(|f| {
        let trimmed = f.trim_matches('/');
        if trimmed.is_empty() { String::new() } else { format!("{trimmed}/") }
    });

    let mut hits: Vec<SearchHit> = Vec::new();
    for note in index.notes() {
        if let Some(prefix) = &folder {
            if !prefix.is_empty() && !note.meta.path.starts_with(prefix.as_str()) {
                continue;
            }
        }
        if let Some(tag) = &options.tag {
            let needle = tag.trim_start_matches('#').to_lowercase();
            let tagged = note.meta.tags.iter().any(|t| {
                let t = t.to_lowercase();
                t == needle || t.starts_with(&format!("{needle}/"))
            });
            if !tagged {
                continue;
            }
        }

        let title = note.meta.title.to_lowercase();
        let path = note.meta.path.to_lowercase();
        let body = note.body.to_lowercase();
        let tags = note.meta.tags.join(" ").to_lowercase();

        // Every term must land somewhere, otherwise this is not a hit.
        if !terms.iter().all(|term| {
            title.contains(term) || path.contains(term) || body.contains(term) || tags.contains(term)
        }) {
            continue;
        }

        let mut score = 0u32;
        for term in &terms {
            if title == *term {
                score += 100;
            } else if title.contains(term) {
                score += 40;
            }
            if tags.contains(term) {
                score += 25;
            }
            if path.contains(term) {
                score += 10;
            }
            score += (body.matches(term.as_str()).count() as u32).min(20);
        }

        let matches = collect_matches(&note.body, &terms, options.max_matches_per_note);
        hits.push(SearchHit { path: note.meta.path.clone(), title: note.meta.title.clone(), score, matches });
    }

    hits.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.path.cmp(&b.path)));
    hits.truncate(options.limit);
    hits
}

/// Split a query into lowercase terms, honouring `"quoted phrases"`.
fn parse_terms(query: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for ch in query.chars() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                if !in_quotes && !current.trim().is_empty() {
                    terms.push(current.trim().to_lowercase());
                    current.clear();
                }
            }
            c if c.is_whitespace() && !in_quotes => {
                if !current.trim().is_empty() {
                    terms.push(current.trim().to_lowercase());
                }
                current.clear();
            }
            c => current.push(c),
        }
    }
    if !current.trim().is_empty() {
        terms.push(current.trim().to_lowercase());
    }
    terms.retain(|t| !t.is_empty());
    terms.dedup();
    terms
}

fn collect_matches(body: &str, terms: &[String], limit: usize) -> Vec<SearchMatch> {
    let mut matches = Vec::new();
    for (index, line) in body.lines().enumerate() {
        if matches.len() >= limit {
            break;
        }
        let lowered = line.to_lowercase();
        if terms.iter().any(|term| lowered.contains(term)) {
            let text = line.trim();
            if text.is_empty() {
                continue;
            }
            matches.push(SearchMatch { line: index + 1, text: truncate(text, 240) });
        }
    }
    matches
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let clipped: String = text.chars().take(max).collect();
    format!("{clipped}…")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::Vault;
    use std::fs;

    fn index_with(name: &str, files: &[(&str, &str)]) -> VaultIndex {
        let dir = std::env::temp_dir().join(format!("int-search-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let vault = Vault::create(&dir).expect("vault");
        for (path, content) in files {
            vault.write_note(path, content).expect("write");
        }
        VaultIndex::build(&vault)
    }

    #[test]
    fn requires_every_term() {
        let index = index_with("all-terms", &[
            ("A.md", "alpha and beta\n"),
            ("B.md", "alpha only\n"),
        ]);
        let hits = search(&index, "alpha beta", &SearchOptions::default());
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, "A.md");
    }

    #[test]
    fn ranks_title_matches_above_body_matches() {
        let index = index_with("ranking", &[
            ("Roadmap.md", "nothing relevant here\n"),
            ("Other.md", "the roadmap is mentioned once\n"),
        ]);
        let hits = search(&index, "roadmap", &SearchOptions::default());
        assert_eq!(hits[0].path, "Roadmap.md");
    }

    #[test]
    fn matches_quoted_phrases_verbatim() {
        let index = index_with("phrase", &[
            ("A.md", "ship the release train\n"),
            ("B.md", "release the ship train\n"),
        ]);
        let hits = search(&index, "\"release train\"", &SearchOptions::default());
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, "A.md");
    }

    #[test]
    fn returns_line_numbers_for_matches() {
        let index = index_with("lines", &[("A.md", "one\ntwo\nalpha here\n")]);
        let hits = search(&index, "alpha", &SearchOptions::default());
        assert_eq!(hits[0].matches[0].line, 3);
        assert_eq!(hits[0].matches[0].text, "alpha here");
    }

    #[test]
    fn filters_by_folder_and_tag() {
        let index = index_with("filters", &[
            ("Work/A.md", "---\ntags: [work]\n---\n\nalpha\n"),
            ("Home/B.md", "alpha\n"),
        ]);
        let folder = SearchOptions { folder: Some("Work".into()), ..Default::default() };
        assert_eq!(search(&index, "alpha", &folder).len(), 1);
        let tag = SearchOptions { tag: Some("work".into()), ..Default::default() };
        assert_eq!(search(&index, "alpha", &tag)[0].path, "Work/A.md");
    }

    #[test]
    fn empty_query_finds_nothing() {
        let index = index_with("empty", &[("A.md", "alpha\n")]);
        assert!(search(&index, "   ", &SearchOptions::default()).is_empty());
    }
}
