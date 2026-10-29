//! `myco check [--fix]` — validate the ledger and repair drift.
//!
//! Checks: strict parses (miette-rendered spans), `meta.toml`
//! counters ≥ max id per kind, duplicate ids across the ledger,
//! dangling `blocks:` refs, kind/dir filename mismatches, `[x]`/
//! `status:done` items (the rule: done items are deleted — git
//! keeps history), title/frontmatter id mismatches (a file whose
//! title or H1 still carries its old `ADR-0008:` number), and
//! non-canonical emit drift.
//!
//! `--fix` applies the safe repairs — strip done items, rename
//! misplaced files, renumber title-mismatched items (repointing
//! index links, `blocks:`/`supersedes` refs and handoffs),
//! reconcile counters, re-emit canonical — then re-analyzes;
//! without it every finding is just reported.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use camino::Utf8PathBuf;
use myco_core::{Item, ItemId, ItemKind, ProjectMeta};
use myco_store_md::{self, frontmatter_id, ParseError};
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
    /// `unexpected-file`, `emit-drift`, `orphan-handoff`,
    /// `id-title-mismatch`.
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

/// `<KIND>-<digits>` at the start of a title — `ADR-0008: x`,
/// `T-003 y`, `PL-001 — z`. The id token must end at a boundary
/// (`:`, whitespace, a dash) or the string.
fn leading_id(text: &str) -> Option<ItemId> {
    let end = text
        .find(|c: char| c.is_whitespace() || matches!(c, ':' | '—' | '–'))
        .unwrap_or(text.len());
    text[..end].parse::<ItemId>().ok()
}

/// First heading's text in a markdown body — any depth, the H1
/// convention (`# ADR-0008: …`) or a `##` that opens the file.
fn body_heading(body: &str) -> Option<String> {
    for line in body.lines() {
        let t = line.trim_start();
        if t.starts_with('#') {
            let text = t.trim_start_matches('#').trim();
            if !text.is_empty() {
                return Some(text.to_string());
            }
        }
    }
    None
}

/// The id a file claims for itself in prose — frontmatter `title`
/// and body first-heading both count (`ADR-0008: …`). `One` when
/// they agree (or only one speaks), `Conflict` when they name two
/// different ids of the same kind, `None` when neither does.
enum Claim {
    None,
    One(ItemId),
    Conflict(ItemId, ItemId),
}

fn title_claim(file: &myco_core::ItemFile) -> Claim {
    let kind = file.meta.id.kind();
    let same_kind = |i: &ItemId| i.kind() == kind;
    let title_id = leading_id(&file.meta.title).filter(same_kind);
    let head_id = body_heading(&file.body)
        .and_then(|h| leading_id(&h))
        .filter(same_kind);
    match (title_id, head_id) {
        (Some(a), Some(b)) if a != b => Claim::Conflict(a, b),
        (Some(a), _) => Claim::One(a),
        (None, h) => h.map_or(Claim::None, Claim::One),
    }
}

/// A file wanting renumbering — its title claims a number its
/// frontmatter id doesn't have. Collected during the file walk,
/// resolved once the whole `ids` map exists.
struct Renum {
    /// Current rel path.
    rel: String,
    /// The parsed item file (id still `current`).
    file: myco_core::ItemFile,
    /// Frontmatter id today.
    current: ItemId,
    /// The id the title claims.
    wanted: ItemId,
    /// Canonical path for `current` — where filename/emit-drift
    /// fixes aimed; dropped when the renumber wins.
    old_target: String,
}

/// Rewrite id references inside a `+++` frontmatter block —
/// `blocks = […]` arrays and `superseded_by`/`supersedes` scalars,
/// quoted ids only. `None` when nothing changed.
fn repoint_frontmatter(src: &str, map: &BTreeMap<ItemId, ItemId>) -> Option<String> {
    let mut out = String::with_capacity(src.len());
    let mut in_fm = false;
    let mut changed = false;
    for (i, line) in src.split_inclusive('\n').enumerate() {
        let t = line.trim_end();
        if t == "+++" {
            if i == 0 {
                in_fm = true;
            } else if in_fm {
                in_fm = false;
            }
        }
        if in_fm && t != "+++" {
            let key = t.split('=').next().map(str::trim).unwrap_or("");
            if matches!(key, "blocks" | "superseded_by" | "supersedes") {
                let mut l = line.to_string();
                for (old, new) in map {
                    let (o, n) = (format!("\"{old}\""), format!("\"{new}\""));
                    if l.contains(&o) {
                        l = l.replace(&o, &n);
                        changed = true;
                    }
                }
                out.push_str(&l);
                continue;
            }
        }
        out.push_str(line);
    }
    changed.then_some(out)
}

/// Rewrite `· blocks:<csv>` tokens in TODO/BACKLOG item lines.
/// `None` when nothing changed.
fn repoint_blocks_tokens(src: &str, map: &BTreeMap<ItemId, ItemId>) -> Option<String> {
    let mut out = String::with_capacity(src.len());
    let mut changed = false;
    for line in src.split_inclusive('\n') {
        let (body, nl) = line.strip_suffix('\n').map_or((line, ""), |b| (b, "\n"));
        let mut parts = body.split('·');
        let mut rebuilt = parts.next().unwrap_or("").to_string();
        for tok in parts {
            rebuilt.push('·');
            let t = tok.trim_start();
            if let Some(rest) = t.strip_prefix("blocks:") {
                rebuilt.push_str(&tok[..tok.len() - t.len()]);
                rebuilt.push_str("blocks:");
                for (j, idtok) in rest.split(',').enumerate() {
                    if j > 0 {
                        rebuilt.push(',');
                    }
                    match idtok
                        .trim()
                        .parse::<ItemId>()
                        .ok()
                        .and_then(|id| map.get(&id))
                    {
                        Some(new) => {
                            rebuilt.push_str(&new.to_string());
                            changed = true;
                        }
                        None => rebuilt.push_str(idtok),
                    }
                }
            } else {
                rebuilt.push_str(tok);
            }
        }
        out.push_str(&rebuilt);
        out.push_str(nl);
    }
    changed.then_some(out)
}

/// Rewrite a handoff file to a new item id — `item = "<OLD>"` plus
/// the `# Handoffs — <OLD>` title (both must agree per the spec).
fn repoint_handoff(src: &str, old: &ItemId, new: &ItemId) -> Option<String> {
    let from_item = format!("item = \"{old}\"");
    let to_item = format!("item = \"{new}\"");
    let from_title = format!("# Handoffs — {old}");
    let to_title = format!("# Handoffs — {new}");
    if !src.contains(&from_item) {
        return None;
    }
    Some(
        src.replace(&from_item, &to_item)
            .replace(&from_title, &to_title),
    )
}

/// Apply `f` to a file's pending Write fix at `write_rel` when one
/// exists, otherwise transform the on-disk contents at `read_rel`
/// into a new Write fix at `write_rel`. (The two differ when the
/// file is itself being renamed.) `f` returns `None` when the
/// contents don't change.
fn transform_file(
    wt: &Path,
    fixes: &mut Vec<Fix>,
    read_rel: &str,
    write_rel: &str,
    f: &dyn Fn(&str) -> Option<String>,
) {
    for fix in fixes.iter_mut() {
        if let Fix::Write { rel: r, contents } = fix {
            if r == write_rel {
                if let Some(new) = f(contents) {
                    *contents = new;
                }
                return;
            }
        }
    }
    if let Some(new) = read(wt, read_rel).and_then(|src| f(&src)) {
        fixes.push(Fix::Write {
            rel: write_rel.to_string(),
            contents: new,
        });
    }
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

/// Delete done-item lines and their deeper-indented continuations
/// — a done item's nested children die with it, but a done child
/// leaves its parent's other children alone.
fn strip_done(path: &Path) -> Result<(), String> {
    let src = std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let mut out = String::with_capacity(src.len());
    // indent width of the done line currently being skipped under
    let mut skipping_at: Option<usize> = None;
    for line in src.split_inclusive('\n') {
        let indented = line.starts_with(' ') || line.starts_with('\t');
        let indent = line.len() - line.trim_start().len();
        if let Some(at) = skipping_at {
            if indented && indent > at {
                continue; // continuation/nested line of the done item
            }
            skipping_at = None;
        }
        if is_done_line(line) {
            skipping_at = Some(indent);
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
    let mut candidates: Vec<Renum> = Vec::new();

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
                    // nested children count as items — dup ids and
                    // dangling blocks see the whole tree
                    for it in doc.items().flat_map(|i| i.subtree()) {
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
                    for it in doc.items().flat_map(|i| i.subtree()) {
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
    // FILE_KINDS is all file-backed, so item_dir never yields None
    for (kind, dir_name) in FILE_KINDS
        .into_iter()
        .filter_map(|k| myco_store_md::item_dir(k).map(|d| (k, d)))
    {
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
                                rel: target.clone(),
                                contents: text,
                            });
                        }
                    }
                    // title/H1 carrying a different number than the
                    // frontmatter id — a renumber candidate resolved
                    // once every id is known
                    match title_claim(&file) {
                        Claim::One(w) if w != id => candidates.push(Renum {
                            rel: rel.clone(),
                            file: file.clone(),
                            current: id,
                            wanted: w,
                            old_target: target,
                        }),
                        Claim::Conflict(x, y) => {
                            a.findings.push(Finding::at(
                                &rel,
                                "id-title-mismatch",
                                format!(
                                    "title says {x} but heading says {y} (id is {id}) — renumber manually"
                                ),
                                false,
                            ));
                        }
                        _ => {}
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
                            // parsed under id.kind() → file-backed
                            let dir2 = myco_store_md::item_dir(id.kind()).unwrap_or("");
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
    for (kind, dir_name) in FILE_KINDS
        .into_iter()
        .filter_map(|k| myco_store_md::item_dir(k).map(|d| (k, d)))
    {
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

    // --- id-title-mismatch: renumber, repoint refs -------------------
    // A title's claimed number wins the id when free — the repair
    // for ids that outran the numbers their titles still carry.
    // Collisions (two titles want one number, or an item holds it)
    // are reported and skipped for manual resolution.
    {
        let mut granted: BTreeSet<ItemId> = BTreeSet::new();
        let mut winners: BTreeSet<String> = BTreeSet::new();
        let mut renames: BTreeMap<String, String> = BTreeMap::new();
        let mut id_map: BTreeMap<ItemId, ItemId> = BTreeMap::new();
        for cand in &candidates {
            let w = cand.wanted;
            let blocked: Vec<&String> = ids
                .get(&w)
                .map(|locs| locs.iter().filter(|l| !winners.contains(*l)).collect())
                .unwrap_or_default();
            if granted.contains(&w) || !blocked.is_empty() {
                let why = if granted.contains(&w) {
                    format!("another file takes {w} first")
                } else {
                    format!(
                        "{w} is held by {}",
                        blocked
                            .iter()
                            .map(|s| s.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                };
                a.findings.push(Finding::at(
                    &cand.rel,
                    "id-title-mismatch",
                    format!(
                        "title says {w} but id is {} — {why}; renumber manually",
                        cand.current
                    ),
                    false,
                ));
                continue;
            }
            let mut nf = cand.file.clone();
            nf.meta.id = w;
            // `w` is a file kind and emit can't fail on a file that
            // parsed — the renumber always has a destination
            let to = myco_store_md::item_dir(w.kind())
                .and_then(|d| {
                    myco_store_md::item_filename(&w, &nf.meta.title).map(|f| format!("{d}/{f}"))
                })
                .unwrap_or_else(|| cand.rel.clone());
            let text = myco_store_md::emit_item_file(&nf).unwrap_or_default();
            a.findings.push(Finding::at(
                &cand.rel,
                "id-title-mismatch",
                format!(
                    "title says {w} but id is {} — renumber to {w}",
                    cand.current
                ),
                true,
            ));
            // filename/emit-drift fixes against the old id
            // are superseded by the renumber
            a.fixes.retain(|f| {
                !matches!(f, Fix::Rename { from, .. } if *from == cand.rel)
                    && !matches!(f, Fix::Write { rel: r, .. } if *r == cand.rel || *r == cand.old_target)
            });
            a.fixes.push(Fix::Rename {
                from: cand.rel.clone(),
                to: to.clone(),
            });
            a.fixes.push(Fix::Write {
                rel: to.clone(),
                contents: text,
            });
            granted.insert(w);
            winners.insert(cand.rel.clone());
            renames.insert(cand.rel.clone(), to);
            id_map.insert(cand.current, w);
        }
        // Repoint what referenced the renumbered ids — structured
        // refs only: index paths, `blocks:`/`supersedes` frontmatter
        // and todo tokens, handoff files. Prose `ADR-NNNN` mentions
        // stay (ambiguous — they may mean another repo's item).
        if !id_map.is_empty() {
            transform_file(wt, &mut a.fixes, "index.md", "index.md", &|src: &str| {
                let mut s = src.to_string();
                let mut ch = false;
                for (from, to) in &renames {
                    if s.contains(from.as_str()) {
                        s = s.replace(from.as_str(), to.as_str());
                        ch = true;
                    }
                }
                ch.then_some(s)
            });
            for l in &items {
                let touches = l.item.blocks.iter().any(|b| id_map.contains_key(b))
                    || l.item
                        .superseded_by
                        .is_some_and(|i| id_map.contains_key(&i))
                    || l.item.supersedes.is_some_and(|i| id_map.contains_key(&i));
                if !touches {
                    continue;
                }
                if l.loc == "TODO.md" || l.loc == "BACKLOG.md" {
                    transform_file(wt, &mut a.fixes, &l.loc, &l.loc, &|src: &str| {
                        repoint_blocks_tokens(src, &id_map)
                    });
                } else {
                    // write at the canonical target (it may be a
                    // renumber winner or a filename-fix mover)
                    let to = renames.get(&l.loc).cloned().unwrap_or_else(|| {
                        myco_store_md::item_dir(l.item.id.kind())
                            .and_then(|d| {
                                myco_store_md::item_filename(&l.item.id, &l.item.title)
                                    .map(|f| format!("{d}/{f}"))
                            })
                            .unwrap_or_else(|| l.loc.clone())
                    });
                    transform_file(wt, &mut a.fixes, &l.loc, &to, &|src: &str| {
                        repoint_frontmatter(src, &id_map)
                    });
                }
            }
            for (old, new) in &id_map {
                let hrel = format!("handoffs/{old}.md");
                if let Some(src) = read(wt, &hrel) {
                    let hto = format!("handoffs/{new}.md");
                    a.fixes.push(Fix::Rename {
                        from: hrel,
                        to: hto.clone(),
                    });
                    if let Some(contents) = repoint_handoff(&src, old, new) {
                        a.fixes.push(Fix::Write { rel: hto, contents });
                    }
                }
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
            to.parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .map_err(|e| e.to_string())?;
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
        // a done *child* leaves same-level siblings alone; a done
        // *parent* still takes its nested children with it
        std::fs::write(
            &p,
            "- [ ] parent\n  - [x] done child\n  - [ ] kept sibling\n- [x] gone\n  - [ ] kid\n- [ ] last\n",
        )
        .unwrap();
        strip_done(&p).unwrap();
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            "- [ ] parent\n  - [ ] kept sibling\n- [ ] last\n"
        );
    }

    #[test]
    fn leading_id_parsing() {
        let adr = |n| Some(ItemId::new(ItemKind::Adr, n));
        assert_eq!(leading_id("ADR-0008: web first"), adr(8));
        assert_eq!(leading_id("ADR-0001"), adr(1));
        assert_eq!(
            leading_id("T-012 do a thing"),
            Some(ItemId::new(ItemKind::Task, 12))
        );
        assert_eq!(
            leading_id("PL-001 — phases"),
            Some(ItemId::new(ItemKind::Plan, 1))
        );
        // not a leading id
        for no in [
            "Plain title",
            "0036 — Modular VM", // bare number, no prefix
            "x ADR-0001: mention",
            "ADR-0001x: glued",
            "adr-0001 lowercase",
            "ADR-0000: zero",
        ] {
            assert_eq!(leading_id(no), None, "{no}");
        }
    }

    #[test]
    fn body_heading_finds_first() {
        assert_eq!(
            body_heading("# ADR-0001: t\n\nbody\n").as_deref(),
            Some("ADR-0001: t")
        );
        assert_eq!(body_heading("## ADR-0008\n").as_deref(), Some("ADR-0008"));
        assert_eq!(
            body_heading("prose\n\n## later\n").as_deref(),
            Some("later")
        );
        assert_eq!(body_heading("no headings\n"), None);
    }

    #[test]
    fn title_claim_sources() {
        let mk = |id: u32, title: &str, body: &str| myco_core::ItemFile {
            meta: {
                let mut i = Item::new(ItemId::new(ItemKind::Adr, id), title);
                i.status = myco_core::Status::Open;
                i
            },
            body: body.to_string(),
        };
        // both agree → claim
        assert!(matches!(
            title_claim(&mk(3, "ADR-0001: x", "# ADR-0001: x\n")),
            Claim::One(w) if w == ItemId::new(ItemKind::Adr, 1)
        ));
        // title only (body has no heading / no id)
        assert!(matches!(
            title_claim(&mk(3, "ADR-0001: x", "no heading\n")),
            Claim::One(w) if w.number() == 1
        ));
        // heading only
        assert!(matches!(
            title_claim(&mk(3, "x", "# ADR-0002: y\n")),
            Claim::One(w) if w.number() == 2
        ));
        // disagreement → conflict
        assert!(matches!(
            title_claim(&mk(3, "ADR-0001: x", "# ADR-0002: y\n")),
            Claim::Conflict(a, b) if a.number() == 1 && b.number() == 2
        ));
        // other-kind prefixes don't claim an ADR file
        assert!(matches!(title_claim(&mk(3, "T-001: x", "")), Claim::None));
        // no claim at all
        assert!(matches!(
            title_claim(&mk(3, "plain", "# plain\n")),
            Claim::None
        ));
    }

    #[test]
    fn repoint_frontmatter_refs() {
        let mut map = BTreeMap::new();
        map.insert(ItemId::new(ItemKind::Adr, 3), ItemId::new(ItemKind::Adr, 1));
        let src = "+++\nid = \"ADR-0005\"\ntitle = \"x\"\nstatus = \"open\"\nblocks = [\"ADR-0003\", \"T-001\"]\nsuperseded_by = \"ADR-0003\"\n+++\n\nbody mentions ADR-0003 — untouched\n";
        let out = repoint_frontmatter(src, &map).unwrap();
        assert!(out.contains("blocks = [\"ADR-0001\", \"T-001\"]"), "{out}");
        assert!(out.contains("superseded_by = \"ADR-0001\""), "{out}");
        // body mentions and non-ref lines stay
        assert!(out.contains("body mentions ADR-0003 — untouched"), "{out}");
        assert!(out.contains("id = \"ADR-0005\""), "{out}");
        // no match → None
        assert!(repoint_frontmatter("+++\nid = \"I-001\"\n+++\n", &map).is_none());
    }

    #[test]
    fn repoint_blocks_token_lines() {
        let mut map = BTreeMap::new();
        map.insert(
            ItemId::new(ItemKind::Adr, 16),
            ItemId::new(ItemKind::Adr, 7),
        );
        let src = "# TODO — x\n\n- [ ] T-001 do it · pri:high · blocks:ADR-0016,T-002\n- [ ] T-002 other · blocks:T-001\n";
        let out = repoint_blocks_tokens(src, &map).unwrap();
        assert!(out.contains("blocks:ADR-0007,T-002"), "{out}");
        assert!(out.contains("blocks:T-001"), "{out}");
        assert!(repoint_blocks_tokens("- [ ] T-001 x\n", &map).is_none());
    }

    #[test]
    fn repoint_handoff_file() {
        let src = "+++\nitem = \"ADR-0003\"\n+++\n\n# Handoffs — ADR-0003\n\n## 2026-01-01 · devin\nnote\n";
        let out = repoint_handoff(
            src,
            &ItemId::new(ItemKind::Adr, 3),
            &ItemId::new(ItemKind::Adr, 1),
        )
        .unwrap();
        assert!(out.contains("item = \"ADR-0001\""), "{out}");
        assert!(out.contains("# Handoffs — ADR-0001"), "{out}");
        // unrelated file → None
        assert!(repoint_handoff(
            "+++\nitem = \"T-001\"\n+++\n",
            &ItemId::new(ItemKind::Adr, 3),
            &ItemId::new(ItemKind::Adr, 1),
        )
        .is_none());
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

    // ---- analyze() over tempdir worktrees ----

    fn write(wt: &Path, rel: &str, src: &str) {
        let p = wt.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, src).unwrap();
    }

    fn write_bytes(wt: &Path, rel: &str, src: &[u8]) {
        let p = wt.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, src).unwrap();
    }

    fn codes(a: &Analysis) -> Vec<&'static str> {
        a.findings.iter().map(|f| f.code).collect()
    }

    #[test]
    fn analyze_empty_worktree_reports_everything_missing() {
        let d = tempfile::tempdir().unwrap();
        let a = analyze(d.path());
        assert!(codes(&a).contains(&"missing-file"));
        // every scaffold piece got a repair
        for rel in [
            "meta.toml",
            "TODO.md",
            "BACKLOG.md",
            "index.md",
            "handoffs/.gitkeep",
        ] {
            assert!(
                a.fixes
                    .iter()
                    .any(|f| matches!(f, Fix::Write { rel: r, .. } if r == rel)),
                "no write fix for {rel}"
            );
        }
        assert!(a
            .findings
            .iter()
            .any(|f| f.msg.contains("meta.toml missing")));
    }

    #[test]
    fn analyze_meta_parse_error_is_unfixable() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "meta.toml", "name = [unclosed\n");
        let a = analyze(d.path());
        let f = a
            .findings
            .iter()
            .find(|f| f.file == "meta.toml" && f.code == "parse")
            .expect("parse finding");
        assert!(!f.fixable);
        assert!(f.rendered.is_some());
        // meta_ok false → no counter reconciliation, no meta write
        assert!(!a
            .fixes
            .iter()
            .any(|f| matches!(f, Fix::Write { rel, .. } if rel == "meta.toml")));
    }

    #[test]
    fn analyze_drift_done_and_stray_files() {
        let d = tempfile::tempdir().unwrap();
        let wt = d.path();
        // canonical meta via emit — no drift there
        let meta = ProjectMeta::new("x");
        write(wt, "meta.toml", &myco_store_md::emit_meta(&meta).unwrap());
        // a done line → done-item finding + strip fix (the line
        // itself isn't strict-parseable — the raw scan finds it)
        let todo = myco_store_md::emit_todo(&myco_core::TodoDoc::new("x")).unwrap();
        write(wt, "TODO.md", &format!("{todo}- [x] T-009 done\n"));
        // parseable but uncanonical (token order) → emit-drift;
        // plus a done line → done-item finding
        let mut bdoc = myco_core::BacklogDoc::default();
        bdoc.sections.push(myco_core::Section {
            name: Some("s".into()),
            items: vec![Item::new(ItemId::new(ItemKind::Task, 1), "x")
                .status(myco_core::Status::Deferred)
                .priority(myco_core::Priority::High)],
        });
        let canon = myco_store_md::emit_backlog(&bdoc).unwrap();
        let line = canon.lines().find(|l| l.starts_with("- ")).unwrap();
        // `status:deferred` is implicit in BACKLOG — writing it
        // explicitly parses to the same doc but emits differently
        let drifted = canon.replace(line, &format!("{line} · status:deferred"));
        write(wt, "BACKLOG.md", &drifted);
        write(wt, "index.md", "# x\n");
        // item dir: a stray non-md, a non-utf8 file, a parse
        // failure, and a file that belongs to a different kind
        write(wt, "adrs/notes.txt", "not an item\n");
        write_bytes(wt, "adrs/noutf8.md", &[0xff, 0xfe, 0x00]);
        write(wt, "adrs/garbage.md", "# Garbage body heading\n");
        // parses cleanly; fm title claims nothing, but the body
        // heading claims ADR-0009 — exercises body_heading via
        // title_claim (a renumber candidate)
        write(
            wt,
            "adrs/0008-plain.md",
            "+++\nid = \"ADR-0008\"\ntitle = \"plain title\"\nstatus = \"open\"\n+++\n\n#\nADR-0009: plain\n",
        );
        write(
            wt,
            "adrs/misplaced.md",
            "+++\nid = \"I-001\"\ntitle = \"real issue\"\nstatus = \"open\"\n+++\n\nbody\n",
        );
        write(wt, "handoffs/.gitkeep", "");
        // a non-utf8 handoff file → read() fails → skipped
        write_bytes(wt, "handoffs/T-001.md", &[0xff, 0x00]);
        let a = analyze(wt);
        let cs = codes(&a);
        assert!(cs.contains(&"emit-drift"), "{cs:?}");
        assert!(cs.contains(&"done-item"), "{cs:?}");
        assert!(cs.contains(&"unexpected-file"), "{cs:?}");
        assert!(cs.contains(&"parse"), "{cs:?}");
        assert!(
            a.findings.iter().any(|f| f.msg.contains("belongs to")),
            "{cs:?}"
        );
        assert!(a.fixes.iter().any(|f| matches!(f, Fix::StripDone { .. })));
        assert!(a
            .fixes
            .iter()
            .any(|f| matches!(f, Fix::Rename { from, .. } if from == "adrs/misplaced.md")));
        // counter reconciliation sees the adopted I-001
        assert!(a.findings.iter().any(|f| f.code == "counter-behind"));
    }

    #[test]
    fn analyze_todo_emit_drift() {
        let d = tempfile::tempdir().unwrap();
        let wt = d.path();
        let meta = ProjectMeta::new("x");
        write(wt, "meta.toml", &myco_store_md::emit_meta(&meta).unwrap());
        // parseable but uncanonical — explicit `status:open` is
        // implicit in TODO.md and gets stripped on re-emit
        write(wt, "TODO.md", "# TODO — x\n\n- [ ] T-001 x · status:open\n");
        write(
            wt,
            "BACKLOG.md",
            &myco_store_md::emit_backlog(&myco_core::BacklogDoc::default()).unwrap(),
        );
        write(wt, "index.md", "# x\n");
        let a = analyze(wt);
        assert!(a.findings.iter().any(|f| f.code == "emit-drift"));
        assert!(a
            .fixes
            .iter()
            .any(|f| matches!(f, Fix::Write { rel, .. } if rel == "TODO.md")));
    }

    #[test]
    fn analyze_unreadable_dirs_are_empty_not_errors() {
        let d = tempfile::tempdir().unwrap();
        let wt = d.path();
        write(wt, "meta.toml", "name = \"x\"\n\n[next]\n");
        write(wt, "TODO.md", "# TODO — x\n");
        write(wt, "BACKLOG.md", "# Backlog\n");
        write(wt, "index.md", "# x\n");
        for dir in ["adrs", "handoffs"] {
            std::fs::create_dir_all(wt.join(dir)).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(wt.join(dir), std::fs::Permissions::from_mode(0o000))
                    .unwrap();
            }
        }
        let a = analyze(wt);
        // unreadable dirs yield no findings and no crash
        #[cfg(unix)]
        for dir in ["adrs", "handoffs"] {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(wt.join(dir), std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        assert!(!a.findings.iter().any(
            |f| f.file == "adrs" || f.file.starts_with("adrs/") && f.code == "unexpected-file"
        ));
    }

    /// One file claiming a new number while another item blocks
    /// the old id — exercises the renumber + repoint machinery.
    #[test]
    fn analyze_renumber_wins_and_repoints_pending_writes() {
        let d = tempfile::tempdir().unwrap();
        let wt = d.path();
        write(wt, "meta.toml", "name = \"x\"\n\n[next]\nadr = 3\n");
        write(wt, "TODO.md", "# TODO — x\n");
        write(wt, "BACKLOG.md", "# Backlog\n");
        write(wt, "index.md", "# x\n[adr](adrs/0003-three.md)\n");
        write(wt, "handoffs/ADR-0003.md", "+++\nitem = \"ADR-0003\"\n+++\n\n# Handoffs — ADR-0003\n\n## 2026-01-01 · devin\nnote\n");
        // title + heading both claim ADR-0001; filename also drifts
        write(
            wt,
            "adrs/0003-three.md",
            "+++\nid = \"ADR-0003\"\ntitle = \"ADR-0001: First\"\nstatus = \"open\"\n+++\n\n# ADR-0001: First\n\nbody\n",
        );
        // non-canonical frontmatter order → emit-drift pending
        // Write; blocks: the renumbered id → repoint must edit the
        // pending write, not the file on disk
        write(
            wt,
            "adrs/0002-two.md",
            "+++\ntitle = \"two\"\nstatus = \"open\"\nblocks = [\"ADR-0003\"]\nid = \"ADR-0002\"\n+++\n\nbody\n",
        );
        let a = analyze(wt);
        assert!(a
            .findings
            .iter()
            .any(|f| f.code == "id-title-mismatch" && f.fixable));
        // the renumber supersedes the filename fix on the same file
        assert!(a
            .fixes
            .iter()
            .any(|f| matches!(f, Fix::Rename { from, to } if from == "adrs/0003-three.md" && to.starts_with("adrs/0001-"))));
        // the pending drift Write for 0002 was repointed in place
        let w = a
            .fixes
            .iter()
            .find_map(|f| match f {
                Fix::Write { rel, contents } if rel == "adrs/0002-two.md" => Some(contents),
                _ => None,
            })
            .expect("write fix for 0002");
        assert!(w.contains("\"ADR-0001\""), "{w}");
        // index.md repoint queued (as a Write — it exists on disk)
        assert!(a
            .fixes
            .iter()
            .any(|f| matches!(f, Fix::Write { rel, .. } if rel == "index.md")));
        // handoff renamed + repointed
        assert!(a.fixes.iter().any(|f| matches!(
            f,
            Fix::Rename { from, to } if from == "handoffs/ADR-0003.md" && to == "handoffs/ADR-0001.md"
        )));
    }

    #[test]
    fn analyze_renumber_collisions_report_manually() {
        let d = tempfile::tempdir().unwrap();
        let wt = d.path();
        write(wt, "meta.toml", "name = \"x\"\n\n[next]\nadr = 4\n");
        write(wt, "TODO.md", "# TODO — x\n");
        write(wt, "BACKLOG.md", "# Backlog\n");
        write(wt, "index.md", "# x\n");
        write(wt, "handoffs/.gitkeep", "");
        // two files claim ADR-0001 — the first wins, the second
        // reports a manual finding
        write(
            wt,
            "adrs/0003-a.md",
            "+++\nid = \"ADR-0003\"\ntitle = \"ADR-0001: A\"\nstatus = \"open\"\n+++\n\nbody\n",
        );
        write(
            wt,
            "adrs/0004-b.md",
            "+++\nid = \"ADR-0004\"\ntitle = \"ADR-0001: B\"\nstatus = \"open\"\n+++\n\nbody\n",
        );
        // ADR-0002 is held — a claim on it can't be granted
        write(
            wt,
            "adrs/0005-c.md",
            "+++\nid = \"ADR-0005\"\ntitle = \"ADR-0002: C\"\nstatus = \"open\"\n+++\n\nbody\n",
        );
        write(
            wt,
            "adrs/0002-held.md",
            "+++\nid = \"ADR-0002\"\ntitle = \"held\"\nstatus = \"open\"\n+++\n\nbody\n",
        );
        let a = analyze(wt);
        let msgs: Vec<&str> = a
            .findings
            .iter()
            .filter(|f| f.code == "id-title-mismatch" && !f.fixable)
            .map(|f| f.msg.as_str())
            .collect();
        assert!(
            msgs.iter().any(|m| m.contains("takes ADR-0001 first")),
            "{msgs:?}"
        );
        assert!(msgs.iter().any(|m| m.contains("is held by")), "{msgs:?}");
    }

    #[test]
    fn run_on_prints_findings_in_human_mode() {
        let d = tempfile::tempdir().unwrap();
        let proj = Project {
            root: d.path().to_path_buf(),
            worktree: d.path().to_path_buf(),
        };
        // human mode on a broken worktree → findings + fixable hint
        assert_eq!(run_on(&proj, false, false, true).unwrap(), 1);
        // --fix repairs the whole missing-scaffold set → clean
        assert_eq!(run_on(&proj, true, false, true).unwrap(), 0);
        // and json output agrees
        assert_eq!(run_on(&proj, false, true, true).unwrap(), 0);
    }

    #[test]
    fn apply_fixes_errors_on_blocked_target() {
        let d = tempfile::tempdir().unwrap();
        let wt = d.path();
        std::fs::write(wt.join("a.md"), "x").unwrap();
        // a regular file where the rename's parent dir must go
        std::fs::write(wt.join("blocker"), "x").unwrap();
        let fixes = vec![Fix::Rename {
            from: "a.md".into(),
            to: "blocker/b.md".into(),
        }];
        assert!(apply_fixes(wt, &fixes).is_err());
    }
}
