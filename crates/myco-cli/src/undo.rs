//! `myco undo` — rewind the `project` branch tip one commit. Every
//! mutating verb is one commit, so the reset is the inverse
//! mutation; the undone commit is recorded in the undo journal
//! (`<git-common-dir>/myco-history.json`) for `redo`. `--list`
//! shows the redoable stack without mutating.

use serde_json::json;

use crate::discover;
use crate::ops;
use crate::registry::{Args, Ctx};

/// The `undo` verb — guards live in [`ops::undo_last`].
pub fn h(ctx: &Ctx, a: &Args) -> Result<i32, String> {
    let force = a.flag("force");
    let json_out = ctx.json;
    let proj = discover::discover(ctx.dir.as_deref(), ctx.root.as_deref())?;
    if a.flag("list") {
        let entries = ops::history_list(&proj.worktree);
        if json_out {
            ops::print_json(&json!({
                "redoable": entries.iter().map(|e| json!({
                    "hash": e.target, "subject": e.subject, "date": e.date,
                })).collect::<Vec<_>>(),
            }));
        } else if entries.is_empty() {
            println!("nothing to redo — no undone commits are waiting");
        } else {
            for e in &entries {
                println!(
                    "{} {} — {}",
                    &e.target[..12.min(e.target.len())],
                    e.date,
                    e.subject
                );
            }
        }
        return Ok(0);
    }
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
