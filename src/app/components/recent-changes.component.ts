import { ChangeDetectionStrategy, Component, EventEmitter, Input, Output } from "@angular/core";
import { CommonModule } from "@angular/common";

export interface Change {
  path: string;
  kind: string;
  author?: string;
  at: string;
  summary?: string;
}

/**
 * What has changed in the vault lately, and who changed it.
 *
 * The author is the reason this exists: in a shared vault the useful question
 * is not "what did I edit" but "what has the other person been doing". Without
 * Git there is no answer to that, so the list falls back to modification times
 * and says nothing it cannot know.
 */
@Component({
  selector: "app-recent-changes",
  standalone: true,
  imports: [CommonModule],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (changes.length) {
      <ol class="changes">
        @for (group of grouped(); track group.day) {
          <li class="day">
            <span class="day-label">{{ group.day }}</span>
            <ol class="entries">
              @for (change of group.items; track change.path + change.at) {
                <li class="entry" [class.deleted]="change.kind === 'deleted'">
                  <button type="button" class="open" (click)="opened.emit(change.path)">
                    <span class="kind" [ngClass]="change.kind">{{ label(change.kind) }}</span>
                    <span class="name">{{ name(change.path) }}</span>
                    <span class="folder">{{ folder(change.path) }}</span>
                  </button>
                  <span class="by">
                    <span class="time">{{ time(change.at) }}</span>
                    <span class="author" *ngIf="change.author">· {{ change.author }}</span>
                  </span>
                </li>
              }
            </ol>
          </li>
        }
      </ol>
    } @else {
      <p class="empty">Nothing recorded yet.</p>
    }
  `,
  styles: [
    `
      :host {
        display: block;
        overflow-y: auto;
        padding: 0.5rem 1rem 2rem;
      }
      ol {
        list-style: none;
        margin: 0;
        padding: 0;
      }
      .day + .day {
        margin-top: 1rem;
      }
      .day-label {
        display: block;
        padding: 0.3rem 0;
        font-size: 0.72rem;
        text-transform: uppercase;
        letter-spacing: 0.06em;
        color: var(--ink-faint, #8aa);
        border-bottom: 1px solid var(--border, #2a3a44);
      }
      .entry {
        display: flex;
        align-items: baseline;
        gap: 0.6rem;
        padding: 0.28rem 0;
      }
      .entry.deleted .name {
        text-decoration: line-through;
        opacity: 0.65;
      }
      .open {
        display: flex;
        align-items: baseline;
        gap: 0.5rem;
        flex: 1;
        min-width: 0;
        border: none;
        background: transparent;
        color: inherit;
        font: inherit;
        text-align: left;
        cursor: pointer;
        padding: 0;
      }
      .open:hover .name {
        color: var(--accent, #f05f36);
      }
      .kind {
        flex: none;
        width: 4.2rem;
        font-size: 0.68rem;
        text-transform: uppercase;
        letter-spacing: 0.04em;
        color: var(--ink-faint, #8aa);
      }
      .kind.added {
        color: var(--accent, #f05f36);
      }
      .name {
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        font-size: 0.88rem;
      }
      .folder {
        flex: 1;
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        font-size: 0.72rem;
        color: var(--ink-faint, #8aa);
      }
      .by {
        flex: none;
        font-size: 0.72rem;
        color: var(--ink-faint, #8aa);
        font-variant-numeric: tabular-nums;
      }
      .empty {
        font-size: 0.82rem;
        color: var(--ink-faint, #8aa);
      }
    `
  ]
})
export class RecentChangesComponent {
  @Input() changes: Change[] = [];
  @Output() readonly opened = new EventEmitter<string>();

  label(kind: string): string {
    return kind === "modified" ? "edited" : kind;
  }

  name(path: string): string {
    return path.split("/").pop()?.replace(/\.md$/, "") ?? path;
  }

  folder(path: string): string {
    const parts = path.split("/");
    parts.pop();
    return parts.join(" / ");
  }

  time(at: string): string {
    const when = new Date(at);
    return Number.isNaN(when.getTime())
      ? ""
      : when.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
  }

  /** Grouped by day, because "when" is only interesting to the day for notes. */
  grouped(): { day: string; items: Change[] }[] {
    const today = new Date().toDateString();
    const yesterday = new Date(Date.now() - 86_400_000).toDateString();
    const groups: { day: string; items: Change[] }[] = [];

    for (const change of this.changes) {
      const when = new Date(change.at);
      const key = Number.isNaN(when.getTime()) ? "Earlier" : when.toDateString();
      const label = key === today ? "Today" : key === yesterday ? "Yesterday" : key;
      const last = groups[groups.length - 1];
      if (last && last.day === label) {
        last.items.push(change);
      } else {
        groups.push({ day: label, items: [change] });
      }
    }
    return groups;
  }
}
