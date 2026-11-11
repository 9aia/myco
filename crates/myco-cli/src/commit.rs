//! `myco commit` — stage + commit worktree changes with a
//! conventional message. Git identity comes from the repo config,
//! like every other myco commit.

use serde_json::json;

use crate::discover;
use crate::ops;

/// Default message — conventional commits inside the worktree.
const DEFAULT_MSG: &str = "chore: update project ledger";

/// The `commit` verb.
pub fn h(ctx: &crate::registry::Ctx, a: &crate::registry::Args) -> Result<i32, String> {
    let message = a.opt("message").map(str::to_string);
    let json_out = ctx.json;
    let no_commit = ctx.no_commit;
    let proj = discover::discover(ctx.dir.as_deref(), ctx.root.as_deref())?;
    // committing sweeps pending writes — don't catch another
    // verb mid-mutation, and don't deepen a behind-remote
    // divergence (`myco sync` closes it)
    ops::ensure_synced(
        &proj.worktree,
        myco_ops::config::sync_auto_fetch_for(&proj.root, ops::CONFIG),
    )?;
    let _lock = ops::lock_worktree(&proj.worktree)?;
    let msg = message.unwrap_or_else(|| DEFAULT_MSG.to_string());
    let enabled = ops::commits_enabled(no_commit);
    let dirty = myco_ops::vcs::has_changes(&proj.worktree).map_err(|e| e.to_string())?;
    // the verb's own message still rides the shared autocommit
    // path so it gains the `Actor:` trailer like every myco commit
    let committed = ops::autocommit(&proj.worktree, &msg, None, enabled && dirty)?;
    if json_out {
        ops::print_json(&json!({
            "worktree": proj.worktree.to_string_lossy(),
            "message": msg,
            "committed": committed,
            "skipped": !enabled || !dirty,
        }));
    } else if committed {
        println!("myco commit: {msg}");
    } else {
        println!(
            "myco commit: nothing to commit{}",
            if enabled { "" } else { " (--no-commit)" }
        );
    }
    Ok(0)
}
