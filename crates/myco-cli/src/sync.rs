//! `myco sync` — reconcile the ledger branch with its remote.
//!
//! The `project` branch is the audit trail, and it's shared: two
//! clones committing independently fork it. Sync is the gate
//! mutating verbs enforce (`ensure_synced` refuses while behind)
//! and the healer: fetch, fast-forward when possible, rebase
//! local commits onto `origin/<branch>` when diverged, then repair
//! what a merge can break — `meta.toml` counters reconcile via
//! `check --fix`, and duplicate ids (two writers minting the same
//! `T-NNN`) are renumbered remote-wins by
//! [`crate::check::renumber_duplicates`].
//!
//! A rebase conflict stops mid-flight with the conflicted files
//! reported — resolve them (`git add` + `git rebase --continue`)
//! and run `myco sync` again, or `myco sync --abort` to bail.
//!
//! With `[sync] auto_push` enabled (the default; `false` opts
//! out), a successful sync also pushes the reconciled tip —
//! publishing the rebased local commits is the natural completion
//! of "reconcile with the remote" (the mutation tail pushes any
//! heal commit on its own; this covers the commit-free paths).
//! Best-effort like every push: a failure warns, never fails the
//! sync.

use serde_json::json;

use crate::discover;
use crate::ops;

/// The `sync` verb.
pub fn h(ctx: &crate::registry::Ctx, a: &crate::registry::Args) -> Result<i32, String> {
    let json_out = ctx.json;
    let no_commit = ctx.no_commit;
    let proj = discover::discover(ctx.dir.as_deref(), ctx.root.as_deref())?;
    let wt = proj.worktree.clone();

    // --abort bails a stopped rebase/merge without touching refs
    if a.flag("abort") {
        myco_ops::vcs::rebase_abort(&wt).map_err(|e| format!("rebase --abort: {e}"))?;
        if json_out {
            ops::print_json(&json!({"action": "abort"}));
        } else {
            println!("myco sync: rebase aborted — ledger back at its pre-sync tip");
        }
        return Ok(0);
    }

    let _lock = ops::lock_worktree(&wt)?;

    // a rebase already stopped mid-flight: report state, don't restart
    if myco_ops::vcs::rebase_in_progress(&wt) {
        let conflicts = myco_ops::vcs::unmerged_files(&wt);
        return finish(
            json_out,
            "conflict",
            &conflicts,
            &[],
            1,
            "myco sync: rebase is stopped on conflicts — resolve the files, \
             `git add` them, `git rebase --continue`, then `myco sync` again \
             (or `myco sync --abort` to bail)",
        );
    }

    let Some(branch) = myco_ops::vcs::worktree_branch(&wt) else {
        return Err(format!(
            "{} is not on a branch — run `myco check --fix` to reattach it",
            wt.display()
        ));
    };
    let remote = format!("origin/{branch}");
    let remote_ref = format!("refs/remotes/{remote}");

    // fetch is best-effort — offline sync still reconciles against
    // the last-known remote refs; a failed fetch is reported, not fatal
    let fetched = myco_ops::vcs::sync(&proj.root);
    let offline = if fetched {
        ""
    } else {
        " (fetch failed — offline?)"
    };
    if !myco_ops::vcs::has_commit(&proj.root, &remote_ref) {
        return finish(
            json_out,
            "no-remote",
            &[],
            &[],
            0,
            &format!(
                "myco sync: no `{remote}` ref — the ledger was never published; push it when ready"
            ),
        );
    }

    let ahead = myco_ops::vcs::commits_ahead(&wt, &remote_ref).unwrap_or(0);
    let behind = myco_ops::vcs::commits_behind(&wt, &remote_ref).unwrap_or(0);

    // fast-forward when local has nothing to replay; rebase the
    // local commits onto the remote tip when diverged. Up-to-date
    // skips the git op but still runs the heal tail — a conflicted
    // rebase resumed by hand (`--continue`) lands here with the
    // duplicate ids it produced still in the tree.
    let action = if behind == 0 {
        "up-to-date"
    } else if ahead == 0 {
        "ff"
    } else {
        "rebase"
    };
    if behind > 0 {
        let result = if ahead == 0 {
            myco_ops::vcs::merge_ff(&wt, &remote)
        } else {
            myco_ops::vcs::rebase(&wt, &remote)
        };
        if let Err(e) = result {
            let conflicts = myco_ops::vcs::unmerged_files(&wt);
            if conflicts.is_empty() {
                return Err(format!("git {action} onto `{remote}` failed: {e}"));
            }
            return finish(
                json_out,
                "conflict",
                &conflicts,
                &[],
                1,
                "myco sync: rebase stopped on conflicts — resolve the files, \
                 `git add` them, `git rebase --continue`, then `myco sync` again \
                 (or `myco sync --abort` to bail)",
            );
        }
    }

    // heal what the merge broke: duplicate ids get renumbered
    // (remote wins), counters/refs/drift get repaired — the check
    // --fix tail owns its own lock and commit, so drop ours first
    let kinds = crate::check::repo_kinds(&proj.root);
    let (renumbered, renum_report) = crate::check::renumber_duplicates(&wt, &kinds, &remote)
        .unwrap_or_else(|e| (0, vec![format!("renumber failed: {e}")]));
    let committed = renumbered > 0
        && ops::autocommit(
            &wt,
            "chore: renumber colliding ids (myco sync)",
            None,
            ops::commits_enabled(no_commit),
        )
        .unwrap_or(false);
    drop(_lock);

    // the canonical repair pass — counters, emit drift, filename
    // drift, and the final verdict all ride `check --fix`. It runs
    // in human mode even under `--json`: its findings go to stderr,
    // keeping stdout a single sync payload.
    let check_code = crate::check::run_on(&proj, true, false, no_commit)?;
    // `sync.auto_push` (default on) publishes what the rebase/heal
    // just landed — the mutation tail already pushed any commit it
    // made, so this covers the commit-free paths (pure ff/rebase).
    // Best-effort: a warning, never a sync failure.
    if let Some(w) = ops::maybe_push(&wt) {
        eprintln!("myco: warning: {w}");
    }
    finish(
        json_out,
        action,
        &[],
        &renum_report,
        check_code,
        &format!(
            "myco sync: {action} onto `{remote}` ({behind} behind, {ahead} ahead){offline}{}",
            if committed {
                " — colliding ids renumbered"
            } else {
                ""
            }
        ),
    )
}

/// Shared exit — JSON payload or one human line, plus the code.
fn finish(
    json_out: bool,
    action: &str,
    conflicts: &[String],
    renumbered: &[String],
    code: i32,
    human: &str,
) -> Result<i32, String> {
    if json_out {
        ops::print_json(&json!({
            "action": action,
            "conflicts": conflicts,
            "renumbered": renumbered,
        }));
    } else {
        println!("{human}");
        for c in conflicts {
            println!("  conflict: {c}");
        }
        for r in renumbered {
            println!("  renumbered: {r}");
        }
    }
    Ok(code)
}
