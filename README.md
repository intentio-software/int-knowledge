# Intentio Knowledge

A local-first markdown knowledge base with MCP built in. Your notes are plain `.md` files in a
folder you choose — readable by any editor, versionable with git, and directly editable by AI
agents through the companion MCP server.

![License](https://img.shields.io/badge/license-personal%20use-blue)
![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux-lightgrey)

## What it is

- **A vault is a folder.** No database, no proprietary format, no lock-in. Point the app at a
  directory and it becomes a vault.
- **Wikilinks.** `[[Note Name]]`, `[[Note|Alias]]`, `[[Note#Heading]]` and `![[Embeds]]`, with
  autocomplete, click-to-follow, and backlinks.
- **Source-first editing.** CodeMirror over the raw markdown, so a file an agent wrote comes back
  byte-identical unless you actually edit it.
- **MCP built in.** `int-knowledge-mcp` is a standalone stdio server over the same vault. It runs
  whether or not the app is open, needs no ports or tokens, and shares the app's link resolution,
  search and frontmatter parsing.

## Giving an agent access

```bash
# Claude Code
claude mcp add knowledge -- int-knowledge-mcp ~/Notes

# Anything that reads a standard MCP client config
{
  "mcpServers": {
    "knowledge": {
      "command": "int-knowledge-mcp",
      "args": ["/Users/you/Notes"]
    }
  }
}
```

Several vaults can be served at once — pass more than one folder, and tools take a `vault` argument
naming which to act on:

```bash
int-knowledge-mcp ~/Notes ~/TeamVault
```

### Tools

| Tool | What it does |
|---|---|
| `list_vaults` | Vaults this server has open |
| `vault_info` | Note, folder and tag counts; unresolved link count |
| `list_notes` | Notes with titles and tags, filterable by folder or tag |
| `list_folders` | Folders in the vault |
| `read_note` | Frontmatter, body, headings, outgoing links and backlinks |
| `create_note` | New note; refuses to overwrite an existing one |
| `write_note` | Replace a note's full contents |
| `append_note` | Append to a note, optionally under a heading |
| `update_frontmatter` | Set or remove frontmatter fields, body untouched |
| `delete_note` | Delete a note |
| `move_note` | Move or rename, rewriting wikilinks across the vault |
| `search_notes` | Full-text search with matching lines |
| `get_backlinks` | Notes linking to a note, with context lines |
| `get_links` | A note's outgoing links, resolved and unresolved |
| `list_tags` | Every tag with its note count |
| `unresolved_links` | Links pointing at notes that do not exist yet |
| `create_folder` | Create a folder |

Every path is vault-relative and validated against the vault root, so a tool call cannot reach
anything outside the folder you named.

## Reading and writing

Notes open **rendered** by default. `Ctrl/Cmd + E` switches to markdown source, and the choice
sticks. Source mode is CodeMirror over the raw file, so a note an agent wrote round-trips
byte-identically unless you edit it.

`![[Note]]` transcludes another note inline, `![[Note#Heading]]` pulls in just that section. Cycles
are detected, so a note that embeds itself renders as a link rather than recursing.

Raw HTML in notes is **not** rendered. A vault is agent-writable and the renderer runs in a webview
with access to the app's IPC bridge, so an agent-authored `<script>` would be a real injection path.
This is the one place Knowledge deliberately renders less than Obsidian does.

## Graph

`Ctrl/Cmd + G` opens the vault as a force-directed graph. Nodes size by link count, the open note is
ringed, and hovering dims everything except a note and its neighbours. Links to notes that do not
exist yet appear as dashed ghost nodes — double-click one to create it.

## Live updates

The vault is watched while the app is open, so notes an agent writes through MCP — or a `git pull`,
or another editor — appear without a refresh. If a note changes on disk while you have unsaved edits
to it, the app says so and asks rather than overwriting your work.

## Menus and shortcuts

File, Edit, View, Go and Help live in the native menu bar, which owns the shortcuts below.

| Action | Shortcut |
|---|---|
| Jump to / create a note | `Ctrl/Cmd + O` or `Ctrl/Cmd + N` |
| Search all notes | `Ctrl/Cmd + Shift + F` |
| Toggle read / source | `Ctrl/Cmd + E` |
| Graph view | `Ctrl/Cmd + G` |
| Save | `Ctrl/Cmd + S` (autosaves anyway) |
| Open vault | `Ctrl/Cmd + Shift + O` |
| Follow a link | `Ctrl/Cmd + click` |
| Link autocomplete | Type `[[` |
| Toggle sidebar | `Ctrl/Cmd + B` |
| Toggle side panel | `Ctrl/Cmd + \` |

## Tests

```bash
npm test          # frontend checks plus the Rust suite
npm run test:ui   # renderer, transclusion and graph layout only
cargo test        # vault, MCP server and watcher
```

## Releases

Pushes to `main` run semantic-release: conventional-commit messages decide the version, `CHANGELOG.md`
and every version file are updated, and a tagged draft release triggers platform builds. The app
auto-updates from those releases; the MCP server is attached to each one as a separate download.

## Repository layout

```
crates/int-vault/          Vault model: frontmatter, wikilinks, backlinks, search
crates/int-knowledge-mcp/  Standalone stdio MCP server
src-tauri/                 Desktop app backend, built on int-vault
src/                       Angular 20 frontend
```

`int-vault` is the shared engine: the desktop app and the MCP server both go through it, so an
agent and a human see identical link resolution and search results.

## Building from source

**Prerequisites**

- [Node.js](https://nodejs.org/) 20+
- [Rust](https://www.rust-lang.org/tools/install) (stable)
- On Linux: `libwebkit2gtk-4.1-dev`, `libappindicator3-dev`, `librsvg2-dev`, `patchelf`

```bash
git clone https://github.com/intentio-software/int-knowledge.git
cd int-knowledge
npm install
npx tauri dev
```

Build a release binary:

```bash
npx tauri build
```

The MCP server is a separate binary — it must be on your PATH to register with an agent,
so it is published as its own release asset rather than bundled inside the app:

```bash
cargo build --release -p int-knowledge-mcp
# ./target/release/int-knowledge-mcp
```

Run the Rust test suite:

```bash
cargo test
```

## Tech stack

- [Tauri v2](https://tauri.app) — native shell
- [Angular 20](https://angular.dev) — UI framework
- [CodeMirror 6](https://codemirror.net) — markdown editor
- [PrimeNG](https://primeng.org) — UI components

## Related

- [Intentio Mind Map](https://github.com/intentio-software/int-mind-map) — mind mapping, with its
  own `int-mindmap-mcp` server.

## License

Free for personal use. Commercial license coming soon — contact
[intentiosoftware.com](https://intentiosoftware.com) for enquiries.
