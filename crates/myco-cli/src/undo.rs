//! `myco undo` — rewind the `project` branch tip one commit. Every
//! mutating verb is one commit, so the reset is the inverse
//! mutation; the commit stays in the reflog.

use serde_json::json;

use crate::discover;
use crate::ops;
use crate::registry::{Args, Ctx};

/// The `undo` verb — guards live in [`ops::undo_last`].
pub fn h(ctx: &Ctx, a: &Args) -> Result<i32, String> {
    let force = a.flag("force");
    let json_out = ctx.json;
    let proj = discover::discover(ctx.dir.as_deref(), ctx.root.as_deref())?;
    // rewinding the branch tip is a mutation — same lock every
    // writer holds
    let _lock = ops::lock_worktree(&proj.worktree)?;
    let out = ops::undo_last(&proj.worktree, force)?;
    let payload = json!({
        "undone": {"hash": out.hash, "subject": out.subject, "date": out.date},
        "warnings": out.warnings,
    });
    if json_out {
        ops::print_json(&payload);
    } else {
        println!(
            "undid {} {} — {}",
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
