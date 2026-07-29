//! Extraction of the structure that makes a pile of markdown a knowledge base:
//! wikilinks, markdown links, tags, headings and block ids.
//!
//! Everything here works on the note *body* (frontmatter already stripped) and
//! reports 1-based line numbers relative to that body. Code spans and fenced
//! code blocks are skipped, so `` `[[not a link]]` `` stays inert.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LinkKind {
    /// `[[Target]]`, the vault-native form.
    Wiki,
    /// `[label](target.md)`, a relative markdown link.
    Markdown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinkRef {
    pub kind: LinkKind,
    /// Link target as written, before vault resolution.
    pub target: String,
    /// `#heading` or `#^block-id` fragment, without the leading `#`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heading: Option<String>,
    /// Display text, when it differs from the target.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    /// `![[…]]` / `![…](…)` transclusion rather than a plain link.
    pub embed: bool,
    /// 1-based line within the note body.
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TagRef {
    pub name: String,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Heading {
    pub level: usize,
    pub text: String,
    pub line: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Scan {
    pub links: Vec<LinkRef>,
    pub tags: Vec<TagRef>,
    pub headings: Vec<Heading>,
    /// `^block-id` anchors found at end of line.
    pub block_ids: Vec<String>,
}

/// Walk a note body and pull out every structural reference it contains.
pub fn scan(body: &str) -> Scan {
    let mut scan = Scan::default();
    let mut fence: Option<(char, usize)> = None;

    for (index, raw_line) in body.lines().enumerate() {
        let line_no = index + 1;
        let trimmed = raw_line.trim_start();
        let indent = raw_line.len() - trimmed.len();

        // Fenced code blocks: a closing fence must use the same character and
        // be at least as long as the one that opened the block.
        if indent < 4 {
            if let Some(marker) = fence_marker(trimmed) {
                match fence {
                    Some((ch, len)) if ch == marker.0 && marker.1 >= len => {
                        fence = None;
                        continue;
                    }
                    Some(_) => continue,
                    None => {
                        fence = Some(marker);
                        continue;
                    }
                }
            }
        }
        if fence.is_some() {
            continue;
        }

        let line = mask_inline_code(raw_line);

        if let Some(heading) = parse_heading(&line, line_no) {
            scan.headings.push(heading);
        }
        if let Some(id) = parse_block_id(&line) {
            scan.block_ids.push(id);
        }
        collect_wikilinks(&line, line_no, &mut scan.links);
        collect_markdown_links(&line, line_no, &mut scan.links);
        collect_tags(&line, line_no, &mut scan.tags);
    }

    scan
}

/// Convenience for callers that only need "what does this note point at".
pub fn link_targets(body: &str) -> Vec<String> {
    let mut targets: Vec<String> = scan(body).links.into_iter().map(|link| link.target).collect();
    targets.sort();
    targets.dedup();
    targets
}

// ---------------------------------------------------------------------------
// line-level helpers
// ---------------------------------------------------------------------------

fn fence_marker(trimmed: &str) -> Option<(char, usize)> {
    for ch in ['`', '~'] {
        let len = trimmed.chars().take_while(|c| *c == ch).count();
        if len >= 3 {
            return Some((ch, len));
        }
    }
    None
}

/// Replace inline code spans with spaces so their contents cannot match, while
/// keeping every byte offset and the line length intact.
fn mask_inline_code(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out: Vec<char> = chars.clone();
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] != '`' {
            i += 1;
            continue;
        }
        let open_len = chars[i..].iter().take_while(|c| **c == '`').count();
        let search_from = i + open_len;
        let mut j = search_from;
        let mut close: Option<usize> = None;
        while j < chars.len() {
            if chars[j] == '`' {
                let run = chars[j..].iter().take_while(|c| **c == '`').count();
                if run == open_len {
                    close = Some(j);
                    break;
                }
                j += run;
                continue;
            }
            j += 1;
        }
        match close {
            Some(end) => {
                for slot in out.iter_mut().take(end + open_len).skip(i) {
                    *slot = ' ';
                }
                i = end + open_len;
            }
            // Unclosed span: leave the rest of the line as-is.
            None => break,
        }
    }
    out.into_iter().collect()
}

fn parse_heading(line: &str, line_no: usize) -> Option<Heading> {
    let trimmed = line.trim_start();
    let level = trimmed.chars().take_while(|c| *c == '#').count();
    if level == 0 || level > 6 {
        return None;
    }
    let rest = &trimmed[level..];
    if !rest.starts_with(' ') && !rest.is_empty() {
        return None;
    }
    let text = rest.trim().trim_end_matches('#').trim().to_string();
    if text.is_empty() {
        return None;
    }
    Some(Heading { level, text, line: line_no })
}

fn parse_block_id(line: &str) -> Option<String> {
    let trimmed = line.trim_end();
    let last = trimmed.rsplit(char::is_whitespace).next()?;
    let id = last.strip_prefix('^')?;
    if id.is_empty() || !id.chars().all(|c| c.is_alphanumeric() || c == '-') {
        return None;
    }
    Some(id.to_string())
}

// ---------------------------------------------------------------------------
// links
// ---------------------------------------------------------------------------

fn collect_wikilinks(line: &str, line_no: usize, out: &mut Vec<LinkRef>) {
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0usize;
    while i + 1 < chars.len() {
        if chars[i] != '[' || chars[i + 1] != '[' {
            i += 1;
            continue;
        }
        // No close before the next `[[`: this opener is stray, step past it so
        // a well-formed link later on the line is still found.
        let Some(end) = find_close(&chars, i + 2) else {
            i += 1;
            continue;
        };
        let inner: String = chars[i + 2..end].iter().collect();
        let embed = i > 0 && chars[i - 1] == '!';
        if let Some(link) = parse_wikilink(&inner, embed, line_no) {
            out.push(link);
        }
        i = end + 2;
    }
}

fn find_close(chars: &[char], from: usize) -> Option<usize> {
    let mut i = from;
    while i + 1 < chars.len() {
        if chars[i] == ']' && chars[i + 1] == ']' {
            return Some(i);
        }
        // A newline can never appear here (we scan per line), but an unmatched
        // `[[` should not swallow a later link.
        if chars[i] == '[' && chars[i + 1] == '[' {
            return None;
        }
        i += 1;
    }
    None
}

fn parse_wikilink(inner: &str, embed: bool, line: usize) -> Option<LinkRef> {
    if inner.trim().is_empty() {
        return None;
    }
    let (path_part, alias) = match inner.split_once('|') {
        Some((path, alias)) => {
            let alias = alias.trim();
            (path, if alias.is_empty() { None } else { Some(alias.to_string()) })
        }
        None => (inner, None),
    };
    let (target, heading) = match path_part.split_once('#') {
        Some((target, fragment)) => {
            let fragment = fragment.trim();
            (target, if fragment.is_empty() { None } else { Some(fragment.to_string()) })
        }
        None => (path_part, None),
    };
    let target = target.trim().to_string();
    // `[[#Heading]]` points inside the current note; still a real reference.
    if target.is_empty() && heading.is_none() {
        return None;
    }
    Some(LinkRef { kind: LinkKind::Wiki, target, heading, alias, embed, line })
}

fn collect_markdown_links(line: &str, line_no: usize, out: &mut Vec<LinkRef>) {
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] != '[' {
            i += 1;
            continue;
        }
        // Wikilinks are handled separately; skip their brackets entirely.
        if i + 1 < chars.len() && chars[i + 1] == '[' {
            i += 2;
            continue;
        }
        let Some(label_end) = chars[i + 1..].iter().position(|c| *c == ']').map(|p| p + i + 1) else {
            break;
        };
        if label_end + 1 >= chars.len() || chars[label_end + 1] != '(' {
            i = label_end + 1;
            continue;
        }
        let Some(target_end) = matching_paren(&chars, label_end + 1) else {
            i = label_end + 1;
            continue;
        };
        let label: String = chars[i + 1..label_end].iter().collect();
        let raw_target: String = chars[label_end + 2..target_end].iter().collect();
        let embed = i > 0 && chars[i - 1] == '!';
        if let Some(link) = parse_markdown_link(&raw_target, &label, embed, line_no) {
            out.push(link);
        }
        i = target_end + 1;
    }
}

fn matching_paren(chars: &[char], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (offset, ch) in chars.iter().enumerate().skip(open) {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(offset);
                }
            }
            _ => {}
        }
    }
    None
}

fn parse_markdown_link(raw: &str, label: &str, embed: bool, line: usize) -> Option<LinkRef> {
    // Drop an optional title: `(path.md "Title")`.
    let target_part = raw.split_whitespace().next().unwrap_or("").trim();
    let target_part = target_part.trim_start_matches('<').trim_end_matches('>');
    if target_part.is_empty() || is_external(target_part) {
        return None;
    }
    let (target, heading) = match target_part.split_once('#') {
        Some((target, fragment)) if !target.is_empty() => {
            (target, if fragment.is_empty() { None } else { Some(decode(fragment)) })
        }
        _ => (target_part, None),
    };
    let target = decode(target);
    if target.is_empty() {
        return None;
    }
    let alias = if label.trim().is_empty() { None } else { Some(label.trim().to_string()) };
    Some(LinkRef { kind: LinkKind::Markdown, target, heading, alias, embed, line })
}

fn is_external(target: &str) -> bool {
    if target.starts_with('#') || target.starts_with("//") {
        return true;
    }
    // Any `scheme:` prefix (http, https, mailto, obsidian, file, …).
    match target.find(':') {
        Some(idx) if idx > 0 => {
            target[..idx].chars().all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.')
        }
        _ => false,
    }
}

/// Minimal percent-decoding, enough for the `%20` that editors emit in paths.
fn decode(text: &str) -> String {
    if !text.contains('%') {
        return text.to_string();
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok());
            if let Some(byte) = hex {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_string())
}

// ---------------------------------------------------------------------------
// tags
// ---------------------------------------------------------------------------

fn collect_tags(line: &str, line_no: usize, out: &mut Vec<TagRef>) {
    // A heading's leading `#`s are structure, not tags.
    if parse_heading(line, line_no).is_some() {
        return;
    }
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] != '#' {
            i += 1;
            continue;
        }
        let preceded_ok = i == 0 || matches!(chars[i - 1], ' ' | '\t' | '(' | '[' | '>' | '-' | '*' | ',' | ';' | ':');
        if !preceded_ok {
            i += 1;
            continue;
        }
        let start = i + 1;
        let end = start + chars[start..].iter().take_while(|c| is_tag_char(**c)).count();
        if end == start {
            i += 1;
            continue;
        }
        let name: String = chars[start..end].iter().collect();
        let name = name.trim_end_matches('/').to_string();
        // `#2026` is a number, not a tag.
        if !name.is_empty() && name.chars().any(|c| !c.is_numeric()) {
            out.push(TagRef { name, line: line_no });
        }
        i = end;
    }
}

fn is_tag_char(ch: char) -> bool {
    ch.is_alphanumeric() || matches!(ch, '-' | '_' | '/')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn targets(body: &str) -> Vec<String> {
        scan(body).links.into_iter().map(|l| l.target).collect()
    }

    #[test]
    fn finds_plain_wikilinks() {
        assert_eq!(targets("See [[Alpha]] and [[Beta]].\n"), vec!["Alpha", "Beta"]);
    }

    #[test]
    fn parses_alias_heading_and_embed() {
        let scan = scan("![[Notes/Alpha#Section|Read this]]\n");
        let link = &scan.links[0];
        assert_eq!(link.target, "Notes/Alpha");
        assert_eq!(link.heading.as_deref(), Some("Section"));
        assert_eq!(link.alias.as_deref(), Some("Read this"));
        assert!(link.embed);
    }

    #[test]
    fn keeps_block_reference_fragments() {
        let scan = scan("[[Alpha#^abc123]]\n");
        assert_eq!(scan.links[0].heading.as_deref(), Some("^abc123"));
    }

    #[test]
    fn ignores_links_inside_code() {
        let body = "```\n[[Hidden]]\n```\n`[[AlsoHidden]]` but [[Visible]]\n";
        assert_eq!(targets(body), vec!["Visible"]);
    }

    #[test]
    fn respects_fence_length_and_character() {
        let body = "````\n```\n[[Hidden]]\n```\n````\n[[Visible]]\n";
        assert_eq!(targets(body), vec!["Visible"]);
    }

    #[test]
    fn collects_relative_markdown_links_only() {
        let body = "[a](notes/alpha.md) [b](https://example.com) [c](mailto:x@y.z) [d](../beta.md#Top)\n";
        let links = scan(body).links;
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].target, "notes/alpha.md");
        assert_eq!(links[1].target, "../beta.md");
        assert_eq!(links[1].heading.as_deref(), Some("Top"));
    }

    #[test]
    fn decodes_percent_escapes_in_paths() {
        assert_eq!(targets("[a](my%20note.md)\n"), vec!["my note.md"]);
    }

    #[test]
    fn finds_headings_but_not_hash_in_text() {
        let scan = scan("# Title\n\nsome #tag here\n## Sub ##\n");
        assert_eq!(scan.headings.len(), 2);
        assert_eq!(scan.headings[0].text, "Title");
        assert_eq!(scan.headings[1].text, "Sub");
        assert_eq!(scan.headings[1].level, 2);
    }

    #[test]
    fn collects_tags_excluding_headings_and_numbers() {
        let scan = scan("# Heading\n\n#project/alpha and #2026 and mid#word\n");
        let names: Vec<_> = scan.tags.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["project/alpha"]);
    }

    #[test]
    fn finds_block_ids() {
        let scan = scan("A paragraph. ^ref-1\n");
        assert_eq!(scan.block_ids, vec!["ref-1"]);
    }

    #[test]
    fn unclosed_wikilink_does_not_eat_the_next_one() {
        assert_eq!(targets("[[broken and [[Good]]\n"), vec!["Good"]);
    }

    #[test]
    fn heading_only_link_has_empty_target() {
        let scan = scan("[[#Section]]\n");
        assert_eq!(scan.links[0].target, "");
        assert_eq!(scan.links[0].heading.as_deref(), Some("Section"));
    }
}
