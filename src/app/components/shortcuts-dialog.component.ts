import { ChangeDetectionStrategy, Component, EventEmitter, Input, Output } from "@angular/core";
import { CommonModule } from "@angular/common";

/** One shortcut: the keys, and what they do. */
export interface Shortcut {
  keys: string;
  does: string;
}

export interface ShortcutGroup {
  title: string;
  shortcuts: Shortcut[];
}

/**
 * The keyboard, written down.
 *
 * Built to the same shape as the About box so the suite reads as one thing.
 * What it lists is deliberately not only the menu items: those already appear
 * in the menu bar with their keys beside them, and a second copy of a list you
 * can already see is not worth a dialog. The ones worth writing down are the
 * keys that only exist while you are working - Tab for a child node, Enter for
 * a sibling - which nothing else in the app ever tells you about.
 */
@Component({
  selector: "app-shortcuts-dialog",
  standalone: true,
  imports: [CommonModule],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="about-backdrop" (click)="closed.emit()">
      <div
        class="about-dialog shortcuts-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="shortcutsTitle"
        (click)="$event.stopPropagation()"
      >
        <button
          type="button"
          class="about-close"
          aria-label="Close keyboard shortcuts"
          (click)="closed.emit()"
        >
          <svg viewBox="0 0 16 16" width="11" height="11" aria-hidden="true">
            <path
              d="M 2 2 L 14 14 M 14 2 L 2 14"
              stroke="currentColor"
              stroke-width="2"
              stroke-linecap="round"
            />
          </svg>
        </button>

        <h2 id="shortcutsTitle">Keyboard shortcuts</h2>

        <div class="shortcut-groups">
          <div class="shortcut-group" *ngFor="let group of groups">
            <h3>{{ group.title }}</h3>
            <div class="shortcut" *ngFor="let shortcut of group.shortcuts">
              <kbd>{{ shortcut.keys }}</kbd>
              <span>{{ shortcut.does }}</span>
            </div>
          </div>
        </div>
      </div>
    </div>
  `,
  styles: [
    `
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
        width: min(460px, calc(100% - 32px));
        /* Taller than About and allowed to scroll: a shortcut list grows, and
           a dialog that runs off a small screen hides the half that matters. */
        max-height: min(560px, calc(100vh - 80px));
        overflow-y: auto;
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

      h2 {
        margin: 0 0 16px;
        font-size: 1.2rem;
      }

      .shortcut-group {
        margin-bottom: 18px;
      }

      .shortcut-group:last-child {
        margin-bottom: 0;
      }

      h3 {
        margin: 0 0 7px;
        font-size: 0.72rem;
        font-weight: 600;
        text-transform: uppercase;
        letter-spacing: 0.07em;
        opacity: 0.6;
      }

      .shortcut {
        display: flex;
        align-items: baseline;
        gap: 12px;
        padding: 3px 0;
        font-size: 0.88rem;
      }

      kbd {
        flex: none;
        min-width: 92px;
        font-family: inherit;
        font-size: 0.82rem;
        font-weight: 500;
        color: #ffd6c9;
        white-space: nowrap;
      }

      .shortcut span {
        opacity: 0.88;
        line-height: 1.4;
      }
    `
  ]
})
export class ShortcutsDialogComponent {
  @Input() groups: ShortcutGroup[] = [];
  @Output() readonly closed = new EventEmitter<void>();
}
