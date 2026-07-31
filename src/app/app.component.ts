import {
  Component,
  HostListener,
  OnDestroy,
  OnInit,
  ViewChild,
  computed,
  effect,
  inject,
  signal
} from "@angular/core";
import { CommonModule } from "@angular/common";
import { invoke } from "@tauri-apps/api/core";
import { UnlistenFn, listen } from "@tauri-apps/api/event";
import { ask, open as openDialog } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";

import { Button } from "primeng/button";
import { Toast } from "primeng/toast";

import { AboutDialogComponent } from "./components/about-dialog.component";
import { CommandPaletteComponent, PaletteMode } from "./components/command-palette.component";
import { ContextMenuComponent, MenuItem } from "./components/context-menu.component";
import { ContextPanelComponent } from "./components/context-panel.component";
import { GraphViewComponent } from "./components/graph-view.component";
import { MarkdownViewComponent } from "./components/markdown-view.component";
import { NoteEditorComponent } from "./components/note-editor.component";
import { NoteTreeComponent, TreeContextEvent } from "./components/note-tree.component";
import { PromptDialogComponent } from "./components/prompt-dialog.component";
import { VaultLauncherComponent } from "./components/vault-launcher.component";
import { GraphData, Heading, ResolvedLink, SearchHit } from "./models/vault.models";

/** Read mode is the default; source mode is the toggle. */
export type ViewMode = "read" | "source";

/** The two panes flanking the note, either of which can be dragged wider. */
export type Pane = "sidebar" | "context";

const VIEW_MODE_KEY = "intentio-knowledge:view-mode";
const PANE_WIDTH_KEY = "intentio-knowledge:pane-widths";

/** Defaults match the CSS the layout shipped with: 15rem and 17rem. */
const DEFAULT_WIDTHS: Record<Pane, number> = { sidebar: 240, context: 272 };
const MIN_PANE = 160;
/** Neither pane may take so much room that the note has nowhere to go. */
const MAX_PANE_FRACTION = 0.4;
import { ThemeService } from "./services/theme.service";
import { UpdaterService } from "./services/updater.service";
import { VaultService } from "./services/vault.service";

/**
 * The application shell.
 *
 * Owns layout, keyboard shortcuts and the vault-open flow; everything else is
 * delegated to the child components and `VaultService`.
 */
@Component({
  selector: "app-root",
  standalone: true,
  imports: [
    CommonModule,
    Toast,
    Button,
    AboutDialogComponent,
    CommandPaletteComponent,
    ContextMenuComponent,
    ContextPanelComponent,
    GraphViewComponent,
    MarkdownViewComponent,
    NoteEditorComponent,
    NoteTreeComponent,
    PromptDialogComponent,
    VaultLauncherComponent
  ],
  templateUrl: "./app.component.html",
  styleUrls: ["./app.component.css"]
})
export class AppComponent implements OnInit, OnDestroy {
  readonly vaultService = inject(VaultService);
  readonly theme = inject(ThemeService);
  readonly updater = inject(UpdaterService);

  @ViewChild(NoteEditorComponent) editor?: NoteEditorComponent;

  readonly paletteMode = signal<PaletteMode | null>(null);
  readonly renaming = signal<string | null>(null);
  readonly renamingFolder = signal<string | null>(null);
  readonly newFolder = signal(false);
  readonly newFolderSeed = signal("");
  /** Folder a note created from the palette should land in, "" for the root. */
  readonly newNoteSeed = signal("");
  /** The open explorer context menu, with what it was opened on. */
  readonly contextMenu = signal<{ items: MenuItem[]; x: number; y: number; target: TreeContextEvent } | null>(null);
  readonly searchHits = signal<SearchHit[]>([]);
  readonly sidebarOpen = signal(true);
  readonly contextOpen = signal(true);
  readonly status = signal<string | null>(null);

  /** Pane widths in pixels, restored from the last session. */
  readonly paneWidth = signal<Record<Pane, number>>(this.loadPaneWidths());
  /** Which pane is being dragged, if any. */
  readonly resizing = signal<Pane | null>(null);

  /**
   * Column track sizes for the shell grid.
   *
   * Written inline so it overrides the class-based fallbacks in CSS, which still
   * own the grid areas for each combination of open panes.
   */
  readonly gridColumns = computed(() => {
    const widths = this.paneWidth();
    const tracks = [
      this.sidebarOpen() ? `${widths.sidebar}px` : null,
      "minmax(0, 1fr)",
      this.contextOpen() ? `${widths.context}px` : null
    ];
    return tracks.filter(Boolean).join(" ");
  });

  /** Rendered markdown by default; source is the toggle. */
  readonly viewMode = signal<ViewMode>(this.loadViewMode());
  readonly graphOpen = signal(false);
  readonly graphData = signal<GraphData | null>(null);
  readonly aboutOpen = signal(false);
  readonly appVersion = signal("0.1.0");

  /** True once the native menu is driving actions, so keys are not handled twice. */
  private nativeMenu = false;
  private menuUnlisten: UnlistenFn | null = null;

  readonly activePath = computed(() => this.vaultService.activeNote()?.path ?? null);

  constructor() {
    // The vault changed on disk. Refresh the graph if it is on screen; the
    // note list and open note are handled inside the service.
    effect(() => {
      this.vaultService.revision();
      if (this.graphOpen()) {
        void this.refreshGraph();
      }
    });
  }

  /** Hand a found update to the updater, from the toast's action button. */
  installUpdate(message: { data?: unknown }): void {
    void this.updater.installUpdate(message.data);
  }

  async ngOnInit(): Promise<void> {
    await this.connectNativeMenu();
    void this.loadVersion();

    // Delay slightly so the app shell is fully rendered before showing a toast.
    setTimeout(() => void this.updater.checkForUpdates(), 3000);

    // Reopening the last vault is the overwhelmingly common intent; a failure
    // here just lands the user on the launcher.
    const last = this.vaultService.lastVaultPath();
    if (last) {
      await this.vaultService.openVault(last);
      if (this.vaultService.isOpen()) {
        await this.vaultService.startWatching();
        await this.openFirstNote();
      }
    }
  }

  ngOnDestroy(): void {
    this.menuUnlisten?.();
    this.menuUnlisten = null;
  }

  private async loadVersion(): Promise<void> {
    try {
      const { getVersion } = await import("@tauri-apps/api/app");
      this.appVersion.set(await getVersion());
    } catch {
      // Running outside the desktop shell; the fallback version stands.
    }
  }

  /**
   * Wire the native menu bar to the handlers the app already uses.
   *
   * Once connected the menu owns its accelerators — the OS claims those keys
   * before the webview sees them — so `handleShortcut` stands down to avoid
   * running the same action twice on platforms that pass the key through.
   */
  private async connectNativeMenu(): Promise<void> {
    if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
      return;
    }
    try {
      this.menuUnlisten = await listen<string>("menu-action", (event) => {
        void this.runMenuAction(event.payload);
      });
      this.nativeMenu = true;
      await this.pushRecentsToMenu();
    } catch (error) {
      // Without the native menu the in-app keyboard shortcuts still work.
      console.warn("Native menu unavailable", error);
    }
  }

  private async runMenuAction(action: string): Promise<void> {
    if (action.startsWith("recent:")) {
      const index = Number.parseInt(action.slice("recent:".length), 10);
      const entry = this.vaultService.recents()[index];
      if (entry) {
        await this.openRecent(entry.path);
      }
      return;
    }

    switch (action) {
      case "new-note":
      case "jump":
        this.openPalette("jump");
        break;
      case "search":
        this.openPalette("search");
        break;
      case "open-vault":
        await this.chooseVault(false);
        break;
      case "new-vault":
        await this.chooseVault(true);
        break;
      case "close-vault":
        await this.closeVault();
        break;
      case "save":
        this.editor?.flushPendingSave();
        break;
      case "rename":
        this.startRename();
        break;
      case "delete":
        await this.deleteActive();
        break;
      case "toggle-view":
        this.toggleViewMode();
        break;
      case "toggle-graph":
        await this.toggleGraph();
        break;
      case "toggle-sidebar":
        this.toggleSidebar();
        break;
      case "toggle-panel":
        this.toggleContext();
        break;
      case "toggle-theme":
        this.theme.cycle();
        break;
      case "about":
        this.aboutOpen.set(true);
        break;
      case "check-updates":
        // Reports through the toast, so the dialog is not needed to see it.
        await this.updater.manualCheck();
        break;
      case "website":
        await openUrl("https://intentiosoftware.com").catch(() => undefined);
        break;
      default:
        break;
    }
  }

  /** Keep the native Open Recent submenu in step with stored recents. */
  private async pushRecentsToMenu(): Promise<void> {
    if (!this.nativeMenu) {
      return;
    }
    try {
      await invoke("set_recent_vaults", {
        recents: this.vaultService.recents().map((entry) => ({ name: entry.name }))
      });
    } catch (error) {
      console.warn("Could not update recent vaults menu", error);
    }
  }

  private async refreshGraph(): Promise<void> {
    this.graphData.set(await this.vaultService.graph());
  }

  // -------------------------------------------------------------------------
  // vault lifecycle
  // -------------------------------------------------------------------------

  async chooseVault(create: boolean): Promise<void> {
    const selected = await openDialog({
      directory: true,
      multiple: false,
      title: create ? "Choose a folder for the new vault" : "Open a vault folder"
    });
    if (typeof selected !== "string") {
      return;
    }
    if (create) {
      await this.vaultService.createVault(selected);
    } else {
      await this.vaultService.openVault(selected);
    }
    await this.afterVaultOpened();
  }

  async openRecent(path: string): Promise<void> {
    await this.vaultService.openVault(path);
    await this.afterVaultOpened();
  }

  async closeVault(): Promise<void> {
    this.editor?.flushPendingSave();
    this.graphOpen.set(false);
    await this.vaultService.closeVault();
  }

  private async afterVaultOpened(): Promise<void> {
    if (!this.vaultService.isOpen()) {
      return;
    }
    await this.vaultService.startWatching();
    // Opening a vault reorders recents, which the native menu holds a copy of.
    await this.pushRecentsToMenu();
    await this.openFirstNote();
  }

  private async openFirstNote(): Promise<void> {
    const first = this.vaultService.notes()[0];
    if (first) {
      await this.vaultService.openNote(first.path);
    }
  }

  // -------------------------------------------------------------------------
  // notes
  // -------------------------------------------------------------------------

  async openNote(path: string): Promise<void> {
    this.editor?.flushPendingSave();
    await this.vaultService.openNote(path);
    this.closePalette();
  }

  async saveActive(content: string): Promise<void> {
    const path = this.activePath();
    if (!path) {
      return;
    }
    await this.vaultService.saveNote(path, content);
    this.flash("Saved");
  }

  /**
   * Create a note from a name typed in the palette or a link that resolves to
   * nothing. Seeds it with an H1 so the note has a title straight away.
   */
  async createNote(name: string): Promise<void> {
    const from = this.activePath() ?? "";
    // "New note here" asked for a specific folder; a name typed with its own
    // path still wins, since that is the more explicit instruction.
    const seed = this.newNoteSeed();
    this.newNoteSeed.set("");
    const target = seed && !name.includes("/") ? `${seed}${name}` : name;
    const path = this.uniquePath(this.vaultService.pathForNewNote(from, target));
    const title = path.replace(/\.md$/i, "").split("/").pop() ?? name;
    const created = await this.vaultService.createNote(path, `# ${title}\n\n`);
    this.closePalette();
    if (created) {
      this.flash(`Created ${created}`);
      this.editor?.focus();
    }
  }

  /**
   * Suffix a path until it is free.
   *
   * Creating a note must never fail just because "Untitled" is taken — the user
   * asked for a new note, not for a specific filename.
   */
  private uniquePath(path: string): string {
    const taken = new Set(this.vaultService.notes().map((note) => note.path.toLowerCase()));
    if (!taken.has(path.toLowerCase())) {
      return path;
    }
    const base = path.replace(/\.md$/i, "");
    for (let suffix = 2; suffix < 1000; suffix += 1) {
      const candidate = `${base} ${suffix}.md`;
      if (!taken.has(candidate.toLowerCase())) {
        return candidate;
      }
    }
    return path;
  }

  async deleteActive(): Promise<void> {
    const path = this.activePath();
    if (!path) {
      return;
    }
    // The platform dialog, not `window.confirm`, which the webview may ignore.
    const confirmed = await ask(`Delete ${path}?\n\nThis cannot be undone from here.`, {
      title: "Delete note",
      kind: "warning"
    });
    if (!confirmed) {
      return;
    }
    await this.vaultService.deleteNote(path);
    await this.openFirstNote();
  }

  startRename(): void {
    const path = this.activePath();
    if (path) {
      this.renaming.set(path);
    }
  }

  // -------------------------------------------------------------------------
  // folders
  // -------------------------------------------------------------------------

  /**
   * Ask for a new folder name, seeded with the open note's folder so nesting
   * one inside the folder you are already looking at takes no typing.
   */
  startNewFolder(): void {
    const active = this.activePath() ?? "";
    const slash = active.lastIndexOf("/");
    const parent = slash === -1 ? "" : active.slice(0, slash);
    this.newFolderSeed.set(parent ? `${parent}/` : "");
    this.newFolder.set(true);
  }

  async confirmNewFolder(path: string): Promise<void> {
    this.newFolder.set(false);
    const clean = path.trim().replace(/^\/+|\/+$/g, "");
    if (!clean) {
      return;
    }
    const created = await this.vaultService.createFolder(clean);
    if (created) {
      this.flash(`Created ${created}`);
    }
  }

  /** A note was dragged onto a folder in the sidebar. */
  async moveNote(event: { path: string; folder: string }): Promise<void> {
    this.editor?.flushPendingSave();
    const moved = await this.vaultService.moveNoteToFolder(event.path, event.folder);
    if (moved) {
      this.flash(`Moved to ${moved}`);
    }
  }

  /** A folder was dragged onto another folder, or onto the vault root. */
  async moveFolder(event: { path: string; folder: string }): Promise<void> {
    const name = event.path.split("/").pop() ?? event.path;
    const target = event.folder ? `${event.folder}/${name}` : name;
    this.editor?.flushPendingSave();
    const moved = await this.vaultService.moveFolder(event.path, target);
    if (moved) {
      this.flash(`Moved to ${moved}`);
    }
  }

  startFolderRename(path: string): void {
    this.renamingFolder.set(path);
  }

  async confirmFolderRename(next: string): Promise<void> {
    const path = this.renamingFolder();
    this.renamingFolder.set(null);
    const clean = next.trim().replace(/^\/+|\/+$/g, "");
    if (!path || !clean || clean === path) {
      return;
    }
    this.editor?.flushPendingSave();
    const moved = await this.vaultService.moveFolder(path, clean);
    if (moved) {
      this.flash(`Moved to ${moved}`);
    }
  }

  /**
   * Delete a folder and everything inside it.
   *
   * The count comes from the backend rather than the loaded note list so the
   * warning covers what is actually on disk, including anything the sidebar has
   * not caught up with.
   */
  async deleteFolder(path: string): Promise<void> {
    const notes = await this.vaultService.notesInFolder(path);
    const summary = notes.length === 1 ? "1 note" : `${notes.length} notes`;
    const confirmed = await ask(
      `Delete ${path} and the ${summary} inside it?\n\nThis cannot be undone from here.`,
      { title: "Delete folder", kind: "warning" }
    );
    if (!confirmed) {
      return;
    }
    await this.vaultService.deleteFolder(path);
    this.flash(`Deleted ${path}`);
    if (!this.vaultService.activeNote()) {
      await this.openFirstNote();
    }
  }

  async confirmRename(next: string): Promise<void> {
    const path = this.renaming();
    this.renaming.set(null);
    if (!path || next === path) {
      return;
    }
    this.editor?.flushPendingSave();
    const moved = await this.vaultService.renameNote(path, next);
    if (moved) {
      this.flash(`Moved to ${moved}`);
    }
  }

  /**
   * Follow a `[[link]]`, creating the note when the target does not exist yet.
   *
   * Resolution goes through the backend so it matches what agents and backlinks
   * see, rather than re-implementing the rules in the editor.
   */
  async followLink(target: string): Promise<void> {
    const from = this.activePath();
    if (!from) {
      return;
    }
    const resolved = await this.vaultService.resolveLink(from, target);
    if (resolved) {
      await this.openNote(resolved);
      return;
    }
    await this.createNote(target);
  }

  async followUnresolvedLink(link: ResolvedLink): Promise<void> {
    await this.followLink(link.target);
  }

  // -------------------------------------------------------------------------
  // explorer context menu
  // -------------------------------------------------------------------------

  openContextMenu(event: TreeContextEvent): void {
    this.contextMenu.set({ items: this.menuFor(event), x: event.x, y: event.y, target: event });
  }

  private menuFor(event: TreeContextEvent): MenuItem[] {
    if (event.kind === "note") {
      return [
        { action: "open", label: "Open", icon: "pi-file" },
        { separator: true },
        { action: "rename-note", label: "Rename or move…", icon: "pi-pencil" },
        { action: "delete-note", label: "Delete note", icon: "pi-trash", danger: true }
      ];
    }
    if (event.kind === "folder") {
      return [
        { action: "new-note", label: "New note here", icon: "pi-file-plus" },
        { action: "new-folder", label: "New folder inside", icon: "pi-folder" },
        { separator: true },
        { action: "rename-folder", label: "Rename or move…", icon: "pi-pencil" },
        { action: "delete-folder", label: "Delete folder", icon: "pi-trash", danger: true }
      ];
    }
    return [
      { action: "new-note", label: "New note", icon: "pi-file-plus" },
      { action: "new-folder", label: "New folder", icon: "pi-folder" }
    ];
  }

  async runContextAction(action: string): Promise<void> {
    const menu = this.contextMenu();
    this.contextMenu.set(null);
    if (!menu) {
      return;
    }
    const { kind, path } = menu.target;
    // For a note the folder is the one it sits in; for a folder it is itself.
    const folder = kind === "note" ? (path.includes("/") ? path.slice(0, path.lastIndexOf("/")) : "") : path;

    switch (action) {
      case "open":
        await this.openNote(path);
        return;
      case "rename-note":
        this.renaming.set(path);
        return;
      case "delete-note":
        await this.deleteNoteAt(path);
        return;
      case "new-note":
        // Seeded with the folder so the note is created where it was asked for.
        this.newNoteSeed.set(folder ? `${folder}/` : "");
        this.openPalette("jump");
        return;
      case "new-folder":
        this.newFolderSeed.set(folder ? `${folder}/` : "");
        this.newFolder.set(true);
        return;
      case "rename-folder":
        this.startFolderRename(path);
        return;
      case "delete-folder":
        await this.deleteFolder(path);
        return;
    }
  }

  /** Delete any note, not only the open one. */
  private async deleteNoteAt(path: string): Promise<void> {
    const confirmed = await ask(`Delete ${path}?\n\nThis cannot be undone from here.`, {
      title: "Delete note",
      kind: "warning"
    });
    if (!confirmed) {
      return;
    }
    const wasActive = this.activePath() === path;
    await this.vaultService.deleteNote(path);
    this.flash(`Deleted ${path}`);
    if (wasActive) {
      await this.openFirstNote();
    }
  }

  // -------------------------------------------------------------------------
  // palette
  // -------------------------------------------------------------------------

  openPalette(mode: PaletteMode): void {
    this.searchHits.set([]);
    this.paletteMode.set(mode);
  }

  closePalette(): void {
    this.paletteMode.set(null);
    // Dismissing the palette abandons the "new note here" folder with it.
    this.newNoteSeed.set("");
  }

  async onPaletteQuery(query: string): Promise<void> {
    if (this.paletteMode() !== "search") {
      return;
    }
    this.searchHits.set(await this.vaultService.search(query));
  }

  async searchTag(tag: string): Promise<void> {
    this.openPalette("search");
    this.searchHits.set(await this.vaultService.search(`#${tag}`));
  }

  /** Jump the editor to a heading by re-opening the note focused. */
  scrollToHeading(heading: Heading): void {
    this.flash(`${heading.text}`);
    this.editor?.focus();
  }

  // -------------------------------------------------------------------------
  // view mode and graph
  // -------------------------------------------------------------------------

  setViewMode(mode: ViewMode): void {
    // Switching away from source must not lose an edit still in the debounce.
    if (mode !== "source") {
      this.editor?.flushPendingSave();
    }
    this.viewMode.set(mode);
    try {
      window.localStorage.setItem(VIEW_MODE_KEY, mode);
    } catch {
      // Persistence is a convenience, not a requirement.
    }
    if (mode === "source") {
      // The editor is created on this tick; focus after it exists.
      setTimeout(() => this.editor?.focus());
    }
  }

  toggleViewMode(): void {
    this.setViewMode(this.viewMode() === "read" ? "source" : "read");
  }

  private loadViewMode(): ViewMode {
    try {
      return window.localStorage.getItem(VIEW_MODE_KEY) === "source" ? "source" : "read";
    } catch {
      return "read";
    }
  }

  /**
   * Show or hide the graph.
   *
   * The graph is rebuilt each time it opens rather than kept in sync, because
   * the vault can change underneath the app between viewings.
   */
  async toggleGraph(): Promise<void> {
    const opening = !this.graphOpen();
    this.graphOpen.set(opening);
    if (opening) {
      this.editor?.flushPendingSave();
      await this.refreshGraph();
    }
  }

  async openFromGraph(path: string): Promise<void> {
    this.graphOpen.set(false);
    await this.openNote(path);
  }

  async createFromGraph(target: string): Promise<void> {
    this.graphOpen.set(false);
    await this.createNote(target);
  }

  // -------------------------------------------------------------------------
  // chrome
  // -------------------------------------------------------------------------

  toggleSidebar(): void {
    this.sidebarOpen.update((open) => !open);
  }

  toggleContext(): void {
    this.contextOpen.update((open) => !open);
  }

  // -------------------------------------------------------------------------
  // resizing
  // -------------------------------------------------------------------------

  /**
   * Begin a pane drag.
   *
   * The pointer is captured so the drag survives the cursor leaving the thin
   * handle, which is otherwise very easy to do.
   */
  startResize(event: PointerEvent, pane: Pane): void {
    event.preventDefault();
    const handle = event.target as HTMLElement;
    handle.setPointerCapture?.(event.pointerId);
    this.resizing.set(pane);

    const startX = event.clientX;
    const startWidth = this.paneWidth()[pane];

    const onMove = (move: PointerEvent): void => {
      // The sidebar grows as the pointer moves right; the right-hand panel
      // grows as it moves left.
      const delta = pane === "sidebar" ? move.clientX - startX : startX - move.clientX;
      this.setPaneWidth(pane, startWidth + delta);
    };

    const onUp = (): void => {
      handle.releasePointerCapture?.(event.pointerId);
      handle.removeEventListener("pointermove", onMove);
      handle.removeEventListener("pointerup", onUp);
      handle.removeEventListener("pointercancel", onUp);
      this.resizing.set(null);
      this.savePaneWidths();
    };

    handle.addEventListener("pointermove", onMove);
    handle.addEventListener("pointerup", onUp);
    handle.addEventListener("pointercancel", onUp);
  }

  resetPane(pane: Pane): void {
    this.setPaneWidth(pane, DEFAULT_WIDTHS[pane]);
    this.savePaneWidths();
  }

  private setPaneWidth(pane: Pane, width: number): void {
    const max = Math.max(MIN_PANE, window.innerWidth * MAX_PANE_FRACTION);
    const clamped = Math.round(Math.min(max, Math.max(MIN_PANE, width)));
    this.paneWidth.update((current) => ({ ...current, [pane]: clamped }));
  }

  private loadPaneWidths(): Record<Pane, number> {
    try {
      const raw = window.localStorage.getItem(PANE_WIDTH_KEY);
      if (!raw) {
        return { ...DEFAULT_WIDTHS };
      }
      const parsed = JSON.parse(raw) as Partial<Record<Pane, number>>;
      const pick = (pane: Pane): number =>
        typeof parsed[pane] === "number" && Number.isFinite(parsed[pane]) ? (parsed[pane] as number) : DEFAULT_WIDTHS[pane];
      return { sidebar: pick("sidebar"), context: pick("context") };
    } catch {
      return { ...DEFAULT_WIDTHS };
    }
  }

  private savePaneWidths(): void {
    try {
      window.localStorage.setItem(PANE_WIDTH_KEY, JSON.stringify(this.paneWidth()));
    } catch {
      // Losing the widths only costs the layout on next launch.
    }
  }

  @HostListener("window:keydown", ["$event"])
  handleShortcut(event: KeyboardEvent): void {
    if (event.key === "Escape" && this.contextMenu()) {
      event.preventDefault();
      this.contextMenu.set(null);
      return;
    }
    if (event.key === "Escape" && this.aboutOpen()) {
      event.preventDefault();
      this.aboutOpen.set(false);
      return;
    }
    const mod = event.metaKey || event.ctrlKey;
    if (!mod) {
      return;
    }
    // The native menu owns these accelerators once it is connected; handling
    // them here as well would run each action twice.
    if (this.nativeMenu) {
      return;
    }
    const key = event.key.toLowerCase();

    // Cmd+S is handled inside the editor so it can save the live document.
    if (key === "o" && !event.shiftKey) {
      event.preventDefault();
      this.openPalette("jump");
    } else if (key === "p" && event.shiftKey) {
      event.preventDefault();
      this.openPalette("jump");
    } else if (key === "f" && event.shiftKey) {
      event.preventDefault();
      this.openPalette("search");
    } else if (key === "n" && !event.shiftKey) {
      event.preventDefault();
      // The palette already offers "Create …" for whatever is typed, so naming
      // a new note and jumping to an existing one are the same interaction.
      this.openPalette("jump");
    } else if (key === "e" && !event.shiftKey) {
      event.preventDefault();
      this.toggleViewMode();
    } else if (key === "g" && !event.shiftKey) {
      event.preventDefault();
      void this.toggleGraph();
    } else if (key === "b" && !event.shiftKey) {
      event.preventDefault();
      this.toggleSidebar();
    } else if (key === "\\") {
      event.preventDefault();
      this.toggleContext();
    }
  }

  @HostListener("window:beforeunload")
  saveBeforeUnload(): void {
    this.editor?.flushPendingSave();
  }

  /** Briefly show a message in the status bar. */
  private flash(message: string): void {
    this.status.set(message);
    setTimeout(() => {
      if (this.status() === message) {
        this.status.set(null);
      }
    }, 2000);
  }
}
