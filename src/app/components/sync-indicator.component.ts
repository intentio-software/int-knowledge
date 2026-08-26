import {
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  HostListener,
  Input,
  OnDestroy,
  OnInit,
  inject,
  signal
} from "@angular/core";
import { CommonModule } from "@angular/common";
import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";

interface SyncStatus {
  isRepo: boolean;
  hasRemote: boolean;
  branch?: string;
  dirty: boolean;
  ahead: number;
  behind: number;
  blocked?: string;
}

interface SyncOutcome {
  changed: boolean;
  message: string;
  blocked?: string;
}

interface VaultSync {
  enabled: boolean;
  intervalSeconds: number;
}

/**
 * Where the vault stands with its Git remote, and the switch that keeps it there.
 *
 * It lives in the status bar rather than a settings dialog because sync is a
 * property of the vault you have open, and because the thing you want most of
 * the time is not the setting but the reassurance that it ran.
 */
@Component({
  selector: "app-sync-indicator",
  standalone: true,
  imports: [CommonModule],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (status(); as s) {
      @if (s.isRepo) {
        <span class="sync" [class.blocked]="!!blockedReason()">
          <button type="button" class="face" (click)="open.set(!open())" [title]="tooltip()">
            <i class="pi" [ngClass]="icon()"></i>
            <span class="label">{{ label() }}</span>
          </button>

          @if (open()) {
            <div class="panel">
              <label class="row">
                <input type="checkbox" [checked]="settings().enabled" (change)="toggle($event)" />
                <span>Keep this vault in sync</span>
              </label>

              <p class="detail">
                {{ s.branch }}<span *ngIf="!s.hasRemote"> — no remote</span>
                <span *ngIf="s.ahead"> · {{ s.ahead }} to push</span>
                <span *ngIf="s.behind"> · {{ s.behind }} to pull</span>
              </p>

              <label class="row interval" [class.dim]="!settings().enabled">
                <span>Check for changes every</span>
                <select
                  [disabled]="!settings().enabled"
                  (change)="chooseInterval($event)"
                >
                  @for (choice of intervals; track choice.seconds) {
                    <option
                      [value]="choice.seconds"
                      [selected]="choice.seconds === settings().intervalSeconds"
                    >
                      {{ choice.label }}
                    </option>
                  }
                </select>
              </label>

              <p class="behaviour">
                Your own changes are committed once you have stopped writing for
                a couple of minutes, so a session becomes one commit rather than
                a run of them.
              </p>

              @if (blockedReason(); as reason) {
                <p class="reason">{{ reason }}</p>
              }

              <button type="button" class="now" [disabled]="busy()" (click)="syncNow()">
                {{ busy() ? "Syncing…" : "Sync now" }}
              </button>
            </div>
          }
        </span>
      }
    }
  `,
  styles: [
    `
      .sync {
        position: relative;
        display: inline-flex;
      }
      .face {
        display: inline-flex;
        align-items: center;
        gap: 0.3rem;
        border: none;
        background: transparent;
        color: var(--ink-faint, #8aa);
        font: inherit;
        font-size: 0.75rem;
        cursor: pointer;
      }
      .face:hover {
        color: var(--accent, #f05f36);
      }
      .sync.blocked .face {
        color: var(--accent, #f05f36);
      }
      .panel {
        position: absolute;
        bottom: 1.6rem;
        right: 0;
        z-index: 40;
        width: 17rem;
        padding: 0.7rem 0.8rem;
        border: 1px solid var(--border, #2a3a44);
        border-radius: 10px;
        background: var(--panel-raised, #101c24);
        box-shadow: 0 18px 40px rgba(0, 0, 0, 0.45);
        text-align: left;
      }
      .row {
        display: flex;
        align-items: center;
        gap: 0.45rem;
        font-size: 0.82rem;
      }
      .detail {
        margin: 0.45rem 0 0;
        font-size: 0.72rem;
        color: var(--ink-faint, #8aa);
      }
      .reason {
        margin: 0.45rem 0 0;
        font-size: 0.72rem;
        line-height: 1.5;
        color: var(--accent, #f05f36);
      }
      .interval {
        margin-top: 0.6rem;
        justify-content: space-between;
        font-size: 0.78rem;
      }
      .interval.dim {
        opacity: 0.5;
      }
      .interval select {
        padding: 0.15rem 0.3rem;
        border: 1px solid var(--border, #2a3a44);
        border-radius: 6px;
        background: var(--surface, #0b141a);
        color: inherit;
        font: inherit;
        font-size: 0.75rem;
      }
      .behaviour {
        margin: 0.5rem 0 0;
        font-size: 0.7rem;
        line-height: 1.5;
        color: var(--ink-faint, #8aa);
      }
      .now {
        margin-top: 0.6rem;
        padding: 0.25rem 0.6rem;
        border: 1px solid var(--border, #2a3a44);
        border-radius: 7px;
        background: transparent;
        color: inherit;
        font: inherit;
        font-size: 0.75rem;
        cursor: pointer;
      }
      .now:disabled {
        opacity: 0.5;
        cursor: default;
      }
    `
  ]
})
export class SyncIndicatorComponent implements OnInit, OnDestroy {
  @Input({ required: true }) set vault(path: string | null) {
    this.path = path;
    void this.refresh();
  }

  readonly status = signal<SyncStatus | null>(null);
  readonly settings = signal<VaultSync>({ enabled: false, intervalSeconds: 180 });
  readonly open = signal(false);
  readonly busy = signal(false);
  /** The last thing that stopped a sync, cleared by the next successful one. */
  readonly lastBlock = signal<string | null>(null);

  /** Deliberately few: the interval is the only one worth varying by person. */
  readonly intervals = [
    { seconds: 60, label: "1 minute" },
    { seconds: 180, label: "3 minutes" },
    { seconds: 300, label: "5 minutes" },
    { seconds: 900, label: "15 minutes" },
    { seconds: 1800, label: "30 minutes" }
  ];

  private readonly host = inject(ElementRef<HTMLElement>);
  private path: string | null = null;
  private unlisten: UnlistenFn | null = null;
  private poll: ReturnType<typeof setInterval> | null = null;

  async ngOnInit(): Promise<void> {
    this.unlisten = await listen<SyncOutcome>("vault-sync", (event) => {
      this.lastBlock.set(event.payload.blocked ?? null);
      void this.refresh();
    }).catch(() => null as unknown as UnlistenFn);
    // The counts drift as the other person pushes, so they are re-read
    // periodically rather than only when something happens here.
    this.poll = setInterval(() => void this.refresh(), 30_000);
  }

  /** A panel that only closes by pressing the thing that opened it is a trap. */
  @HostListener("document:pointerdown", ["$event"])
  onPointerDown(event: PointerEvent): void {
    if (!this.open()) {
      return;
    }
    const target = event.target as Node | null;
    if (target && !this.host.nativeElement.contains(target)) {
      this.open.set(false);
    }
  }

  @HostListener("document:keydown.escape")
  onEscape(): void {
    this.open.set(false);
  }

  ngOnDestroy(): void {
    this.unlisten?.();
    if (this.poll) {
      clearInterval(this.poll);
    }
  }

  blockedReason(): string | null {
    return this.lastBlock() ?? this.status()?.blocked ?? null;
  }

  icon(): string {
    if (this.blockedReason()) return "pi-exclamation-triangle";
    if (!this.settings().enabled) return "pi-cloud";
    return this.busy() ? "pi-spin pi-spinner" : "pi-sync";
  }

  label(): string {
    const status = this.status();
    if (!status) return "";
    if (this.blockedReason()) return "Sync paused";
    if (!this.settings().enabled) return "Sync off";
    if (status.ahead || status.behind) return "Syncing";
    return "In sync";
  }

  tooltip(): string {
    return this.blockedReason() ?? "Git sync for this vault";
  }

  async toggle(event: Event): Promise<void> {
    const enabled = (event.target as HTMLInputElement).checked;
    if (!this.path) return;
    await invoke("set_vault_sync", { vault: this.path, enabled, intervalSeconds: null });
    this.settings.set({ ...this.settings(), enabled });
    // Turning it on should do something immediately; waiting three minutes to
    // find out whether it works is not reassuring.
    if (enabled) {
      await this.syncNow();
    }
  }

  async chooseInterval(event: Event): Promise<void> {
    const seconds = Number((event.target as HTMLSelectElement).value);
    if (!this.path || !Number.isFinite(seconds)) {
      return;
    }
    await invoke("set_vault_sync", {
      vault: this.path,
      enabled: this.settings().enabled,
      intervalSeconds: seconds
    });
    this.settings.set({ ...this.settings(), intervalSeconds: seconds });
  }

  async syncNow(): Promise<void> {
    if (!this.path || this.busy()) return;
    this.busy.set(true);
    try {
      const outcome = await invoke<SyncOutcome>("git_sync_now", { vault: this.path });
      this.lastBlock.set(outcome.blocked ?? null);
    } finally {
      this.busy.set(false);
      await this.refresh();
    }
  }

  private async refresh(): Promise<void> {
    if (!this.path) {
      this.status.set(null);
      return;
    }
    try {
      this.status.set(await invoke<SyncStatus>("git_sync_status", { vault: this.path }));
      this.settings.set(await invoke<VaultSync>("vault_sync_settings", { vault: this.path }));
    } catch {
      // Outside Tauri, or the vault went away. Showing nothing is right.
      this.status.set(null);
    }
  }
}
