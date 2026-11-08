//! Re-export — lives in `myco_ops::discover` so other front-ends
//! (`myco-driver`) share it. `discover` itself is cli-local: ops
//! takes the config loader as a parameter and the composition
//! root (this crate) names the impl, so call sites stay two-arg.
//!
//! The cli `discover` adds the org-ledger fallback on top of ops'
//! strict discovery: inside a *registered* repo with no ledger of
//! its own, verbs resolve to the `ledger = "org"` entry's
//! worktree. The index opens lazily — only when plain discovery
//! already failed — so the happy path never touches it, and
//! unregistered repos fail exactly as before.

use std::path::Path;

pub use myco_ops::discover::{qualifies, repo_root_for, worktree_path, Project, BRANCH, DIR};

/// [`myco_ops::discover::discover`] wired to [`crate::ops::CONFIG`]
/// plus the org-ledger fallback (see module docs).
pub fn discover(dir: Option<&Path>, root: Option<&Path>) -> Result<Project, String> {
    match myco_ops::discover::discover(dir, root, crate::ops::CONFIG) {
        Ok(p) => Ok(p),
        Err(e) => {
            let Ok(index) = crate::indexcmd::open_index() else {
                return Err(e);
            };
            myco_ops::discover::discover_with(dir, root, crate::ops::CONFIG, Some(&index))
                .or(Err(e))
        }
    }
}
