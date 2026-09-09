import { Injectable, inject } from "@angular/core";
import { convertFileSrc } from "@tauri-apps/api/core";
import type MarkdownIt from "markdown-it";

import { MAX_EMBED_DEPTH, createRenderer, renderMarkdown } from "../editor/markdown-renderer";
import { NoteDetail, NoteMeta } from "../models/vault.models";
import { VaultService } from "./vault.service";

/**
 * Turning a note into HTML.
 *
 * Extracted so reading a note and printing one cannot drift apart. Two copies
 * of "how a wikilink resolves" would eventually disagree, and the version you
 * hand a client is the worst place to discover it.
 */
@Injectable({ providedIn: "root" })
export class NoteRenderService {
  private readonly vaultService = inject(VaultService);
  private readonly renderer: MarkdownIt = createRenderer();

  /** Resolve a wikilink target against the vault, by path, stem or alias. */
  resolve(target: string, notes: readonly NoteMeta[]): string | null {
    const needle = target.toLowerCase().replace(/\.md$/i, "");
    if (!needle) {
      return null;
    }
    for (const note of notes) {
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

  /** A vault-relative attachment path as a URL the webview can load. */
  assetUrl(relative: string, vaultRoot: string): string {
    if (/^[a-z][a-z0-9+.-]*:/i.test(relative)) {
      return relative;
    }
    if (!vaultRoot) {
      return relative;
    }
    return convertFileSrc(`${vaultRoot}/${relative.replace(/^\.\//, "")}`);
  }

  /** A note as HTML, with `![[embeds]]` inlined. */
  async render(note: NoteDetail, notes: readonly NoteMeta[], vaultRoot: string): Promise<string> {
    const { embedded, titles } = await this.collectEmbeds(note, notes);
    return renderMarkdown(this.renderer, note.body, {
      resolve: (target) => this.resolve(target, notes),
      assetUrl: (path) => this.assetUrl(path, vaultRoot),
      embedded,
      titles
    });
  }

  /**
   * Bodies for every `![[embed]]` reachable from a note.
   *
   * Fetched up front because markdown-it renders synchronously; a body that
   * has not arrived would silently render as a broken link.
   */
  async collectEmbeds(
    note: NoteDetail,
    notes: readonly NoteMeta[]
  ): Promise<{ embedded: Map<string, string>; titles: Map<string, string> }> {
    const embedded = new Map<string, string>();
    const titles = new Map<string, string>();
    const seen = new Set<string>([note.path]);
    let frontier = this.embedTargets(note.body, notes);

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
        next.push(...this.embedTargets(embed.body, notes));
      }
      frontier = next;
    }

    return { embedded, titles };
  }

  /** Resolved paths of every note-embed in a body, images excluded. */
  private embedTargets(body: string, notes: readonly NoteMeta[]): string[] {
    const targets: string[] = [];
    const pattern = /!\[\[([^\]\n]+)\]\]/g;
    let match: RegExpExecArray | null;
    while ((match = pattern.exec(body)) !== null) {
      const target = match[1].split("|")[0].split("#")[0].trim();
      if (!target || /\.(png|jpe?g|gif|webp|svg|avif|bmp)$/i.test(target)) {
        continue;
      }
      const resolved = this.resolve(target, notes);
      if (resolved) {
        targets.push(resolved);
      }
    }
    return targets;
  }
}
