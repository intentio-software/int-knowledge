import {
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  EventEmitter,
  Input,
  OnInit,
  Output,
  ViewChild,
  inject,
  signal
} from "@angular/core";
import { CommonModule } from "@angular/common";

/** One line in the menu. A separator has no action. */
export interface MenuItem {
  action?: string;
  label?: string;
  icon?: string;
  danger?: boolean;
  separator?: boolean;
}

/**
 * A right-click menu, positioned at the pointer.
 *
 * Placement is corrected after the first paint: the menu's size is not known
 * until it is in the DOM, and a menu opened near the bottom or right edge would
 * otherwise hang off the window.
 */
@Component({
  selector: "app-context-menu",
  standalone: true,
  imports: [CommonModule],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="backdrop" (click)="dismissed.emit()" (contextmenu)="$event.preventDefault(); dismissed.emit()"></div>
    <div class="menu" #menu role="menu" [style.left.px]="left()" [style.top.px]="top()">
      <ng-container *ngFor="let item of items">
        <div class="separator" *ngIf="item.separator"></div>
        <button
          *ngIf="!item.separator"
          type="button"
          role="menuitem"
          [class.danger]="item.danger"
          (click)="chosen.emit(item.action!)"
        >
          <i class="pi" [ngClass]="item.icon ?? 'pi-circle-fill'"></i>
          <span>{{ item.label }}</span>
        </button>
      </ng-container>
    </div>
  `,
  styles: [
    `
      .backdrop {
        position: fixed;
        inset: 0;
        z-index: 60;
      }
      .menu {
        position: fixed;
        z-index: 61;
        min-width: 12rem;
        padding: 0.25rem;
        border: 1px solid var(--border);
        border-radius: 9px;
        background: var(--panel);
        box-shadow: 0 12px 32px rgba(0, 0, 0, 0.28);
      }
      button {
        display: flex;
        align-items: center;
        gap: 0.6rem;
        width: 100%;
        padding: 0.4rem 0.6rem;
        border: none;
        border-radius: 6px;
        background: transparent;
        color: var(--ink);
        font-size: 0.83rem;
        text-align: left;
        cursor: pointer;
      }
      button:hover {
        background: var(--hover);
        color: var(--ink-strong);
      }
      button.danger:hover {
        color: var(--danger);
      }
      button i {
        font-size: 0.72rem;
        opacity: 0.7;
        width: 0.9rem;
      }
      .separator {
        height: 1px;
        margin: 0.25rem 0.35rem;
        background: var(--border);
      }
    `
  ]
})
export class ContextMenuComponent implements OnInit {
  private readonly host = inject(ElementRef<HTMLElement>);

  @Input({ required: true }) items: MenuItem[] = [];
  @Input() x = 0;
  @Input() y = 0;

  @Output() readonly chosen = new EventEmitter<string>();
  @Output() readonly dismissed = new EventEmitter<void>();

  @ViewChild("menu", { static: true }) menuRef!: ElementRef<HTMLDivElement>;

  readonly left = signal(0);
  readonly top = signal(0);

  ngOnInit(): void {
    this.left.set(this.x);
    this.top.set(this.y);
    // Measure once laid out, then pull back inside the window if needed.
    requestAnimationFrame(() => {
      const menu = this.menuRef.nativeElement.getBoundingClientRect();
      const margin = 8;
      if (this.x + menu.width > window.innerWidth - margin) {
        this.left.set(Math.max(margin, window.innerWidth - menu.width - margin));
      }
      if (this.y + menu.height > window.innerHeight - margin) {
        this.top.set(Math.max(margin, window.innerHeight - menu.height - margin));
      }
    });
  }

  /** Escape closes the menu, matching every other dismissible surface here. */
  onEscape(): void {
    this.dismissed.emit();
  }

  focusFirst(): void {
    this.host.nativeElement.querySelector("button")?.focus();
  }
}
