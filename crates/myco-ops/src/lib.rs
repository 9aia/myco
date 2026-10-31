//! myco-ops — the shared ops layer every front-end calls.
//!
//! Verb logic lives here, not in the binary: `discover` resolves the
//! `project/` worktree, `ops` owns the scaffold +
//! commit-per-mutation machinery, and `items` is the
//! TODO↔BACKLOG ledger + state-machine walks the item verbs share.
//! `myco` (CLI) is a thin clap shell over this; `myco-tui` calls the
//! same functions, so both front-ends produce identical writes and
//! identical commits.

pub mod discover;
pub mod items;
pub mod ops;
