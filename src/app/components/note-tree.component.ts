import { ChangeDetectionStrategy, Component, EventEmitter, Input, Output, signal } from "@angular/core";
import { CommonModule } from "@angular/common";

import { TreeEntry } from "../models/vault.models";

/** What a drag is carrying. */
export type DragKind = "note" | "folder";

/** A right-click in the tree: on a note, a folder, or on empty space. */
export interface TreeContextEvent {
  kind: "note" | "folder" | "root";
  /** "" when the target is the vault root. */
  path: string;
  x: number;
  y: number;
}

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
    <!-- The container is the root drop zone: dropping anywhere that is not a
         folder row moves the note to the top level of the vault. -->
    <div
      class="tree"
      role="tree"
      [class.drop-root]="dropTarget() === ''"
      (dragover)="onDragOver($event, '')"
      (dragleave)="onDragLeave($event, '')"
      (drop)="onDrop($event, '')"
      (contextmenu)="onContextMenu($event, 'root', '')"
    >
      <ng-container *ngFor="let entry of visible(); trackBy: trackEntry">
        <button
          *ngIf="entry.kind === 'folder'"
          type="button"
          class="row folder"
          role="treeitem"
          draggable="true"
          [class.drop-into]="dropTarget() === entry.path"
          [class.dragging]="dragging() === entry.path"
          [style.paddingLeft.rem]="0.65 + entry.depth * 0.85"
          [attr.aria-expanded]="!collapsed().has(entry.path)"
          (click)="toggleFolder(entry.path)"
          (contextmenu)="onContextMenu($event, 'folder', entry.path)"
          (dragstart)="onDragStart($event, entry.path, 'folder')"
          (dragend)="onDragEnd()"
          (dragover)="onDragOver($event, entry.path)"
          (dragleave)="onDragLeave($event, entry.path)"
          (drop)="onDrop($event, entry.path)"
        >
          <i class="pi" [class.pi-chevron-down]="!collapsed().has(entry.path)" [class.pi-chevron-right]="collapsed().has(entry.path)"></i>
          <span class="label">{{ entry.name }}</span>
        </button>

        <button
          *ngIf="entry.kind === 'note'"
          type="button"
          class="row note"
          role="treeitem"
          draggable="true"
          [class.active]="entry.path === activePath"
          [class.dragging]="dragging() === entry.path"
          [style.paddingLeft.rem]="0.65 + entry.depth * 0.85"
          (click)="noteSelected.emit(entry.path)"
          (contextmenu)="onContextMenu($event, 'note', entry.path)"
          (dragstart)="onDragStart($event, entry.path, 'note')"
          (dragend)="onDragEnd()"
          (dragover)="onDragOver($event, folderOf(entry.path))"
          (dragleave)="onDragLeave($event, folderOf(entry.path))"
          (drop)="onDrop($event, folderOf(entry.path))"
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
      /* Full height, so the blank space under the last row is still the
         root drop zone rather than dead space. */
      :host {
        display: flex;
        flex-direction: column;
        min-height: 100%;
      }
      .tree {
        display: flex;
        flex: 1;
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
      /* WebKit does not drag arbitrary elements — least of all form controls —
         without being told to, so rows opt in explicitly. Without this the
         drag never starts in the macOS webview. */
      .row[draggable="true"] {
        -webkit-user-drag: element;
        user-select: none;
      }
      .row.dragging {
        opacity: 0.4;
      }
      /* Where the note will land if it is dropped now. */
      .row.drop-into {
        background: var(--selected);
        color: var(--ink-strong);
        box-shadow: inset 0 0 0 1px var(--accent);
      }
      .tree.drop-root {
        box-shadow: inset 0 0 0 1px var(--accent);
        border-radius: 8px;
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

  /** Folders to start folded, restored from wherever the caller kept them. */
  @Input() set folded(paths: string[]) {
    this.collapsed.set(new Set(paths ?? []));
  }

  /** Emitted whenever a fold changes, so the caller can remember it. */
  @Output() readonly foldedChange = new EventEmitter<string[]>();

  @Output() readonly noteSelected = new EventEmitter<string>();
  /** Right-click on a row, or on empty space ("root"). */
  @Output() readonly contextRequested = new EventEmitter<TreeContextEvent>();
  /** A note or folder was dropped on a folder. `folder` is "" for the vault root. */
  @Output() readonly noteMoved = new EventEmitter<{ path: string; folder: string }>();
  @Output() readonly folderMoved = new EventEmitter<{ path: string; folder: string }>();

  readonly collapsed = signal<Set<string>>(new Set());

  /** What is being dragged, and the folder currently under the cursor. */
  readonly dragging = signal<string | null>(null);
  readonly draggingKind = signal<DragKind>("note");
  readonly dropTarget = signal<string | null>(null);

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
    this.foldedChange.emit([...next]);
  }

  onContextMenu(event: MouseEvent, kind: TreeContextEvent["kind"], path: string): void {
    event.preventDefault();
    // Rows sit inside the container, which also offers a menu; without this the
    // container's handler would replace the row's with the root menu.
    event.stopPropagation();
    this.contextRequested.emit({ kind, path, x: event.clientX, y: event.clientY });
  }

  // ---------------------------------------------------------------- drag/drop

  onDragStart(event: DragEvent, path: string, kind: DragKind): void {
    this.dragging.set(path);
    this.draggingKind.set(kind);
    // The path also goes on the drag payload so a drop that somehow arrives
    // without our signal set still knows what it is carrying.
    event.dataTransfer?.setData("text/plain", path);
    if (event.dataTransfer) {
      event.dataTransfer.effectAllowed = "move";
    }
    event.stopPropagation();
  }

  onDragEnd(): void {
    this.dragging.set(null);
    this.dropTarget.set(null);
  }

  onDragOver(event: DragEvent, folder: string): void {
    if (!this.dragging()) {
      return;
    }
    // Preventing the default is what marks this element as a valid drop target.
    event.preventDefault();
    event.stopPropagation();
    const allowed = this.canDrop(folder);
    if (event.dataTransfer) {
      event.dataTransfer.dropEffect = allowed ? "move" : "none";
    }
    this.dropTarget.set(allowed ? folder : null);
  }

  onDragLeave(event: DragEvent, folder: string): void {
    event.stopPropagation();
    if (this.dropTarget() === folder) {
      this.dropTarget.set(null);
    }
  }

  onDrop(event: DragEvent, folder: string): void {
    event.preventDefault();
    event.stopPropagation();
    const path = this.dragging() ?? event.dataTransfer?.getData("text/plain") ?? "";
    const kind = this.draggingKind();
    const allowed = this.canDrop(folder, path);
    this.onDragEnd();
    if (!allowed) {
      return;
    }
    if (kind === "folder") {
      this.folderMoved.emit({ path, folder });
    } else {
      this.noteMoved.emit({ path, folder });
    }
  }

  /**
   * Whether the drag in flight may be dropped on `folder`.
   *
   * Two things are refused: dropping something back where it already is, and
   * dropping a folder inside itself, which has no meaning and would lose the
   * folder if the filesystem allowed it.
   */
  private canDrop(folder: string, path = this.dragging() ?? ""): boolean {
    if (!path) {
      return false;
    }
    if (this.draggingKind() === "folder" && (folder === path || folder.startsWith(`${path}/`))) {
      return false;
    }
    return this.folderOf(path) !== folder;
  }

  /**
   * The folder a note sits in, "" at the vault root.
   *
   * Note rows are drop targets too, standing in for their own folder — dropping
   * onto a note is a natural way to aim at the folder it is in.
   */
  folderOf(path: string): string {
    const slash = path.lastIndexOf("/");
    return slash === -1 ? "" : path.slice(0, slash);
  }

  trackEntry(_: number, entry: TreeEntry): string {
    return `${entry.kind}:${entry.path}`;
  }
}
