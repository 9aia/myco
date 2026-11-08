//! `myco migrate` — bring legacy ledgers to the v2 schema.
//!
//! Two modes, auto-detected:
//!
//! - **`pm/` → `project/`** — a `pm/` dir at the repo root is the
//!   pre-worktree layout. `TODO.md`/`BACKLOG.md` bullets become
//!   T-items, `ROADMAP.md` becomes a `plans/` file, any other
//!   `*.md` becomes an issue; the `pm/` dir is removed and its
//!   deletion committed on the current branch.
//! - **legacy `project/` → v2** — files already on the `project`
//!   branch are normalized in place: id-less items get ids,
//!   `[x]`/done items drop, heading-like lines become sections,
//!   frontmatter-less files in item dirs are adopted (the
//!   filename's number is honored when free — `0001-foo.md` →
//!   `ADR-0001` — collisions take the next unclaimed number with a
//!   warning; synthesized frontmatter + canonical name),
//!   everything re-emitted canonically.
//!
//! Either way, bullet shapes the target doc can't express demote
//! to plain-task text with a warning instead of failing the run —
//! a `status:deferred` in TODO.md folds into the title, a
//! self-block drops, a textless `##` merges into the section
//! before it ([`demote`]).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use myco_core::{BacklogDoc, Item, ItemFile, ItemId, ItemKind, Registry, Section, Status, TodoDoc};
use myco_store::Store;
use myco_store_md::frontmatter_id;
use serde_json::json;

use crate::discover;
use crate::init;
use crate::ops;

/// The file-backed kinds, in processing order — migrate's
/// vocabulary is the prelude (legacy trees predate `[[kinds]]`).
fn file_specs() -> impl Iterator<Item = &'static myco_core::KindSpec> {
    myco_core::kind::prelude_table().file_backed()
}

/// The prelude spec for `kind` — migrate only speaks builtins.
fn pspec(kind: ItemKind) -> Option<&'static myco_core::KindSpec> {
    myco_core::kind::prelude_table().spec_of(kind)
}

/// What a `pm/` file converts into.
enum PmDest {
    Todo,
    Backlog,
    Plan,
    Issue,
}

fn pm_dest(name: &str) -> Option<PmDest> {
    if name.eq_ignore_ascii_case("todo.md") {
        Some(PmDest::Todo)
    } else if name.eq_ignore_ascii_case("backlog.md") {
        Some(PmDest::Backlog)
    } else if name.eq_ignore_ascii_case("roadmap.md") {
        Some(PmDest::Plan)
    } else if name.ends_with(".md") {
        Some(PmDest::Issue)
    } else {
        None
    }
}

/// First `#`-heading text in a markdown body, else a fallback.
fn first_heading(src: &str, fallback: &str) -> String {
    for line in src.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix('#') {
            let title = rest.trim_start_matches('#').trim();
            if !title.is_empty() {
                return title.to_string();
            }
        }
    }
    fallback.to_string()
}

/// A title for an adopted/converted item file — `·` is the line
/// grammar's metadata separator and can't live in a title, so a
/// heading or filename that uses it degrades to `-` rather than
/// failing the emit.
fn adopt_title(src: &str, fallback: &str) -> String {
    first_heading(src, fallback).replace('·', "-")
}

/// Per-run counters for the JSON report.
#[derive(Default)]
struct Stats {
    converted_items: usize,
    ids_allocated: usize,
    dropped_done: usize,
    files_moved: BTreeMap<String, String>,
    unparsed: Vec<String>,
    warnings: Vec<String>,
    /// Ids issued to ledger items this run — a kept source id is
    /// honored only while free; a source naming it again collides
    /// to a fresh alloc with a warning, never a duplicate.
    issued: BTreeSet<ItemId>,
}

/// Filename-number claim table for frontmatter-less adoption —
/// one per `migrate` run, spanning all item dirs. A frontmatter-less
/// `NNNN-slug.md` is *borrowing* its number: adoption honors it when
/// no real item owns it (a `taken` id), and a collision probes
/// upward past numbers other unprocessed adoptees still want
/// (`pending`) so `0001-a` + `0001-b` + `0002-c` land 1, 3, 2 —
/// never a fresh counter id.
#[derive(Default)]
struct AdoptClaims {
    /// Ids owned by real items — frontmatter ids, non-adoptee
    /// filenames, TODO/BACKLOG items, archived files.
    taken: BTreeSet<ItemId>,
    /// Filename numbers adoptees not yet processed still want.
    pending: BTreeMap<ItemId, usize>,
}

impl AdoptClaims {
    /// Claim `desired` for the adoptee now being processed: the
    /// number itself when free, else the next number neither taken
    /// nor wanted by a remaining adoptee. Returns `(id, collided)`.
    fn claim(&mut self, desired: ItemId) -> (ItemId, bool) {
        // desired was registered into pending in the claim pass —
        // or_insert keeps the unreachable unclaimed case honest
        let c = self.pending.entry(desired).or_insert(1);
        *c -= 1;
        if *c == 0 {
            self.pending.remove(&desired);
        }
        if !self.taken.contains(&desired) {
            self.taken.insert(desired);
            return (desired, false);
        }
        let mut n = desired.next();
        while self.taken.contains(&n) || self.pending.contains_key(&n) {
            n = n.next();
        }
        self.taken.insert(n);
        (n, true)
    }

    /// Record an id allocated outside the claim path (e.g.
    /// `alloc_id` for a filename without a number).
    fn reserve(&mut self, id: ItemId) {
        self.taken.insert(id);
    }
}

/// Turn a legacy item into a real one — keep a same-kind id or
/// allocate from the run's [`Registry`]. The store's `alloc_id`
/// can't serve here: it scans `all_ids`, which strict-parses
/// `TODO.md`/`BACKLOG.md` — the very files a project-mode
/// migration is mid-way through normalizing.
fn materialize(
    reg: &mut Registry,
    li: &myco_store_md::LegacyItem,
    kind: ItemKind,
    stats: &mut Stats,
) -> Result<Item, String> {
    let mut item = li.item.clone();
    item.id = match li.id {
        Some(id) if id.kind() == kind => {
            if stats.issued.insert(id) {
                // a kept id must still raise the counter floor —
                // `all_ids` never sees pm/-source ids, so without
                // this a later alloc could reissue it mid-run
                reg.observe(id);
                id
            } else {
                // the source named an id already issued this run —
                // keeping it would write a duplicate (id'd lines
                // must still validate), so the item converts on a
                // fresh number with a warning
                let fresh = reg.alloc(kind);
                stats.issued.insert(fresh);
                stats.ids_allocated += 1;
                stats
                    .warnings
                    .push(format!("{id} already issued — item converted as {fresh}"));
                fresh
            }
        }
        _ => {
            let fresh = reg.alloc(kind);
            stats.issued.insert(fresh);
            stats.ids_allocated += 1;
            fresh
        }
    };
    stats.converted_items += 1;
    Ok(item)
}

/// Legacy sections → v2 `Section`s for `file` (`TODO.md` /
/// `BACKLOG.md`), allocating ids as needed. Shapes the target
/// doc's grammar can't express demote to plain-task text rather
/// than failing the migration — see [`demote`].
fn convert_sections(
    reg: &mut Registry,
    doc: &myco_store_md::LegacyDoc,
    kind: ItemKind,
    file: &'static str,
    stats: &mut Stats,
) -> Result<Vec<Section>, String> {
    let mut out: Vec<Section> = Vec::new();
    for sec in &doc.sections {
        // a `##` heading that flattens to no text (`##`, `## ###`)
        // marks no real boundary — an empty name can't emit, so the
        // section's items merge into the one before it (or the
        // leading unsectioned run when nothing precedes)
        let name = sec.name.as_deref().filter(|n| !n.trim().is_empty());
        if name.is_some() || out.is_empty() {
            out.push(Section {
                name: name.map(String::from),
                items: Vec::new(),
            });
        }
        // a nameless section that isn't first lands its items on
        // the previous one — `out` is non-empty either way
        let target = out.len() - 1;
        for li in &sec.items {
            let mut item = materialize(reg, li, kind, stats)?;
            demote(&mut item, li.id, file, stats);
            out[target].items.push(item);
        }
    }
    Ok(out)
}

/// Demote the parts of a converted item `file`'s grammar can't
/// express to plain text, instead of failing the whole migration:
/// a `status:` the doc forbids folds back into the title (the
/// ` - ` separator unknown `·` tokens already use), and a
/// self-block drops — each with a warning, nothing silent. The
/// id stands: an id'd line keeps its number, only its unemittable
/// *state* demotes. `src_id` is the id the line carried — the
/// item's own number may have moved on a collision.
fn demote(item: &mut Item, src_id: Option<ItemId>, file: &str, stats: &mut Stats) {
    // `status:deferred` has no home in TODO.md — the marker becomes
    // title text and the task lands open like any freeform bullet
    if file == "TODO.md" && item.status == Status::Deferred {
        let tok = "status:deferred";
        item.title = match item.title.is_empty() {
            true => tok.to_string(),
            false => format!("{} - {tok}", item.title),
        };
        item.status = Status::Open;
        stats.warnings.push(format!(
            "{file}: {} — deferred tasks live in BACKLOG.md, folded `{tok}` into the title",
            item.id
        ));
    }
    // a `blocks:` at the line's own source id is meaningless at
    // any address — it drops whether the item kept its number or
    // collided onto a fresh one
    if src_id.is_some_and(|id| item.blocks.remove(&id)) {
        stats
            .warnings
            .push(format!("{file}: {} — self-block dropped", item.id));
    }
}

/// Mode A — `pm/` at the repo root.
fn migrate_pm(
    dir: Option<&Path>,
    root_hint: Option<&Path>,
    no_commit: bool,
) -> Result<(serde_json::Value, i32), String> {
    let root = discover::repo_root_for(dir, root_hint)?;
    let pm = root.join("pm");
    // run() only routes here when pm/ exists — a vanish between
    // the two probes just fails at read_dir below
    debug_assert!(pm.is_dir(), "migrate_pm without pm/ at {}", root.display());
    // ensure the project worktree exists (init when needed) —
    // its lock releases with it; the migration's own RMW takes
    // a fresh one held through the commit
    let ensured = init::ensure(dir, Some(&root), no_commit)?;
    let wt = ensured.worktree.clone();
    let _lock = ops::lock_worktree(&wt)?;
    let store = ops::store_at(&wt, myco_core::kind::prelude_table())?;
    let mut stats = Stats::default();

    let mut todo = store.todo().map_err(|e| e.to_string())?;
    let mut backlog = store.backlog().map_err(|e| e.to_string())?;
    // run-scoped allocator — one persisted reconcile at the end
    // instead of alloc_id's per-call meta rewrite
    let mut meta = store.meta().map_err(|e| e.to_string())?;
    let mut reg = meta.registry();
    reg.observe_all(store.all_ids().map_err(|e| e.to_string())?);
    let mut names: Vec<String> = std::fs::read_dir(&pm)
        .map_err(|e| e.to_string())?
        .flatten()
        .filter(|e| e.path().is_file())
        .filter_map(|e| e.file_name().to_str().map(String::from))
        .collect();
    names.sort();
    let mut leftovers: Vec<String> = Vec::new();
    for name in &names {
        let src = std::fs::read_to_string(pm.join(name)).map_err(|e| e.to_string())?;
        let rel_from = format!("pm/{name}");
        match pm_dest(name) {
            Some(PmDest::Todo) => {
                let (_repo, doc) = myco_store_md::parse_legacy_todo(&src, "migrated");
                stats.dropped_done += doc.dropped;
                todo.sections.extend(convert_sections(
                    &mut reg,
                    &doc,
                    ItemKind::Task,
                    "TODO.md",
                    &mut stats,
                )?);
                stats.files_moved.insert(rel_from, "TODO.md".into());
            }
            Some(PmDest::Backlog) => {
                let doc = myco_store_md::parse_legacy_backlog(&src);
                stats.dropped_done += doc.dropped;
                let mut secs =
                    convert_sections(&mut reg, &doc, ItemKind::Task, "BACKLOG.md", &mut stats)?;
                for s in &mut secs {
                    for i in &mut s.items {
                        i.status = Status::Deferred;
                    }
                }
                backlog.sections.extend(secs);
                stats.files_moved.insert(rel_from, "BACKLOG.md".into());
            }
            Some(PmDest::Plan) => {
                let id = reg.alloc(ItemKind::Plan);
                stats.ids_allocated += 1;
                let title = adopt_title(&src, "Roadmap");
                let file = ItemFile::new(Item::new(id, &title)).body(src);
                store.save_item(&file).map_err(|e| e.to_string())?;
                let rel = format!(
                    "plans/{}",
                    pspec(ItemKind::Plan)
                        .and_then(|s| myco_store_md::item_filename(s, &id, &title))
                        .unwrap_or_default()
                );
                stats.converted_items += 1;
                stats.files_moved.insert(rel_from, rel);
            }
            Some(PmDest::Issue) => {
                let id = reg.alloc(ItemKind::Issue);
                stats.ids_allocated += 1;
                let stem = name.trim_end_matches(".md");
                let title = adopt_title(&src, stem);
                let file = ItemFile::new(Item::new(id, &title)).body(src);
                store.save_item(&file).map_err(|e| e.to_string())?;
                let rel = format!(
                    "issues/{}",
                    pspec(ItemKind::Issue)
                        .and_then(|s| myco_store_md::item_filename(s, &id, &title))
                        .unwrap_or_default()
                );
                stats.converted_items += 1;
                stats.files_moved.insert(rel_from, rel);
            }
            None => leftovers.push(rel_from),
        }
    }
    store.save_todo(&todo).map_err(|e| e.to_string())?;
    store.save_backlog(&backlog).map_err(|e| e.to_string())?;
    meta.next = reg.counters();
    store.save_meta(&meta).map_err(|e| e.to_string())?;

    // remove converted files; the dir only goes when it's empty
    for name in &names {
        if pm_dest(name).is_some() {
            let _ = std::fs::remove_file(pm.join(name));
        }
    }
    let empty = std::fs::read_dir(&pm)
        .map(|mut it| it.next().is_none())
        .unwrap_or(false);
    let pm_removed = if empty {
        std::fs::remove_dir(&pm).is_ok()
    } else {
        false
    };

    let enabled = ops::commits_enabled(no_commit);
    let msg = format!(
        "chore: migrate pm/ → project/ ({} items)",
        stats.converted_items
    );
    // pm/ files are already deleted — a failed commit must not
    // leave them gone-but-uncommitted; restore them on the way out
    let commits = (|| -> Result<(bool, bool), String> {
        Ok((
            ops::autocommit(&wt, &msg, None, enabled)?,
            commit_pm_removal(&root, enabled)?,
        ))
    })();
    let (c1, c2) = match commits {
        Ok(pair) => pair,
        Err(e) => {
            let _ = myco_git::git_out(&root, &["restore", "--source=HEAD", "--", "pm"]);
            return Err(e);
        }
    };
    let committed = c1 || c2;

    let out = json!({
        "mode": "pm",
        "converted_items": stats.converted_items,
        "ids_allocated": stats.ids_allocated,
        "dropped_done": stats.dropped_done,
        "files_moved": stats.files_moved.iter()
            .map(|(from, to)| json!({"from": from, "to": to}))
            .collect::<Vec<_>>(),
        "pm_removed": pm_removed,
        "pm_leftover": leftovers,
        "warnings": stats.warnings,
        "worktree": wt.to_string_lossy(),
        "committed": committed,
    });
    Ok((out, 0))
}

/// `git add -u pm/ && git commit -- pm/` — stage only tracked
/// deletions so untracked leftovers never get swept in.
fn commit_pm_removal(root: &Path, enabled: bool) -> Result<bool, String> {
    if !enabled {
        return Ok(false);
    }
    // `add -u` errors when pm/ holds no tracked files at all —
    // untracked leftovers aren't ours to commit
    if myco_git::git_out(root, &["ls-files", "--", "pm"])
        .map_err(|e| e.to_string())?
        .is_empty()
    {
        return Ok(false);
    }
    myco_git::git_out(root, &["add", "-u", "--", "pm"]).map_err(|e| e.to_string())?;
    let dirty = myco_git::git_out(root, &["status", "--porcelain", "--", "pm"])
        .map_err(|e| e.to_string())?;
    if dirty.is_empty() {
        return Ok(false);
    }
    let msg = ops::commit_msg("chore: migrate pm/ → project/", None);
    myco_git::git_ident(root, &["commit", "-m", &msg, "--", "pm"])
        .map_err(|e| e.to_string())
        .map(|_| true)
}

/// Mode B — normalize files already on the `project` branch.
fn migrate_project(
    dir: Option<&Path>,
    root_hint: Option<&Path>,
    no_commit: bool,
) -> Result<(serde_json::Value, i32), String> {
    // strict discovery, no org-ledger fallback — migrate
    // rewrites *this* repo's ledger; silently landing on the org
    // ledger would rewrite the wrong tree
    let proj = myco_ops::discover::discover(dir, root_hint, crate::ops::CONFIG)?;
    let wt = &proj.worktree;
    let _lock = ops::lock_worktree(wt)?;
    let store = ops::store_at(wt, myco_core::kind::prelude_table())?;
    let mut stats = Stats::default();

    // seed counters from ids we're about to keep so allocation
    // can't collide with them. A fresh default meta takes the
    // worktree's dir name ("project") — the repo dir name is the
    // better guess.
    let mut meta = store.meta().map_err(|e| e.to_string())?;
    if !wt.join("meta.toml").exists() {
        if let Some(n) = proj.root.file_name() {
            meta.name = n.to_string_lossy().into_owned();
        }
    }
    // run-scoped allocator over the tolerant id scan — the store's
    // `all_ids` strict-parses the ledger docs, which fails on
    // exactly the legacy files migrate exists to fix
    let mut reg = meta.registry();
    reg.observe_all(migrate_ids(wt)?);

    // --- TODO.md ---
    if let Some(src) = read_str(wt, "TODO.md") {
        let doc = match myco_store_md::parse_todo(&src) {
            Ok(doc) => doc, // already v2 — re-emit normalizes drift
            Err(_) => {
                let (repo, legacy) = myco_store_md::parse_legacy_todo(&src, &meta.name);
                stats.dropped_done += legacy.dropped;
                let mut doc = TodoDoc::new(if repo.is_empty() { &meta.name } else { &repo });
                doc.sections =
                    convert_sections(&mut reg, &legacy, ItemKind::Task, "TODO.md", &mut stats)?;
                doc
            }
        };
        store.save_todo(&doc).map_err(|e| e.to_string())?;
    }

    // --- BACKLOG.md ---
    if let Some(src) = read_str(wt, "BACKLOG.md") {
        let doc = match myco_store_md::parse_backlog(&src) {
            Ok(doc) => doc,
            Err(_) => {
                let legacy = myco_store_md::parse_legacy_backlog(&src);
                stats.dropped_done += legacy.dropped;
                let mut secs =
                    convert_sections(&mut reg, &legacy, ItemKind::Task, "BACKLOG.md", &mut stats)?;
                for s in &mut secs {
                    for i in &mut s.items {
                        i.status = Status::Deferred;
                    }
                }
                BacklogDoc { sections: secs }
            }
        };
        store.save_backlog(&doc).map_err(|e| e.to_string())?;
    }

    // --- item dirs: normalize filenames + adopt unfrontmattered ---
    //
    // Claim table for adoption: `taken` = ids a real item owns —
    // everything `all_ids` sees plus frontmatter ids (which can
    // outrun the filename view), minus numbers merely *borrowed* by
    // adoptee filenames. `pending` = filename numbers adoptees
    // still want, so a collision probes past them instead of
    // jumping to the counter.
    let mut claims = AdoptClaims {
        taken: migrate_ids(wt)?,
        pending: BTreeMap::new(),
    };
    for spec in file_specs() {
        let dir_name = spec.dir_name();
        let dir = wt.join(&dir_name);
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut owned: BTreeSet<ItemId> = BTreeSet::new();
        let mut desired: Vec<ItemId> = Vec::new();
        for entry in read.flatten().filter(|e| e.path().is_file()) {
            let Some(name) = entry.file_name().to_str().map(String::from) else {
                continue;
            };
            if !name.ends_with(".md") || name.starts_with('.') {
                continue;
            }
            let fname_id = myco_store_md::parse_item_filename(spec, &name);
            match read_str(wt, &format!("{dir_name}/{name}"))
                .as_deref()
                .and_then(frontmatter_id)
            {
                // adoptee — the filename number is a claim to honor
                None => desired.extend(fname_id),
                // real item — it lands on its frontmatter id, so
                // that number is off-limits (the filename number is
                // vacated by the canonical rename, not reserved)
                Some(fm_id) => {
                    owned.insert(fm_id);
                }
            }
        }
        // archived files keep their numbers reserved forever
        if let Ok(read) = std::fs::read_dir(dir.join("archive")) {
            for entry in read.flatten().filter(|e| e.path().is_file()) {
                if let Some(name) = entry.file_name().to_str() {
                    owned.extend(myco_store_md::parse_item_filename(spec, name));
                }
            }
        }
        for d in &desired {
            if !owned.contains(d) {
                claims.taken.remove(d);
            }
            *claims.pending.entry(*d).or_default() += 1;
        }
        claims.taken.extend(owned);
    }

    let mut renames: BTreeMap<String, String> = BTreeMap::new();
    for spec in file_specs() {
        let dir_name = spec.dir_name();
        let dir = wt.join(&dir_name);
        if !dir.is_dir() {
            continue;
        }
        let mut names: Vec<String> = std::fs::read_dir(&dir)
            .map_err(|e| e.to_string())?
            .flatten()
            .filter(|e| e.path().is_file())
            .filter_map(|e| e.file_name().to_str().map(String::from))
            .filter(|n| n.ends_with(".md") && !n.starts_with('.'))
            .collect();
        names.sort();
        for name in names {
            let rel = format!("{dir_name}/{name}");
            let src = match read_str(wt, &rel) {
                Some(s) => s,
                None => continue,
            };
            match adopt_or_normalize(&store, &mut reg, &src, spec, &rel, &mut stats, &mut claims)? {
                Some(new_rel) if new_rel != rel => {
                    renames.insert(rel.clone(), new_rel.clone());
                    stats.files_moved.insert(rel, new_rel);
                }
                _ => {}
            }
        }
    }

    // index.md: repoint links at renamed files (raw passthrough,
    // so a plain path replace is the whole fix)
    if !renames.is_empty() {
        if let Some(mut index) = read_str(wt, "index.md") {
            let orig = index.clone();
            for (from, to) in &renames {
                index = index.replace(from.as_str(), to.as_str());
            }
            if index != orig {
                store.save_index(&index).map_err(|e| e.to_string())?;
            }
        }
    }

    // meta.toml: create if missing + reconcile counters
    {
        reg.observe_all(store.all_ids().map_err(|e| e.to_string())?);
        meta.next = reg.counters();
        store.save_meta(&meta).map_err(|e| e.to_string())?;
    }

    // fill any remaining scaffold gaps (dirs, index, docs)
    let created = ops::scaffold(&store, wt, &meta.name).map_err(|e| e.to_string())?;

    let enabled = ops::commits_enabled(no_commit);
    let msg = format!(
        "chore: migrate project/ → v2 schema ({} items)",
        stats.converted_items
    );
    let committed = ops::autocommit(wt, &msg, None, enabled)?;

    let out = json!({
        "mode": "project",
        "converted_items": stats.converted_items,
        "ids_allocated": stats.ids_allocated,
        "dropped_done": stats.dropped_done,
        "files_moved": stats.files_moved.iter()
            .map(|(from, to)| json!({"from": from, "to": to}))
            .collect::<Vec<_>>(),
        "unparsed": stats.unparsed,
        "warnings": stats.warnings,
        "scaffold_created": created,
        "worktree": wt.to_string_lossy(),
        "committed": committed,
    });
    Ok((out, 0))
}

/// The id scan migrate needs mid-run — `Store::all_ids`
/// strict-parses `TODO.md`/`BACKLOG.md`, which fails on exactly
/// the legacy files a migration is there to fix. Same coverage,
/// tolerant reads: an unparseable ledger doc falls back to the
/// legacy parser (its kept ids still reserve their numbers), and
/// item files reserve by filename with the same filename-vs-
/// frontmatter cross-check `MdStore` does.
fn migrate_ids(wt: &Path) -> Result<BTreeSet<ItemId>, String> {
    let mut ids = BTreeSet::new();
    if let Some(src) = read_str(wt, "TODO.md") {
        match myco_store_md::parse_todo(&src) {
            Ok(doc) => ids.extend(doc.items().flat_map(|i| i.subtree()).map(|i| i.id)),
            Err(_) => {
                let (_, legacy) = myco_store_md::parse_legacy_todo(&src, "");
                ids.extend(legacy.items().filter_map(|li| li.id));
            }
        }
    }
    if let Some(src) = read_str(wt, "BACKLOG.md") {
        match myco_store_md::parse_backlog(&src) {
            Ok(doc) => ids.extend(doc.items().flat_map(|i| i.subtree()).map(|i| i.id)),
            Err(_) => {
                let legacy = myco_store_md::parse_legacy_backlog(&src);
                ids.extend(legacy.items().filter_map(|li| li.id));
            }
        }
    }
    for spec in file_specs() {
        let dir_name = spec.dir_name();
        for archived in [false, true] {
            let rel_dir = match archived {
                false => dir_name.to_string(),
                true => format!("{dir_name}/archive"),
            };
            let Ok(read) = std::fs::read_dir(wt.join(&rel_dir)) else {
                continue;
            };
            for entry in read.flatten().filter(|e| e.path().is_file()) {
                let Some(name) = entry.file_name().to_str().map(String::from) else {
                    continue;
                };
                let Some(fname_id) = myco_store_md::parse_item_filename(spec, &name) else {
                    continue;
                };
                let rel = format!("{rel_dir}/{name}");
                match read_str(wt, &rel).as_deref().and_then(frontmatter_id) {
                    Some(fm_id) if fm_id != fname_id => {
                        return Err(format!(
                            "{rel}: filename says {fname_id} but frontmatter id is {fm_id}"
                        ));
                    }
                    // a frontmatter-less file borrows its filename
                    // number — reserved either way
                    Some(id) => {
                        ids.insert(id);
                    }
                    None => {
                        ids.insert(fname_id);
                    }
                };
            }
        }
    }
    Ok(ids)
}

/// Bring one file in an item dir to spec. Returns its new rel path
/// when it moved (or was adopted), `None` when left as-is.
fn adopt_or_normalize(
    store: &myco_store_md::MdStore,
    reg: &mut Registry,
    src: &str,
    spec: &myco_core::KindSpec,
    rel: &str,
    stats: &mut Stats,
    claims: &mut AdoptClaims,
) -> Result<Option<String>, String> {
    let wt = store.root();
    let kind = spec.id_kind();
    // 1) parses under its own kind → save_item re-emits canonically
    //    and renames the file when the slug drifted
    if let Ok(file) = myco_store_md::parse_item_file(src, spec, rel) {
        let id = file.meta.id;
        store.save_item(&file).map_err(|e| e.to_string())?;
        let canonical = myco_store_md::item_filename(spec, &id, &file.meta.title).map(|f| {
            format!(
                "{}/{}",
                myco_store_md::item_dir(spec).unwrap_or_default(),
                f
            )
        });
        // save_item removes a drifted name only when it can tie it
        // to the id — a name without a number is ours to clean up
        if canonical.as_deref() != Some(rel) {
            let _ = std::fs::remove_file(wt.join(rel));
        }
        return Ok(canonical.filter(|c| *c != rel));
    }
    // 2) frontmatter id of a different kind → move + re-emit
    if let Some(id) = frontmatter_id(src) {
        if id.kind() != kind {
            let moved = myco_core::kind::prelude_table()
                .spec_of(id.kind())
                .and_then(|s2| myco_store_md::parse_item_file(src, s2, rel).ok());
            if let Some(file) = moved {
                let s2 = pspec(id.kind());
                store.save_item(&file).map_err(|e| e.to_string())?;
                let _ = std::fs::remove_file(wt.join(rel));
                let canonical = s2.and_then(|s2| {
                    let d = myco_store_md::item_dir(s2)?;
                    myco_store_md::item_filename(s2, &id, &file.meta.title)
                        .map(|f| format!("{d}/{f}"))
                });
                return Ok(canonical);
            }
            // has frontmatter but can't be parsed under any kind —
            // leave it for `myco check` to report
            stats.unparsed.push(rel.to_string());
            return Ok(None);
        }
        // same kind but unparseable — leave to check
        stats.unparsed.push(rel.to_string());
        return Ok(None);
    }
    // 3) no frontmatter at all → adopt: honor the filename number
    //    when free (legacy ids like `0001-gha-compat.md` encode
    //    real numbering), else probe the next number no other
    //    adoptee wants; number-less names take a fresh alloc
    let name = rel.rsplit('/').next().unwrap_or(rel);
    let id = match myco_store_md::parse_item_filename(spec, name) {
        Some(desired) => {
            let (id, collided) = claims.claim(desired);
            if collided {
                stats
                    .warnings
                    .push(format!("{rel}: {desired} already taken — adopted as {id}"));
            }
            reg.observe(id);
            id
        }
        None => {
            let id = reg.alloc(kind);
            claims.reserve(id);
            id
        }
    };
    stats.ids_allocated += 1;
    stats.converted_items += 1;
    let stem = name.trim_end_matches(".md");
    let title = adopt_title(src, stem);
    let file = ItemFile::new(Item::new(id, &title)).body(src.to_string());
    store.save_item(&file).map_err(|e| e.to_string())?;
    // an already-canonical adoptee lands on its own name — deleting
    // `rel` then would remove the file we just wrote
    let canonical = myco_store_md::item_filename(spec, &id, &title)
        .map(|f| format!("{}/{f}", myco_store_md::item_dir(spec).unwrap_or_default()));
    if canonical.as_deref() != Some(rel) {
        let _ = std::fs::remove_file(wt.join(rel));
    }
    Ok(canonical)
}

fn read_str(wt: &Path, rel: &str) -> Option<String> {
    std::fs::read_to_string(wt.join(rel)).ok()
}

/// The `migrate` verb.
pub fn h(ctx: &crate::registry::Ctx, _a: &crate::registry::Args) -> Result<i32, String> {
    let (dir, root, json_out, no_commit) =
        (ctx.dir.clone(), ctx.root.clone(), ctx.json, ctx.no_commit);
    let repo = discover::repo_root_for(dir.as_deref(), root.as_deref());
    let pm_mode = repo
        .as_ref()
        .map(|r| r.join("pm").is_dir())
        .unwrap_or(false);
    let (out, code) = if pm_mode {
        migrate_pm(dir.as_deref(), root.as_deref(), no_commit)?
    } else {
        migrate_project(dir.as_deref(), root.as_deref(), no_commit)?
    };
    if json_out {
        ops::print_json(&out);
    } else {
        println!(
            "myco migrate ({}): {} items, {} ids allocated, {} done dropped",
            out["mode"], out["converted_items"], out["ids_allocated"], out["dropped_done"]
        );
        for m in out["files_moved"].as_array().into_iter().flatten() {
            println!("  {} → {}", m["from"], m["to"]);
        }
        for f in out["unparsed"].as_array().into_iter().flatten() {
            eprintln!("  unparsed: {f}");
        }
        for w in out["warnings"].as_array().into_iter().flatten() {
            eprintln!("  warning: {}", w.as_str().unwrap_or("?"));
        }
    }
    Ok(code)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn pm_dest_mapping() {
        assert!(matches!(pm_dest("TODO.md"), Some(PmDest::Todo)));
        assert!(matches!(pm_dest("todo.md"), Some(PmDest::Todo)));
        assert!(matches!(pm_dest("BACKLOG.md"), Some(PmDest::Backlog)));
        assert!(matches!(pm_dest("ROADMAP.md"), Some(PmDest::Plan)));
        assert!(matches!(pm_dest("NOTES.md"), Some(PmDest::Issue)));
        assert!(pm_dest("x.txt").is_none());
    }

    #[test]
    fn first_heading_extracts() {
        assert_eq!(first_heading("# Title here\n\nbody", "fb"), "Title here");
        assert_eq!(first_heading("## deep\n", "fb"), "deep");
        assert_eq!(first_heading("no heading\n", "fb"), "fb");
    }

    #[test]
    fn frontmatter_id_probe() {
        assert_eq!(
            frontmatter_id("+++\nid = \"I-2\"\nt = \"x\"\n+++\n"),
            Some(ItemId::new(ItemKind::Issue, 2))
        );
        assert_eq!(frontmatter_id("# heading\n"), None);
    }

    #[test]
    fn adopt_title_strips_metadata_separator() {
        // `·` can't live in an item title — the separator degrades
        // to `-` on headings and filename-stem fallbacks alike
        assert_eq!(adopt_title("# plan · v2\n", "x"), "plan - v2");
        assert_eq!(adopt_title("no heading\n", "fb·x"), "fb-x");
    }

    #[test]
    fn demote_deferred_folds_status_into_title() {
        let mut stats = Stats::default();
        let mut item =
            Item::new(ItemId::new(ItemKind::Task, 3), "park me").status(Status::Deferred);
        demote(&mut item, None, "TODO.md", &mut stats);
        assert_eq!(item.status, Status::Open);
        assert_eq!(item.title, "park me - status:deferred");
        assert_eq!(stats.warnings.len(), 1);

        // an empty title folds to the bare token, no leading ` - `
        let mut bare = Item::new(ItemId::new(ItemKind::Task, 4), "").status(Status::Deferred);
        demote(&mut bare, None, "TODO.md", &mut stats);
        assert_eq!(bare.title, "status:deferred");

        // BACKLOG.md is deferred's home — nothing to demote
        let mut kept = Item::new(ItemId::new(ItemKind::Task, 5), "x").status(Status::Deferred);
        let before = stats.warnings.len();
        demote(&mut kept, None, "BACKLOG.md", &mut stats);
        assert_eq!(kept.status, Status::Deferred);
        assert_eq!(stats.warnings.len(), before);
    }

    #[test]
    fn demote_drops_self_block_only() {
        let mut stats = Stats::default();
        let src = ItemId::new(ItemKind::Task, 1);
        let mut item = Item::new(src, "x")
            .blocks(src)
            .blocks(ItemId::new(ItemKind::Task, 2));
        demote(&mut item, Some(src), "TODO.md", &mut stats);
        assert_eq!(item.blocks.len(), 1);
        assert!(item.blocks.contains(&ItemId::new(ItemKind::Task, 2)));
        assert!(stats.warnings[0].contains("self-block"));

        // a re-idded line still drops the edge at its *source* id —
        // blocks: meant itself, not whoever holds the number now
        let mut moved = Item::new(ItemId::new(ItemKind::Task, 9), "x").blocks(src);
        demote(&mut moved, Some(src), "TODO.md", &mut stats);
        assert!(moved.blocks.is_empty());
    }

    #[test]
    fn materialize_rejects_reissued_ids() {
        // an id is honored while free; a source naming it twice —
        // or after an earlier line's alloc — collides to a fresh
        // number with a warning, never a duplicate
        let mut reg = Registry::default();
        let mut stats = Stats::default();
        let li = |id| myco_store_md::LegacyItem {
            id,
            item: Item::new(ItemId::new(ItemKind::Task, 0), "x"),
        };
        let first = materialize(&mut reg, &li(None), ItemKind::Task, &mut stats).unwrap();
        let dup = materialize(&mut reg, &li(Some(first.id)), ItemKind::Task, &mut stats).unwrap();
        assert_ne!(dup.id, first.id);
        assert!(
            stats.warnings.iter().any(|w| w.contains("already issued")),
            "{:?}",
            stats.warnings
        );
    }

    #[test]
    fn convert_sections_merges_textless_headings() {
        // `##`/`## ###` sections flatten to no name — their items
        // merge into the section before them (an empty name would
        // fail the emit); a nameless first section stays unsectioned
        let li = |t: &str| myco_store_md::LegacyItem {
            id: None,
            item: Item::new(ItemId::new(ItemKind::Task, 0), t),
        };
        let doc = myco_store_md::LegacyDoc {
            sections: vec![
                myco_store_md::LegacySection {
                    name: Some("Real".into()),
                    items: vec![li("a")],
                },
                myco_store_md::LegacySection {
                    name: Some("  ".into()),
                    items: vec![li("b")],
                },
                myco_store_md::LegacySection {
                    name: None,
                    items: vec![li("c")],
                },
            ],
            dropped: 0,
        };
        let mut reg = Registry::default();
        let mut stats = Stats::default();
        let secs = convert_sections(&mut reg, &doc, ItemKind::Task, "TODO.md", &mut stats).unwrap();
        assert_eq!(secs.len(), 1);
        assert_eq!(secs[0].name.as_deref(), Some("Real"));
        assert_eq!(
            secs[0]
                .items
                .iter()
                .map(|i| i.title.as_str())
                .collect::<Vec<_>>(),
            ["a", "b", "c"]
        );
    }
}
