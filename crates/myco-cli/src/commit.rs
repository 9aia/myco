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
    // verb mid-mutation
    let _lock = ops::lock_worktree(&proj.worktree)?;
    let msg = message.unwrap_or_else(|| DEFAULT_MSG.to_string());
    let enabled = ops::commits_enabled(no_commit);
    let dirty = myco_git::has_changes(&proj.worktree).map_err(|e| e.to_string())?;
    let committed = if dirty && enabled {
        myco_git::commit_all(&proj.worktree, &msg)
            .map_err(|e| format!("git commit failed in {}: {e}", proj.worktree.display()))?;
        true
    } else {
        false
    };
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
