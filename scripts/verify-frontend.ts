import { createRenderer, renderMarkdown, sectionOf } from "../src/app/editor/markdown-renderer";
import { ForceGraph } from "../src/app/editor/force-graph";

const md = createRenderer();
const existing = new Set(["Welcome.md", "Ideas.md", "logo.png"]);
const context = {
  resolve: (target: string) => {
    const hit = [...existing].find((p) => p.replace(/\.md$/, "").toLowerCase() === target.toLowerCase());
    return hit ?? null;
  },
  assetUrl: (path: string) => `asset://localhost/vault/${path}`
};

const render = (src: string) => renderMarkdown(md, src, context).trim();

let failures = 0;
function check(name: string, actual: string, expected: string | RegExp) {
  const ok = expected instanceof RegExp ? expected.test(actual) : actual.includes(expected);
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}`);
  if (!ok) {
    failures += 1;
    console.log(`      got: ${actual.slice(0, 200)}`);
  }
}

check("resolved wikilink", render("See [[Welcome]]."), 'class="wikilink" data-wikilink="Welcome"');
check("unresolved wikilink", render("See [[Ghost]]."), 'class="wikilink unresolved"');
check("wikilink alias", render("[[Welcome|home]]"), ">home</a>");
check("wikilink heading stripped", render("[[Welcome#Setup]]"), 'data-wikilink="Welcome"');
check("image embed", render("![[logo.png]]"), 'img class="embed" src="asset://localhost/vault/logo.png"');
check("wikilink in code span is inert", render("`[[Welcome]]`"), /<code>\[\[Welcome\]\]<\/code>/);
check("wikilink in fenced code is inert", render("```\n[[Welcome]]\n```"), /<pre><code>\[\[Welcome\]\]/);
check("tag", render("tagged #project/knowledge here"), 'class="tag" data-tag="project/knowledge"');
check("heading is not a tag", render("# Heading"), /<h1>Heading<\/h1>/);
check("number is not a tag", render("issue #2026"), /#2026/);
check("no tag mid-word", render("C#"), /^<p>C#<\/p>$/);
check("task unchecked", render("- [ ] todo"), '<input class="task" type="checkbox" disabled />');
check("task checked", render("- [x] done"), 'type="checkbox" disabled checked');
check("external link routed", render("[site](https://example.com)"), 'data-external="https://example.com"');
check("relative link becomes wikilink", render("[a](Ideas.md)"), 'data-wikilink="Ideas.md"');
check("raw html is escaped", render("<script>alert(1)</script>"), /&lt;script&gt;/);
check("img onerror is escaped", render('<img src=x onerror="alert(1)">'), /&lt;img/);
check("html in wikilink alias escaped", render("[[Welcome|<b>x</b>]]"), /&lt;b&gt;/);
check("table renders", render("| a | b |\n|---|---|\n| 1 | 2 |"), "<table>");
check("blockquote renders", render("> quoted"), "<blockquote>");

// --- transclusion ----------------------------------------------------------
// Bodies are prefetched by the view; here they are supplied directly.
const bodies = new Map<string, string>([
  ["Welcome.md", "# Welcome\n\nIntro text.\n\n## Setup\n\nRun the server.\n\n## Other\n\nNot this bit.\n"],
  ["Ideas.md", "Ideas body, which embeds ![[Welcome]].\n"],
  ["Loop.md", "Loop body embedding itself: ![[Loop]]\n"],
  ["A.md", "A embeds ![[B]]\n"],
  ["B.md", "B embeds ![[A]]\n"]
]);
const titles = new Map([...bodies.keys()].map((k) => [k, k.replace(/\.md$/, "")]));
const embedContext = {
  resolve: (t: string) => [...bodies.keys()].find((p) => p.replace(/\.md$/, "").toLowerCase() === t.toLowerCase()) ?? null,
  assetUrl: (p: string) => `asset://localhost/vault/${p}`,
  embedded: bodies,
  titles
};
const renderEmbed = (src: string) => renderMarkdown(md, src, embedContext).trim();

check("note embed is transcluded", renderEmbed("![[Welcome]]"), 'class="transclusion"');
check("transclusion shows source title", renderEmbed("![[Welcome]]"), ">Welcome");
check("transcluded body is rendered", renderEmbed("![[Welcome]]"), "Run the server");
check("heading embed takes only that section", renderEmbed("![[Welcome#Setup]]"), "Run the server");
check(
  "heading embed excludes other sections",
  renderEmbed("![[Welcome#Setup]]").includes("Not this bit") ? "leaked" : "ok",
  "ok"
);
check("missing heading reports empty", renderEmbed("![[Welcome#Nope]]"), "Nothing under that heading");
check("nested embed renders", renderEmbed("![[Ideas]]"), "Run the server");
check("self-embed is caught", renderEmbed("![[Loop]]"), "already embedded above");
check("mutual embed terminates", renderEmbed("![[A]]"), "already embedded above");
check(
  "same note embedded twice is not a false cycle",
  (renderEmbed("![[Welcome]]\n\n![[Welcome]]").match(/already embedded/g) ?? []).length === 0 ? "ok" : "false positive",
  "ok"
);
check(
  "sibling embeds both render",
  (renderEmbed("![[Welcome]]\n\n![[Welcome]]").match(/Run the server/g) ?? []).length === 2 ? "ok" : "missing",
  "ok"
);
check("unfetched embed falls back to a link", renderMarkdown(md, "![[Welcome]]", context), 'class="wikilink"');
check("sectionOf stops at same-level heading", sectionOf(bodies.get("Welcome.md")!, "Setup"), "## Setup");
check(
  "sectionOf excludes the next section",
  sectionOf(bodies.get("Welcome.md")!, "Setup").includes("Not this bit") ? "leaked" : "ok",
  "ok"
);

// --- force graph -----------------------------------------------------------
const graph = new ForceGraph();
const nodes = Array.from({ length: 60 }, (_, i) => ({
  id: `n${i}`,
  label: `Node ${i}`,
  exists: i % 7 !== 0,
  degree: 2
}));
const edges = Array.from({ length: 59 }, (_, i) => ({ source: `n${i}`, target: `n${i + 1}` }));
graph.load(nodes, edges);

let ticks = 0;
while (graph.tick() && ticks < 5000) ticks += 1;
const finite = graph.nodes.every((n) => Number.isFinite(n.x) && Number.isFinite(n.y));
const b = graph.bounds();
const spread = Math.max(b.maxX - b.minX, b.maxY - b.minY);
// No two nodes should end up on top of each other.
let minGap = Infinity;
for (let i = 0; i < graph.nodes.length; i++)
  for (let j = i + 1; j < graph.nodes.length; j++) {
    const dx = graph.nodes[i].x - graph.nodes[j].x;
    const dy = graph.nodes[i].y - graph.nodes[j].y;
    minGap = Math.min(minGap, Math.sqrt(dx * dx + dy * dy));
  }

console.log(`\nforce graph: settled in ${ticks} ticks, spread ${spread.toFixed(0)}, min gap ${minGap.toFixed(1)}`);
check("layout converges", ticks < 2000 ? "ok" : "slow", "ok");
check("all coordinates finite", finite ? "ok" : "NaN", "ok");
check("nodes do not overlap", minGap > 8 ? "ok" : `too close: ${minGap.toFixed(1)}`, "ok");
check("graph does not collapse or explode", spread > 100 && spread < 20000 ? "ok" : `spread ${spread}`, "ok");

// Positions must survive a reload (vault changed on disk).
const before = graph.nodes[10].x;
graph.load([...nodes, { id: "new", label: "New", exists: true, degree: 0 }], edges);
check("reload keeps positions", graph.nodes[10].x === before ? "ok" : "moved", "ok");

console.log(failures === 0 ? "\nALL CHECKS PASSED" : `\n${failures} FAILURES`);
process.exit(failures === 0 ? 0 : 1);
