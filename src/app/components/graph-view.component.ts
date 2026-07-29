import {
  AfterViewInit,
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  EventEmitter,
  Input,
  NgZone,
  OnChanges,
  OnDestroy,
  Output,
  SimpleChanges,
  ViewChild,
  inject
} from "@angular/core";
import { CommonModule } from "@angular/common";

import { GraphData } from "../models/vault.models";
import { ForceGraph, SimNode, radiusFor } from "../editor/force-graph";

/**
 * The vault as a link graph, drawn on canvas.
 *
 * Canvas rather than SVG because a few hundred nodes redrawn at 60fps means a
 * few hundred DOM mutations per frame in SVG, and none here. The simulation and
 * render loop both run outside Angular; only discrete events re-enter it.
 */
@Component({
  selector: "app-graph-view",
  standalone: true,
  imports: [CommonModule],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="graph" #host>
      <canvas
        #canvas
        (pointerdown)="onPointerDown($event)"
        (pointermove)="onPointerMove($event)"
        (pointerup)="onPointerUp($event)"
        (pointerleave)="onPointerUp($event)"
        (wheel)="onWheel($event)"
        (dblclick)="onDoubleClick($event)"
      ></canvas>

      <div class="overlay">
        <div class="legend">
          <span><i class="dot"></i>{{ noteCount }} notes</span>
          <span><i class="dot ghost"></i>{{ ghostCount }} not yet written</span>
          <span>{{ data?.edges?.length ?? 0 }} links</span>
        </div>
        <div class="controls">
          <button type="button" (click)="fit()" title="Fit to view">
            <i class="pi pi-expand"></i>
          </button>
          <button type="button" (click)="reheat()" title="Re-run layout">
            <i class="pi pi-refresh"></i>
          </button>
        </div>
      </div>

      <div class="hint">Drag to pan · scroll to zoom · drag a node to pin it · double-click to open</div>
    </div>
  `,
  styles: [
    `
      :host {
        display: block;
        height: 100%;
        min-height: 0;
      }
      .graph {
        position: relative;
        height: 100%;
        background: var(--surface);
        overflow: hidden;
      }
      canvas {
        display: block;
        width: 100%;
        height: 100%;
        cursor: grab;
      }
      canvas:active {
        cursor: grabbing;
      }
      .overlay {
        position: absolute;
        top: 0.85rem;
        left: 0.85rem;
        right: 0.85rem;
        display: flex;
        align-items: flex-start;
        justify-content: space-between;
        gap: 1rem;
        pointer-events: none;
      }
      .legend {
        display: flex;
        flex-wrap: wrap;
        gap: 0.9rem;
        padding: 0.45rem 0.8rem;
        border: 1px solid var(--border);
        border-radius: 999px;
        background: color-mix(in srgb, var(--panel) 85%, transparent);
        backdrop-filter: blur(6px);
        font-size: 0.73rem;
        color: var(--ink-muted);
      }
      .legend span {
        display: inline-flex;
        align-items: center;
        gap: 0.35rem;
      }
      .dot {
        width: 7px;
        height: 7px;
        border-radius: 50%;
        background: var(--accent);
      }
      .dot.ghost {
        background: transparent;
        border: 1px dashed var(--link-unresolved);
      }
      .controls {
        display: flex;
        gap: 0.3rem;
        pointer-events: auto;
      }
      .controls button {
        width: 1.9rem;
        height: 1.9rem;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        border: 1px solid var(--border);
        border-radius: 8px;
        background: color-mix(in srgb, var(--panel) 85%, transparent);
        backdrop-filter: blur(6px);
        color: var(--ink-muted);
        font-size: 0.75rem;
        cursor: pointer;
      }
      .controls button:hover {
        color: var(--ink-strong);
        border-color: var(--accent);
      }
      .hint {
        position: absolute;
        bottom: 0.8rem;
        left: 50%;
        transform: translateX(-50%);
        font-size: 0.7rem;
        color: var(--ink-faint);
        pointer-events: none;
        white-space: nowrap;
      }
    `
  ]
})
export class GraphViewComponent implements AfterViewInit, OnChanges, OnDestroy {
  private readonly zone = inject(NgZone);

  @ViewChild("host", { static: true }) hostRef!: ElementRef<HTMLDivElement>;
  @ViewChild("canvas", { static: true }) canvasRef!: ElementRef<HTMLCanvasElement>;

  @Input() data: GraphData | null = null;
  /** Path of the open note, drawn highlighted. */
  @Input() activePath: string | null = null;

  @Output() readonly noteOpened = new EventEmitter<string>();
  /** Emitted when a ghost node is opened, so the caller can create the note. */
  @Output() readonly noteCreateRequested = new EventEmitter<string>();

  noteCount = 0;
  ghostCount = 0;

  private readonly graph = new ForceGraph();
  private context: CanvasRenderingContext2D | null = null;
  private frame = 0;
  private resizeObserver: ResizeObserver | null = null;

  // Viewport
  private scale = 1;
  private offsetX = 0;
  private offsetY = 0;

  // Interaction
  private dragging: SimNode | null = null;
  private panning = false;
  private pointerStart = { x: 0, y: 0 };
  private movedDistance = 0;
  private hovered: SimNode | null = null;

  ngAfterViewInit(): void {
    const canvas = this.canvasRef.nativeElement;
    this.context = canvas.getContext("2d");
    this.resizeObserver = new ResizeObserver(() => this.resize());
    this.resizeObserver.observe(this.hostRef.nativeElement);
    this.resize();
    this.start();
  }

  ngOnChanges(changes: SimpleChanges): void {
    if (changes["data"] && this.data) {
      this.graph.load(this.data.nodes, this.data.edges);
      this.noteCount = this.data.nodes.filter((node) => node.exists).length;
      this.ghostCount = this.data.nodes.length - this.noteCount;
      // Let the layout settle before framing it, otherwise fit() captures the
      // seed spiral rather than the resolved graph.
      window.setTimeout(() => this.fit(), 600);
    }
    if (changes["activePath"]) {
      this.draw();
    }
  }

  ngOnDestroy(): void {
    cancelAnimationFrame(this.frame);
    this.resizeObserver?.disconnect();
  }

  // -------------------------------------------------------------------------
  // controls
  // -------------------------------------------------------------------------

  /** Frame the whole graph with a margin. */
  fit(): void {
    const canvas = this.canvasRef.nativeElement;
    const width = canvas.clientWidth;
    const height = canvas.clientHeight;
    if (!width || !height || !this.graph.nodes.length) {
      return;
    }
    const { minX, minY, maxX, maxY } = this.graph.bounds();
    const spanX = Math.max(maxX - minX, 1);
    const spanY = Math.max(maxY - minY, 1);
    this.scale = Math.min(width / (spanX + 120), height / (spanY + 120), 2.2);
    this.offsetX = width / 2 - ((minX + maxX) / 2) * this.scale;
    this.offsetY = height / 2 - ((minY + maxY) / 2) * this.scale;
    this.draw();
  }

  reheat(): void {
    this.graph.nodes.forEach((node) => (node.fixed = false));
    this.graph.reheat(1);
    this.start();
  }

  // -------------------------------------------------------------------------
  // interaction
  // -------------------------------------------------------------------------

  onPointerDown(event: PointerEvent): void {
    this.canvasRef.nativeElement.setPointerCapture(event.pointerId);
    this.pointerStart = { x: event.clientX, y: event.clientY };
    this.movedDistance = 0;

    const node = this.nodeAt(event);
    if (node) {
      this.dragging = node;
      node.fixed = true;
    } else {
      this.panning = true;
    }
  }

  onPointerMove(event: PointerEvent): void {
    const dx = event.clientX - this.pointerStart.x;
    const dy = event.clientY - this.pointerStart.y;

    if (this.dragging) {
      this.movedDistance += Math.abs(dx) + Math.abs(dy);
      const world = this.toWorld(event);
      this.dragging.x = world.x;
      this.dragging.y = world.y;
      this.pointerStart = { x: event.clientX, y: event.clientY };
      this.graph.reheat(0.35);
      this.start();
      return;
    }

    if (this.panning) {
      this.movedDistance += Math.abs(dx) + Math.abs(dy);
      this.offsetX += dx;
      this.offsetY += dy;
      this.pointerStart = { x: event.clientX, y: event.clientY };
      this.draw();
      return;
    }

    const hovered = this.nodeAt(event);
    if (hovered !== this.hovered) {
      this.hovered = hovered;
      this.canvasRef.nativeElement.style.cursor = hovered ? "pointer" : "grab";
      this.draw();
    }
  }

  onPointerUp(event: PointerEvent): void {
    if (this.canvasRef.nativeElement.hasPointerCapture(event.pointerId)) {
      this.canvasRef.nativeElement.releasePointerCapture(event.pointerId);
    }
    // A drag that barely moved was a click; unpin so the node rejoins the layout.
    if (this.dragging && this.movedDistance < 4) {
      this.dragging.fixed = false;
    }
    this.dragging = null;
    this.panning = false;
  }

  onDoubleClick(event: MouseEvent): void {
    const node = this.nodeAt(event);
    if (!node) {
      return;
    }
    this.zone.run(() => {
      if (node.exists) {
        this.noteOpened.emit(node.id);
      } else {
        this.noteCreateRequested.emit(node.label);
      }
    });
  }

  onWheel(event: WheelEvent): void {
    event.preventDefault();
    const factor = Math.exp(-event.deltaY * 0.0015);
    const next = Math.min(Math.max(this.scale * factor, 0.08), 6);
    // Zoom about the cursor rather than the origin, so the point under the
    // pointer stays put.
    const rect = this.canvasRef.nativeElement.getBoundingClientRect();
    const px = event.clientX - rect.left;
    const py = event.clientY - rect.top;
    this.offsetX = px - ((px - this.offsetX) / this.scale) * next;
    this.offsetY = py - ((py - this.offsetY) / this.scale) * next;
    this.scale = next;
    this.draw();
  }

  // -------------------------------------------------------------------------
  // loop
  // -------------------------------------------------------------------------

  private start(): void {
    cancelAnimationFrame(this.frame);
    // The simulation runs at animation rate and must not trigger change
    // detection on every frame.
    this.zone.runOutsideAngular(() => {
      const step = (): void => {
        const running = this.graph.tick();
        this.draw();
        if (running) {
          this.frame = requestAnimationFrame(step);
        }
      };
      this.frame = requestAnimationFrame(step);
    });
  }

  private resize(): void {
    const canvas = this.canvasRef.nativeElement;
    const ratio = window.devicePixelRatio || 1;
    const width = this.hostRef.nativeElement.clientWidth;
    const height = this.hostRef.nativeElement.clientHeight;
    if (!width || !height) {
      return;
    }
    canvas.width = Math.round(width * ratio);
    canvas.height = Math.round(height * ratio);
    canvas.style.width = `${width}px`;
    canvas.style.height = `${height}px`;
    this.context?.setTransform(ratio, 0, 0, ratio, 0, 0);
    this.draw();
  }

  private draw(): void {
    const context = this.context;
    const canvas = this.canvasRef.nativeElement;
    if (!context) {
      return;
    }
    const width = canvas.clientWidth;
    const height = canvas.clientHeight;
    const styles = getComputedStyle(document.body);
    const accent = styles.getPropertyValue("--accent").trim() || "#f05f36";
    const ink = styles.getPropertyValue("--ink").trim() || "#dbe6f0";
    const inkFaint = styles.getPropertyValue("--ink-faint").trim() || "#6d859b";
    const border = styles.getPropertyValue("--border").trim() || "rgba(180,210,235,0.13)";
    const ghost = styles.getPropertyValue("--link-unresolved").trim() || "#b98cd8";

    context.clearRect(0, 0, width, height);

    const focus = this.hovered ?? this.graph.nodes.find((node) => node.id === this.activePath) ?? null;
    const highlighted = focus ? this.graph.neighbours(focus.id) : null;

    // Edges first so nodes sit on top of them.
    context.lineWidth = 1;
    for (const edge of this.graph.edges) {
      const related =
        !focus || edge.source.id === focus.id || edge.target.id === focus.id;
      context.strokeStyle = related && focus ? accent : border;
      context.globalAlpha = focus ? (related ? 0.85 : 0.16) : 0.6;
      context.beginPath();
      context.moveTo(edge.source.x * this.scale + this.offsetX, edge.source.y * this.scale + this.offsetY);
      context.lineTo(edge.target.x * this.scale + this.offsetX, edge.target.y * this.scale + this.offsetY);
      context.stroke();
    }

    context.globalAlpha = 1;
    const showLabels = this.scale > 0.55;

    for (const node of this.graph.nodes) {
      const x = node.x * this.scale + this.offsetX;
      const y = node.y * this.scale + this.offsetY;
      const radius = radiusFor(node) * Math.min(Math.max(this.scale, 0.5), 1.6);

      // Skip anything comfortably outside the viewport.
      if (x < -80 || y < -80 || x > width + 80 || y > height + 80) {
        continue;
      }

      const isActive = node.id === this.activePath;
      const isFocus = focus?.id === node.id;
      const isNeighbour = highlighted?.has(node.id) ?? false;
      const dimmed = Boolean(focus) && !isFocus && !isNeighbour;

      context.globalAlpha = dimmed ? 0.25 : 1;
      context.beginPath();
      context.arc(x, y, radius, 0, Math.PI * 2);

      if (!node.exists) {
        context.strokeStyle = ghost;
        context.setLineDash([3, 3]);
        context.lineWidth = 1.4;
        context.stroke();
        context.setLineDash([]);
      } else {
        context.fillStyle = isActive || isFocus ? accent : inkFaint;
        context.fill();
        if (isActive) {
          context.strokeStyle = accent;
          context.lineWidth = 2;
          context.beginPath();
          context.arc(x, y, radius + 4, 0, Math.PI * 2);
          context.stroke();
        }
      }

      if (showLabels && !dimmed) {
        context.globalAlpha = dimmed ? 0.2 : isFocus || isActive ? 1 : 0.75;
        context.fillStyle = isActive || isFocus ? ink : inkFaint;
        context.font = `${isFocus || isActive ? 600 : 400} 11px ${styles.getPropertyValue("--font-body") || "sans-serif"}`;
        context.textAlign = "center";
        context.textBaseline = "top";
        context.fillText(truncate(node.label, 24), x, y + radius + 4);
      }
    }
    context.globalAlpha = 1;
  }

  // -------------------------------------------------------------------------
  // hit testing
  // -------------------------------------------------------------------------

  private toWorld(event: MouseEvent): { x: number; y: number } {
    const rect = this.canvasRef.nativeElement.getBoundingClientRect();
    return {
      x: (event.clientX - rect.left - this.offsetX) / this.scale,
      y: (event.clientY - rect.top - this.offsetY) / this.scale
    };
  }

  private nodeAt(event: MouseEvent): SimNode | null {
    const world = this.toWorld(event);
    let best: SimNode | null = null;
    let bestDistance = Infinity;
    for (const node of this.graph.nodes) {
      const dx = node.x - world.x;
      const dy = node.y - world.y;
      const distance = Math.sqrt(dx * dx + dy * dy);
      // Grow the hit area at low zoom so small nodes stay clickable.
      const reach = radiusFor(node) + 6 / this.scale;
      if (distance <= reach && distance < bestDistance) {
        best = node;
        bestDistance = distance;
      }
    }
    return best;
  }
}

function truncate(text: string, max: number): string {
  return text.length <= max ? text : `${text.slice(0, max - 1)}…`;
}
