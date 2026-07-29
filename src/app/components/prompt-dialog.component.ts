import {
  AfterViewInit,
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  EventEmitter,
  Input,
  Output,
  ViewChild
} from "@angular/core";
import { CommonModule } from "@angular/common";
import { FormsModule } from "@angular/forms";

/**
 * A small in-app text prompt.
 *
 * `window.prompt` is not dependable inside the platform webviews Tauri uses, so
 * anything that needs a line of text from the user goes through this instead.
 */
@Component({
  selector: "app-prompt-dialog",
  standalone: true,
  imports: [CommonModule, FormsModule],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="backdrop" (click)="cancelled.emit()">
      <form class="dialog" (click)="$event.stopPropagation()" (ngSubmit)="submit()">
        <h2>{{ title }}</h2>
        <p class="hint" *ngIf="hint">{{ hint }}</p>
        <input
          #input
          type="text"
          [(ngModel)]="value"
          name="value"
          autocomplete="off"
          spellcheck="false"
          (keydown.escape)="cancelled.emit()"
        />
        <div class="actions">
          <button type="button" class="ghost" (click)="cancelled.emit()">Cancel</button>
          <button type="submit" class="primary" [disabled]="!value.trim()">{{ confirmLabel }}</button>
        </div>
      </form>
    </div>
  `,
  styles: [
    `
      .backdrop {
        position: fixed;
        inset: 0;
        z-index: 70;
        display: flex;
        align-items: center;
        justify-content: center;
        background: rgba(2, 10, 20, 0.55);
        backdrop-filter: blur(3px);
      }
      .dialog {
        width: min(26rem, calc(100vw - 3rem));
        padding: 1.4rem;
        background: var(--panel-raised);
        border: 1px solid var(--border);
        border-radius: 14px;
        box-shadow: 0 30px 70px rgba(0, 0, 0, 0.45);
      }
      h2 {
        margin: 0 0 0.35rem;
        font-size: 1rem;
        font-weight: 600;
        color: var(--ink-strong);
      }
      .hint {
        margin: 0 0 0.9rem;
        font-size: 0.8rem;
        color: var(--ink-faint);
        line-height: 1.5;
      }
      input {
        width: 100%;
        padding: 0.55rem 0.75rem;
        border: 1px solid var(--border);
        border-radius: 9px;
        background: var(--surface);
        color: var(--ink-strong);
        outline: none;
      }
      input:focus {
        border-color: var(--accent);
      }
      .actions {
        display: flex;
        justify-content: flex-end;
        gap: 0.5rem;
        margin-top: 1.1rem;
      }
      button {
        padding: 0.45rem 1rem;
        border-radius: 999px;
        border: 1px solid transparent;
        font-size: 0.85rem;
        cursor: pointer;
      }
      button.primary {
        background: var(--accent);
        color: #fff;
      }
      button.primary:disabled {
        opacity: 0.5;
        cursor: default;
      }
      button.ghost {
        background: transparent;
        border-color: var(--border);
        color: var(--ink);
      }
      button.ghost:hover {
        background: var(--hover);
      }
    `
  ]
})
export class PromptDialogComponent implements AfterViewInit {
  @ViewChild("input", { static: true }) inputRef!: ElementRef<HTMLInputElement>;

  @Input({ required: true }) title = "";
  @Input() hint = "";
  @Input() confirmLabel = "Confirm";
  @Input() set initialValue(value: string) {
    this.value = value;
  }

  @Output() readonly confirmed = new EventEmitter<string>();
  @Output() readonly cancelled = new EventEmitter<void>();

  value = "";

  ngAfterViewInit(): void {
    const input = this.inputRef.nativeElement;
    input.focus();
    input.select();
  }

  submit(): void {
    const trimmed = this.value.trim();
    if (trimmed) {
      this.confirmed.emit(trimmed);
    }
  }
}
