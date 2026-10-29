//! `myco check [--fix]` — validate the ledger and repair drift.
//!
//! Checks: strict parses (miette-rendered spans), `meta.toml`
//! counters ≥ max id per kind, duplicate ids across the ledger,
//! dangling `blocks:` refs, kind/dir filename mismatches, `[x]`/
//! `status:done` items (the rule: done items are deleted — git
//! keeps history), and non-canonical emit drift.
//!
//! `--fix` applies the safe repairs — strip done items, rename
//! misplaced files, reconcile counters, re-emit canonical — then
//! re-analyzes; without it every finding is just reported.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use camino::Utf8PathBuf;
use myco_core::{Item, ItemId, ItemKind, ProjectMeta};
use myco_store_md::{self, ParseError};
use serde_json::json;

use crate::discover::Project;
use crate::ops;

/// A validation finding. `fixable` means `--fix` repairs it.
#[derive(Debug, Clone)]
pub struct Finding {
    /// File the finding points at (worktree-relative).
    pub file: String,
    /// 1-based line (0 when not line-specific).
    pub line: usize,
    /// 1-based column (0 when not column-specific).
    pub col: usize,
    /// Stable machine code — `parse`, `done-item`, `counter-behind`,
    /// `duplicate-id`, `dangling-blocks`, `filename`, `missing-file`,
    /// `unexpected-file`, `emit-drift`, `orphan-handoff`.
    pub code: &'static str,
    /// Human/agent-readable message.
    pub msg: String,
    /// `--fix` repairs this.
    pub fixable: bool,
    /// miette rendering for human output (parse errors only).
    pub rendered: Option<String>,
}

impl Finding {
    fn at(file: &str, code: &'static str, msg: impl Into<String>, fixable: bool) -> Self {
        Finding {
            file: file.to_string(),
            line: 0,
            col: 0,
            code,
            msg: msg.into(),
            fixable,
            rendered: None,
        }
    }

    fn parse(e: &ParseError, fixable: bool) -> Self {
        let msg = e.message().to_string();
        let rendered = format!("{:?}", miette::Report::new(e.clone()));
        Finding {
            file: e.file().to_string(),
            line: e.line(),
            col: e.column(),
            code: "parse",
            msg,
            fixable,
            rendered: Some(rendered),
        }
    }
}

/// A repair `--fix` applies.
#[derive(Debug)]
enum Fix {
    /// Delete `[x]`/`status:done` lines + their continuations.
    StripDone { rel: String },
    /// Move an item file to its canonical location.
    Rename { from: String, to: String },
    /// Write canonical content (missing file, drift, counters).
    Write { rel: String, contents: String },
}

/// One analysis pass over the worktree.
#[derive(Default)]
struct Analysis {
    findings: Vec<Finding>,
    fixes: Vec<Fix>,
}

fn normalize(src: &str) -> String {
    src.replace("\r\n", "\n").replace('\r', "\n")
}

fn read(wt: &Path, rel: &str) -> Option<String> {
    std::fs::read_to_string(wt.join(rel)).ok()
}

/// `id = "…"` pulled out of a `+++` frontmatter block — the cheap
/// kind-mismatch probe when the strict parse already failed.
fn frontmatter_id(src: &str) -> Option<ItemId> {
    let mut lines = src.lines();
    if lines.next()?.trim() != "+++" {
        return None;
    }
    for l in lines {
        let t = l.trim();
        if t == "+++" {
            break;
        }
        if let Some(rest) = t.strip_prefix("id") {
            if let Some(v) = rest.trim_start().strip_prefix('=') {
                if let Ok(id) = v.trim().trim_matches('"').parse::<ItemId>() {
                    return Some(id);
                }
            }
        }
    }
    None
}

/// `file/kind` pairs — which file-backed kinds live in which dirs.
const FILE_KINDS: [ItemKind; 4] = [
    ItemKind::Issue,
    ItemKind::Proposal,
    ItemKind::Adr,
    ItemKind::Plan,
];

/// Is `line` a done item? `[x]`/`[X]` checkbox, or a `· status:done`
/// metadata token.
fn is_done_line(line: &str) -> bool {
    let t = line.trim_start();
    if ["- [x]", "- [X]", "* [x]", "* [X]", "+ [x]", "+ [X]"]
        .iter()
        .any(|m| t.starts_with(m))
    {
        return true;
    }
    t.split('·').skip(1).any(|p| p.trim() == "status:done")
}

/// Delete done-item lines and their indented continuations.
fn strip_done(path: &Path) -> Result<(), String> {
    let src = std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let mut out = String::with_capacity(src.len());
    let mut skipping = false;
    for line in src.split_inclusive('\n') {
        if skipping {
            if line.starts_with(' ') || line.starts_with('\t') {
                continue; // continuation/nested line of the done item
            }
            skipping = false;
        }
        if is_done_line(line) {
            skipping = true;
            continue;
        }
        out.push_str(line);
    }
    std::fs::write(path, out).map_err(|e| format!("write {}: {e}", path.display()))
}

/// One item anywhere in the ledger, with where it lives for
/// diagnostics.
struct Located {
    item: Item,
    loc: String,
}

fn analyze(wt: &Path) -> Analysis {
    let mut a = Analysis::default();
    let mut items: Vec<Located> = Vec::new();
    let mut ids: BTreeMap<ItemId, Vec<String>> = BTreeMap::new();

    let mut meta = ProjectMeta::new(
        wt.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "project".into()),
    );
    let mut meta_ok = true;
    let mut meta_dirty = false; // missing or drift — needs a write

    // --- meta.toml -------------------------------------------------
    match read(wt, "meta.toml") {
        None => {
            a.findings.push(Finding::at(
                "meta.toml",
                "missing-file",
                "meta.toml missing — `myco init` creates it",
                true,
            ));
            meta_dirty = true;
        }
        Some(src) => match myco_store_md::parse_meta(&src) {
            Ok(m) => {
                meta = m;
                if myco_store_md::emit_meta(&meta)
                    .map(|e| normalize(&src) != e)
                    .unwrap_or(true)
                {
                    a.findings.push(Finding::at(
                        "meta.toml",
                        "emit-drift",
                        "meta.toml isn't canonical — re-emit",
                        true,
                    ));
                    meta_dirty = true;
                }
            }
            Err(e) => {
                a.findings.push(Finding::parse(&e, false));
                meta_ok = false;
            }
        },
    }

    // --- TODO.md ---------------------------------------------------
    match read(wt, "TODO.md") {
        None => {
            a.findings.push(Finding::at(
                "TODO.md",
                "missing-file",
                "TODO.md missing — `myco init` creates it",
                true,
            ));
            let doc = myco_core::TodoDoc::new(&meta.name);
            if let Ok(text) = myco_store_md::emit_todo(&doc) {
                a.fixes.push(Fix::Write {
                    rel: "TODO.md".into(),
                    contents: text,
                });
            }
        }
        Some(src) => {
            let done: Vec<(usize, &str)> = src
                .lines()
                .enumerate()
                .filter(|(_, l)| is_done_line(l))
                .collect();
            if !done.is_empty() {
                a.findings.push(Finding {
                    file: "TODO.md".into(),
                    line: done[0].0 + 1,
                    col: 1,
                    code: "done-item",
                    msg: format!(
                        "{} done item(s) — done tasks are removed from TODO.md",
                        done.len()
                    ),
                    fixable: true,
                    rendered: None,
                });
                a.fixes.push(Fix::StripDone {
                    rel: "TODO.md".into(),
                });
            }
            match myco_store_md::parse_todo(&src) {
                Ok(doc) => {
                    for it in doc.items() {
                        ids.entry(it.id).or_default().push("TODO.md".into());
                        items.push(Located {
                            item: it.clone(),
                            loc: "TODO.md".into(),
                        });
                    }
                    if myco_store_md::emit_todo(&doc)
                        .map(|e| normalize(&src) != e)
                        .unwrap_or(true)
                    {
                        a.findings.push(Finding::at(
                            "TODO.md",
                            "emit-drift",
                            "TODO.md isn't canonical — re-emit",
                            true,
                        ));
                        if let Ok(text) = myco_store_md::emit_todo(&doc) {
                            a.fixes.push(Fix::Write {
                                rel: "TODO.md".into(),
                                contents: text,
                            });
                        }
                    }
                }
                Err(e) => a.findings.push(Finding::parse(&e, false)),
            }
        }
    }

    // --- BACKLOG.md ------------------------------------------------
    match read(wt, "BACKLOG.md") {
        None => {
            a.findings.push(Finding::at(
                "BACKLOG.md",
                "missing-file",
                "BACKLOG.md missing — `myco init` creates it",
                true,
            ));
            if let Ok(text) = myco_store_md::emit_backlog(&myco_core::BacklogDoc::default()) {
                a.fixes.push(Fix::Write {
                    rel: "BACKLOG.md".into(),
                    contents: text,
                });
            }
        }
        Some(src) => {
            if src.lines().any(is_done_line) {
                a.findings.push(Finding::at(
                    "BACKLOG.md",
                    "done-item",
                    "done item(s) — done tasks are removed, not parked",
                    true,
                ));
                a.fixes.push(Fix::StripDone {
                    rel: "BACKLOG.md".into(),
                });
            }
            match myco_store_md::parse_backlog(&src) {
                Ok(doc) => {
                    for it in doc.items() {
                        ids.entry(it.id).or_default().push("BACKLOG.md".into());
                        items.push(Located {
                            item: it.clone(),
                            loc: "BACKLOG.md".into(),
                        });
                    }
                    if myco_store_md::emit_backlog(&doc)
                        .map(|e| normalize(&src) != e)
                        .unwrap_or(true)
                    {
                        a.findings.push(Finding::at(
                            "BACKLOG.md",
                            "emit-drift",
                            "BACKLOG.md isn't canonical — re-emit",
                            true,
                        ));
                        if let Ok(text) = myco_store_md::emit_backlog(&doc) {
                            a.fixes.push(Fix::Write {
                                rel: "BACKLOG.md".into(),
                                contents: text,
                            });
                        }
                    }
                }
                Err(e) => a.findings.push(Finding::parse(&e, false)),
            }
        }
    }

    // --- index.md ---------------------------------------------------
    if read(wt, "index.md").is_none() {
        a.findings.push(Finding::at(
            "index.md",
            "missing-file",
            "index.md missing — `myco init` creates it",
            true,
        ));
        a.fixes.push(Fix::Write {
            rel: "index.md".into(),
            contents: ops::index_template(&meta.name),
        });
    }

    // --- item dirs --------------------------------------------------
    for kind in FILE_KINDS {
        let Some(dir_name) = myco_store_md::item_dir(kind) else {
            continue;
        };
        let dir = wt.join(dir_name);
        if !dir.is_dir() {
            a.findings.push(Finding::at(
                dir_name,
                "missing-file",
                format!("{dir_name}/ missing — `myco init` creates it"),
                true,
            ));
            a.fixes.push(Fix::Write {
                rel: format!("{dir_name}/.gitkeep"),
                contents: String::new(),
            });
            continue;
        }
        let mut names: Vec<String> = match std::fs::read_dir(&dir) {
            Ok(rd) => rd
                .flatten()
                .filter(|e| e.path().is_file())
                .filter_map(|e| e.file_name().to_str().map(String::from))
                .collect(),
            Err(_) => Vec::new(),
        };
        names.sort();
        for name in names {
            if name.starts_with('.') {
                continue; // .gitkeep et al
            }
            let rel = format!("{dir_name}/{name}");
            if !name.ends_with(".md") {
                a.findings.push(Finding::at(
                    &rel,
                    "unexpected-file",
                    format!("{rel} isn't an item file — expected *-<NNNN>-<slug>.md"),
                    false,
                ));
                continue;
            }
            let Some(src) = read(wt, &rel) else {
                continue;
            };
            match myco_store_md::parse_item_file(&src, kind, &rel) {
                Ok(file) => {
                    let id = file.meta.id;
                    let canonical = myco_store_md::item_filename(&id, &file.meta.title)
                        .map(|f| format!("{dir_name}/{f}"));
                    let target = canonical.clone().unwrap_or_else(|| rel.clone());
                    if canonical.as_deref() != Some(rel.as_str()) {
                        a.findings.push(Finding::at(
                            &rel,
                            "filename",
                            format!("{rel} should be {target} (kind/id/slug mismatch)"),
                            true,
                        ));
                        a.fixes.push(Fix::Rename {
                            from: rel.clone(),
                            to: target.clone(),
                        });
                    }
                    if myco_store_md::emit_item_file(&file)
                        .map(|e| normalize(&src) != e)
                        .unwrap_or(true)
                    {
                        a.findings.push(Finding::at(
                            &rel,
                            "emit-drift",
                            format!("{rel} isn't canonical — re-emit"),
                            true,
                        ));
                        if let Ok(text) = myco_store_md::emit_item_file(&file) {
                            a.fixes.push(Fix::Write {
                                rel: target,
                                contents: text,
                            });
                        }
                    }
                    ids.entry(id).or_default().push(rel.clone());
                    items.push(Located {
                        item: file.meta,
                        loc: rel,
                    });
                }
                Err(e) => {
                    // a file that's perfectly valid under another
                    // kind is a rename, not a parse error
                    let moved = frontmatter_id(&src)
                        .filter(|id| id.kind() != kind)
                        .and_then(|id| {
                            myco_store_md::parse_item_file(&src, id.kind(), &rel)
                                .ok()
                                .map(|f| (id, f))
                        });
                    match moved {
                        Some((id, file)) => {
                            let Some(dir2) = myco_store_md::item_dir(id.kind()) else {
                                continue;
                            };
                            let to = myco_store_md::item_filename(&id, &file.meta.title)
                                .map(|f| format!("{dir2}/{f}"))
                                .unwrap_or_else(|| rel.clone());
                            a.findings.push(Finding::at(
                                &rel,
                                "filename",
                                format!("{rel} belongs to {} — move to {to}", id.kind().prefix()),
                                true,
                            ));
                            a.fixes.push(Fix::Rename {
                                from: rel.clone(),
                                to,
                            });
                            ids.entry(id).or_default().push(rel.clone());
                            items.push(Located {
                                item: file.meta,
                                loc: rel,
                            });
                        }
                        None => a.findings.push(Finding::parse(&e, false)),
                    }
                }
            }
        }
    }

    // archived item files keep their ids — a handoff for an
    // archived item isn't orphaned (filenames only; cold storage
    // isn't parsed or normalized)
    for kind in FILE_KINDS {
        let Some(dir_name) = myco_store_md::item_dir(kind) else {
            continue;
        };
        let adir = wt.join(dir_name).join("archive");
        let names: Vec<String> = match std::fs::read_dir(&adir) {
            Ok(rd) => rd
                .flatten()
                .filter(|e| e.path().is_file())
                .filter_map(|e| e.file_name().to_str().map(String::from))
                .collect(),
            Err(_) => Vec::new(),
        };
        for name in names {
            if let Some(id) = myco_store_md::parse_item_filename(kind, &name) {
                ids.entry(id)
                    .or_default()
                    .push(format!("{dir_name}/archive/{name}"));
            }
        }
    }

    // --- handoffs/ ---------------------------------------------------
    // one append-only file per item; a live file whose item is
    // gone (done tasks are removed!) goes to handoffs/archive/
    let hdir = wt.join("handoffs");
    if !hdir.is_dir() {
        a.findings.push(Finding::at(
            "handoffs",
            "missing-file",
            "handoffs/ missing — `myco init` creates it",
            true,
        ));
        a.fixes.push(Fix::Write {
            rel: "handoffs/.gitkeep".into(),
            contents: String::new(),
        });
    } else {
        for archived in [false, true] {
            let dir_rel = if archived {
                "handoffs/archive"
            } else {
                "handoffs"
            };
            let dir = wt.join(dir_rel);
            if !dir.is_dir() {
                continue; // archive/ is created on demand
            }
            let mut names: Vec<String> = match std::fs::read_dir(&dir) {
                Ok(rd) => rd
                    .flatten()
                    .filter(|e| e.path().is_file())
                    .filter_map(|e| e.file_name().to_str().map(String::from))
                    .collect(),
                Err(_) => Vec::new(),
            };
            names.sort();
            for name in names {
                if name.starts_with('.') {
                    continue; // .gitkeep et al
                }
                let rel = format!("{dir_rel}/{name}");
                if myco_store_md::parse_handoff_filename(&name).is_none() {
                    a.findings.push(Finding::at(
                        &rel,
                        "unexpected-file",
                        format!("{rel} isn't a handoff file — expected <ID>.md"),
                        false,
                    ));
                    continue;
                }
                let Some(src) = read(wt, &rel) else {
                    continue;
                };
                let fid = match myco_store_md::parse_handoff_file(&src, &rel) {
                    Ok((fid, _)) => fid,
                    Err(e) => {
                        a.findings.push(Finding::parse(&e, false));
                        continue;
                    }
                };
                let canon = myco_store_md::handoff_filename(&fid);
                if !archived && !ids.contains_key(&fid) {
                    // the item is gone — the knowledge stays, cold
                    let to = format!("handoffs/archive/{canon}");
                    a.findings.push(Finding::at(
                        &rel,
                        "orphan-handoff",
                        format!("{fid} no longer exists — move {rel} to {to}"),
                        true,
                    ));
                    a.fixes.push(Fix::Rename {
                        from: rel.clone(),
                        to,
                    });
                } else if name != canon {
                    let to = format!("{dir_rel}/{canon}");
                    a.findings.push(Finding::at(
                        &rel,
                        "filename",
                        format!("{rel} should be {to} (id/frontmatter mismatch)"),
                        true,
                    ));
                    a.fixes.push(Fix::Rename {
                        from: rel.clone(),
                        to,
                    });
                }
            }
        }
    }

    // --- semantic checks --------------------------------------------
    for (id, locs) in &ids {
        if locs.len() > 1 {
            a.findings.push(Finding::at(
                &locs[0],
                "duplicate-id",
                format!("{id} appears in {}", locs.join(", ")),
                false,
            ));
        }
    }
    for l in &items {
        for b in &l.item.blocks {
            if !ids.contains_key(b) {
                a.findings.push(Finding::at(
                    &l.loc,
                    "dangling-blocks",
                    format!("{} blocks {b} which doesn't exist", l.item.id),
                    false,
                ));
            }
        }
    }

    // counters ≥ max id per kind; a single meta.toml write covers
    // missing + drift + reconcile
    if meta_ok {
        let mut reg = meta.registry();
        reg.observe_all(ids.keys().copied());
        let reconciled = reg.counters();
        if reconciled != meta.next {
            let lagging: Vec<String> = ItemKind::ALL
                .into_iter()
                .filter(|k| reconciled.get(*k) != meta.next.get(*k))
                .map(|k| {
                    format!(
                        "{}: {} → {}",
                        k.counter_key(),
                        meta.next.get(k),
                        reconciled.get(k)
                    )
                })
                .collect();
            a.findings.push(Finding::at(
                "meta.toml",
                "counter-behind",
                format!("meta.toml counters behind max ids ({})", lagging.join(", ")),
                true,
            ));
            meta_dirty = true;
        }
        if meta_dirty {
            let mut m = meta.clone();
            m.next = reconciled;
            if let Ok(text) = myco_store_md::emit_meta(&m) {
                a.fixes.push(Fix::Write {
                    rel: "meta.toml".into(),
                    contents: text,
                });
            }
        }
    }

    a
}

fn apply_fixes(wt: &Path, fixes: &[Fix]) -> Result<usize, String> {
    let mut n = 0;
    // order matters: strip → rename → write
    for f in fixes {
        if let Fix::StripDone { rel } = f {
            strip_done(&wt.join(rel))?;
            n += 1;
        }
    }
    for f in fixes {
        if let Fix::Rename { from, to } = f {
            let (from, to) = (wt.join(from), wt.join(to));
            if let Some(p) = to.parent() {
                std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
            }
            std::fs::rename(&from, &to)
                .map_err(|e| format!("rename {} → {}: {e}", from.display(), to.display()))?;
            n += 1;
        }
    }
    for f in fixes {
        if let Fix::Write { rel, contents } = f {
            let path = Utf8PathBuf::from_path_buf(wt.join(rel))
                .map_err(|p| format!("{}: non-utf8 path", p.display()))?;
            myco_store_md::write_atomic(&path, contents).map_err(|e| e.to_string())?;
            n += 1;
        }
    }
    Ok(n)
}

/// The `check` verb. Exit 1 with findings, 0 when clean.
pub fn run(
    dir: Option<PathBuf>,
    root: Option<PathBuf>,
    fix: bool,
    json_out: bool,
    no_commit: bool,
) -> Result<i32, String> {
    let proj = crate::discover::discover(dir.as_deref(), root.as_deref())?;
    run_on(&proj, fix, json_out, no_commit)
}

/// Check on an already-resolved project (migrate reuses it).
pub fn run_on(proj: &Project, fix: bool, json_out: bool, no_commit: bool) -> Result<i32, String> {
    let wt = &proj.worktree;
    let mut analysis = analyze(wt);
    let mut fixes_applied = 0usize;
    if fix {
        // a fix can unlock another (strip done → the file parses →
        // drift shows) — loop until nothing left to repair
        for _ in 0..4 {
            if analysis.fixes.is_empty() {
                break;
            }
            fixes_applied += apply_fixes(wt, &analysis.fixes)?;
            analysis = analyze(wt);
        }
    }
    let remaining: Vec<&Finding> = analysis.findings.iter().collect();
    let fixes_available = analysis.fixes.len();
    if fixes_applied > 0 {
        let msg = format!("chore: check --fix ({fixes_applied} repairs)");
        ops::autocommit(wt, &msg, ops::commits_enabled(no_commit))?;
    }
    let ok = remaining.is_empty();
    if json_out {
        ops::print_json(&json!({
            "ok": ok,
            "worktree": wt.to_string_lossy(),
            "repo_root": proj.root.to_string_lossy(),
            "errors": remaining.iter().map(|f| json!({
                "file": f.file, "line": f.line, "col": f.col,
                "code": f.code, "msg": f.msg, "fixable": f.fixable,
            })).collect::<Vec<_>>(),
            "fixes_available": fixes_available,
            "fixes_applied": fixes_applied,
        }));
    } else {
        for f in &remaining {
            if let Some(rendered) = &f.rendered {
                eprintln!("{rendered}");
            } else {
                eprintln!("{}:{}:{} [{}] {}", f.file, f.line, f.col, f.code, f.msg);
            }
        }
        if fixes_applied > 0 {
            eprintln!("check --fix: applied {fixes_applied} repairs");
        }
        if !ok {
            eprintln!(
                "check: {} finding(s){}",
                remaining.len(),
                if fix || fixes_available == 0 {
                    String::new()
                } else {
                    format!(" — {fixes_available} fixable via `myco check --fix`")
                }
            );
        } else {
            eprintln!("check: clean");
        }
    }
    Ok(if ok { 0 } else { 1 })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn done_line_detection() {
        for yes in [
            "- [x] a",
            "- [X] a",
            "  * [x] a",
            "- [ ] a · status:done",
            "- a · status:done",
            "- [ ] T-1 x · pri:high · status:done",
        ] {
            assert!(is_done_line(yes), "{yes}");
        }
        for no in [
            "- [ ] a",
            "- a",
            "text status:done",
            "- [ ] a · status:doing",
            "- [ ] a ·status:doneextra",
        ] {
            assert!(!is_done_line(no), "{no}");
        }
    }

    #[test]
    fn strip_done_removes_continuations() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("TODO.md");
        std::fs::write(
            &p,
            "- [x] done one\n      continuation\n- [ ] keep\n- [ ] drop · status:done\n  nested\n",
        )
        .unwrap();
        strip_done(&p).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "- [ ] keep\n");
    }

    #[test]
    fn frontmatter_id_probe() {
        let src = "+++\nid = \"P-003\"\ntitle = \"x\"\n+++\n";
        assert_eq!(
            frontmatter_id(src),
            Some(ItemId::new(ItemKind::Proposal, 3))
        );
        assert_eq!(frontmatter_id("no fence\nid = \"I-1\"\n"), None);
        assert_eq!(frontmatter_id("+++\ntitle = \"x\"\n+++\n"), None);
        assert_eq!(frontmatter_id("+++\nid = \"bogus\"\n+++\n"), None);
        // `identifier` must not be mistaken for `id`
        assert_eq!(
            frontmatter_id("+++\nidentifier = \"I-2\"\nid = \"I-1\"\n+++\n"),
            Some(ItemId::new(ItemKind::Issue, 1))
        );
    }
}
