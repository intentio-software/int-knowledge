import { ChangeDetectionStrategy, Component, EventEmitter, Input, Output, inject } from "@angular/core";
import { CommonModule } from "@angular/common";
import { openUrl } from "@tauri-apps/plugin-opener";

import { UpdaterService } from "../services/updater.service";

/**
 * The About dialog.
 *
 * Styled to match Intentio Mind Map's — same gradient card, same layout, same
 * inline update check — so the two apps read as one suite rather than two
 * products that happen to share a logo.
 */
@Component({
  selector: "app-about-dialog",
  standalone: true,
  imports: [CommonModule],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="overlay" (click)="closed.emit()">
      <div
        class="dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="aboutTitle"
        (click)="$event.stopPropagation()"
      >
        <button type="button" class="close" aria-label="Close" (click)="closed.emit()">
          <i class="pi pi-times"></i>
        </button>

        <div class="header">
          <img src="assets/intentio-logo.svg" alt="Intentio" class="logo" />
          <div class="titles">
            <h2 id="aboutTitle">Intentio Knowledge</h2>
            <p>Your notes, on your disk, open to your agents.</p>
            <div class="version">
              <span>v{{ version }}</span>
              <button
                type="button"
                class="check"
                [disabled]="updater.busy"
                (click)="updater.check()"
              >
                <i class="pi" [ngClass]="updater.busy ? 'pi-spin pi-spinner' : 'pi-refresh'"></i>
                {{ updater.busy ? "Checking…" : "Check for updates" }}
              </button>
            </div>
          </div>
        </div>

        @switch (updater.state().kind) {
          @case ("current") {
            <p class="status ok"><i class="pi pi-check-circle"></i> You're up to date.</p>
          }
          @case ("unsupported") {
            <p class="status"><i class="pi pi-info-circle"></i> Updates only work in the desktop app.</p>
          }
          @case ("failed") {
            <p class="status warn">
              <i class="pi pi-exclamation-triangle"></i>
              {{ failureMessage() }}
            </p>
          }
          @case ("downloading") {
            <p class="status"><i class="pi pi-spin pi-spinner"></i> Downloading — the app will restart.</p>
          }
          @case ("available") {
            <div class="status update">
              <div>
                <strong>Version {{ availableVersion() }} is available.</strong>
                <span class="notes" *ngIf="releaseNotes()">{{ releaseNotes() }}</span>
              </div>
              <button type="button" class="install" (click)="updater.install()">Update now</button>
            </div>
          }
          @default {}
        }

        <div class="body">
          <p>
            A vault is an ordinary folder of markdown on your own disk — linked with
            <code>[[wikilinks]]</code>, searchable, and versionable with git. The bundled MCP server
            gives AI agents the same view of it that you have.
          </p>
          <p class="license">Free for personal use. Commercial licence coming soon.</p>
          <a class="link" (click)="openSite()">
            <i class="pi pi-external-link"></i>
            intentiosoftware.com
          </a>
        </div>
      </div>
    </div>
  `,
  styles: [
    `
      .overlay {
        position: fixed;
        inset: 0;
        z-index: 80;
        display: flex;
        align-items: center;
        justify-content: center;
        background: rgba(2, 10, 20, 0.55);
        backdrop-filter: blur(3px);
      }
      /* Deliberately the same gradient card as Mind Map's About dialog. */
      .dialog {
        position: relative;
        width: min(440px, calc(100% - 32px));
        padding: 32px 24px 24px;
        border-radius: 16px;
        background: linear-gradient(135deg, rgba(6, 42, 68, 0.96), rgba(12, 73, 108, 0.92));
        border: 1px solid rgba(255, 255, 255, 0.15);
        box-shadow: 0 18px 50px rgba(0, 0, 0, 0.45);
        color: #fff;
      }
      .close {
        position: absolute;
        top: 0;
        right: 0;
        transform: translate(50%, -50%);
        width: 32px;
        height: 32px;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        border-radius: 50%;
        border: 1px solid rgba(255, 255, 255, 0.25);
        background: rgba(255, 255, 255, 0.12);
        color: inherit;
        cursor: pointer;
        box-shadow: 0 8px 20px rgba(0, 0, 0, 0.4);
      }
      .close:hover {
        background: rgba(255, 255, 255, 0.2);
      }
      .header {
        display: flex;
        gap: 14px;
        align-items: center;
        margin-bottom: 14px;
      }
      .logo {
        width: 56px;
        height: 56px;
        border-radius: 12px;
        border: 1px solid rgba(255, 255, 255, 0.2);
        background: rgba(255, 255, 255, 0.08);
        padding: 8px;
        box-shadow: 0 10px 30px rgba(0, 0, 0, 0.35);
      }
      .titles h2 {
        margin: 0;
        font-size: 1.2rem;
      }
      .titles p {
        margin: 2px 0 0;
        font-size: 0.9rem;
        opacity: 0.85;
      }
      .version {
        margin-top: 6px;
        font-size: 0.8rem;
        opacity: 0.9;
        display: flex;
        align-items: center;
        gap: 10px;
        flex-wrap: wrap;
      }
      .check {
        appearance: none;
        border: 1px solid rgba(255, 255, 255, 0.25);
        background: rgba(255, 255, 255, 0.08);
        color: inherit;
        border-radius: 6px;
        padding: 3px 10px;
        font-size: 0.78rem;
        font-weight: 500;
        display: inline-flex;
        align-items: center;
        gap: 5px;
        cursor: pointer;
        transition: background 0.2s ease, border-color 0.2s ease;
      }
      .check:hover:not(:disabled) {
        background: rgba(255, 255, 255, 0.16);
        border-color: rgba(255, 255, 255, 0.45);
      }
      .check:disabled {
        opacity: 0.6;
        cursor: default;
      }
      .status {
        display: flex;
        align-items: center;
        gap: 0.5rem;
        margin: 0 0 14px;
        padding: 8px 12px;
        border-radius: 8px;
        background: rgba(255, 255, 255, 0.08);
        font-size: 0.85rem;
      }
      .status.ok {
        color: #bff3d2;
      }
      .status.warn {
        color: #ffd7c9;
      }
      .status.update {
        align-items: flex-start;
        justify-content: space-between;
        gap: 0.9rem;
        background: rgba(240, 95, 54, 0.2);
        border: 1px solid rgba(240, 95, 54, 0.45);
      }
      .status.update strong {
        display: block;
      }
      .notes {
        display: block;
        margin-top: 3px;
        font-size: 0.78rem;
        opacity: 0.85;
        max-height: 4.5em;
        overflow: hidden;
      }
      .install {
        flex: none;
        border: none;
        border-radius: 999px;
        padding: 5px 14px;
        background: #f05f36;
        color: #fff;
        font-size: 0.8rem;
        cursor: pointer;
      }
      .install:hover {
        background: #d94d26;
      }
      .body {
        font-size: 0.9rem;
        line-height: 1.5;
      }
      .body p {
        margin: 0 0 12px;
      }
      .body code {
        font-family: var(--font-mono);
        font-size: 0.85em;
        padding: 0.05em 0.3em;
        border-radius: 4px;
        background: rgba(255, 255, 255, 0.12);
      }
      .license {
        font-size: 0.85rem;
        opacity: 0.95;
      }
      .link {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        color: #ffd6c9;
        text-decoration: none;
        font-weight: 600;
        cursor: pointer;
      }
      .link:hover {
        color: #fff;
      }
    `
  ]
})
export class AboutDialogComponent {
  readonly updater = inject(UpdaterService);

  @Input() version = "0.0.0";

  @Output() readonly closed = new EventEmitter<void>();

  availableVersion(): string {
    const state = this.updater.state();
    return state.kind === "available" ? state.version : "";
  }

  releaseNotes(): string {
    const state = this.updater.state();
    return state.kind === "available" ? (state.notes ?? "") : "";
  }

  failureMessage(): string {
    const state = this.updater.state();
    return state.kind === "failed" ? state.message : "";
  }

  async openSite(): Promise<void> {
    try {
      await openUrl("https://intentiosoftware.com");
    } catch {
      // Nothing useful to do if the browser cannot be launched.
    }
  }
}
