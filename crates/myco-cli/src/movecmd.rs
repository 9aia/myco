//! `myco item move` — the cross-repo transfer. The item is
//! re-created in the destination ledger's own id sequence (kind,
//! title, body, `pri`/`owner`/`#label`/`epic`/`ms` carried;
//! `blocks:` dropped — it names source-ledger ids), both ledgers
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
use myco_index::Index;
use myco_store::Store;
use serde_json::json;

use crate::registry::{Args, Ctx};
use crate::{continuity, discover, indexcmd, items, todo};

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
        if let Ok(repos) = index.repos() {
            if let Some(r) = repos.iter().find(|r| r.name == to) {
                return indexcmd::repo_at(r.path.as_std_path());
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

/// The metadata that survives the crossing. `blocks:` is dropped
/// on purpose — it names source-ledger ids and would dangle in
/// the destination (`check` would flag it).
fn carried_meta(src: &Item) -> NewItemMeta {
    NewItemMeta {
        priority: Some(src.priority),
        owner: src.owner.clone(),
        labels: src.labels.iter().cloned().collect(),
        blocks: Vec::new(),
        epic: src.epic.clone(),
        milestone: src.milestone.clone(),
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

    // destination first: create + provenance + its own commit
    let new_item =
        create_in(&dst, &item, body.as_deref()).map_err(|e| format!("{dest_name}: {e}"))?;
    continuity::record_handoff(
        &dst,
        &dest_proj.worktree,
        new_item.id,
        format!("moved from {src_name}:{id}"),
        actor.clone(),
        None,
    )
    .map_err(|e| format!("{dest_name}: {e}"))?;
    let committed_dest = items::commit(
        &dest_proj.worktree,
        &format!("chore: move {} in from {src_name}:{id}", new_item.id),
        actor.as_ref(),
        no_commit,
    )?;

    // then the source: audit pointer, close, its own commit — a
    // failure here names the created id so the close can be
    // finished by hand
    let mut note_text = format!("moved to {dest_name}:{}", new_item.id);
    if let Some(extra) = note.map(str::trim).filter(|n| !n.is_empty()) {
        note_text.push_str(&format!(" — {extra}"));
    }
    let left = |e: String| {
        format!(
            "{e} — destination {dest_name}:{} was created; \
             close {id} here by hand",
            new_item.id
        )
    };
    let (handoff, _rel) =
        continuity::record_handoff(&src, &src_proj.worktree, id, note_text, actor.clone(), None)
            .map_err(left)?;
    close_source(&src, &src_proj.worktree, id).map_err(left)?;
    let committed_source = items::commit(
        &src_proj.worktree,
        &format!("chore: move {id} to {dest_name}:{}", new_item.id),
        actor.as_ref(),
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
    })
}

/// The `item move` verb.
pub fn h_move(ctx: &Ctx, a: &Args) -> Result<i32, String> {
    let id = a.req::<ItemId>("id")?;
    let m = do_move(
        ctx.dir.as_deref(),
        ctx.root.as_deref(),
        id,
        a.text("to")?,
        a.opt("note"),
        a.parsed("actor")?,
        ctx.no_commit,
    )?;
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
        };
        let v = moved_json(&m);
        assert_eq!(v["id"], "I-004");
        assert_eq!(v["to"]["id"], "I-007");
        assert_eq!(v["to"]["repo"], "peregrin");
        assert_eq!(v["committed_source"], true);
        assert_eq!(v["committed_destination"], false);
    }
}
