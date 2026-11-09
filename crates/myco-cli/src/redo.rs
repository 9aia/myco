//! `myco redo` — re-apply the commit the last `undo` moved away
//! from (`git reset --hard HEAD@{1}`). The undone commit stays in
//! the reflog, so the reset restores it exactly.

use serde_json::json;

use crate::discover;
use crate::ops;
use crate::registry::{Args, Ctx};

/// The `redo` verb — guards live in [`ops::redo_last`].
pub fn h(ctx: &Ctx, a: &Args) -> Result<i32, String> {
    let _ = a;
    let json_out = ctx.json;
    let proj = discover::discover(ctx.dir.as_deref(), ctx.root.as_deref())?;
    // rewinding the branch tip is a mutation — same lock every
    // writer holds
    let _lock = ops::lock_worktree(&proj.worktree)?;
    let out = ops::redo_last(&proj.worktree)?;
    let payload = json!({
        "redone": {"hash": out.hash, "subject": out.subject, "date": out.date},
        "warnings": out.warnings,
    });
    if json_out {
        ops::print_json(&payload);
    } else {
        println!(
            "redid {} {} — {}",
            &out.hash[..12.min(out.hash.len())],
            out.date,
            out.subject
        );
        for w in &out.warnings {
            println!("warning: {w}");
        }
    }
    Ok(0)
}
