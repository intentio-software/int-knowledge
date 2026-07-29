/**
 * A small force-directed layout.
 *
 * Hand-rolled rather than pulling in d3-force: a vault graph is a few hundred
 * nodes, where the naive O(n²) repulsion costs well under a millisecond a tick,
 * and this keeps the bundle and the API surface small.
 */

export interface SimNode {
  id: string;
  label: string;
  exists: boolean;
  degree: number;
  x: number;
  y: number;
  vx: number;
  vy: number;
  /** Set while the user drags a node, which pins it. */
  fixed: boolean;
}

export interface SimEdge {
  source: SimNode;
  target: SimNode;
}

export interface ForceSettings {
  /** Node-node separation. Higher spreads the graph out. */
  repulsion: number;
  /** Edge stiffness, 0–1. */
  springStrength: number;
  /** Natural edge length in world units. */
  springLength: number;
  /** Pull toward the origin, which stops islands drifting away forever. */
  centering: number;
  /** Velocity retained per tick. */
  damping: number;
}

export const DEFAULT_FORCES: ForceSettings = {
  repulsion: 5200,
  springStrength: 0.035,
  springLength: 90,
  centering: 0.012,
  damping: 0.82
};

export class ForceGraph {
  nodes: SimNode[] = [];
  edges: SimEdge[] = [];
  /** Simulation temperature: 1 is hot, 0 is settled. */
  alpha = 1;

  private readonly adjacency = new Map<string, Set<string>>();

  constructor(public settings: ForceSettings = { ...DEFAULT_FORCES }) {}

  /**
   * Load a graph, keeping positions of nodes that already existed.
   *
   * Preserving positions matters because the graph is rebuilt whenever the vault
   * changes on disk — without it, every save would fling the layout apart.
   */
  load(
    nodes: Array<{ id: string; label: string; exists: boolean; degree: number }>,
    edges: Array<{ source: string; target: string }>
  ): void {
    const previous = new Map(this.nodes.map((node) => [node.id, node]));

    // Seed on a phyllotaxis spiral: an even spread with no two nodes coincident,
    // which a random scatter cannot guarantee and a grid makes too regular.
    this.nodes = nodes.map((node, index) => {
      const existing = previous.get(node.id);
      if (existing) {
        return { ...existing, label: node.label, exists: node.exists, degree: node.degree };
      }
      const radius = 12 * Math.sqrt(index + 1);
      const angle = (index + 1) * 2.399963; // golden angle
      return {
        ...node,
        x: radius * Math.cos(angle),
        y: radius * Math.sin(angle),
        vx: 0,
        vy: 0,
        fixed: false
      };
    });

    const byId = new Map(this.nodes.map((node) => [node.id, node]));
    this.edges = edges
      .map((edge) => ({ source: byId.get(edge.source), target: byId.get(edge.target) }))
      .filter((edge): edge is SimEdge => Boolean(edge.source && edge.target));

    this.adjacency.clear();
    for (const edge of this.edges) {
      this.neighboursOf(edge.source.id).add(edge.target.id);
      this.neighboursOf(edge.target.id).add(edge.source.id);
    }

    this.alpha = 1;
  }

  neighbours(id: string): Set<string> {
    return this.adjacency.get(id) ?? new Set();
  }

  /** Advance the simulation one step. Returns false once it has settled. */
  tick(): boolean {
    if (this.alpha < 0.005) {
      return false;
    }
    const { repulsion, springStrength, springLength, centering, damping } = this.settings;

    // Repulsion: every pair pushes apart, computed once per pair.
    for (let i = 0; i < this.nodes.length; i += 1) {
      const a = this.nodes[i];
      for (let j = i + 1; j < this.nodes.length; j += 1) {
        const b = this.nodes[j];
        let dx = b.x - a.x;
        let dy = b.y - a.y;
        let distanceSquared = dx * dx + dy * dy;
        if (distanceSquared < 0.01) {
          // Perfectly coincident nodes have no direction to separate along;
          // nudge them deterministically by index so the layout stays stable.
          dx = (i % 7) - 3 || 1;
          dy = (j % 5) - 2 || 1;
          distanceSquared = dx * dx + dy * dy;
        }
        const distance = Math.sqrt(distanceSquared);
        const force = repulsion / distanceSquared;
        const fx = (dx / distance) * force;
        const fy = (dy / distance) * force;
        a.vx -= fx;
        a.vy -= fy;
        b.vx += fx;
        b.vy += fy;
      }
    }

    // Springs pull linked notes together.
    for (const edge of this.edges) {
      const dx = edge.target.x - edge.source.x;
      const dy = edge.target.y - edge.source.y;
      const distance = Math.sqrt(dx * dx + dy * dy) || 0.01;
      const displacement = (distance - springLength) * springStrength;
      const fx = (dx / distance) * displacement;
      const fy = (dy / distance) * displacement;
      edge.source.vx += fx;
      edge.source.vy += fy;
      edge.target.vx -= fx;
      edge.target.vy -= fy;
    }

    for (const node of this.nodes) {
      if (node.fixed) {
        node.vx = 0;
        node.vy = 0;
        continue;
      }
      node.vx -= node.x * centering;
      node.vy -= node.y * centering;
      node.vx *= damping;
      node.vy *= damping;
      node.x += node.vx * this.alpha;
      node.y += node.vy * this.alpha;
    }

    this.alpha *= 0.985;
    return true;
  }

  /** Reheat the simulation, e.g. after a drag or a settings change. */
  reheat(to = 0.6): void {
    this.alpha = Math.max(this.alpha, to);
  }

  /** Bounding box of the laid-out graph, for fit-to-view. */
  bounds(): { minX: number; minY: number; maxX: number; maxY: number } {
    if (!this.nodes.length) {
      return { minX: -1, minY: -1, maxX: 1, maxY: 1 };
    }
    let minX = Infinity;
    let minY = Infinity;
    let maxX = -Infinity;
    let maxY = -Infinity;
    for (const node of this.nodes) {
      minX = Math.min(minX, node.x);
      minY = Math.min(minY, node.y);
      maxX = Math.max(maxX, node.x);
      maxY = Math.max(maxY, node.y);
    }
    return { minX, minY, maxX, maxY };
  }

  private neighboursOf(id: string): Set<string> {
    let set = this.adjacency.get(id);
    if (!set) {
      set = new Set();
      this.adjacency.set(id, set);
    }
    return set;
  }
}

/** Node radius from its link count, flattened so hubs stay on screen. */
export function radiusFor(node: SimNode): number {
  return 4 + Math.sqrt(node.degree) * 2.4;
}
