//! Re-export — lives in `myco_ops::discover` so other front-ends
//! (`myco-driver`) share it. `discover` itself is cli-local: ops
//! takes the config loader as a parameter and the composition
//! root (this crate) names the impl, so call sites stay two-arg.
//!
//! The cli `discover` is the user-facing path: ops'
//! [`myco_ops::discover::discover_selected`] applies the
//! current-ledger selector (P-015/P-016) plus the org-ledger
//! fallback. The index opens lazily — only when the first pass
//! fails or a selector target needs it — so the happy path never
//! touches it, and unregistered repos fail exactly as before.

use std::path::Path;

pub use myco_ops::discover::{qualifies, repo_root_for, worktree_path, Project, BRANCH, DIR};

/// [`myco_ops::discover::discover_selected`] wired to
/// [`crate::ops::CONFIG`], with the index opened on demand.
pub fn discover(dir: Option<&Path>, root: Option<&Path>) -> Result<Project, String> {
    match myco_ops::discover::discover_selected(dir, root, crate::ops::CONFIG, None) {
        Ok(p) => Ok(p),
        Err(e) => {
            let Ok(index) = crate::indexcmd::open_index() else {
                return Err(e);
            };
            myco_ops::discover::discover_selected(dir, root, crate::ops::CONFIG, Some(&index))
                .or(Err(e))
        }
    }
}
