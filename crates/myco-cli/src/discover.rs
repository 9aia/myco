//! Re-export — lives in `myco_ops::discover` so other front-ends
//! (`myco-driver`) share it. `discover` itself is cli-local: ops
//! takes the config loader as a parameter and the composition
//! root (this crate) names the impl, so call sites stay two-arg.

use std::path::Path;

pub use myco_ops::discover::{qualifies, repo_root_for, worktree_path, Project, BRANCH, DIR};

/// [`myco_ops::discover::discover`] wired to [`crate::ops::CONFIG`].
pub fn discover(dir: Option<&Path>, root: Option<&Path>) -> Result<Project, String> {
    myco_ops::discover::discover(dir, root, crate::ops::CONFIG)
}
