import { ChangeDetectionStrategy, Component, EventEmitter, Input, Output, signal } from "@angular/core";
import { CommonModule } from "@angular/common";

import { TreeEntry } from "../models/vault.models";

/**
 * The vault's folder tree.
 *
 * Collapse state lives here rather than in the parent because it is pure view
 * state — it should survive note switches and vanish with the component.
 */
@Component({
  selector: "app-note-tree",
  standalone: true,
  imports: [CommonModule],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="tree" role="tree">
      <ng-container *ngFor="let entry of visible(); trackBy: trackEntry">
        <button
          *ngIf="entry.kind === 'folder'"
          type="button"
          class="row folder"
          role="treeitem"
          [style.paddingLeft.rem]="0.65 + entry.depth * 0.85"
          [attr.aria-expanded]="!collapsed().has(entry.path)"
          (click)="toggleFolder(entry.path)"
        >
          <i class="pi" [class.pi-chevron-down]="!collapsed().has(entry.path)" [class.pi-chevron-right]="collapsed().has(entry.path)"></i>
          <span class="label">{{ entry.name }}</span>
        </button>

        <button
          *ngIf="entry.kind === 'note'"
          type="button"
          class="row note"
          role="treeitem"
          [class.active]="entry.path === activePath"
          [style.paddingLeft.rem]="0.65 + entry.depth * 0.85"
          (click)="noteSelected.emit(entry.path)"
          (contextmenu)="onContextMenu($event, entry.path)"
        >
          <i class="pi pi-file"></i>
          <span class="label">{{ entry.name }}</span>
        </button>
      </ng-container>

      <p class="empty" *ngIf="!entries.length">
        No notes yet. Press <kbd>{{ modifierLabel }}</kbd> + <kbd>N</kbd> to write the first one.
      </p>
    </div>
  `,
  styles: [
    `
      .tree {
        display: flex;
        flex-direction: column;
        padding: 0.25rem 0.4rem 1rem;
        gap: 1px;
      }
      .row {
        display: flex;
        align-items: center;
        gap: 0.45rem;
        width: 100%;
        padding: 0.3rem 0.6rem;
        border: none;
        border-radius: 7px;
        background: transparent;
        color: var(--ink-muted);
        font-size: 0.85rem;
        text-align: left;
        cursor: pointer;
        transition: background 0.12s ease, color 0.12s ease;
      }
      .row:hover {
        background: var(--hover);
        color: var(--ink);
      }
      .row.active {
        background: var(--selected);
        color: var(--ink-strong);
        font-weight: 550;
      }
      .row i {
        font-size: 0.7rem;
        opacity: 0.55;
        flex: none;
      }
      .row.folder {
        color: var(--ink-faint);
        font-weight: 600;
        letter-spacing: 0.01em;
      }
      .label {
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
      }
      .empty {
        margin: 1rem 0.7rem;
        font-size: 0.8rem;
        line-height: 1.6;
        color: var(--ink-faint);
      }
      kbd {
        font-family: var(--font-mono);
        font-size: 0.7rem;
        padding: 0.05rem 0.28rem;
        border: 1px solid var(--border);
        border-radius: 4px;
      }
    `
  ]
})
export class NoteTreeComponent {
  @Input({ required: true }) entries: TreeEntry[] = [];
  @Input() activePath: string | null = null;

  @Output() readonly noteSelected = new EventEmitter<string>();
  @Output() readonly noteContextMenu = new EventEmitter<{ path: string; x: number; y: number }>();

  readonly collapsed = signal<Set<string>>(new Set());

  readonly modifierLabel = navigator.platform.toLowerCase().includes("mac") ? "⌘" : "Ctrl";

  /** Entries with everything inside a collapsed folder filtered out. */
  visible(): TreeEntry[] {
    const folded = this.collapsed();
    if (folded.size === 0) {
      return this.entries;
    }
    return this.entries.filter((entry) => {
      for (const folder of folded) {
        // A collapsed folder hides its descendants but stays visible itself.
        if (entry.path !== folder && entry.path.startsWith(`${folder}/`)) {
          return false;
        }
      }
      return true;
    });
  }

  toggleFolder(path: string): void {
    const next = new Set(this.collapsed());
    if (!next.delete(path)) {
      next.add(path);
    }
    this.collapsed.set(next);
  }

  onContextMenu(event: MouseEvent, path: string): void {
    event.preventDefault();
    this.noteContextMenu.emit({ path, x: event.clientX, y: event.clientY });
  }

  trackEntry(_: number, entry: TreeEntry): string {
    return `${entry.kind}:${entry.path}`;
  }
}
