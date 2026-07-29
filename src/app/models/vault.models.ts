/**
 * Shapes returned by the Rust side. These mirror the `int-vault` types exactly;
 * changing one without the other is the main way this app can break, so keep the
 * field names in step with `src-tauri/src/lib.rs`.
 */

export interface NoteMeta {
  path: string;
  title: string;
  aliases?: string[];
  tags?: string[];
  size: number;
  modified?: number;
}

export interface Heading {
  level: number;
  text: string;
  line: number;
}

export interface LinkRef {
  kind: "wiki" | "markdown";
  target: string;
  heading?: string;
  alias?: string;
  embed: boolean;
  line: number;
}

export interface ResolvedLink extends LinkRef {
  resolved_path?: string;
}

export interface Backlink {
  source: string;
  source_title: string;
  line: number;
  context: string;
  embed: boolean;
}

export interface UnresolvedLink {
  source: string;
  target: string;
  line: number;
}

export interface NoteDetail extends NoteMeta {
  /** Full file text including frontmatter — what the editor edits. */
  content: string;
  /** Body only, frontmatter stripped. */
  body: string;
  frontmatter: Record<string, unknown>;
  headings: Heading[];
  links: ResolvedLink[];
  backlinks: Backlink[];
}

export interface TagCount {
  tag: string;
  notes: number;
}

export interface VaultSummary {
  name: string;
  path: string;
  notes: number;
  folders: string[];
  tags: TagCount[];
  unresolved: number;
}

export interface SearchMatch {
  line: number;
  text: string;
}

export interface SearchHit {
  path: string;
  title: string;
  score: number;
  matches: SearchMatch[];
}

export interface GraphNode {
  /** Vault path for real notes; the raw link target for not-yet-written ones. */
  id: string;
  label: string;
  exists: boolean;
  degree: number;
  tags?: string[];
}

export interface GraphEdge {
  source: string;
  target: string;
}

export interface GraphData {
  nodes: GraphNode[];
  edges: GraphEdge[];
}

/** A vault the user has opened before, for the launcher. */
export interface RecentVault {
  path: string;
  name: string;
  openedAt: number;
}

/** One entry in the sidebar's folder tree. */
export interface TreeEntry {
  kind: "folder" | "note";
  /** Vault-relative path; for folders, without a trailing slash. */
  path: string;
  name: string;
  depth: number;
  note?: NoteMeta;
}
