//! `myco item move` — the cross-repo transfer. The item is
//! re-created in the destination ledger's own id sequence (kind,
//! title, body, `pri`/`owner`/`#label`/`epic`/`ms` carried;
//! `blocks:`/`links:` dropped — they name source-ledger ids), both ledgers
//! get a provenance handoff (`moved from`/`moved to`), then the
//! source walks to `done`: a file-backed item keeps its file with
//! status `done`; a task's line leaves the ledger and its handoff
//! file drops to `handoffs/archive/` — the `check`-prescribed
//! home for items that are gone. Two commits, one per worktree,
//! both under the usual `--no-commit`/`$MYCO_NO_COMMIT` rules.
//!
//! There's no distributed transaction across the two worktrees —
//! the destination half lands first (a copy left behind beats an
//! item deleted on both ends), and a source-side failure reports
//! the new id so the close can be finished by hand.

use std::path::{Path, PathBuf};

use myco_core::{Actor, HandoffEntry, Item, ItemFile, ItemId, ItemKind, NewItemMeta, Status};
use myco_store::Store;
use serde_json::json;

use crate::registry::{Args, Ctx};
use crate::{continuity, discover, indexcmd, items, ops, todo};

/// What a move produced — the CLI report and the wire payload
/// share it.
pub(crate) struct Moved {
    /// The source id (closed `done` / line removed).
    pub from: ItemId,
    /// The destination copy — fresh id in the dest sequence.
    pub item: Item,
    /// Destination repo label (`meta.toml` name, else the root
    /// basename).
    pub repo: String,
    /// The destination repo root.
    pub root: PathBuf,
    /// The source-side handoff entry — the audit pointer.
    pub handoff: HandoffEntry,
    /// Source-worktree autocommit result.
    pub committed_source: bool,
    /// Destination-worktree autocommit result.
    pub committed_dest: bool,
    /// `--finish` found the source already closed — only commits
    /// (if any) were needed.
    pub already_closed: bool,
}

/// Canonical form for comparisons — never fails, like discovery's.
fn canon(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

/// Same ledger on both ends — the same worktree path, or two
/// worktrees sharing one git common dir (linked worktrees of a
/// single repo). Plain-dir ledgers (no git) compare by path only.
fn same_repo(a: &Path, b: &Path) -> bool {
    if canon(a) == canon(b) {
        return true;
    }
    match (myco_git::common_dir(a), myco_git::common_dir(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

/// `--to` → the destination project. A path resolves like
/// `index add`'s arg (repo root *or* its `project/` worktree); a
/// non-path falls back to the registry's repo *names* (`myco
/// index list`).
fn resolve_dest(to: &str) -> Result<discover::Project, String> {
    if let Ok(p) = indexcmd::repo_at(Path::new(to)) {
        return Ok(p);
    }
    if let Ok(index) = indexcmd::open_index() {
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        if let Ok(uni) = myco_ops::universe::repos(&cwd, &index, crate::ops::CONFIG) {
            if let Some(u) = uni.iter().find(|u| u.entry.name == to) {
                return indexcmd::repo_at(u.entry.path.as_std_path());
            }
        }
    }
    Err(format!(
        "{to}: not a myco repo — pass a repo root, its project/ worktree, \
         or a repo name from `myco index list`"
    ))
}

/// The `repo` half of the `repo:ID` pointer — `meta.toml`'s name,
/// else the root basename.
fn repo_label(store: &impl Store, root: &Path) -> String {
    store
        .meta()
        .map(|m| m.name)
        .ok()
        .filter(|n| !n.is_empty())
        .or_else(|| root.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "repo".into())
}

/// The metadata that survives the crossing. `blocks:`/`links:` are
/// dropped on purpose — they name source-ledger ids and would
/// dangle in the destination (`check` would flag them); the drop
/// is recorded as plain text in the provenance handoffs
/// ([`dropped_refs_line`]).
fn carried_meta(src: &Item) -> NewItemMeta {
    NewItemMeta {
        priority: Some(src.priority),
        owner: src.owner.clone(),
        labels: src.labels.iter().cloned().collect(),
        blocks: Vec::new(),
        links: Vec::new(),
        epic: src.epic.clone(),
        milestone: src.milestone.clone(),
        due: src.due,
        estimate: src.estimate,
        cycle: src.cycle.clone(),
    }
}

/// Re-create `src` in `dest` — a task lands at TODO.md's tail via
/// the `todo add` path; file kinds go through `save_item` with the
/// source body verbatim. The id is the destination sequence's
/// next; `created` carries over (a moved item keeps its filing
/// date). Caller commits.
fn create_in(dest: &impl Store, src: &Item, body: Option<&str>) -> Result<Item, String> {
    let meta = carried_meta(src);
    if src.id.kind() == ItemKind::Task {
        let (item, _file) = todo::add_item(dest, &src.title, None, None, &meta)?;
        return Ok(item);
    }
    let id = dest.alloc_id(src.id.kind()).map_err(|e| e.to_string())?;
    let mut item = Item::new(id, &src.title).created(src.created.unwrap_or_else(items::today));
    meta.apply(&mut item)?;
    let file = ItemFile::new(item.clone()).body(body.unwrap_or_default().to_string());
    dest.save_item(&file).map_err(|e| e.to_string())?;
    Ok(item)
}

/// The `dropped refs:` note line — the source-ledger ids that
/// didn't carry (`blocks:`/`links:` name ids in the *source*
/// sequence). `None` when there was nothing to drop.
fn dropped_refs_line(src: &Item) -> Option<String> {
    let mut parts = Vec::new();
    if !src.blocks.is_empty() {
        parts.push(format!(
            "blocks {}",
            src.blocks
                .iter()
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !src.links.is_empty() {
        parts.push(format!(
            "links {}",
            src.links
                .iter()
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    (!parts.is_empty()).then(|| format!("dropped refs: {}", parts.join("; ")))
}

/// `moved to <repo>:<ID>` pointer parse — the first line of a
/// source-side handoff note, ` — <extra>` stripped, plus the
/// `dest: <root>` resume line later moves carry (the canonical
/// destination root — the label alone only resolves through the
/// repo index). The repo label may itself contain `:`; the id is
/// always the last `:`-separated token.
fn parse_moved_to(note: &str) -> Option<(String, ItemId, Option<PathBuf>)> {
    let line = note.lines().next()?;
    let head = line.strip_prefix("moved to ")?;
    let head = head.split(" — ").next().unwrap_or(head);
    let (label, id) = head.rsplit_once(':')?;
    let id = id.trim().parse().ok()?;
    let root = note
        .lines()
        .find_map(|l| l.strip_prefix("dest: "))
        .map(PathBuf::from);
    Some((label.to_string(), id, root))
}

/// The source close after the audit handoff lands. A task's line
/// is taken out of its ledger doc and its handoff file moves to
/// `handoffs/archive/` (where `check` wants the files of gone
/// items); a file-backed item walks the state machine to `done`
/// (an already-done item just keeps its file).
fn close_source(store: &impl Store, worktree: &Path, id: ItemId) -> Result<(), String> {
    if id.kind() == ItemKind::Task {
        let mut ledger = items::load_ledger(store)?;
        let loc = ledger
            .locate(id)
            .ok_or_else(|| format!("{id}: no such task"))?;
        ledger.take(&loc);
        items::save_ledger(store, &ledger)?;
        // the item is gone — its handoff goes cold, exactly the
        // rename `check --fix` prescribes for orphan files. Both
        // files existing is an already-inconsistent ledger: don't
        // clobber the cold one, leave the live file for `check`
        // to flag.
        let name = myco_store_md::handoff_filename(&id);
        let live = worktree.join("handoffs").join(&name);
        let archive = worktree.join("handoffs").join("archive").join(&name);
        if live.is_file() && !archive.exists() {
            std::fs::create_dir_all(archive.parent().unwrap_or(worktree))
                .map_err(|e| format!("handoffs/archive: {e}"))?;
            std::fs::rename(&live, &archive).map_err(|e| format!("archive {name}: {e}"))?;
        }
        return Ok(());
    }
    let mut file = items::item_for_edit(store, id)?;
    if file.meta.status != Status::Done {
        items::walk_to(&mut file.meta, Status::Done)?;
        store.save_item(&file).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// The move itself — shared by the CLI handler and `item.move` on
/// the wire. `dir`/`root` are the global discovery flags (the
/// source side); `to` resolves the destination.
pub(crate) fn do_move(
    dir: Option<&Path>,
    root: Option<&Path>,
    id: ItemId,
    to: &str,
    note: Option<&str>,
    actor: Option<Actor>,
    no_commit: bool,
) -> Result<Moved, String> {
    let (src_proj, src) = items::open(dir, root)?;
    let (item, _location, body) =
        items::find_item(&src, id)?.ok_or_else(|| format!("{id}: no such item"))?;
    if src.is_archived(id).map_err(|e| e.to_string())? {
        return Err(format!("{id} is archived — unarchive it before moving"));
    }
    if !item.children.is_empty() {
        return Err(format!(
            "{id} has {} child item(s) — move them individually first \
             (`done`/`defer` acts on the whole subtree)",
            item.children.len()
        ));
    }
    let dest_proj = resolve_dest(to)?;
    if same_repo(&src_proj.worktree, &dest_proj.worktree) {
        return Err(format!(
            "{to}: the destination is this repo — move is cross-repo only"
        ));
    }
    let (dest_proj, dst) = items::open(Some(&dest_proj.worktree), Some(&dest_proj.root))?;
    let src_name = repo_label(&src, &src_proj.root);
    let dest_name = repo_label(&dst, &dest_proj.root);
    // one resolved actor for both worktrees' handoffs + commits
    let me = ops::resolve_actor(Some(&src_proj.worktree), actor.as_ref());

    // destination first: create + provenance. `blocks:`/`links:`
    // name source-ledger ids — they don't carry; the drop rides
    // both provenance handoffs as plain text.
    let new_item =
        create_in(&dst, &item, body.as_deref()).map_err(|e| format!("{dest_name}: {e}"))?;
    let dropped = dropped_refs_line(&item);
    let mut from_note = format!("moved from {src_name}:{id}");
    if let Some(d) = &dropped {
        from_note.push_str(&format!("\n{d}"));
    }
    continuity::record_handoff(&dst, &dest_proj.worktree, new_item.id, from_note, &me, None)
        .map_err(|e| format!("{dest_name}: {e}"))?;

    // then the source audit pointer — written *before* the dest
    // commit so a crash mid-move leaves the resume pointer on
    // disk (`item move <id> --finish` picks it up)
    let mut note_text = format!("moved to {dest_name}:{}", new_item.id);
    if let Some(extra) = note.map(str::trim).filter(|n| !n.is_empty()) {
        note_text.push_str(&format!(" — {extra}"));
    }
    // the resume handle for `--finish` — the canonical root,
    // since the label only resolves through the repo index
    note_text.push_str(&format!("\ndest: {}", dest_proj.root.display()));
    if let Some(d) = &dropped {
        note_text.push_str(&format!("\n{d}"));
    }
    let left = |e: String| {
        format!(
            "{e} — destination {dest_name}:{} was created; \
             run `myco item move {id} --finish` to complete the source close",
            new_item.id
        )
    };
    let (handoff, _rel) =
        continuity::record_handoff(&src, &src_proj.worktree, id, note_text, &me, None)
            .map_err(left)?;
    let committed_dest = items::commit(
        &dest_proj.worktree,
        &format!("chore: move {} in from {src_name}:{id}", new_item.id),
        Some(&me),
        no_commit,
    )?;

    // source close last — a failure here is resumable via
    // `--finish`, which finds the pointer above
    close_source(&src, &src_proj.worktree, id).map_err(left)?;
    let committed_source = items::commit(
        &src_proj.worktree,
        &format!("chore: move {id} to {dest_name}:{}", new_item.id),
        Some(&me),
        no_commit,
    )?;

    Ok(Moved {
        from: id,
        item: new_item,
        repo: dest_name,
        root: dest_proj.root.clone(),
        handoff,
        committed_source,
        committed_dest,
        already_closed: false,
    })
}

/// A read-only preview of [`do_move`] — the same guards, nothing
/// written. The prospective destination id comes from the dest
/// registry (in-memory only — no `meta.toml` touched).
pub(crate) struct MovePlan {
    /// Source id.
    pub from: ItemId,
    /// The source item.
    pub item: Item,
    /// Source repo label.
    pub src_name: String,
    /// Destination label + root (`to` resolved).
    pub repo: String,
    pub root: PathBuf,
    /// The id the destination copy would get.
    pub dest_id: ItemId,
    /// `blocks:`/`links:` that won't carry — plain text for the
    /// provenance notes.
    pub dropped: Option<String>,
    /// User-supplied extra note (as given, `--note`).
    pub note: Option<String>,
}

/// `item move --dry-run` — validate the whole move, report the
/// plan. No writes anywhere.
pub(crate) fn move_plan(
    dir: Option<&Path>,
    root: Option<&Path>,
    id: ItemId,
    to: &str,
    note: Option<&str>,
) -> Result<MovePlan, String> {
    let (src_proj, src) = items::open(dir, root)?;
    let (item, _location, _body) =
        items::find_item(&src, id)?.ok_or_else(|| format!("{id}: no such item"))?;
    if src.is_archived(id).map_err(|e| e.to_string())? {
        return Err(format!("{id} is archived — unarchive it before moving"));
    }
    if !item.children.is_empty() {
        return Err(format!(
            "{id} has {} child item(s) — move them individually first",
            item.children.len()
        ));
    }
    let dest_proj = resolve_dest(to)?;
    if same_repo(&src_proj.worktree, &dest_proj.worktree) {
        return Err(format!(
            "{to}: the destination is this repo — move is cross-repo only"
        ));
    }
    let (dest_proj, dst) = items::open(Some(&dest_proj.worktree), Some(&dest_proj.root))?;
    let src_name = repo_label(&src, &src_proj.root);
    let dest_name = repo_label(&dst, &dest_proj.root);
    // prospective id — observe the live ids so `alloc` lands on
    // the same number the real move would take
    let mut reg = dst.meta().map_err(|e| e.to_string())?.registry();
    reg.observe_all(dst.all_ids().map_err(|e| e.to_string())?);
    let dest_id = reg.alloc(item.id.kind());
    let dropped = dropped_refs_line(&item);
    Ok(MovePlan {
        from: id,
        item,
        src_name,
        repo: dest_name,
        root: dest_proj.root.clone(),
        dest_id,
        dropped,
        note: note.map(str::to_string),
    })
}

/// `item move --finish` — complete a move that died after the
/// destination copy + pointer landed but before the source close
/// committed. The `moved to <repo>:<ID>` handoff entry is the
/// resume pointer; the dest copy must exist, then both sides
/// commit (dest first — it may be uncommitted from the crash).
/// Idempotent: an already-closed source only commits what's
/// pending.
pub(crate) fn finish_move(
    dir: Option<&Path>,
    root: Option<&Path>,
    id: ItemId,
    actor: Option<Actor>,
    no_commit: bool,
) -> Result<Moved, String> {
    let (src_proj, src) = items::open(dir, root)?;
    let me = ops::resolve_actor(Some(&src_proj.worktree), actor.as_ref());
    // the newest `moved to` pointer — live or archived handoff
    let entries = src.read_handoffs(id).map_err(|e| e.to_string())?;
    let pointer = entries
        .iter()
        .rev()
        .find_map(|e| parse_moved_to(&e.note).map(|p| (p, e.clone())));
    let Some(((label, dest_id, dest_root), entry)) = pointer else {
        return Err(format!(
            "{id}: no `moved to` handoff — nothing to finish \
             (a finished move leaves the pointer in the archive)"
        ));
    };
    // the `dest:` root is canonical; the label is the fallback
    // for pointers written before it existed
    let dest_proj = match dest_root {
        Some(root) => indexcmd::repo_at(&root).map_err(|e| format!("{label}: {e}"))?,
        None => resolve_dest(&label)?,
    };
    let (dest_proj, dst) = items::open(Some(&dest_proj.worktree), Some(&dest_proj.root))?;
    // the destination copy must exist — committed or still
    // pending in the worktree (the store reads the files)
    let Some((ditem, _loc, _body)) = items::find_item(&dst, dest_id).map_err(|e| e.to_string())?
    else {
        return Err(format!(
            "{label}:{dest_id} not found in the destination — refusing \
             to close {id} here; reconcile by hand"
        ));
    };
    let src_name = repo_label(&src, &src_proj.root);
    let dest_name = repo_label(&dst, &dest_proj.root);

    // source side: close only what's still open — `close_source`
    // is itself idempotent for done file items; a task whose line
    // is already gone needs nothing
    let already_closed = match items::find_item(&src, id).map_err(|e| e.to_string())? {
        None => true,
        Some((i, _, _)) => i.status == Status::Done,
    };
    if !already_closed {
        close_source(&src, &src_proj.worktree, id).map_err(|e| {
            format!(
                "{e} — destination {dest_name}:{dest_id} exists; \
                 retry `myco item move {id} --finish`"
            )
        })?;
    }
    let committed_dest = items::commit(
        &dest_proj.worktree,
        &format!("chore: move {dest_id} in from {src_name}:{id}"),
        Some(&me),
        no_commit,
    )?;
    let committed_source = items::commit(
        &src_proj.worktree,
        &format!("chore: move {id} to {dest_name}:{dest_id}"),
        Some(&me),
        no_commit,
    )?;
    Ok(Moved {
        from: id,
        item: ditem,
        repo: dest_name,
        root: dest_proj.root.clone(),
        handoff: entry,
        committed_source,
        committed_dest,
        already_closed,
    })
}

/// The `--json` payload — the destination item plus both sides'
/// pointers and commit flags (two worktrees, two autocommits).
/// `committed` itself stays the scalar [`items::report`] merges.
pub(crate) fn moved_json(m: &Moved) -> serde_json::Value {
    json!({
        "id": m.from.to_string(),
        "item": m.item,
        "to": {"repo": m.repo, "root": m.root, "id": m.item.id.to_string()},
        "handoff": m.handoff,
        "committed_source": m.committed_source,
        "committed_destination": m.committed_dest,
        "already_closed": m.already_closed,
    })
}

/// `--dry-run` payload — the plan, nothing committed anywhere.
pub(crate) fn plan_json(p: &MovePlan) -> serde_json::Value {
    json!({
        "id": p.from.to_string(),
        "item": p.item,
        "from": p.src_name,
        "to": {"repo": p.repo, "root": p.root, "id": p.dest_id.to_string()},
        "dropped": p.dropped,
        "note": p.note,
        "dry_run": true,
    })
}

/// The `item move` verb — `to` opens a fresh move, `--dry-run`
/// previews it, `--finish` completes one that died mid-flight.
pub fn h_move(ctx: &Ctx, a: &Args) -> Result<i32, String> {
    let id = a.req::<ItemId>("id")?;
    let finish = a.flag("finish");
    let dry_run = a.flag("dry-run");
    let to = a.opt("to");
    if finish && (dry_run || to.is_some()) {
        return Err("item move: --finish takes no --to/--dry-run".into());
    }
    if !finish && to.is_none() {
        return Err("item move: --to <repo|path> is required (or --finish)".into());
    }
    if dry_run && !finish {
        let p = move_plan(
            ctx.dir.as_deref(),
            ctx.root.as_deref(),
            id,
            to.unwrap_or_default(),
            a.opt("note"),
        )?;
        return items::report(
            plan_json(&p),
            true,
            ctx.json,
            &format!(
                "would move {} → {}:{} ({})",
                p.from,
                p.repo,
                p.dest_id,
                p.root.display()
            ),
        );
    }
    if crate::confirm::needed(a.flag("yes")) {
        let what = match &to {
            Some(t) => format!("move {id} → {t}? the source closes."),
            None => format!("finish the move of {id}? the source closes."),
        };
        if !crate::confirm::ask(&what) {
            eprintln!("myco: cancelled");
            return Ok(1);
        }
    }
    let m = if finish {
        finish_move(
            ctx.dir.as_deref(),
            ctx.root.as_deref(),
            id,
            a.parsed("actor")?,
            ctx.no_commit,
        )?
    } else {
        do_move(
            ctx.dir.as_deref(),
            ctx.root.as_deref(),
            id,
            to.unwrap_or_default(),
            a.opt("note"),
            a.parsed("actor")?,
            ctx.no_commit,
        )?
    };
    items::report(
        moved_json(&m),
        m.committed_source && m.committed_dest,
        ctx.json,
        &format!("moved {} → {}:{}", m.from, m.repo, m.item.id),
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) {
        let st = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(st.success(), "git {args:?}");
    }

    /// A repo with a `project/` worktree on the `project` branch.
    fn repo_with_project() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        git(r, &["init", "-q"]);
        git(r, &["config", "user.email", "t@t"]);
        git(r, &["config", "user.name", "t"]);
        myco_git::create_orphan_branch(r, "project", "init").unwrap();
        git(r, &["worktree", "add", "project", "project"]);
        std::fs::write(r.join("project/TODO.md"), "# TODO — x\n").unwrap();
        d
    }

    /// Drive a verb like dispatch does — argv through the
    /// registry decoder into the handler, source `--dir` pinned.
    fn run_verb(dir: &Path, name: &str, argv: &[String]) -> Result<i32, String> {
        let mut ctx = crate::registry::Ctx {
            json: true,
            no_commit: true,
            dir: Some(dir.to_path_buf()),
            ..Default::default()
        };
        let v = crate::registry::find(name).unwrap();
        let a = crate::registry::Args::from_argv(v, argv, &mut ctx, false)?;
        (v.cli.unwrap())(&ctx, &a)
    }

    fn s(v: &str) -> String {
        v.to_string()
    }

    /// A move against two scratch repos — `$MYCO_HOME` pinned so
    /// the registry-name fallback never touches the real index.
    fn with_repos(f: impl Fn(&Path, &Path)) {
        let home = tempfile::tempdir().unwrap();
        temp_env::with_var("MYCO_HOME", Some(home.path().to_str().unwrap()), || {
            let src = repo_with_project();
            let dst = repo_with_project();
            f(&src.path().join("project"), &dst.path().join("project"));
        });
    }

    #[test]
    fn move_task_end_to_end() {
        with_repos(|src_wt, dst_wt| {
            run_verb(
                src_wt,
                "todo.add",
                &[
                    s("port the thing"),
                    s("--pri"),
                    s("high"),
                    s("--label"),
                    s("core"),
                ],
            )
            .unwrap();
            run_verb(
                src_wt,
                "item.move",
                &[
                    s("T-001"),
                    s("--to"),
                    dst_wt.parent().unwrap().to_string_lossy().into_owned(),
                ],
            )
            .unwrap();
            // destination got a fresh id in its own sequence
            let dst = crate::ops::store_at(dst_wt, myco_core::kind::prelude_table()).unwrap();
            let (item, _, _) = items::find_item(&dst, "T-001".parse().unwrap())
                .unwrap()
                .unwrap();
            assert_eq!(item.title, "port the thing");
            assert_eq!(item.priority.to_string(), "high");
            assert!(item.labels.contains("core"));
            assert_eq!(item.status, Status::Open);
            // provenance on the destination copy
            let entries = dst.read_handoffs(item.id).unwrap();
            assert!(entries[0].note.contains("moved from"), "{entries:?}");
            // source line is gone; the handoff went cold with the
            // `moved to` pointer intact
            let src = crate::ops::store_at(src_wt, myco_core::kind::prelude_table()).unwrap();
            assert!(items::find_item(&src, "T-001".parse().unwrap())
                .unwrap()
                .is_none());
            assert!(!src_wt.join("handoffs/T-001.md").exists());
            let archived = src_wt.join("handoffs/archive/T-001.md");
            assert!(archived.is_file());
            let text = std::fs::read_to_string(archived).unwrap();
            assert!(text.contains("moved to"), "{text}");
            assert!(text.contains("T-001"), "{text}");
        });
    }

    #[test]
    fn move_issue_closes_source_file() {
        with_repos(|src_wt, dst_wt| {
            run_verb(src_wt, "issue.new", &[s("leaky vault")]).unwrap();
            run_verb(
                src_wt,
                "item.move",
                &[s("I-001"), s("--to"), dst_wt.to_string_lossy().into_owned()],
            )
            .unwrap();
            let src = crate::ops::store_at(src_wt, myco_core::kind::prelude_table()).unwrap();
            let (old, _, _) = items::find_item(&src, "I-001".parse().unwrap())
                .unwrap()
                .unwrap();
            assert_eq!(old.status, Status::Done);
            // the audit entry stays live — the item file still
            // exists, only its status moved
            let entries = src.read_handoffs(old.id).unwrap();
            assert!(
                entries.last().unwrap().note.contains("moved to"),
                "{entries:?}"
            );
            let dst = crate::ops::store_at(dst_wt, myco_core::kind::prelude_table()).unwrap();
            let (new, _, body) = items::find_item(&dst, "I-001".parse().unwrap())
                .unwrap()
                .unwrap();
            assert_eq!(new.title, "leaky vault");
            assert!(body.is_some());
        });
    }

    #[test]
    fn move_errors() {
        with_repos(|src_wt, dst_wt| {
            run_verb(src_wt, "todo.add", &[s("x")]).unwrap();
            // unknown id
            assert!(run_verb(
                src_wt,
                "item.move",
                &[s("T-099"), s("--to"), dst_wt.to_string_lossy().into_owned()],
            )
            .unwrap_err()
            .contains("no such item"));
            // same repo — the source worktree is not a destination
            let e = run_verb(
                src_wt,
                "item.move",
                &[s("T-001"), s("--to"), src_wt.to_string_lossy().into_owned()],
            )
            .unwrap_err();
            assert!(e.contains("this repo"), "{e}");
            // a dest that isn't a myco repo
            let plain = tempfile::tempdir().unwrap();
            let e = run_verb(
                src_wt,
                "item.move",
                &[
                    s("T-001"),
                    s("--to"),
                    plain.path().to_string_lossy().into_owned(),
                ],
            )
            .unwrap_err();
            assert!(e.contains("not a myco repo"), "{e}");
            // nothing burned: T-001 is still live, the dest ledger
            // is still empty
            let src = crate::ops::store_at(src_wt, myco_core::kind::prelude_table()).unwrap();
            assert!(items::find_item(&src, "T-001".parse().unwrap())
                .unwrap()
                .is_some());
            let dst = crate::ops::store_at(dst_wt, myco_core::kind::prelude_table()).unwrap();
            assert!(items::find_item(&dst, "T-001".parse().unwrap())
                .unwrap()
                .is_none());
        });
    }

    #[test]
    fn move_parent_with_children_refused() {
        with_repos(|src_wt, dst_wt| {
            run_verb(src_wt, "todo.add", &[s("parent")]).unwrap();
            run_verb(src_wt, "todo.add", &[s("child"), s("--parent"), s("T-001")]).unwrap();
            let e = run_verb(
                src_wt,
                "item.move",
                &[s("T-001"), s("--to"), dst_wt.to_string_lossy().into_owned()],
            )
            .unwrap_err();
            assert!(e.contains("child"), "{e}");
        });
    }

    #[test]
    fn moved_json_shape() {
        let m = Moved {
            from: "I-004".parse().unwrap(),
            item: Item::new("I-007".parse().unwrap(), "x"),
            repo: "peregrin".into(),
            root: PathBuf::from("/tmp/peregrin"),
            handoff: HandoffEntry {
                date: items::today(),
                actor: Actor::new("cli").unwrap(),
                session: None,
                note: "moved to peregrin:I-007".into(),
            },
            committed_source: true,
            committed_dest: false,
            already_closed: false,
        };
        let v = moved_json(&m);
        assert_eq!(v["id"], "I-004");
        assert_eq!(v["to"]["id"], "I-007");
        assert_eq!(v["to"]["repo"], "peregrin");
        assert_eq!(v["committed_source"], true);
        assert_eq!(v["committed_destination"], false);
    }
}
