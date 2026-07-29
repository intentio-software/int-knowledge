import { Injectable, computed, signal } from "@angular/core";
import { invoke } from "@tauri-apps/api/core";
import { UnlistenFn, listen } from "@tauri-apps/api/event";

import {
  Backlink,
  GraphData,
  NoteDetail,
  NoteMeta,
  RecentVault,
  SearchHit,
  TreeEntry,
  UnresolvedLink,
  VaultSummary
} from "../models/vault.models";

const RECENTS_KEY = "intentio-knowledge:recent-vaults";
const LAST_VAULT_KEY = "intentio-knowledge:last-vault";
const RECENTS_LIMIT = 8;

/**
 * How long a path this app wrote is ignored when the watcher reports it.
 *
 * Our own saves land on disk and come straight back as filesystem events;
 * without this the app would refresh in response to its own writes.
 */
const SELF_WRITE_GRACE = 1500;

interface VaultChangedEvent {
  root: string;
  paths: string[];
  truncated: boolean;
}

/**
 * The app's single source of truth for vault state.
 *
 * All filesystem work happens in Rust; this service holds the signals the UI
 * renders from and keeps them in step after every write. Note lists are
 * refreshed from disk rather than patched in memory, because an agent or another
 * editor may have changed the same folder.
 */
@Injectable({ providedIn: "root" })
export class VaultService {
  readonly vault = signal<VaultSummary | null>(null);
  readonly notes = signal<NoteMeta[]>([]);
  readonly activeNote = signal<NoteDetail | null>(null);
  readonly recents = signal<RecentVault[]>(this.loadRecents());
  readonly loading = signal(false);
  readonly error = signal<string | null>(null);

  /**
   * Set by the editor. While true, an external change to the open note is
   * reported rather than applied — silently replacing text someone is typing
   * would lose their work.
   */
  readonly editorDirty = signal(false);

  /** Path of the open note that changed on disk while it had unsaved edits. */
  readonly conflict = signal<string | null>(null);

  /** Bumped whenever the vault changed on disk, so views can refresh. */
  readonly revision = signal(0);

  private unlisten: UnlistenFn | null = null;
  /** Paths this app wrote recently, with the time they were written. */
  private readonly selfWrites = new Map<string, number>();

  /**
   * Notes grouped into a flat, ordered folder tree for the sidebar.
   *
   * Folders come from the vault summary as well as from note paths, so one the
   * user just made — and has not put anything in yet — still shows up.
   */
  readonly tree = computed<TreeEntry[]>(() => buildTree(this.notes(), this.vault()?.folders ?? []));

  readonly backlinks = computed<Backlink[]>(() => this.activeNote()?.backlinks ?? []);

  readonly isOpen = computed(() => this.vault() !== null);

  /** Path of the last vault, so the app can reopen it on launch. */
  lastVaultPath(): string | null {
    return readStorage(LAST_VAULT_KEY);
  }

  async openVault(root: string): Promise<void> {
    await this.guard(async () => {
      const summary = await invoke<VaultSummary>("open_vault", { root });
      this.applyVault(summary);
      await this.refreshNotes();
    });
  }

  async createVault(root: string): Promise<void> {
    await this.guard(async () => {
      const summary = await invoke<VaultSummary>("create_vault", { root });
      this.applyVault(summary);
      await this.refreshNotes();
    });
  }

  async closeVault(): Promise<void> {
    await this.stopWatching();
    this.vault.set(null);
    this.notes.set([]);
    this.activeNote.set(null);
    this.conflict.set(null);
    writeStorage(LAST_VAULT_KEY, null);
  }

  // -------------------------------------------------------------------------
  // watching
  // -------------------------------------------------------------------------

  /**
   * Watch the open vault for changes made outside the app.
   *
   * The same folder is writable by the MCP server, by git, and by any other
   * editor, so the UI has to treat disk as the source of truth rather than its
   * own last read.
   */
  async startWatching(): Promise<void> {
    await this.stopWatching();
    const root = this.requireRoot();
    try {
      this.unlisten = await listen<VaultChangedEvent>("vault-changed", (event) => {
        void this.onExternalChange(event.payload);
      });
      await invoke("watch_vault", { root });
    } catch (error) {
      // Losing the watcher costs live updates, not correctness; the app still
      // re-reads on every navigation.
      console.warn("vault watch unavailable", error);
    }
  }

  async stopWatching(): Promise<void> {
    this.unlisten?.();
    this.unlisten = null;
    try {
      await invoke("unwatch_vault");
    } catch {
      // Nothing was being watched.
    }
  }

  private async onExternalChange(payload: VaultChangedEvent): Promise<void> {
    const now = Date.now();
    for (const [path, at] of this.selfWrites) {
      if (now - at > SELF_WRITE_GRACE) {
        this.selfWrites.delete(path);
      }
    }

    // A truncated event means a bulk change; refresh regardless of attribution.
    const external = payload.truncated
      ? payload.paths
      : payload.paths.filter((path) => !this.selfWrites.has(path));
    if (!external.length) {
      return;
    }

    await this.refreshNotes();
    this.revision.update((value) => value + 1);

    const active = this.activeNote()?.path;
    if (!active || !external.includes(active)) {
      return;
    }
    if (this.editorDirty()) {
      // Do not overwrite in-progress edits; let the user decide.
      this.conflict.set(active);
      return;
    }
    await this.openNote(active);
  }

  /** Reload the open note from disk, discarding unsaved edits. */
  async resolveConflict(): Promise<void> {
    const path = this.conflict();
    this.conflict.set(null);
    if (path) {
      this.editorDirty.set(false);
      await this.openNote(path);
    }
  }

  private markSelfWrite(path: string): void {
    this.selfWrites.set(path, Date.now());
  }

  /** Re-read the note list and vault summary from disk. */
  async refreshNotes(): Promise<void> {
    const root = this.requireRoot();
    const [notes, summary] = await Promise.all([
      invoke<NoteMeta[]>("list_notes", { root }),
      invoke<VaultSummary>("open_vault", { root })
    ]);
    this.notes.set(notes);
    this.vault.set(summary);
  }

  async openNote(path: string): Promise<NoteDetail | null> {
    const root = this.requireRoot();
    this.conflict.set(null);
    return this.guard(async () => {
      const note = await invoke<NoteDetail>("read_note", { root, path });
      this.activeNote.set(note);
      return note;
    });
  }

  /** Read a note without making it the active one — used for transclusion. */
  async peekNote(path: string): Promise<NoteDetail | null> {
    const root = this.requireRoot();
    try {
      return await invoke<NoteDetail>("read_note", { root, path });
    } catch {
      return null;
    }
  }

  /**
   * Persist a note's text.
   *
   * Backlinks and tags can change with any edit, so the note is re-read
   * afterwards rather than assuming the in-memory copy is still accurate.
   */
  async saveNote(path: string, content: string): Promise<void> {
    const root = this.requireRoot();
    await this.guard(async () => {
      const written = await invoke<string>("save_note", { root, path, content });
      this.markSelfWrite(written);
      await this.refreshNotes();
      const current = this.activeNote();
      if (current?.path === path) {
        await this.openNote(path);
      }
    });
  }

  async createNote(path: string, content = ""): Promise<string | null> {
    const root = this.requireRoot();
    return this.guard(async () => {
      const created = await invoke<string>("create_note", { root, path, content });
      this.markSelfWrite(created);
      await this.refreshNotes();
      await this.openNote(created);
      return created;
    });
  }

  async deleteNote(path: string): Promise<void> {
    const root = this.requireRoot();
    await this.guard(async () => {
      await invoke<string>("delete_note", { root, path });
      if (this.activeNote()?.path === path) {
        this.activeNote.set(null);
      }
      await this.refreshNotes();
    });
  }

  async createFolder(path: string): Promise<string | null> {
    const root = this.requireRoot();
    return this.guard(async () => {
      const created = await invoke<string>("create_folder", { root, path });
      await this.refreshNotes();
      return created;
    });
  }

  /**
   * Move or rename a folder, with the notes inside it.
   *
   * The open note may be one of them, so it is reopened at its new path rather
   * than left pointing at a file that no longer exists.
   */
  async moveFolder(from: string, to: string): Promise<string | null> {
    const root = this.requireRoot();
    if (!to || to === from) {
      return null;
    }
    return this.guard(async () => {
      const moved = await invoke<string>("move_folder", { root, from, to, updateLinks: true });
      const active = this.activeNote()?.path;
      await this.refreshNotes();
      if (active?.startsWith(`${from}/`)) {
        await this.openNote(`${moved}${active.slice(from.length)}`);
      }
      return moved;
    });
  }

  async deleteFolder(path: string): Promise<void> {
    const root = this.requireRoot();
    await this.guard(async () => {
      await invoke<string>("delete_folder", { root, path });
      if (this.activeNote()?.path.startsWith(`${path}/`)) {
        this.activeNote.set(null);
      }
      await this.refreshNotes();
    });
  }

  /** Notes inside a folder, at any depth — used to warn before deleting. */
  async notesInFolder(path: string): Promise<string[]> {
    const root = this.requireRoot();
    try {
      return await invoke<string[]>("notes_in_folder", { root, path });
    } catch {
      return [];
    }
  }

  /**
   * Move a note into a folder, keeping its filename.
   *
   * Passing "" moves it to the vault root. Returns null when the note is already
   * where it was dropped, so callers can stay quiet rather than flashing a
   * "moved" message for a move that did not happen.
   */
  async moveNoteToFolder(path: string, folder: string): Promise<string | null> {
    const fileName = path.split("/").pop() ?? path;
    const target = folder ? `${folder}/${fileName}` : fileName;
    if (target === path) {
      return null;
    }
    return this.renameNote(path, target);
  }

  async renameNote(from: string, to: string): Promise<string | null> {
    const root = this.requireRoot();
    return this.guard(async () => {
      const moved = await invoke<string>("rename_note", { root, from, to, updateLinks: true });
      await this.refreshNotes();
      if (this.activeNote()?.path === from) {
        await this.openNote(moved);
      }
      return moved;
    });
  }

  async search(query: string, limit = 50): Promise<SearchHit[]> {
    const root = this.requireRoot();
    if (!query.trim()) {
      return [];
    }
    return (await this.guard(() => invoke<SearchHit[]>("search_notes", { request: { root, query, limit } }))) ?? [];
  }

  /** The whole vault as a link graph, including not-yet-written notes. */
  async graph(includeGhosts = true): Promise<GraphData | null> {
    const root = this.requireRoot();
    return this.guard(() => invoke<GraphData>("graph", { root, includeGhosts }));
  }

  async unresolvedLinks(): Promise<UnresolvedLink[]> {
    const root = this.requireRoot();
    return (await this.guard(() => invoke<UnresolvedLink[]>("unresolved_links", { root }))) ?? [];
  }

  /** Resolve a `[[target]]` written in `from`, or null when nothing matches. */
  async resolveLink(from: string, target: string): Promise<string | null> {
    const root = this.requireRoot();
    try {
      return await invoke<string | null>("resolve_link", { root, from, target });
    } catch {
      return null;
    }
  }

  /**
   * Turn a wikilink target into the path a new note should take.
   *
   * Bare names land beside the note that linked to them, which is what a user
   * expects when they follow a link they just wrote.
   */
  pathForNewNote(from: string, target: string): string {
    const clean = target.trim().replace(/\.md$/i, "");
    if (clean.includes("/")) {
      return `${clean}.md`;
    }
    const folder = from.includes("/") ? from.slice(0, from.lastIndexOf("/")) : "";
    return folder ? `${folder}/${clean}.md` : `${clean}.md`;
  }

  // -------------------------------------------------------------------------
  // internals
  // -------------------------------------------------------------------------

  private requireRoot(): string {
    const vault = this.vault();
    if (!vault) {
      throw new Error("no vault is open");
    }
    return vault.path;
  }

  private applyVault(summary: VaultSummary): void {
    this.vault.set(summary);
    this.activeNote.set(null);
    writeStorage(LAST_VAULT_KEY, summary.path);
    this.rememberRecent(summary);
  }

  private rememberRecent(summary: VaultSummary): void {
    const entry: RecentVault = { path: summary.path, name: summary.name, openedAt: Date.now() };
    const next = [entry, ...this.recents().filter((item) => item.path !== summary.path)].slice(0, RECENTS_LIMIT);
    this.recents.set(next);
    writeStorage(RECENTS_KEY, JSON.stringify(next));
  }

  private loadRecents(): RecentVault[] {
    const raw = readStorage(RECENTS_KEY);
    if (!raw) {
      return [];
    }
    try {
      const parsed = JSON.parse(raw) as RecentVault[];
      return Array.isArray(parsed) ? parsed.filter((item) => typeof item?.path === "string") : [];
    } catch {
      return [];
    }
  }

  /** Run an operation with loading and error state handled once. */
  private async guard<T>(action: () => Promise<T>): Promise<T | null> {
    this.loading.set(true);
    this.error.set(null);
    try {
      return await action();
    } catch (error) {
      this.error.set(messageFor(error));
      return null;
    } finally {
      this.loading.set(false);
    }
  }
}

function messageFor(error: unknown): string {
  if (typeof error === "string") {
    return error;
  }
  if (error instanceof Error) {
    return error.message;
  }
  return "Something went wrong.";
}

function readStorage(key: string): string | null {
  try {
    return window.localStorage.getItem(key);
  } catch {
    return null;
  }
}

function writeStorage(key: string, value: string | null): void {
  try {
    if (value === null) {
      window.localStorage.removeItem(key);
    } else {
      window.localStorage.setItem(key, value);
    }
  } catch {
    // Storage being unavailable only costs us the convenience of recents.
  }
}

/**
 * Flatten note paths into an ordered folder tree.
 *
 * Folders come before notes at each level and both are sorted by name, so the
 * sidebar order is stable no matter what order the backend listed files in.
 *
 * `folders` lists directories that exist on disk. Passing them in is what keeps
 * an empty folder visible: everything else here is derived from note paths, so
 * a folder with nothing in it would otherwise leave no trace.
 */
export function buildTree(notes: NoteMeta[], folders: string[] = []): TreeEntry[] {
  interface Folder {
    name: string;
    path: string;
    folders: Map<string, Folder>;
    notes: NoteMeta[];
  }

  const root: Folder = { name: "", path: "", folders: new Map(), notes: [] };

  /** Walk to a folder, creating each level that is missing on the way down. */
  const descend = (segments: string[]): Folder => {
    let current = root;
    let walked = "";
    for (const segment of segments) {
      walked = walked ? `${walked}/${segment}` : segment;
      let next = current.folders.get(segment);
      if (!next) {
        next = { name: segment, path: walked, folders: new Map(), notes: [] };
        current.folders.set(segment, next);
      }
      current = next;
    }
    return current;
  };

  for (const folder of folders) {
    const segments = folder.split("/").filter(Boolean);
    if (segments.length) {
      descend(segments);
    }
  }

  for (const note of notes) {
    const segments = note.path.split("/");
    const fileName = segments.pop() ?? note.path;
    descend(segments).notes.push({ ...note, title: note.title || fileName });
  }

  const entries: TreeEntry[] = [];
  const collator = new Intl.Collator(undefined, { sensitivity: "base", numeric: true });

  const walk = (folder: Folder, depth: number): void => {
    const folders = [...folder.folders.values()].sort((a, b) => collator.compare(a.name, b.name));
    for (const child of folders) {
      entries.push({ kind: "folder", path: child.path, name: child.name, depth });
      walk(child, depth + 1);
    }
    const notes = [...folder.notes].sort((a, b) => collator.compare(a.title, b.title));
    for (const note of notes) {
      entries.push({ kind: "note", path: note.path, name: note.title, depth, note });
    }
  };

  walk(root, 0);
  return entries;
}
