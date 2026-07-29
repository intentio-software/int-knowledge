import {
  AfterViewInit,
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  EventEmitter,
  Input,
  Output,
  ViewChild,
  signal
} from "@angular/core";
import { CommonModule } from "@angular/common";
import { FormsModule } from "@angular/forms";

import { NoteMeta, SearchHit } from "../models/vault.models";

export type PaletteMode = "jump" | "search";

export interface PaletteResult {
  path: string;
  title: string;
  detail: string;
  /** Set for full-text hits: the line that matched. */
  snippet?: string;
  /** True when choosing this entry creates a note rather than opening one. */
  create?: boolean;
}

/**
 * One overlay serving both "jump to note" and "search contents".
 *
 * They share a keyboard model and a result list, and users switch between them
 * mid-thought, so splitting them into two overlays would only duplicate the
 * navigation code.
 */
@Component({
  selector: "app-command-palette",
  standalone: true,
  imports: [CommonModule, FormsModule],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="backdrop" (click)="dismissed.emit()">
      <div class="palette" (click)="$event.stopPropagation()">
        <div class="field">
          <i class="pi" [class.pi-search]="mode === 'search'" [class.pi-arrow-right]="mode === 'jump'"></i>
          <input
            #input
            type="text"
            [ngModel]="query"
            (ngModelChange)="onQueryChange($event)"
            (keydown)="onKeydown($event)"
            [placeholder]="mode === 'jump' ? 'Jump to a note, or type a new name…' : 'Search every note…'"
            autocomplete="off"
            spellcheck="false"
          />
          <span class="mode">{{ mode === "jump" ? "Open" : "Search" }}</span>
        </div>

        <ul class="results" *ngIf="results.length">
          <li
            *ngFor="let result of results; let index = index; trackBy: trackResult"
            [class.active]="index === selected()"
            (mouseenter)="selected.set(index)"
            (click)="choose(result)"
          >
            <div class="line">
              <i class="pi" [class.pi-file]="!result.create" [class.pi-plus]="result.create"></i>
              <span class="title">{{ result.title }}</span>
              <span class="detail">{{ result.detail }}</span>
            </div>
            <div class="snippet" *ngIf="result.snippet">{{ result.snippet }}</div>
          </li>
        </ul>

        <p class="empty" *ngIf="!results.length && query.trim()">No matches.</p>

        <div class="footnote">
          <span><kbd>↑</kbd><kbd>↓</kbd> navigate</span>
          <span><kbd>↵</kbd> open</span>
          <span><kbd>esc</kbd> close</span>
        </div>
      </div>
    </div>
  `,
  styles: [
    `
      .backdrop {
        position: fixed;
        inset: 0;
        z-index: 60;
        display: flex;
        justify-content: center;
        padding-top: 12vh;
        background: rgba(2, 10, 20, 0.55);
        backdrop-filter: blur(3px);
      }
      .palette {
        width: min(42rem, calc(100vw - 3rem));
        max-height: 64vh;
        display: flex;
        flex-direction: column;
        background: var(--panel-raised);
        border: 1px solid var(--border);
        border-radius: 14px;
        box-shadow: 0 30px 70px rgba(0, 0, 0, 0.45);
        overflow: hidden;
      }
      .field {
        display: flex;
        align-items: center;
        gap: 0.7rem;
        padding: 0.9rem 1.1rem;
        border-bottom: 1px solid var(--border);
      }
      .field i {
        color: var(--ink-faint);
        font-size: 0.85rem;
      }
      .field input {
        flex: 1;
        border: none;
        background: transparent;
        color: var(--ink-strong);
        font-size: 1rem;
        outline: none;
      }
      .mode {
        font-size: 0.7rem;
        text-transform: uppercase;
        letter-spacing: 0.08em;
        color: var(--ink-faint);
        border: 1px solid var(--border);
        border-radius: 999px;
        padding: 0.15rem 0.55rem;
      }
      .results {
        list-style: none;
        margin: 0;
        padding: 0.4rem;
        overflow-y: auto;
      }
      .results li {
        padding: 0.5rem 0.7rem;
        border-radius: 8px;
        cursor: pointer;
      }
      .results li.active {
        background: var(--selected);
      }
      .line {
        display: flex;
        align-items: baseline;
        gap: 0.55rem;
      }
      .line i {
        font-size: 0.7rem;
        color: var(--ink-faint);
      }
      .title {
        color: var(--ink-strong);
        font-size: 0.9rem;
      }
      .detail {
        color: var(--ink-faint);
        font-size: 0.75rem;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
      }
      .snippet {
        margin-top: 0.2rem;
        padding-left: 1.25rem;
        color: var(--ink-muted);
        font-size: 0.78rem;
        font-family: var(--font-mono);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
      }
      .empty {
        margin: 0;
        padding: 1.2rem;
        color: var(--ink-faint);
        font-size: 0.85rem;
      }
      .footnote {
        display: flex;
        gap: 1rem;
        padding: 0.5rem 1.1rem;
        border-top: 1px solid var(--border);
        font-size: 0.7rem;
        color: var(--ink-faint);
      }
      kbd {
        font-family: var(--font-mono);
        padding: 0.05rem 0.28rem;
        margin-right: 0.15rem;
        border: 1px solid var(--border);
        border-radius: 4px;
      }
    `
  ]
})
export class CommandPaletteComponent implements AfterViewInit {
  @ViewChild("input", { static: true }) inputRef!: ElementRef<HTMLInputElement>;

  @Input({ required: true }) mode: PaletteMode = "jump";
  /** All notes, filtered locally in jump mode. */
  @Input() notes: NoteMeta[] = [];
  /** Backend hits, supplied by the parent in search mode. */
  @Input() hits: SearchHit[] = [];

  @Output() readonly queryChanged = new EventEmitter<string>();
  @Output() readonly noteChosen = new EventEmitter<string>();
  @Output() readonly noteCreateRequested = new EventEmitter<string>();
  @Output() readonly dismissed = new EventEmitter<void>();

  query = "";
  readonly selected = signal(0);

  ngAfterViewInit(): void {
    // The overlay only exists while open, so focusing on init is safe.
    this.inputRef.nativeElement.focus();
  }

  get results(): PaletteResult[] {
    return this.mode === "jump" ? this.jumpResults() : this.searchResults();
  }

  onQueryChange(value: string): void {
    this.query = value;
    this.selected.set(0);
    this.queryChanged.emit(value);
  }

  onKeydown(event: KeyboardEvent): void {
    const results = this.results;
    switch (event.key) {
      case "ArrowDown":
        event.preventDefault();
        this.selected.set(results.length ? (this.selected() + 1) % results.length : 0);
        break;
      case "ArrowUp":
        event.preventDefault();
        this.selected.set(results.length ? (this.selected() - 1 + results.length) % results.length : 0);
        break;
      case "Enter": {
        event.preventDefault();
        const chosen = results[this.selected()];
        if (chosen) {
          this.choose(chosen);
        }
        break;
      }
      case "Escape":
        event.preventDefault();
        this.dismissed.emit();
        break;
      default:
        break;
    }
  }

  choose(result: PaletteResult): void {
    if (result.create) {
      this.noteCreateRequested.emit(result.path);
    } else {
      this.noteChosen.emit(result.path);
    }
  }

  trackResult(_: number, result: PaletteResult): string {
    return `${result.path}:${result.snippet ?? ""}`;
  }

  /**
   * Fuzzy-ish filter over titles and paths.
   *
   * Substring matching on both, ranked by where the match lands — a title that
   * starts with the query is what the user almost always meant.
   */
  private jumpResults(): PaletteResult[] {
    const query = this.query.trim().toLowerCase();
    if (!query) {
      return this.notes
        .slice()
        .sort((a, b) => (b.modified ?? 0) - (a.modified ?? 0))
        .slice(0, 12)
        .map((note) => ({ path: note.path, title: note.title, detail: note.path }));
    }

    const scored = this.notes
      .map((note) => {
        const title = note.title.toLowerCase();
        const path = note.path.toLowerCase();
        const aliasHit = (note.aliases ?? []).some((alias) => alias.toLowerCase().includes(query));
        let score = -1;
        if (title.startsWith(query)) {
          score = 0;
        } else if (title.includes(query)) {
          score = 1;
        } else if (aliasHit) {
          score = 2;
        } else if (path.includes(query)) {
          score = 3;
        }
        return { note, score };
      })
      .filter((entry) => entry.score >= 0)
      .sort((a, b) => a.score - b.score || a.note.title.localeCompare(b.note.title))
      .slice(0, 20)
      .map((entry) => ({ path: entry.note.path, title: entry.note.title, detail: entry.note.path }));

    // Offer creation when nothing matches the name exactly.
    const exact = this.notes.some((note) => note.title.toLowerCase() === query);
    if (!exact) {
      scored.push({
        path: this.query.trim(),
        title: `Create “${this.query.trim()}”`,
        detail: "New note",
        create: true
      } as PaletteResult);
    }
    return scored;
  }

  private searchResults(): PaletteResult[] {
    return this.hits.flatMap((hit) => {
      if (!hit.matches.length) {
        return [{ path: hit.path, title: hit.title, detail: hit.path }];
      }
      return hit.matches.map((match) => ({
        path: hit.path,
        title: hit.title,
        detail: `${hit.path}:${match.line}`,
        snippet: match.text
      }));
    });
  }
}
