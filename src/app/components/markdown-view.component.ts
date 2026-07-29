import {
  ChangeDetectionStrategy,
  ChangeDetectorRef,
  Component,
  ElementRef,
  EventEmitter,
  Input,
  OnChanges,
  Output,
  SimpleChanges,
  ViewChild,
  inject
} from "@angular/core";
import { CommonModule } from "@angular/common";
import { DomSanitizer, SafeHtml } from "@angular/platform-browser";
import { convertFileSrc } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import type MarkdownIt from "markdown-it";

import { NoteDetail, NoteMeta } from "../models/vault.models";
import { MAX_EMBED_DEPTH, createRenderer, renderMarkdown } from "../editor/markdown-renderer";
import { VaultService } from "../services/vault.service";

/**
 * Read mode: the note rendered as HTML.
 *
 * Link routing is handled by one delegated click listener on the container
 * rather than by binding to each anchor, so re-rendering a note costs one
 * `innerHTML` write and no listener churn.
 */
@Component({
  selector: "app-markdown-view",
  standalone: true,
  imports: [CommonModule],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="reader" #container (click)="onClick($event)">
      <article class="rendered" [innerHTML]="html"></article>
    </div>
  `,
  styles: [
    `
      :host {
        display: block;
        min-height: 0;
        height: 100%;
      }
      .reader {
        height: 100%;
        overflow-y: auto;
        padding: 1.75rem 0 40vh;
      }
      .rendered {
        max-width: 48rem;
        margin: 0 auto;
        padding: 0 2rem;
        color: var(--ink);
        font-size: 15px;
        line-height: 1.75;
        user-select: text;
      }

      .rendered ::ng-deep h1,
      .rendered ::ng-deep h2,
      .rendered ::ng-deep h3,
      .rendered ::ng-deep h4,
      .rendered ::ng-deep h5,
      .rendered ::ng-deep h6 {
        color: var(--ink-strong);
        font-weight: 650;
        line-height: 1.3;
        margin: 1.8em 0 0.6em;
        letter-spacing: -0.01em;
      }
      .rendered ::ng-deep h1 {
        font-size: 1.85em;
        margin-top: 0;
      }
      .rendered ::ng-deep h2 {
        font-size: 1.4em;
        padding-bottom: 0.25em;
        border-bottom: 1px solid var(--border);
      }
      .rendered ::ng-deep h3 {
        font-size: 1.16em;
      }
      .rendered ::ng-deep h4,
      .rendered ::ng-deep h5,
      .rendered ::ng-deep h6 {
        font-size: 1em;
      }

      .rendered ::ng-deep p {
        margin: 0 0 1.1em;
      }
      .rendered ::ng-deep ul,
      .rendered ::ng-deep ol {
        margin: 0 0 1.1em;
        padding-left: 1.5em;
      }
      .rendered ::ng-deep li {
        margin: 0.25em 0;
      }
      .rendered ::ng-deep li > ul,
      .rendered ::ng-deep li > ol {
        margin: 0.25em 0;
      }
      .rendered ::ng-deep blockquote {
        margin: 0 0 1.1em;
        padding: 0.1em 0 0.1em 1.1em;
        border-left: 3px solid var(--accent);
        color: var(--ink-muted);
      }
      .rendered ::ng-deep hr {
        margin: 2em 0;
        border: none;
        border-top: 1px solid var(--border);
      }
      .rendered ::ng-deep code {
        font-family: var(--font-mono);
        font-size: 0.88em;
        padding: 0.12em 0.35em;
        border-radius: 5px;
        background: var(--hover);
        color: var(--accent-soft);
      }
      .rendered ::ng-deep pre {
        margin: 0 0 1.1em;
        padding: 0.9rem 1.1rem;
        border: 1px solid var(--border);
        border-radius: 10px;
        background: var(--panel);
        overflow-x: auto;
      }
      .rendered ::ng-deep pre code {
        padding: 0;
        background: none;
        color: var(--ink);
        font-size: 0.85em;
        line-height: 1.6;
      }
      .rendered ::ng-deep table {
        width: 100%;
        margin: 0 0 1.1em;
        border-collapse: collapse;
        font-size: 0.92em;
      }
      .rendered ::ng-deep th,
      .rendered ::ng-deep td {
        padding: 0.45rem 0.7rem;
        border: 1px solid var(--border);
        text-align: left;
      }
      .rendered ::ng-deep th {
        background: var(--hover);
        color: var(--ink-strong);
        font-weight: 600;
      }
      .rendered ::ng-deep img,
      .rendered ::ng-deep img.embed {
        max-width: 100%;
        border-radius: 10px;
        display: block;
        margin: 1.2em auto;
      }
      .rendered ::ng-deep a {
        color: var(--link);
        text-decoration: none;
        border-bottom: 1px solid color-mix(in srgb, var(--link) 35%, transparent);
        cursor: pointer;
      }
      .rendered ::ng-deep a:hover {
        border-bottom-color: var(--link);
      }
      .rendered ::ng-deep a.unresolved {
        color: var(--link-unresolved);
        border-bottom-style: dashed;
      }
      .rendered ::ng-deep .embed-marker {
        margin-right: 0.25em;
        opacity: 0.6;
      }
      .rendered ::ng-deep a.tag {
        display: inline-block;
        padding: 0.05em 0.5em;
        border: 1px solid var(--border);
        border-radius: 999px;
        color: var(--ink-muted);
        font-size: 0.85em;
        border-bottom-width: 1px;
      }
      .rendered ::ng-deep a.tag:hover {
        border-color: var(--accent);
        color: var(--accent);
      }
      .rendered ::ng-deep input.task {
        margin-right: 0.5em;
        accent-color: var(--accent);
      }
      .rendered ::ng-deep strong {
        color: var(--ink-strong);
      }

      /* Transcluded notes read as a quoted card, so it stays obvious which
         words belong to this note and which came from another. */
      .rendered ::ng-deep .transclusion {
        margin: 1.2em 0;
        border: 1px solid var(--border);
        border-left: 3px solid var(--accent);
        border-radius: 10px;
        background: color-mix(in srgb, var(--panel) 60%, transparent);
        overflow: hidden;
      }
      .rendered ::ng-deep .transclusion-title {
        display: block;
        padding: 0.5rem 1rem;
        border-bottom: 1px solid var(--border);
        color: var(--ink-muted);
        font-size: 0.8rem;
        font-weight: 600;
        border-bottom-width: 1px;
      }
      .rendered ::ng-deep .transclusion-title .section {
        font-weight: 400;
        opacity: 0.7;
      }
      .rendered ::ng-deep .transclusion-body {
        padding: 0.9rem 1rem 0.2rem;
      }
      .rendered ::ng-deep .transclusion-body > :last-child {
        margin-bottom: 0.7rem;
      }
      .rendered ::ng-deep .transclusion-body h1,
      .rendered ::ng-deep .transclusion-body h2 {
        font-size: 1.1em;
        border-bottom: none;
        margin-top: 0.4em;
      }
      .rendered ::ng-deep .transclusion.cyclic,
      .rendered ::ng-deep .transclusion.truncated {
        display: flex;
        align-items: baseline;
        gap: 0.6rem;
        padding: 0.5rem 1rem;
        border-left-color: var(--ink-faint);
      }
      .rendered ::ng-deep .transclusion .note {
        color: var(--ink-faint);
        font-size: 0.75rem;
      }
      .rendered ::ng-deep .empty {
        color: var(--ink-faint);
        font-style: italic;
      }
    `
  ]
})
export class MarkdownViewComponent implements OnChanges {
  private readonly sanitizer = inject(DomSanitizer);
  private readonly vaultService = inject(VaultService);
  private readonly changeDetector = inject(ChangeDetectorRef);
  private readonly renderer: MarkdownIt = createRenderer();
  /** Guards against an older render landing after a newer one. */
  private renderToken = 0;

  @ViewChild("container", { static: true }) containerRef!: ElementRef<HTMLDivElement>;

  @Input() note: NoteDetail | null = null;
  /** Used to decide whether a wikilink target exists. */
  @Input() allNotes: NoteMeta[] = [];
  /** Absolute path of the vault root, for resolving attachment URLs. */
  @Input() vaultRoot = "";

  @Output() readonly linkFollowed = new EventEmitter<string>();
  @Output() readonly tagSelected = new EventEmitter<string>();

  html: SafeHtml = "";

  ngOnChanges(changes: SimpleChanges): void {
    if (changes["note"] || changes["allNotes"] || changes["vaultRoot"]) {
      void this.render();
    }
  }

  /** Route clicks on rendered links without binding a listener per anchor. */
  onClick(event: MouseEvent): void {
    const element = (event.target as HTMLElement | null)?.closest?.("a") as HTMLAnchorElement | null;
    if (!element) {
      return;
    }

    const tag = element.dataset["tag"];
    if (tag) {
      event.preventDefault();
      this.tagSelected.emit(tag);
      return;
    }

    const wikilink = element.dataset["wikilink"];
    if (wikilink) {
      event.preventDefault();
      this.linkFollowed.emit(wikilink);
      return;
    }

    const external = element.dataset["external"];
    if (external) {
      // Never navigate the webview itself; that would replace the whole app.
      event.preventDefault();
      void openUrl(external).catch(() => undefined);
    }
  }

  private async render(): Promise<void> {
    const note = this.note;
    const token = ++this.renderToken;
    if (!note) {
      this.html = "";
      return;
    }

    // markdown-it renders synchronously, so every note reachable through an
    // `![[embed]]` has to be on hand before rendering starts.
    const { embedded, titles } = await this.collectEmbeds(note);
    if (token !== this.renderToken) {
      // A newer note was opened while we were fetching; drop this render.
      return;
    }

    const rendered = renderMarkdown(this.renderer, note.body, {
      resolve: (target) => this.resolve(target),
      assetUrl: (path) => this.assetUrl(path),
      embedded,
      titles
    });
    // The renderer escapes everything it emits and never passes through author
    // HTML, so the output is trusted by construction rather than by sanitizing.
    this.html = this.sanitizer.bypassSecurityTrustHtml(rendered);
    this.changeDetector.markForCheck();
  }

  /**
   * Breadth-first walk of `![[embeds]]`, bounded by depth and a visited set.
   *
   * Fetching is level by level so that a note embedded from several places is
   * read once, and a cycle terminates on the first repeat.
   */
  private async collectEmbeds(
    note: NoteDetail
  ): Promise<{ embedded: Map<string, string>; titles: Map<string, string> }> {
    const embedded = new Map<string, string>();
    const titles = new Map<string, string>();
    const seen = new Set<string>([note.path]);
    let frontier = this.embedTargets(note.body);

    for (let depth = 0; depth < MAX_EMBED_DEPTH && frontier.length; depth += 1) {
      const pending = frontier.filter((path) => !seen.has(path));
      pending.forEach((path) => seen.add(path));
      if (!pending.length) {
        break;
      }

      const loaded = await Promise.all(pending.map((path) => this.vaultService.peekNote(path)));
      const next: string[] = [];
      for (const embed of loaded) {
        if (!embed) {
          continue;
        }
        embedded.set(embed.path, embed.body);
        titles.set(embed.path, embed.title);
        next.push(...this.embedTargets(embed.body));
      }
      frontier = next;
    }

    return { embedded, titles };
  }

  /** Resolved paths of every note-embed in a body, images excluded. */
  private embedTargets(body: string): string[] {
    const targets: string[] = [];
    const pattern = /!\[\[([^\]\n]+)\]\]/g;
    let match: RegExpExecArray | null;
    while ((match = pattern.exec(body)) !== null) {
      const target = match[1].split("|")[0].split("#")[0].trim();
      if (!target || /\.(png|jpe?g|gif|webp|svg|avif|bmp)$/i.test(target)) {
        continue;
      }
      const resolved = this.resolve(target);
      if (resolved) {
        targets.push(resolved);
      }
    }
    return targets;
  }

  /**
   * Local approximation of the backend's link resolution, used only to decide
   * how a link is styled. Following a link always re-resolves in Rust.
   */
  private resolve(target: string): string | null {
    const needle = target.toLowerCase().replace(/\.md$/i, "");
    if (!needle) {
      return null;
    }
    for (const note of this.allNotes) {
      const path = note.path.toLowerCase();
      const withoutExt = path.replace(/\.md$/i, "");
      const stem = withoutExt.slice(withoutExt.lastIndexOf("/") + 1);
      if (withoutExt === needle || stem === needle || path === needle) {
        return note.path;
      }
      if ((note.aliases ?? []).some((alias) => alias.toLowerCase() === needle)) {
        return note.path;
      }
    }
    return null;
  }

  /** Attachments live on disk; the webview needs an asset-protocol URL. */
  private assetUrl(relative: string): string {
    if (/^[a-z][a-z0-9+.-]*:/i.test(relative)) {
      return relative;
    }
    if (!this.vaultRoot) {
      return relative;
    }
    const clean = relative.replace(/^\.\//, "");
    return convertFileSrc(`${this.vaultRoot}/${clean}`);
  }
}
