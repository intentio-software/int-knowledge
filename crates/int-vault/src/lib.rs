//! # int-vault
//!
//! The vault model behind Intentio Knowledge: a folder of markdown files on the
//! user's own filesystem, with the structure that turns it into a knowledge base
//! — frontmatter, wikilinks, backlinks, tags and search.
//!
//! Nothing here owns a database or a cache file. The filesystem is the source of
//! truth, so the same vault stays fully usable from a text editor, from git, and
//! from an AI agent driving the MCP server.
//!
//! ```no_run
//! use int_vault::{Vault, VaultIndex, search, SearchOptions};
//!
//! let vault = Vault::open("/Users/me/Notes")?;
//! vault.create_note("Projects/Alpha", "# Alpha\n\nRelated: [[Beta]]\n")?;
//!
//! let index = VaultIndex::build(&vault);
//! let hits = search::search(&index, "alpha", &SearchOptions::default());
//! let backlinks = index.backlinks("Projects/Alpha.md");
//! # Ok::<(), int_vault::VaultError>(())
//! ```

pub mod error;
pub mod frontmatter;
pub mod index;
pub mod links;
pub mod note;
pub mod search;
pub mod vault;

pub use error::{Result, VaultError};
pub use index::{Backlink, IndexedNote, ResolvedLink, UnresolvedLink, VaultIndex};
pub use links::{Heading, LinkKind, LinkRef, Scan, TagRef};
pub use note::{Note, NoteMeta};
pub use search::{SearchHit, SearchMatch, SearchOptions};
pub use vault::{now_millis, Vault};
