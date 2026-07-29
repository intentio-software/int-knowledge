import MarkdownIt from "markdown-it";
import type StateInline from "markdown-it/lib/rules_inline/state_inline.mjs";

/**
 * Markdown → HTML for read mode.
 *
 * Raw HTML in notes is **not** rendered (`html: false`). A vault is written to
 * by AI agents as well as by hand, and this HTML lands in a webview that can
 * reach the Tauri IPC bridge — so an agent-authored `<script>` or `<img onerror>`
 * would be a genuine injection path. Everything below emits escaped text.
 */

export interface RenderContext {
  /** Resolve a wikilink target to a vault path, or null when it does not exist. */
  resolve: (target: string) => string | null;
  /** Turn a vault-relative attachment path into a URL the webview can load. */
  assetUrl: (path: string) => string;
  /**
   * Markdown to inline for `![[Note]]`, keyed by resolved path. Bodies are
   * fetched before rendering because markdown-it renders synchronously.
   */
  embedded?: Map<string, string>;
  /** Titles for embedded notes, shown as the transclusion header. */
  titles?: Map<string, string>;
}

/** How deep `![[embeds]]` may nest before rendering stops descending. */
export const MAX_EMBED_DEPTH = 4;

/** Extract a single `## Heading` section from a note body. */
export function sectionOf(body: string, heading: string): string {
  const lines = body.split("\n");
  const wanted = heading.trim().toLowerCase();
  let start = -1;
  let level = 0;

  for (let i = 0; i < lines.length; i += 1) {
    const match = /^(#{1,6})\s+(.*?)\s*#*\s*$/.exec(lines[i]);
    if (!match) {
      continue;
    }
    if (start < 0 && match[2].trim().toLowerCase() === wanted) {
      start = i;
      level = match[1].length;
      continue;
    }
    // The section ends at the next heading of the same or higher rank.
    if (start >= 0 && match[1].length <= level) {
      return lines.slice(start, i).join("\n").trim();
    }
  }
  return start >= 0 ? lines.slice(start).join("\n").trim() : "";
}

/** Attachment extensions rendered inline rather than linked. */
const IMAGE_EXTENSIONS = ["png", "jpg", "jpeg", "gif", "webp", "svg", "avif", "bmp"];

export function createRenderer(): MarkdownIt {
  const md = new MarkdownIt({
    html: false,
    linkify: true,
    breaks: false,
    typographer: false
  });

  installWikilinks(md);
  installTags(md);
  installTaskLists(md);
  installExternalLinks(md);
  return md;
}

/**
 * Render a note body to HTML.
 *
 * The context is passed through markdown-it's `env`, so one renderer instance
 * serves every note rather than being rebuilt per render.
 */
export function renderMarkdown(md: MarkdownIt, body: string, context: RenderContext): string {
  return md.render(body, { intentio: context, depth: 0, visited: new Set<string>() } as Record<string, unknown>);
}

interface RenderEnv {
  intentio?: RenderContext;
  depth: number;
  /** Paths already being rendered up this branch, so a cycle cannot recurse. */
  visited: Set<string>;
}

function envOf(env: unknown): RenderEnv {
  const value = env as Partial<RenderEnv> | null;
  return {
    intentio: value?.intentio,
    depth: value?.depth ?? 0,
    visited: value?.visited ?? new Set<string>()
  };
}

function contextOf(env: unknown): RenderContext | null {
  return envOf(env).intentio ?? null;
}

// ---------------------------------------------------------------------------
// wikilinks
// ---------------------------------------------------------------------------

/**
 * `[[Target]]`, `[[Target|Alias]]`, `[[Target#Heading]]` and `![[Embed]]`.
 *
 * Implemented as an inline rule rather than a pre-pass over the source, so that
 * `` `[[not a link]]` `` inside a code span is left alone for free.
 */
function installWikilinks(md: MarkdownIt): void {
  md.inline.ruler.before("link", "wikilink", (state: StateInline, silent: boolean) => {
    const start = state.pos;
    const embed = state.src.charCodeAt(start) === 0x21; /* ! */
    const open = embed ? start + 1 : start;

    if (state.src.charCodeAt(open) !== 0x5b || state.src.charCodeAt(open + 1) !== 0x5b) {
      return false;
    }
    const close = state.src.indexOf("]]", open + 2);
    if (close < 0) {
      return false;
    }
    const inner = state.src.slice(open + 2, close);
    // A newline means the brackets were never a link to begin with.
    if (!inner.trim() || inner.includes("\n")) {
      return false;
    }

    if (!silent) {
      const [pathPart, alias] = splitOnce(inner, "|");
      const [target, heading] = splitOnce(pathPart, "#");
      const token = state.push("wikilink", "", 0);
      token.meta = {
        target: target.trim(),
        heading: heading?.trim() ?? null,
        alias: alias?.trim() ?? null,
        embed
      };
    }

    state.pos = close + 2;
    return true;
  });

  md.renderer.rules["wikilink"] = (tokens, index, _options, env) => {
    const meta = tokens[index].meta as {
      target: string;
      heading: string | null;
      alias: string | null;
      embed: boolean;
    };
    const context = contextOf(env);
    const label = meta.alias || meta.target || meta.heading || "";

    if (meta.embed && isImage(meta.target) && context) {
      const resolved = context.resolve(meta.target) ?? meta.target;
      return `<img class="embed" src="${escapeAttr(context.assetUrl(resolved))}" alt="${escapeHtml(label)}" />`;
    }

    const resolved = context?.resolve(meta.target) ?? null;

    if (meta.embed && resolved && context) {
      return renderTransclusion(md, resolved, meta.heading, label, context, env);
    }

    const classes = resolved ? "wikilink" : "wikilink unresolved";
    const title = resolved ? `Open ${meta.target}` : `Create ${meta.target}`;
    const prefix = meta.embed ? '<span class="embed-marker">↪</span>' : "";
    return (
      `<a class="${classes}" data-wikilink="${escapeAttr(meta.target)}" title="${escapeAttr(title)}">` +
      `${prefix}${escapeHtml(label)}</a>`
    );
  };
}

/**
 * Render `![[Note]]` by inlining that note's content.
 *
 * Two things stop this running away: a depth cap, and a visited set carried down
 * the branch so a note that embeds itself — directly or through a chain — is
 * rendered as a plain link instead of recursing forever.
 */
function renderTransclusion(
  md: MarkdownIt,
  path: string,
  heading: string | null,
  label: string,
  context: RenderContext,
  env: unknown
): string {
  const { depth, visited } = envOf(env);
  const key = heading ? `${path}#${heading.toLowerCase()}` : path;

  if (visited.has(key)) {
    return (
      `<div class="transclusion cyclic">` +
      `<a class="wikilink" data-wikilink="${escapeAttr(path)}">${escapeHtml(label)}</a>` +
      `<span class="note">already embedded above — not repeated</span></div>`
    );
  }
  if (depth >= MAX_EMBED_DEPTH) {
    return (
      `<div class="transclusion truncated">` +
      `<a class="wikilink" data-wikilink="${escapeAttr(path)}">${escapeHtml(label)}</a>` +
      `<span class="note">embed nested too deeply</span></div>`
    );
  }

  const source = context.embedded?.get(path);
  if (source === undefined) {
    // Not prefetched (or unreadable): fall back to a link rather than an error.
    return `<a class="wikilink" data-wikilink="${escapeAttr(path)}">${escapeHtml(label)}</a>`;
  }

  const content = heading ? sectionOf(source, heading) : source;
  const title = context.titles?.get(path) ?? label;

  // A fresh visited set per branch: two siblings may embed the same note, and
  // that is not a cycle.
  const childEnv = {
    intentio: context,
    depth: depth + 1,
    visited: new Set([...visited, key])
  };
  const inner = content.trim()
    ? md.render(content, childEnv as Record<string, unknown>)
    : `<p class="empty">Nothing under that heading.</p>`;

  return (
    `<div class="transclusion">` +
    `<a class="transclusion-title wikilink" data-wikilink="${escapeAttr(path)}">${escapeHtml(title)}` +
    `${heading ? ` <span class="section"># ${escapeHtml(heading)}</span>` : ""}</a>` +
    `<div class="transclusion-body">${inner}</div></div>`
  );
}

function isImage(target: string): boolean {
  const extension = target.split(".").pop()?.toLowerCase() ?? "";
  return IMAGE_EXTENSIONS.includes(extension);
}

// ---------------------------------------------------------------------------
// tags
// ---------------------------------------------------------------------------

/** `#tag` and `#nested/tag`, but never a heading or a bare number. */
function installTags(md: MarkdownIt): void {
  md.inline.ruler.push("tag", (state: StateInline, silent: boolean) => {
    const start = state.pos;
    if (state.src.charCodeAt(start) !== 0x23 /* # */) {
      return false;
    }
    // Must start a word: `C#` and `abc#def` are not tags.
    if (start > 0 && !/[\s(\[>\-*,;:]/.test(state.src[start - 1])) {
      return false;
    }

    let end = start + 1;
    while (end < state.src.length && /[\p{L}\p{N}_/-]/u.test(state.src[end])) {
      end += 1;
    }
    const name = state.src.slice(start + 1, end).replace(/\/+$/, "");
    if (!name || !/[^\d]/.test(name)) {
      return false;
    }

    if (!silent) {
      const token = state.push("tag", "", 0);
      token.content = name;
    }
    state.pos = start + 1 + name.length;
    return true;
  });

  md.renderer.rules["tag"] = (tokens, index) => {
    const name = tokens[index].content;
    return `<a class="tag" data-tag="${escapeAttr(name)}">#${escapeHtml(name)}</a>`;
  };
}

// ---------------------------------------------------------------------------
// task lists
// ---------------------------------------------------------------------------

/** Turn `- [ ]` / `- [x]` into real checkboxes, read-only in this view. */
function installTaskLists(md: MarkdownIt): void {
  const defaultRender = md.renderer.rules["text"];
  md.core.ruler.push("tasklist", (state) => {
    for (let i = 0; i < state.tokens.length; i += 1) {
      const token = state.tokens[i];
      if (token.type !== "inline" || !token.children?.length) {
        continue;
      }
      const first = token.children[0];
      if (first.type !== "text") {
        continue;
      }
      const match = /^\[([ xX])\]\s+/.exec(first.content);
      if (!match) {
        continue;
      }
      // Only inside list items, otherwise `[x]` in prose would become a box.
      const parent = state.tokens[i - 1];
      if (!parent || parent.type !== "paragraph_open" || !parent.hidden) {
        continue;
      }
      first.content = first.content.slice(match[0].length);
      const checkbox = new state.Token("html_inline", "", 0);
      const checked = match[1].toLowerCase() === "x";
      checkbox.content = `<input class="task" type="checkbox" disabled${checked ? " checked" : ""} /> `;
      token.children.unshift(checkbox);
    }
    return true;
  });
  // `html_inline` is normally suppressed with html:false; restore just this use.
  md.renderer.rules["html_inline"] = (tokens, index) =>
    tokens[index].content.startsWith('<input class="task"') ? tokens[index].content : escapeHtml(tokens[index].content);
  if (defaultRender) {
    md.renderer.rules["text"] = defaultRender;
  }
}

// ---------------------------------------------------------------------------
// links
// ---------------------------------------------------------------------------

/** Mark up ordinary markdown links so the view can route them correctly. */
function installExternalLinks(md: MarkdownIt): void {
  const defaultOpen =
    md.renderer.rules["link_open"] ??
    ((tokens, index, options, _env, self) => self.renderToken(tokens, index, options));

  md.renderer.rules["link_open"] = (tokens, index, options, env, self) => {
    const href = tokens[index].attrGet("href") ?? "";
    const external = /^[a-z][a-z0-9+.-]*:/i.test(href) || href.startsWith("//");
    if (external) {
      tokens[index].attrSet("data-external", href);
      tokens[index].attrSet("rel", "noreferrer noopener");
    } else {
      // A relative path is a link inside the vault; route it like a wikilink.
      tokens[index].attrSet("data-wikilink", decodeURIComponent(href.split("#")[0]));
      tokens[index].attrJoin("class", "wikilink");
    }
    return defaultOpen(tokens, index, options, env, self);
  };
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

function splitOnce(value: string, separator: string): [string, string | undefined] {
  const index = value.indexOf(separator);
  return index < 0 ? [value, undefined] : [value.slice(0, index), value.slice(index + 1)];
}

function escapeHtml(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function escapeAttr(value: string): string {
  return escapeHtml(value).replace(/'/g, "&#39;");
}
