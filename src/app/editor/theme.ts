import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { Extension } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { tags } from "@lezer/highlight";

/**
 * Editor chrome for Intentio Knowledge.
 *
 * Colours are driven by the same CSS custom properties as the rest of the app,
 * so the editor follows the light/dark switch without rebuilding extensions.
 */
export const editorTheme: Extension = EditorView.theme({
  "&": {
    height: "100%",
    fontSize: "15px",
    color: "var(--ink)",
    backgroundColor: "transparent"
  },
  ".cm-scroller": {
    fontFamily: "var(--font-body)",
    lineHeight: "1.7",
    padding: "1.75rem 0 40vh 0",
    overflow: "auto"
  },
  ".cm-content": {
    maxWidth: "48rem",
    margin: "0 auto",
    padding: "0 2rem",
    caretColor: "var(--accent)"
  },
  "&.cm-focused": { outline: "none" },
  ".cm-line": { padding: "0 2px" },
  ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--accent)", borderLeftWidth: "2px" },
  "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection": {
    backgroundColor: "var(--selection)"
  },
  ".cm-activeLine": { backgroundColor: "var(--active-line)" },
  ".cm-gutters": { display: "none" },
  ".cm-wikilink": {
    color: "var(--link)",
    textDecoration: "none",
    borderBottom: "1px solid color-mix(in srgb, var(--link) 35%, transparent)",
    cursor: "pointer"
  },
  ".cm-wikilink-unresolved": {
    color: "var(--link-unresolved)",
    borderBottomStyle: "dashed"
  },
  ".cm-tooltip": {
    backgroundColor: "var(--panel)",
    border: "1px solid var(--border)",
    borderRadius: "10px",
    overflow: "hidden",
    boxShadow: "0 18px 40px rgba(0, 0, 0, 0.35)"
  },
  ".cm-tooltip-autocomplete ul li": {
    padding: "0.4rem 0.7rem",
    fontFamily: "var(--font-body)"
  },
  ".cm-tooltip-autocomplete ul li[aria-selected]": {
    backgroundColor: "var(--accent)",
    color: "#fff"
  },
  ".cm-completionDetail": { color: "var(--ink-faint)", fontStyle: "normal", marginLeft: "0.6rem" },
  ".cm-searchMatch": { backgroundColor: "color-mix(in srgb, var(--accent) 30%, transparent)" },
  ".cm-searchMatch-selected": { backgroundColor: "color-mix(in srgb, var(--accent) 55%, transparent)" }
});

/** Markdown syntax colouring, tuned to stay readable rather than decorative. */
export const markdownHighlighting: Extension = syntaxHighlighting(
  HighlightStyle.define([
    { tag: tags.heading1, fontSize: "1.6em", fontWeight: "700", color: "var(--ink-strong)" },
    { tag: tags.heading2, fontSize: "1.35em", fontWeight: "700", color: "var(--ink-strong)" },
    { tag: tags.heading3, fontSize: "1.15em", fontWeight: "600", color: "var(--ink-strong)" },
    { tag: [tags.heading4, tags.heading5, tags.heading6], fontWeight: "600", color: "var(--ink-strong)" },
    { tag: tags.strong, fontWeight: "700", color: "var(--ink-strong)" },
    { tag: tags.emphasis, fontStyle: "italic" },
    { tag: tags.strikethrough, textDecoration: "line-through", color: "var(--ink-faint)" },
    { tag: tags.link, color: "var(--link)" },
    { tag: tags.url, color: "var(--link)" },
    { tag: tags.quote, color: "var(--ink-faint)", fontStyle: "italic" },
    { tag: tags.monospace, fontFamily: "var(--font-mono)", color: "var(--accent-soft)" },
    { tag: tags.list, color: "var(--ink)" },
    { tag: tags.meta, color: "var(--ink-faint)" },
    { tag: tags.processingInstruction, color: "var(--ink-faint)" },
    { tag: tags.contentSeparator, color: "var(--ink-faint)" }
  ])
);
