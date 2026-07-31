import { ChangeDetectionStrategy, Component, EventEmitter, Input, Output, inject, signal } from "@angular/core";
import { CommonModule } from "@angular/common";

import { UpdaterService } from "../services/updater.service";

/**
 * The About dialog.
 *
 * Deliberately identical to Intentio Mind Map's — same markup, same class names,
 * same styling, same update flow — so the two apps read as one suite. Update
 * results are reported through the shared toast rather than inline here, which
 * is what Mind Map does and what keeps the dialog dismissible mid-download.
 */
@Component({
  selector: "app-about-dialog",
  standalone: true,
  imports: [CommonModule],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="about-backdrop" (click)="closed.emit()">
      <div
        id="about-dialog"
        class="about-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="aboutDialogTitle"
        aria-describedby="aboutDialogBody"
        (click)="$event.stopPropagation()"
      >
        <button
          type="button"
          class="about-close"
          aria-label="Close about dialog"
          (click)="closed.emit()"
        >
          <i class="pi pi-times" aria-hidden="true"></i>
        </button>
        <div class="about-header">
          <img src="assets/intentio-logo.png" alt="Intentio logo" class="about-logo" />
          <div class="about-title-group">
            <h2 id="aboutDialogTitle">Intentio Knowledge</h2>
            <p>Your notes, on your disk, open to your agents.</p>
            <div class="about-version">
              <span>{{ version }}</span>
              <button
                type="button"
                class="check-updates-btn"
                [disabled]="isCheckingUpdates()"
                (click)="checkForUpdates()"
              >
                <i class="pi" [ngClass]="isCheckingUpdates() ? 'pi-spin pi-spinner' : 'pi-refresh'"></i>
                {{ isCheckingUpdates() ? 'Checking…' : 'Check for updates' }}
              </button>
            </div>
          </div>
        </div>
        <div id="aboutDialogBody" class="about-body">
          <p>
            Intentio Knowledge keeps your notes as plain markdown in a folder you choose, linked
            with wikilinks and searchable in an instant. The companion MCP server gives AI agents
            the same view of your vault that you have.
          </p>
          <p class="about-license">
            {{ licensingNotice }}
          </p>
          <a
            class="about-link"
            href="https://intentiosoftware.com"
            target="_blank"
            rel="noopener noreferrer"
          >
            intentiosoftware.com
            <i class="pi pi-arrow-up-right" aria-hidden="true"></i>
          </a>
        </div>
      </div>
    </div>
  `,
  styles: [
    `
      /* Above the app chrome, and below the toast layer so an update notice
         appears in front of the blur rather than behind it. */
      .about-backdrop {
        position: fixed;
        inset: 0;
        background: rgba(0, 0, 0, 0.35);
        backdrop-filter: blur(6px);
        display: flex;
        align-items: center;
        justify-content: center;
        z-index: 3000;
        animation: aboutFadeIn 0.2s ease;
      }

      @keyframes aboutFadeIn {
        from {
          opacity: 0;
        }
      }

      .about-dialog {
        position: relative;
        width: min(420px, calc(100% - 32px));
        padding: 32px 24px 24px;
        border-radius: 16px;
        background: linear-gradient(135deg, rgba(6, 42, 68, 0.95), rgba(12, 73, 108, 0.9));
        border: 1px solid rgba(255, 255, 255, 0.15);
        box-shadow: 0 18px 50px rgba(0, 0, 0, 0.4);
        color: #fff;
        font-family: "Inter", system-ui, sans-serif;
      }

      .about-close {
        position: absolute;
        top: 0;
        right: 0;
        transform: translate(50%, -50%);
        width: 32px;
        height: 32px;
        border-radius: 50%;
        border: 1px solid rgba(255, 255, 255, 0.25);
        background: rgba(255, 255, 255, 0.12);
        color: inherit;
        cursor: pointer;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        padding: 0;
        line-height: 1;
        box-shadow: 0 8px 20px rgba(0, 0, 0, 0.4);
      }

      .about-header {
        display: flex;
        gap: 14px;
        align-items: center;
        margin-bottom: 12px;
      }

      .about-logo {
        width: 56px;
        height: 56px;
        border-radius: 12px;
        border: 1px solid rgba(255, 255, 255, 0.2);
        box-shadow: 0 10px 30px rgba(0, 0, 0, 0.35);
      }

      .about-title-group h2 {
        margin: 0;
        font-size: 1.2rem;
      }

      .about-title-group p {
        margin: 2px 0 0;
        font-size: 0.9rem;
        opacity: 0.85;
      }

      .about-version {
        font-size: 0.8rem;
        opacity: 0.8;
        display: flex;
        align-items: center;
        gap: 10px;
        flex-wrap: wrap;
      }

      .check-updates-btn {
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

      .check-updates-btn:hover:not(:disabled) {
        background: rgba(255, 255, 255, 0.16);
        border-color: rgba(255, 255, 255, 0.45);
      }

      .check-updates-btn:disabled {
        opacity: 0.6;
        cursor: default;
      }

      .about-license {
        font-size: 0.85rem;
        color: rgba(255, 255, 255, 0.95);
        margin-bottom: 0.5rem;
      }

      .about-body {
        font-size: 0.92rem;
        line-height: 1.45;
      }

      .about-body p {
        margin-bottom: 12px;
      }

      .about-link {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        color: #ffd6c9;
        text-decoration: none;
        font-weight: 600;
      }

      .about-link:hover {
        color: #fff;
      }
    `
  ]
})
export class AboutDialogComponent {
  private readonly updater = inject(UpdaterService);

  @Input() version = "v0.0.0";

  @Output() readonly closed = new EventEmitter<void>();

  readonly licensingNotice = "Free for personal use – commercial license coming soon.";
  readonly isCheckingUpdates = signal(false);

  async checkForUpdates(): Promise<void> {
    this.isCheckingUpdates.set(true);
    await this.updater.manualCheck();
    this.isCheckingUpdates.set(false);
  }
}
