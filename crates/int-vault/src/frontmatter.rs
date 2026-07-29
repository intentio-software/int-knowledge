//! A deliberately small YAML subset, sized for note frontmatter.
//!
//! Frontmatter in a knowledge vault is overwhelmingly flat scalars, lists and
//! the occasional nested map. Pulling in a full YAML engine to serve that would
//! trade a large unmaintained dependency for capability nobody writes by hand,
//! so this module parses the subset directly into `serde_json::Value` and
//! writes it back out in the same shape.
//!
//! Supported: block maps, block sequences, inline `[a, b]` and `{a: b}`,
//! quoted and bare scalars, booleans, numbers, null, and `#` comments.
//! Anything it cannot model is preserved verbatim as a string.

use serde_json::{Map, Value};

const FENCE: &str = "---";

/// The frontmatter block and body of a note, split apart.
#[derive(Debug, Clone, PartialEq)]
pub struct Split<'a> {
    /// Raw YAML between the fences, without the fences themselves.
    pub raw: Option<&'a str>,
    /// Everything after the closing fence (or the whole input when unfenced).
    pub body: &'a str,
    /// Number of lines the frontmatter block occupies, so callers can map
    /// body line numbers back onto the original file.
    pub body_line_offset: usize,
}

/// Separate a leading `---` fenced frontmatter block from the note body.
///
/// A fence only counts when it is the very first line of the file, matching
/// how editors and static site generators treat it.
pub fn split(content: &str) -> Split<'_> {
    let without_bom = content.strip_prefix('\u{feff}').unwrap_or(content);
    let first_line_end = without_bom.find('\n').map(|i| i + 1).unwrap_or(without_bom.len());
    let first_line = without_bom[..first_line_end].trim_end();

    if first_line != FENCE {
        return Split { raw: None, body: content, body_line_offset: 0 };
    }

    let rest = &without_bom[first_line_end..];
    let mut offset = 0usize;
    let mut lines = 1usize; // the opening fence

    for line in rest.split_inclusive('\n') {
        lines += 1;
        if line.trim_end() == FENCE {
            let raw = &rest[..offset];
            let body = &rest[offset + line.len()..];
            return Split { raw: Some(raw), body, body_line_offset: lines };
        }
        offset += line.len();
    }

    // An unterminated fence is not frontmatter; treat the file as all body.
    Split { raw: None, body: content, body_line_offset: 0 }
}

/// Parse a raw frontmatter block into a JSON object.
///
/// Returns an empty map for input that contains no usable keys, so callers can
/// treat "no frontmatter" and "empty frontmatter" identically.
pub fn parse(raw: &str) -> Map<String, Value> {
    let lines = collect_lines(raw);
    if lines.is_empty() {
        return Map::new();
    }
    let mut cursor = 0usize;
    match parse_block(&lines, &mut cursor, lines[0].indent) {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

/// Serialize a frontmatter object back to YAML, without the fences.
///
/// Keys are emitted in insertion order (`serde_json`'s `preserve_order` is not
/// enabled, so this is alphabetical) which keeps rewrites stable in git.
pub fn to_yaml(map: &Map<String, Value>) -> String {
    let mut out = String::new();
    for (key, value) in map {
        write_entry(&mut out, key, value, 0);
    }
    out
}

/// Render a full note from frontmatter plus body.
pub fn compose(map: &Map<String, Value>, body: &str) -> String {
    if map.is_empty() {
        return body.to_string();
    }
    let mut out = String::from("---\n");
    out.push_str(&to_yaml(map));
    out.push_str("---\n");
    if !body.starts_with('\n') && !body.is_empty() {
        out.push('\n');
    }
    out.push_str(body);
    out
}

// ---------------------------------------------------------------------------
// parsing
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct Line<'a> {
    indent: usize,
    text: &'a str,
}

fn collect_lines(raw: &str) -> Vec<Line<'_>> {
    raw.lines()
        .filter_map(|line| {
            let indent = line.len() - line.trim_start().len();
            let text = line.trim_start().trim_end();
            if text.is_empty() || text.starts_with('#') {
                return None;
            }
            Some(Line { indent, text })
        })
        .collect()
}

fn parse_block(lines: &[Line<'_>], cursor: &mut usize, indent: usize) -> Value {
    if *cursor >= lines.len() {
        return Value::Null;
    }
    if lines[*cursor].text.starts_with("- ") || lines[*cursor].text == "-" {
        parse_sequence(lines, cursor, indent)
    } else {
        parse_mapping(lines, cursor, indent)
    }
}

fn parse_sequence(lines: &[Line<'_>], cursor: &mut usize, indent: usize) -> Value {
    let mut items = Vec::new();
    while *cursor < lines.len() {
        let line = lines[*cursor];
        if line.indent < indent || !(line.text.starts_with("- ") || line.text == "-") {
            break;
        }
        if line.indent > indent {
            // Deeper than this sequence: belongs to a nested structure that the
            // item branch below already consumed. Skip to stay in step.
            *cursor += 1;
            continue;
        }
        let inline = line.text.strip_prefix("- ").unwrap_or("").trim();
        *cursor += 1;

        if inline.is_empty() {
            // `-` alone: the value is the indented block that follows.
            if *cursor < lines.len() && lines[*cursor].indent > indent {
                let child_indent = lines[*cursor].indent;
                items.push(parse_block(lines, cursor, child_indent));
            } else {
                items.push(Value::Null);
            }
            continue;
        }

        // `- key: value` starts a map that continues at the item's content indent.
        if let Some((key, rest)) = split_key(inline) {
            let content_indent = indent + 2;
            let mut map = Map::new();
            insert_entry(&mut map, key, rest, lines, cursor, content_indent);
            while *cursor < lines.len() && lines[*cursor].indent >= content_indent {
                let entry = lines[*cursor];
                if entry.text.starts_with("- ") {
                    break;
                }
                let Some((k, r)) = split_key(entry.text) else { break };
                *cursor += 1;
                insert_entry(&mut map, k, r, lines, cursor, entry.indent);
            }
            items.push(Value::Object(map));
        } else {
            items.push(scalar(inline));
        }
    }
    Value::Array(items)
}

fn parse_mapping(lines: &[Line<'_>], cursor: &mut usize, indent: usize) -> Value {
    let mut map = Map::new();
    while *cursor < lines.len() {
        let line = lines[*cursor];
        if line.indent < indent {
            break;
        }
        if line.indent > indent {
            // Orphaned deeper line; consume it rather than spinning forever.
            *cursor += 1;
            continue;
        }
        let Some((key, rest)) = split_key(line.text) else {
            *cursor += 1;
            continue;
        };
        *cursor += 1;
        insert_entry(&mut map, key, rest, lines, cursor, indent);
    }
    Value::Object(map)
}

/// Attach one `key: rest` pair, pulling in an indented block when `rest` is empty.
fn insert_entry(
    map: &mut Map<String, Value>,
    key: String,
    rest: &str,
    lines: &[Line<'_>],
    cursor: &mut usize,
    indent: usize,
) {
    if !rest.is_empty() {
        map.insert(key, scalar(rest));
        return;
    }
    // A sequence may sit at the key's own indent or deeper; a map must be deeper.
    let value = match lines.get(*cursor) {
        Some(next) if next.indent > indent => {
            let child_indent = next.indent;
            parse_block(lines, cursor, child_indent)
        }
        Some(next) if next.indent == indent && next.text.starts_with('-') => {
            parse_sequence(lines, cursor, indent)
        }
        _ => Value::Null,
    };
    map.insert(key, value);
}

/// Split `key: value` on the first structural colon, honouring quoted keys.
fn split_key(text: &str) -> Option<(String, &str)> {
    let bytes = text.as_bytes();
    let mut quote: Option<u8> = None;
    for (i, &b) in bytes.iter().enumerate() {
        match quote {
            Some(q) if b == q => quote = None,
            Some(_) => {}
            None if b == b'"' || b == b'\'' => quote = Some(b),
            None if b == b':' => {
                let is_last = i + 1 == bytes.len();
                if is_last || bytes[i + 1] == b' ' {
                    let key = unquote(text[..i].trim());
                    if key.is_empty() {
                        return None;
                    }
                    return Some((key, text[i + 1..].trim()));
                }
            }
            None => {}
        }
    }
    None
}

fn scalar(raw: &str) -> Value {
    let text = strip_comment(raw);
    if text.is_empty() || text == "~" || text.eq_ignore_ascii_case("null") {
        return Value::Null;
    }
    if text.starts_with('[') && text.ends_with(']') {
        return Value::Array(split_inline(&text[1..text.len() - 1]).into_iter().map(|s| scalar(&s)).collect());
    }
    if text.starts_with('{') && text.ends_with('}') {
        let mut map = Map::new();
        for part in split_inline(&text[1..text.len() - 1]) {
            if let Some((key, rest)) = split_key(&part) {
                map.insert(key, scalar(rest));
            }
        }
        return Value::Object(map);
    }
    if text.starts_with('"') || text.starts_with('\'') {
        return Value::String(unquote(text));
    }
    if text.eq_ignore_ascii_case("true") {
        return Value::Bool(true);
    }
    if text.eq_ignore_ascii_case("false") {
        return Value::Bool(false);
    }
    if let Ok(int) = text.parse::<i64>() {
        return Value::Number(int.into());
    }
    if let Some(float) = text.parse::<f64>().ok().and_then(serde_json::Number::from_f64) {
        return Value::Number(float);
    }
    Value::String(text.to_string())
}

/// Trim a trailing ` # comment`, but never inside quotes or a `#tag`.
fn strip_comment(text: &str) -> &str {
    let bytes = text.as_bytes();
    let mut quote: Option<u8> = None;
    for (i, &b) in bytes.iter().enumerate() {
        match quote {
            Some(q) if b == q => quote = None,
            Some(_) => {}
            None if b == b'"' || b == b'\'' => quote = Some(b),
            None if b == b'#' && i > 0 && bytes[i - 1] == b' ' => return text[..i].trim_end(),
            None => {}
        }
    }
    text.trim_end()
}

/// Split an inline collection on commas that are not nested or quoted.
fn split_inline(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    for ch in text.chars() {
        match quote {
            Some(q) if ch == q => {
                quote = None;
                current.push(ch);
            }
            Some(_) => current.push(ch),
            None => match ch {
                '"' | '\'' => {
                    quote = Some(ch);
                    current.push(ch);
                }
                '[' | '{' => {
                    depth += 1;
                    current.push(ch);
                }
                ']' | '}' => {
                    depth = depth.saturating_sub(1);
                    current.push(ch);
                }
                ',' if depth == 0 => {
                    parts.push(current.trim().to_string());
                    current.clear();
                }
                _ => current.push(ch),
            },
        }
    }
    let tail = current.trim();
    if !tail.is_empty() {
        parts.push(tail.to_string());
    }
    parts.retain(|p| !p.is_empty());
    parts
}

fn unquote(text: &str) -> String {
    let bytes = text.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            let inner = &text[1..text.len() - 1];
            return if first == b'"' {
                inner.replace("\\\"", "\"").replace("\\\\", "\\")
            } else {
                inner.replace("''", "'")
            };
        }
    }
    text.to_string()
}

// ---------------------------------------------------------------------------
// serialization
// ---------------------------------------------------------------------------

fn write_entry(out: &mut String, key: &str, value: &Value, depth: usize) {
    let pad = "  ".repeat(depth);
    match value {
        Value::Array(items) if !items.is_empty() => {
            out.push_str(&format!("{pad}{}:\n", quote_key(key)));
            for item in items {
                write_seq_item(out, item, depth + 1);
            }
        }
        Value::Object(map) if !map.is_empty() => {
            out.push_str(&format!("{pad}{}:\n", quote_key(key)));
            for (k, v) in map {
                write_entry(out, k, v, depth + 1);
            }
        }
        _ => out.push_str(&format!("{pad}{}: {}\n", quote_key(key), write_scalar(value))),
    }
}

fn write_seq_item(out: &mut String, value: &Value, depth: usize) {
    let pad = "  ".repeat(depth);
    match value {
        Value::Object(map) if !map.is_empty() => {
            let mut iter = map.iter();
            if let Some((k, v)) = iter.next() {
                match v {
                    Value::Array(_) | Value::Object(_) => {
                        out.push_str(&format!("{pad}-\n"));
                        write_entry(out, k, v, depth + 1);
                    }
                    _ => out.push_str(&format!("{pad}- {}: {}\n", quote_key(k), write_scalar(v))),
                }
            }
            for (k, v) in iter {
                write_entry(out, k, v, depth + 1);
            }
        }
        Value::Array(items) if !items.is_empty() => {
            out.push_str(&format!("{pad}-\n"));
            for item in items {
                write_seq_item(out, item, depth + 1);
            }
        }
        _ => out.push_str(&format!("{pad}- {}\n", write_scalar(value))),
    }
}

fn write_scalar(value: &Value) -> String {
    match value {
        Value::Null => "".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => quote_scalar(s),
        Value::Array(_) => "[]".to_string(),
        Value::Object(_) => "{}".to_string(),
    }
}

/// Quote only when a bare value would parse back as something else.
fn quote_scalar(text: &str) -> String {
    let needs_quotes = text.is_empty()
        || text != text.trim()
        || text.starts_with(['-', '[', '{', '&', '*', '!', '|', '>', '%', '@', '`', '"', '\''])
        || text.contains(": ")
        || text.ends_with(':')
        || text.contains(" #")
        || text.contains('\n')
        || text.eq_ignore_ascii_case("true")
        || text.eq_ignore_ascii_case("false")
        || text.eq_ignore_ascii_case("null")
        || text.parse::<f64>().is_ok();
    if needs_quotes {
        format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n"))
    } else {
        text.to_string()
    }
}

fn quote_key(key: &str) -> String {
    if key.is_empty() || key.contains([':', ' ', '#', '"', '\'']) {
        format!("\"{}\"", key.replace('"', "\\\""))
    } else {
        key.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_fenced_frontmatter() {
        let split = split("---\ntitle: Hello\n---\n\n# Body\n");
        assert_eq!(split.raw, Some("title: Hello\n"));
        assert_eq!(split.body, "\n# Body\n");
        // Opening fence, one key, closing fence — body line 1 is file line 4.
        assert_eq!(split.body_line_offset, 3);
    }

    #[test]
    fn leaves_unfenced_content_alone() {
        let split = split("# Just a note\n");
        assert_eq!(split.raw, None);
        assert_eq!(split.body, "# Just a note\n");
    }

    #[test]
    fn ignores_a_fence_that_is_not_first() {
        let split = split("intro\n---\ntitle: no\n---\n");
        assert_eq!(split.raw, None);
    }

    #[test]
    fn unterminated_fence_is_body() {
        let split = split("---\ntitle: Hello\n");
        assert_eq!(split.raw, None);
        assert_eq!(split.body, "---\ntitle: Hello\n");
    }

    #[test]
    fn parses_scalars_and_lists() {
        let map = parse("title: Hello\ndraft: true\ncount: 3\nratio: 1.5\nempty:\ntags: [a, b]\n");
        assert_eq!(map["title"], Value::String("Hello".into()));
        assert_eq!(map["draft"], Value::Bool(true));
        assert_eq!(map["count"], Value::Number(3.into()));
        assert_eq!(map["ratio"].as_f64(), Some(1.5));
        assert_eq!(map["empty"], Value::Null);
        assert_eq!(map["tags"], serde_json::json!(["a", "b"]));
    }

    #[test]
    fn parses_block_sequences_at_both_indents() {
        let flush = parse("aliases:\n- one\n- two\n");
        let nested = parse("aliases:\n  - one\n  - two\n");
        assert_eq!(flush["aliases"], serde_json::json!(["one", "two"]));
        assert_eq!(nested["aliases"], serde_json::json!(["one", "two"]));
    }

    #[test]
    fn parses_nested_maps() {
        let map = parse("meta:\n  author: Max\n  links:\n    - a\n    - b\nstatus: live\n");
        assert_eq!(map["meta"]["author"], Value::String("Max".into()));
        assert_eq!(map["meta"]["links"], serde_json::json!(["a", "b"]));
        assert_eq!(map["status"], Value::String("live".into()));
    }

    #[test]
    fn parses_sequences_of_maps() {
        let map = parse("people:\n  - name: Max\n    role: dev\n  - name: Sam\n    role: ops\n");
        assert_eq!(map["people"], serde_json::json!([
            {"name": "Max", "role": "dev"},
            {"name": "Sam", "role": "ops"}
        ]));
    }

    #[test]
    fn keeps_colons_inside_values() {
        let map = parse("url: https://example.com/a\ntime: \"12:30\"\n");
        assert_eq!(map["url"], Value::String("https://example.com/a".into()));
        assert_eq!(map["time"], Value::String("12:30".into()));
    }

    #[test]
    fn strips_comments_but_keeps_hashtags() {
        let map = parse("title: Hello # a comment\ntag: \"#project\"\n");
        assert_eq!(map["title"], Value::String("Hello".into()));
        assert_eq!(map["tag"], Value::String("#project".into()));
    }

    #[test]
    fn round_trips_through_yaml() {
        let source = "aliases:\n  - one\n  - two\ncount: 3\ndraft: true\nmeta:\n  author: Max\ntitle: Hello\n";
        let parsed = parse(source);
        let rendered = to_yaml(&parsed);
        assert_eq!(parse(&rendered), parsed);
    }

    #[test]
    fn quotes_values_that_would_change_type() {
        let mut map = Map::new();
        map.insert("a".into(), Value::String("true".into()));
        map.insert("b".into(), Value::String("2026-07-29".into()));
        map.insert("c".into(), Value::String("- dash".into()));
        let round_tripped = parse(&to_yaml(&map));
        assert_eq!(round_tripped["a"], Value::String("true".into()));
        assert_eq!(round_tripped["c"], Value::String("- dash".into()));
        assert_eq!(round_tripped["b"], map["b"]);
    }

    #[test]
    fn composes_note_with_blank_line_after_fence() {
        let mut map = Map::new();
        map.insert("title".into(), Value::String("Hello".into()));
        let note = compose(&map, "# Body\n");
        assert_eq!(note, "---\ntitle: Hello\n---\n\n# Body\n");
        assert_eq!(split(&note).body, "\n# Body\n");
    }
}
