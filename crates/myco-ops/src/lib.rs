//! myco-ops — the I/O shell every front-end calls.
//!
//! `myco-core` is pure domain (ledger doc surgery, state-machine
//! walks, item metadata); this crate is everything that touches the
//! outside world: `discover` resolves the `project/` worktree via
//! git, `config` is the loader seam (path policy here, file format
//! injected), `ops` owns the scaffold + commit-per-mutation machinery, and
//! `items` is store open/save + the verbs' shared mutation/output
//! tail — re-exporting the core symbols so call sites read
//! `items::X` for either. `myco` (CLI) is a thin clap shell over
//! this; `myco-driver` (the pavo view driver) reads through the
//! same functions, so every front-end sees identical state.

pub mod config;
pub mod discover;
pub mod items;
pub mod links;
pub mod ops;
pub mod verb;
pub mod view;
