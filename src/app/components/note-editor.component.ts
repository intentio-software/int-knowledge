import {
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  EventEmitter,
  Input,
  NgZone,
  OnChanges,
  OnDestroy,
  Output,
  SimpleChanges,
  ViewChild,
  inject
} from "@angular/core";
import { CommonModule } from "@angular/common";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { bracketMatching, indentOnInput } from "@codemirror/language";
import { highlightSelectionMatches, searchKeymap } from "@codemirror/search";
import { Compartment, EditorState } from "@codemirror/state";
import {
  EditorView,
  drawSelection,
  highlightActiveLine,
  keymap,
  placeholder as placeholderExt,
  rectangularSelection
} from "@codemirror/view";

import { NoteDetail, NoteMeta } from "../models/vault.models";
import { editorTheme, markdownHighlighting } from "../editor/theme";
import { WikilinkSuggestion, wikilinks } from "../editor/wikilinks";

/** How long the editor waits after typing stops before saving. */
const AUTOSAVE_DELAY = 700;

/**
 * The markdown editing surface.
 *
 * Owns a CodeMirror instance directly rather than rebuilding it per change, so
 * undo history and cursor position survive everything except switching notes.
 */
@Component({
  selector: "app-note-editor",
  standalone: true,
  imports: [CommonModule],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="editor-host" #host></div>
    <div class="editor-hint" *ngIf="note">
      <span><kbd>{{ modifierLabel }}</kbd> + click a link to follow it</span>
      <span class="dot">·</span>
      <span><kbd>[[</kbd> to link a note</span>
      <span class="dot">·</span>
      <span [class.dirty]="dirty">{{ dirty ? "Unsaved" : "Saved" }}</span>
    </div>
  `,
  styles: [
    `
      :host {
        display: flex;
        flex-direction: column;
        min-height: 0;
        height: 100%;
      }
      .editor-host {
        flex: 1;
        min-height: 0;
        overflow: hidden;
      }
      .editor-hint {
        display: flex;
        align-items: center;
        gap: 0.5rem;
        padding: 0.4rem 1.25rem;
        border-top: 1px solid var(--border);
        font-size: 0.75rem;
        color: var(--ink-faint);
        background: var(--panel);
      }
      .editor-hint .dot {
        opacity: 0.4;
      }
      .editor-hint .dirty {
        color: var(--accent);
      }
      kbd {
        font-family: var(--font-mono);
        font-size: 0.7rem;
        padding: 0.1rem 0.3rem;
        border: 1px solid var(--border);
        border-radius: 4px;
        background: var(--surface);
      }
    `
  ]
})
export class NoteEditorComponent implements OnChanges, OnDestroy {
  private readonly zone = inject(NgZone);

  @ViewChild("host", { static: true }) hostRef!: ElementRef<HTMLDivElement>;

  /** Note currently being edited; `null` shows an empty editor. */
  @Input() note: NoteDetail | null = null;

  /** Every note in the vault, used to power `[[` completion. */
  @Input() allNotes: NoteMeta[] = [];

  @Output() readonly contentSaved = new EventEmitter<string>();
  @Output() readonly linkFollowed = new EventEmitter<string>();
  @Output() readonly dirtyChanged = new EventEmitter<boolean>();

  dirty = false;

  readonly modifierLabel = navigator.platform.toLowerCase().includes("mac") ? "⌘" : "Ctrl";

  private view: EditorView | null = null;
  private saveTimer: ReturnType<typeof setTimeout> | null = null;
  private loadedPath: string | null = null;
  private readonly editable = new Compartment();

  ngOnChanges(changes: SimpleChanges): void {
    if (!changes["note"]) {
      return;
    }
    const path = this.note?.path ?? null;
    // Only reload the document when the note itself changed. Re-rendering the
    // same note after a save must not discard undo history or the cursor.
    if (path !== this.loadedPath) {
      this.loadNote();
    }
  }

  ngOnDestroy(): void {
    this.flushPendingSave();
    this.view?.destroy();
    this.view = null;
  }

  /** Save immediately, e.g. before the app switches notes or closes. */
  flushPendingSave(): void {
    if (this.saveTimer) {
      clearTimeout(this.saveTimer);
      this.saveTimer = null;
    }
    if (this.dirty && this.view) {
      this.emitSave(this.view.state.doc.toString());
    }
  }

  focus(): void {
    this.view?.focus();
  }

  private loadNote(): void {
    this.flushPendingSave();
    this.loadedPath = this.note?.path ?? null;
    const doc = this.note?.content ?? "";

    if (!this.view) {
      this.createView(doc);
      return;
    }
    this.view.dispatch({
      changes: { from: 0, to: this.view.state.doc.length, insert: doc },
      selection: { anchor: 0 },
      // A note switch is not an undoable edit.
      annotations: []
    });
    this.view.dispatch({ effects: this.editable.reconfigure(EditorView.editable.of(this.note !== null)) });
    this.setDirty(false);
  }

  private createView(doc: string): void {
    // CodeMirror fires its own DOM events constantly; keeping it outside Angular
    // avoids a change-detection pass on every keystroke.
    this.zone.runOutsideAngular(() => {
      const state = EditorState.create({
        doc,
        extensions: [
          history(),
          drawSelection(),
          rectangularSelection(),
          highlightActiveLine(),
          highlightSelectionMatches(),
          indentOnInput(),
          bracketMatching(),
          EditorView.lineWrapping,
          markdown({ base: markdownLanguage, codeLanguages: [] }),
          markdownHighlighting,
          editorTheme,
          placeholderExt("Start writing. Type [[ to link another note."),
          wikilinks({
            onFollow: (target) => this.zone.run(() => this.linkFollowed.emit(target)),
            isResolved: (target) => this.isResolved(target),
            suggestions: () => this.suggestions()
          }),
          keymap.of([
            {
              key: "Mod-s",
              preventDefault: true,
              run: () => {
                this.zone.run(() => this.flushPendingSave());
                return true;
              }
            },
            ...defaultKeymap,
            ...historyKeymap,
            ...searchKeymap,
            indentWithTab
          ]),
          this.editable.of(EditorView.editable.of(this.note !== null)),
          EditorView.updateListener.of((update) => {
            if (update.docChanged) {
              this.zone.run(() => this.scheduleSave());
            }
          })
        ]
      });

      this.view = new EditorView({ state, parent: this.hostRef.nativeElement });
    });
  }

  private scheduleSave(): void {
    this.setDirty(true);
    if (this.saveTimer) {
      clearTimeout(this.saveTimer);
    }
    this.saveTimer = setTimeout(() => {
      this.saveTimer = null;
      if (this.view) {
        this.emitSave(this.view.state.doc.toString());
      }
    }, AUTOSAVE_DELAY);
  }

  private emitSave(content: string): void {
    this.setDirty(false);
    this.contentSaved.emit(content);
  }

  private setDirty(value: boolean): void {
    if (this.dirty === value) {
      return;
    }
    this.dirty = value;
    this.dirtyChanged.emit(value);
  }

  /**
   * Whether a link target names a note that exists.
   *
   * Matched against the same candidates offered by autocomplete: filename stem,
   * full path, and aliases. The authoritative resolution lives in Rust; this is
   * the cheap local approximation used only to colour the link.
   */
  private isResolved(target: string): boolean {
    const needle = target.toLowerCase().replace(/\.md$/i, "");
    return this.allNotes.some((note) => {
      const path = note.path.toLowerCase().replace(/\.md$/i, "");
      const stem = path.slice(path.lastIndexOf("/") + 1);
      if (path === needle || stem === needle) {
        return true;
      }
      return (note.aliases ?? []).some((alias) => alias.toLowerCase() === needle);
    });
  }

  private suggestions(): WikilinkSuggestion[] {
    const current = this.note?.path;
    return this.allNotes
      .filter((note) => note.path !== current)
      .map((note) => {
        const withoutExt = note.path.replace(/\.md$/i, "");
        const stem = withoutExt.slice(withoutExt.lastIndexOf("/") + 1);
        // Prefer the bare name; fall back to the path when the name repeats,
        // which is exactly when a bare link would be ambiguous.
        const duplicated = this.allNotes.filter((other) => {
          const otherPath = other.path.replace(/\.md$/i, "");
          return otherPath.slice(otherPath.lastIndexOf("/") + 1) === stem;
        }).length;
        return {
          target: duplicated > 1 ? withoutExt : stem,
          title: note.title,
          path: note.path
        };
      });
  }
}
