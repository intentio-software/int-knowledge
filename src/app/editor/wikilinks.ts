import {
  Completion,
  CompletionContext,
  CompletionResult,
  autocompletion
} from "@codemirror/autocomplete";
import { Extension, RangeSetBuilder } from "@codemirror/state";
import { Decoration, DecorationSet, EditorView, ViewPlugin, ViewUpdate } from "@codemirror/view";

/**
 * Wikilink support for the markdown editor: `[[Target]]`, `[[Target|Alias]]`,
 * `[[Target#Heading]]` and `![[Target]]` embeds.
 *
 * Source-first, deliberately. The text in the buffer is exactly what lands on
 * disk — links are decorated, never rewritten — so a note an agent wrote comes
 * back byte-identical unless the user actually edits it.
 */

/** Matches a whole wikilink, capturing the inner text. */
const WIKILINK = /!?\[\[([^\]\n]+)\]\]/g;

/** Matches an unfinished `[[…` immediately before the cursor. */
const OPEN_WIKILINK = /\[\[([^\]\n|#]*)$/;

export interface WikilinkTarget {
  /** Link target as written, before resolution. */
  target: string;
  /** Whether the target resolves to a note that exists. */
  resolved: boolean;
}

export interface WikilinkHandlers {
  /** Called when the user activates a link. */
  onFollow: (target: string) => void;
  /** Vault paths a link may resolve to, for the unresolved styling. */
  isResolved: (target: string) => boolean;
  /** Candidate notes for autocomplete, newest-relevant first. */
  suggestions: () => WikilinkSuggestion[];
}

export interface WikilinkSuggestion {
  /** Text inserted into the link. */
  target: string;
  /** Note title, shown as the completion label. */
  title: string;
  /** Full vault path, shown as secondary detail. */
  path: string;
}

/** Split `Target#Heading|Alias` into just the target. */
export function targetOf(inner: string): string {
  return inner.split("|")[0].split("#")[0].trim();
}

/**
 * Decorate every wikilink in the visible viewport.
 *
 * Only visible ranges are scanned, so a very long note stays responsive.
 */
function wikilinkDecorations(handlers: WikilinkHandlers): Extension {
  return ViewPlugin.fromClass(
    class {
      decorations: DecorationSet;

      constructor(view: EditorView) {
        this.decorations = this.build(view);
      }

      update(update: ViewUpdate): void {
        if (update.docChanged || update.viewportChanged || update.selectionSet) {
          this.decorations = this.build(update.view);
        }
      }

      build(view: EditorView): DecorationSet {
        const builder = new RangeSetBuilder<Decoration>();
        for (const { from, to } of view.visibleRanges) {
          const text = view.state.doc.sliceString(from, to);
          WIKILINK.lastIndex = 0;
          let match: RegExpExecArray | null;
          while ((match = WIKILINK.exec(text)) !== null) {
            const target = targetOf(match[1]);
            if (!target) {
              continue;
            }
            const resolved = handlers.isResolved(target);
            builder.add(
              from + match.index,
              from + match.index + match[0].length,
              Decoration.mark({
                class: resolved ? "cm-wikilink" : "cm-wikilink cm-wikilink-unresolved",
                attributes: {
                  "data-wikilink": target,
                  title: resolved ? `Open ${target}` : `Create ${target}`
                }
              })
            );
          }
        }
        return builder.finish();
      }
    },
    { decorations: (plugin) => plugin.decorations }
  );
}

/**
 * Follow a link on click.
 *
 * A plain click still moves the cursor, which is what a source editor should do;
 * holding the platform's modifier follows the link instead. Anything else would
 * make it impossible to put the caret inside a link to edit it.
 */
function wikilinkClicks(handlers: WikilinkHandlers): Extension {
  return EditorView.domEventHandlers({
    mousedown(event) {
      const element = event.target as HTMLElement | null;
      const anchor = element?.closest?.("[data-wikilink]") as HTMLElement | null;
      const target = anchor?.dataset["wikilink"];
      if (!target) {
        return false;
      }
      const followed = event.metaKey || event.ctrlKey;
      if (!followed) {
        return false;
      }
      event.preventDefault();
      handlers.onFollow(target);
      return true;
    }
  });
}

/** Suggest notes after `[[`, closing the brackets on accept. */
function wikilinkCompletions(handlers: WikilinkHandlers): Extension {
  const source = (context: CompletionContext): CompletionResult | null => {
    const before = context.state.doc.sliceString(Math.max(0, context.pos - 200), context.pos);
    const open = OPEN_WIKILINK.exec(before);
    if (!open) {
      return null;
    }
    // Only fire once the user has typed `[[`, not on every keystroke.
    if (!context.explicit && open[1].length === 0 && !before.endsWith("[[")) {
      return null;
    }

    const from = context.pos - open[1].length;
    const options: Completion[] = handlers.suggestions().map((suggestion) => ({
      label: suggestion.target,
      detail: suggestion.path,
      info: suggestion.title !== suggestion.target ? suggestion.title : undefined,
      type: "text",
      apply: (view: EditorView, _completion: Completion, rangeFrom: number, rangeTo: number) => {
        // Swallow a `]]` the editor already auto-inserted, so accepting a
        // completion never leaves `[[Note]]]]` behind.
        const after = view.state.doc.sliceString(rangeTo, Math.min(view.state.doc.length, rangeTo + 2));
        const trailing = after === "]]" ? 2 : 0;
        view.dispatch({
          changes: { from: rangeFrom, to: rangeTo + trailing, insert: `${suggestion.target}]]` },
          selection: { anchor: rangeFrom + suggestion.target.length + 2 }
        });
      }
    }));

    return { from, options, filter: true };
  };

  return autocompletion({ override: [source], closeOnBlur: true, activateOnTyping: true });
}

export function wikilinks(handlers: WikilinkHandlers): Extension {
  return [wikilinkDecorations(handlers), wikilinkClicks(handlers), wikilinkCompletions(handlers)];
}
