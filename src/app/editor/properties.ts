/**
 * Frontmatter turned into the property rows shown above a note in read mode.
 *
 * Obsidian infers a type per property and renders each one differently — tags
 * as clickable pills, checkboxes as boxes, lists as separate values. The type
 * is not declared anywhere in the file, so it is inferred from the value here in
 * the same way, with the key name breaking ties (`tags` is a list of tags, not a
 * list of strings).
 */

export type PropertyKind = "text" | "list" | "tags" | "number" | "checkbox" | "date" | "link";

export interface PropertyValue {
  text: string;
  /** Set when the value should be clickable: a tag name or a wikilink target. */
  tag?: string;
  link?: string;
}

export interface Property {
  key: string;
  kind: PropertyKind;
  values: PropertyValue[];
  /** True for a checked checkbox; only meaningful when kind is "checkbox". */
  checked?: boolean;
}

/** Keys whose values are tags however they are written. */
const TAG_KEYS = new Set(["tag", "tags"]);

/** `2026-07-29`, optionally with a time — what a date picker would produce. */
const DATE_PATTERN = /^\d{4}-\d{2}-\d{2}([T ]\d{2}:\d{2}(:\d{2})?)?/;

const WIKILINK_PATTERN = /^\[\[([^\]|]+)(?:\|([^\]]+))?\]\]$/;

/** The icon for each kind, matching the names used elsewhere in the app. */
export const PROPERTY_ICONS: Record<PropertyKind, string> = {
  text: "pi-align-left",
  list: "pi-list",
  tags: "pi-tag",
  number: "pi-hashtag",
  checkbox: "pi-check-square",
  date: "pi-calendar",
  link: "pi-link"
};

/**
 * Build the display rows for a note's frontmatter.
 *
 * Order follows the file rather than being sorted: the order properties are
 * written in is the author's, and reordering them would make the rendered note
 * disagree with its own source.
 */
export function buildProperties(frontmatter: Record<string, unknown> | undefined | null): Property[] {
  if (!frontmatter) {
    return [];
  }

  const properties: Property[] = [];
  for (const [key, raw] of Object.entries(frontmatter)) {
    // An empty property is still worth a row — Obsidian shows the key with no
    // value, which is the cue that it is there and waiting to be filled in.
    if (raw === null || raw === undefined) {
      properties.push({ key, kind: "text", values: [] });
      continue;
    }

    if (typeof raw === "boolean") {
      properties.push({ key, kind: "checkbox", values: [{ text: raw ? "true" : "false" }], checked: raw });
      continue;
    }

    if (typeof raw === "number") {
      properties.push({ key, kind: "number", values: [{ text: String(raw) }] });
      continue;
    }

    if (Array.isArray(raw)) {
      const tags = TAG_KEYS.has(key.toLowerCase());
      const values = raw
        .filter((item) => item !== null && item !== undefined)
        .map((item) => valueFor(String(item), tags));
      properties.push({ key, kind: tags ? "tags" : "list", values });
      continue;
    }

    if (typeof raw === "object") {
      // Nested maps have no obvious row layout; show the source instead of
      // dropping the property silently.
      properties.push({ key, kind: "text", values: [{ text: JSON.stringify(raw) }] });
      continue;
    }

    const text = String(raw);
    if (TAG_KEYS.has(key.toLowerCase())) {
      // `tags: one two` and `tags: one, two` are both common in the wild.
      const parts = text.split(/[,\s]+/).filter(Boolean);
      properties.push({ key, kind: "tags", values: parts.map((part) => valueFor(part, true)) });
      continue;
    }

    const wikilink = WIKILINK_PATTERN.exec(text.trim());
    if (wikilink) {
      properties.push({ key, kind: "link", values: [{ text: wikilink[2] ?? wikilink[1], link: wikilink[1] }] });
      continue;
    }

    if (DATE_PATTERN.test(text)) {
      properties.push({ key, kind: "date", values: [{ text }] });
      continue;
    }

    properties.push({ key, kind: "text", values: [{ text }] });
  }

  return properties;
}

/** One entry of a list, as a tag or as plain text. */
function valueFor(raw: string, asTag: boolean): PropertyValue {
  const text = raw.trim();
  if (asTag) {
    const tag = text.replace(/^#/, "");
    return { text: `#${tag}`, tag };
  }
  const wikilink = WIKILINK_PATTERN.exec(text);
  if (wikilink) {
    return { text: wikilink[2] ?? wikilink[1], link: wikilink[1] };
  }
  return { text };
}
