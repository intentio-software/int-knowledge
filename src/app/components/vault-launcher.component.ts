import { ChangeDetectionStrategy, Component, EventEmitter, Input, Output } from "@angular/core";
import { CommonModule } from "@angular/common";

import { RecentVault } from "../models/vault.models";

/**
 * The first screen: pick a folder to use as a vault.
 *
 * A vault is just a directory, so this deliberately avoids any notion of import
 * or migration — the user points at somewhere on their disk and that is that.
 */
@Component({
  selector: "app-vault-launcher",
  standalone: true,
  imports: [CommonModule],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="launcher">
      <div class="panel">
        <h1>Intentio Knowledge</h1>
        <p class="lede">
          A vault is an ordinary folder of markdown files on your own disk. Open one you already
          have, or start a new one.
        </p>

        <div class="actions">
          <button type="button" class="primary" (click)="openRequested.emit()">
            <i class="pi pi-folder-open"></i>
            Open folder
          </button>
          <button type="button" class="ghost" (click)="createRequested.emit()">
            <i class="pi pi-plus"></i>
            New vault
          </button>
        </div>

        <div class="recents" *ngIf="recents.length">
          <h2>Recent</h2>
          <button
            type="button"
            class="recent"
            *ngFor="let recent of recents; trackBy: trackRecent"
            (click)="recentChosen.emit(recent.path)"
          >
            <span class="name">{{ recent.name }}</span>
            <span class="path">{{ recent.path }}</span>
          </button>
        </div>

        <p class="error" *ngIf="error">{{ error }}</p>

        <p class="foot">
          Point the bundled MCP server at the same folder and an AI agent can read and write these
          notes alongside you.
        </p>
      </div>
    </div>
  `,
  styles: [
    `
      .launcher {
        display: flex;
        align-items: center;
        justify-content: center;
        height: 100%;
        padding: 2rem;
      }
      .panel {
        width: min(34rem, 100%);
      }
      h1 {
        margin: 0 0 0.5rem;
        font-size: 1.7rem;
        font-weight: 650;
        color: var(--ink-strong);
        letter-spacing: -0.01em;
      }
      .lede {
        margin: 0 0 1.6rem;
        color: var(--ink-muted);
        line-height: 1.65;
        font-size: 0.95rem;
      }
      .actions {
        display: flex;
        gap: 0.7rem;
        margin-bottom: 2rem;
      }
      button {
        display: inline-flex;
        align-items: center;
        gap: 0.5rem;
        padding: 0.65rem 1.15rem;
        border-radius: 999px;
        border: 1px solid transparent;
        font-size: 0.9rem;
        cursor: pointer;
        transition: background 0.15s ease, border-color 0.15s ease;
      }
      button.primary {
        background: var(--accent);
        color: #fff;
      }
      button.primary:hover {
        background: var(--accent-strong);
      }
      button.ghost {
        background: transparent;
        border-color: var(--border);
        color: var(--ink);
      }
      button.ghost:hover {
        background: var(--hover);
      }
      h2 {
        margin: 0 0 0.6rem;
        font-size: 0.7rem;
        text-transform: uppercase;
        letter-spacing: 0.1em;
        color: var(--ink-faint);
        font-weight: 600;
      }
      .recent {
        display: flex;
        flex-direction: column;
        align-items: flex-start;
        gap: 0.15rem;
        width: 100%;
        padding: 0.55rem 0.75rem;
        margin-bottom: 0.3rem;
        border-radius: 9px;
        background: transparent;
        border: 1px solid transparent;
        text-align: left;
      }
      .recent:hover {
        background: var(--hover);
        border-color: var(--border);
      }
      .recent .name {
        color: var(--ink-strong);
        font-size: 0.9rem;
      }
      .recent .path {
        color: var(--ink-faint);
        font-size: 0.75rem;
        font-family: var(--font-mono);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        max-width: 100%;
      }
      .error {
        margin: 1rem 0 0;
        padding: 0.6rem 0.8rem;
        border-radius: 8px;
        background: color-mix(in srgb, var(--danger) 15%, transparent);
        color: var(--danger);
        font-size: 0.85rem;
      }
      .foot {
        margin: 2.5rem 0 0;
        padding-top: 1.2rem;
        border-top: 1px solid var(--border);
        color: var(--ink-faint);
        font-size: 0.8rem;
        line-height: 1.6;
      }
    `
  ]
})
export class VaultLauncherComponent {
  @Input() recents: RecentVault[] = [];
  @Input() error: string | null = null;

  @Output() readonly openRequested = new EventEmitter<void>();
  @Output() readonly createRequested = new EventEmitter<void>();
  @Output() readonly recentChosen = new EventEmitter<string>();

  trackRecent(_: number, recent: RecentVault): string {
    return recent.path;
  }
}
