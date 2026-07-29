import { ChangeDetectionStrategy, Component, EventEmitter, Input, Output } from "@angular/core";
import { CommonModule } from "@angular/common";

import { Backlink, Heading, NoteDetail, ResolvedLink } from "../models/vault.models";

/**
 * The right-hand panel: what this note points at, and what points back.
 *
 * Backlinks are the reason a vault beats a folder of files, so they get the top
 * slot rather than being buried behind a toggle.
 */
@Component({
  selector: "app-context-panel",
  standalone: true,
  imports: [CommonModule],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="panel" *ngIf="note as current">
      <section>
        <h3>
          Backlinks
          <span class="count">{{ current.backlinks.length }}</span>
        </h3>
        <p class="empty" *ngIf="!current.backlinks.length">Nothing links here yet.</p>
        <button
          type="button"
          class="entry"
          *ngFor="let backlink of current.backlinks; trackBy: trackBacklink"
          (click)="noteOpened.emit(backlink.source)"
        >
          <span class="title">{{ backlink.source_title }}</span>
          <span class="context">{{ backlink.context }}</span>
        </button>
      </section>

      <section *ngIf="outgoing(current).length">
        <h3>
          Links
          <span class="count">{{ outgoing(current).length }}</span>
        </h3>
        <button
          type="button"
          class="entry compact"
          *ngFor="let link of outgoing(current); trackBy: trackLink"
          [class.unresolved]="!link.resolved_path"
          (click)="onLinkClick(link)"
        >
          <i class="pi" [class.pi-arrow-up-right]="link.resolved_path" [class.pi-plus]="!link.resolved_path"></i>
          <span class="title">{{ link.alias || link.target }}</span>
          <span class="context" *ngIf="!link.resolved_path">not created</span>
        </button>
      </section>

      <section *ngIf="current.headings.length > 1">
        <h3>Outline</h3>
        <button
          type="button"
          class="heading"
          *ngFor="let heading of current.headings; trackBy: trackHeading"
          [style.paddingLeft.rem]="0.55 + (heading.level - 1) * 0.6"
          (click)="headingSelected.emit(heading)"
        >
          {{ heading.text }}
        </button>
      </section>

      <section *ngIf="tagList(current).length">
        <h3>Tags</h3>
        <div class="tags">
          <button
            type="button"
            class="tag"
            *ngFor="let tag of tagList(current); trackBy: trackTag"
            (click)="tagSelected.emit(tag)"
          >
            #{{ tag }}
          </button>
        </div>
      </section>
    </div>
  `,
  styles: [
    `
      .panel {
        display: flex;
        flex-direction: column;
        gap: 1.6rem;
        padding: 1.1rem 0.9rem 3rem;
        overflow-y: auto;
        height: 100%;
      }
      section {
        display: flex;
        flex-direction: column;
        gap: 2px;
      }
      h3 {
        display: flex;
        align-items: center;
        gap: 0.45rem;
        margin: 0 0 0.5rem 0.55rem;
        font-size: 0.68rem;
        text-transform: uppercase;
        letter-spacing: 0.11em;
        color: var(--ink-faint);
        font-weight: 600;
      }
      .count {
        padding: 0.05rem 0.35rem;
        border-radius: 999px;
        background: var(--hover);
        font-size: 0.65rem;
        letter-spacing: 0;
      }
      .empty {
        margin: 0 0.55rem;
        font-size: 0.8rem;
        color: var(--ink-faint);
      }
      .entry {
        display: flex;
        flex-direction: column;
        align-items: flex-start;
        gap: 0.15rem;
        width: 100%;
        padding: 0.45rem 0.55rem;
        border: none;
        border-radius: 8px;
        background: transparent;
        text-align: left;
        cursor: pointer;
      }
      .entry.compact {
        flex-direction: row;
        align-items: center;
        gap: 0.45rem;
      }
      .entry:hover {
        background: var(--hover);
      }
      .entry i {
        font-size: 0.65rem;
        color: var(--ink-faint);
      }
      .entry.unresolved .title {
        color: var(--link-unresolved);
      }
      .title {
        color: var(--ink);
        font-size: 0.83rem;
      }
      .context {
        color: var(--ink-faint);
        font-size: 0.75rem;
        line-height: 1.45;
        overflow: hidden;
        display: -webkit-box;
        -webkit-line-clamp: 2;
        -webkit-box-orient: vertical;
      }
      .heading {
        width: 100%;
        padding: 0.3rem 0.55rem;
        border: none;
        border-radius: 7px;
        background: transparent;
        color: var(--ink-muted);
        font-size: 0.8rem;
        text-align: left;
        cursor: pointer;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
      }
      .heading:hover {
        background: var(--hover);
        color: var(--ink);
      }
      .tags {
        display: flex;
        flex-wrap: wrap;
        gap: 0.35rem;
        padding: 0 0.55rem;
      }
      .tag {
        padding: 0.2rem 0.6rem;
        border-radius: 999px;
        border: 1px solid var(--border);
        background: transparent;
        color: var(--ink-muted);
        font-size: 0.75rem;
        cursor: pointer;
      }
      .tag:hover {
        border-color: var(--accent);
        color: var(--accent);
      }
    `
  ]
})
export class ContextPanelComponent {
  @Input() note: NoteDetail | null = null;

  @Output() readonly noteOpened = new EventEmitter<string>();
  @Output() readonly linkFollowed = new EventEmitter<ResolvedLink>();
  @Output() readonly headingSelected = new EventEmitter<Heading>();
  @Output() readonly tagSelected = new EventEmitter<string>();

  /** Outgoing links, deduplicated by target so a repeated link lists once. */
  outgoing(note: NoteDetail): ResolvedLink[] {
    const seen = new Set<string>();
    return note.links.filter((link) => {
      const key = `${link.target}|${link.resolved_path ?? ""}`;
      if (!link.target || seen.has(key)) {
        return false;
      }
      seen.add(key);
      return true;
    });
  }

  tagList(note: NoteDetail): string[] {
    return note.tags ?? [];
  }

  onLinkClick(link: ResolvedLink): void {
    if (link.resolved_path) {
      this.noteOpened.emit(link.resolved_path);
    } else {
      this.linkFollowed.emit(link);
    }
  }

  trackBacklink(_: number, backlink: Backlink): string {
    return `${backlink.source}:${backlink.line}`;
  }

  trackLink(_: number, link: ResolvedLink): string {
    return `${link.target}:${link.line}`;
  }

  trackHeading(_: number, heading: Heading): string {
    return `${heading.line}:${heading.text}`;
  }

  trackTag(_: number, tag: string): string {
    return tag;
  }
}
