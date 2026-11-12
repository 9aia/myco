//! `myco check [--fix]` — validate the ledger and repair drift.
//!
//! Checks: strict parses (miette-rendered spans), `meta.toml`
//! counters ≥ max id per kind, duplicate ids across the ledger,
//! dangling `blocks:`/`links:` refs, kind/dir filename mismatches, `[x]`/
//! `status:done`/`status:cancelled` items (the rule: terminal
//! items are deleted — git keeps history), title/frontmatter id mismatches (a file whose
//! title or H1 still carries its old `ADR-0008:` number), stale
//! `PREFIX-NNNN` prose references (a mention in a body, title,
//! index or handoff that no live or archived id resolves — code
//! spans/fences and self-references are whitelisted), non-canonical
//! emit drift, and **worktree health**: a `project/` worktree on a
//! detached HEAD, one sitting on a branch that isn't `project`, or
//! a branch that was never pushed — the git-side invariants the
//! audit trail depends on.
//!
//! Two severities (ADR-0002): `error` findings fail the gate,
//! `warning` findings are reported and cost nothing. A stale prose
//! reference is a warning, because doc-storage items are *deleted*
//! when they finish — every handoff and note naming one dangles by
//! design and no fixer can know what the author meant.
//!
//! `--fix` applies the safe repairs — strip done items, rename
//! misplaced files, renumber title-mismatched items (repointing
//! index links, `blocks:`/`supersedes` refs and handoffs),
//! reconcile counters, reattach a detached worktree, re-emit
//! canonical — then re-analyzes; without it every finding is just
//! reported.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use camino::Utf8PathBuf;
use myco_core::{Item, ItemId, ProjectMeta};
use myco_store_md::{self, frontmatter_id, ParseError};
use serde_json::json;

use crate::discover::Project;
use crate::ops;

/// How much a finding costs. `Error` fails the gate; `Warn` is
/// knowledge for the reader and never changes the exit code — the
/// distinction exists because some findings are *by design* and
/// unfixable (a prose `T-NNN` naming an item that was completed and
/// therefore removed: see ADR-0002). A gate that reds on those is a
/// gate everyone learns to ignore.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Reported, exit code unaffected.
    Warn,
    /// Fails `check`.
    Error,
}

impl Severity {
    /// Stable machine token — `warning` / `error`.
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Warn => "warning",
            Severity::Error => "error",
        }
    }
}

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
    /// `duplicate-id`, `dangling-blocks`, `dangling-links`,
    /// `dangling-supersede`, `dangling-ref`, `filename`,
    /// `missing-file`, `unexpected-file`, `emit-drift`,
    /// `orphan-handoff`, `id-title-mismatch`, `unreadable-file`,
    /// `config`, `view-invalid`, `verb-invalid`, `kind-invalid`,
    /// `agents-md`, `worktree-detached`, `worktree-branch`,
    /// `worktree-unpushed`.
    pub code: &'static str,
    /// Human/agent-readable message.
    pub msg: String,
    /// Error (fails the gate) or warning (informational).
    pub severity: Severity,
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
            severity: Severity::Error,
            fixable,
            rendered: None,
        }
    }

    /// A finding that informs without failing the gate.
    fn warn(file: &str, code: &'static str, msg: impl Into<String>) -> Self {
        Finding {
            severity: Severity::Warn,
            ..Finding::at(file, code, msg, false)
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
            severity: Severity::Error,
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
pub(crate) struct Analysis {
    findings: Vec<Finding>,
    fixes: Vec<Fix>,
    /// Every id → the ledger locations claiming it (the
    /// `duplicate-id` input; `myco sync`'s renumber reads it too).
    pub(crate) ids: BTreeMap<ItemId, Vec<String>>,
    /// Every parsed item with its location — the renumber's
    /// title lookup for doc-storage dup ids.
    pub(crate) items: Vec<Located>,
}

fn normalize(src: &str) -> String {
    src.replace("\r\n", "\n").replace('\r', "\n")
}

/// A file read — `Missing` only when the path doesn't exist.
/// Anything else (permissions, non-UTF8) is `Unreadable` so a
/// file that exists but can't be read is never reported missing
/// or scaffolded over by `--fix`.
enum Read {
    Missing,
    Ok(String),
    Unreadable(String),
}

fn read(wt: &Path, rel: &str) -> Read {
    match std::fs::read_to_string(wt.join(rel)) {
        Ok(s) => Read::Ok(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Read::Missing,
        Err(e) => Read::Unreadable(format!("{rel} exists but can't be read: {e}")),
    }
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
            if matches!(key, "blocks" | "links" | "superseded_by" | "supersedes") {
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

/// Rewrite `· blocks:<csv>` / `· links:<csv>` tokens in
/// TODO/BACKLOG item lines. `None` when nothing changed.
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
            let key = if t.starts_with("blocks:") {
                "blocks:"
            } else if t.starts_with("links:") {
                "links:"
            } else {
                ""
            };
            if !key.is_empty() {
                let rest = &t[key.len()..];
                rebuilt.push_str(&tok[..tok.len() - t.len()]);
                rebuilt.push_str(key);
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
    if let Some(new) = match read(wt, read_rel) {
        Read::Ok(src) => f(&src),
        _ => None,
    } {
        fixes.push(Fix::Write {
            rel: write_rel.to_string(),
            contents: new,
        });
    }
}

// ---------- dangling-ref: prose `PREFIX-NNNN` mentions ----------

/// `<KIND>-<digits>` tokens in prose — a hand-rolled scanner (one
/// pass per file, cheaper than a `regex` dep for five prefixes).
/// Returns `(byte offset, id)` pairs; `FOO-123`, `XT-001` and
/// `T-001abc` don't tokenize — the prefix needs a word boundary
/// and the digits a trailing one.
fn id_ref_tokens(src: &str) -> Vec<(usize, ItemId)> {
    fn word(b: u8) -> bool {
        b.is_ascii_alphanumeric() || b == b'_'
    }
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        // longest-prefix first: PL/ADR before P
        let plen = if b[i..].starts_with(b"ADR") {
            3
        } else if b[i..].starts_with(b"PL") {
            2
        } else if matches!(b[i], b'T' | b'I' | b'P') {
            1
        } else {
            0
        };
        if plen == 0
            || (i > 0 && word(b[i - 1]))
            || b.get(i + plen) != Some(&b'-')
            || !b.get(i + plen + 1).is_some_and(u8::is_ascii_digit)
        {
            i += 1;
            continue;
        }
        let mut k = i + plen + 1;
        while k < b.len() && b[k].is_ascii_digit() {
            k += 1;
        }
        if k < b.len() && word(b[k]) {
            i = k; // digits run into a word char — not a token
            continue;
        }
        // parse normalizes width — `T-3` and `T-003` are the same id
        if let Ok(id) = src[i..k].parse::<ItemId>() {
            out.push((i, id));
        }
        i = k;
    }
    out
}

/// Byte offset just past the closing `+++` fence line — the start
/// of an item/handoff file's body. 0 when there's no frontmatter
/// (or it's unclosed — the parse finding covers that case).
fn frontmatter_end(src: &str) -> usize {
    let mut lines = src.split_inclusive('\n');
    let Some(first) = lines.next() else {
        return 0;
    };
    if first.trim_end() != "+++" {
        return 0;
    }
    let mut off = first.len();
    for line in lines {
        off += line.len();
        if line.trim_end() == "+++" {
            return off;
        }
    }
    0
}

/// Per-line "title zones" for TODO.md/BACKLOG.md — the text before
/// the first `·` on each line (metadata tokens like `blocks:` and
/// `note:` live after it and have their own checks), minus the
/// list marker and the item's own leading id (`- [ ] T-001 …` —
/// T-001 is the line's identity, not a reference; a done `[x]`
/// line's id doesn't resolve but is no more a stale ref than the
/// live kind).
fn title_zones(src: &str) -> Vec<(usize, usize)> {
    let mut zones = Vec::new();
    let mut off = 0;
    for line in src.split_inclusive('\n') {
        let head_len = line.find('·').unwrap_or(line.len());
        let head = &line[..head_len];
        let mut t = head.trim_start();
        if let Some(r) = t.strip_prefix(['-', '*', '+']) {
            t = r.trim_start();
        }
        for cb in ["[ ]", "[x]", "[X]"] {
            if let Some(r) = t.strip_prefix(cb) {
                t = r.trim_start();
                break;
            }
        }
        // an item line's first token is its own id — exclude it
        // (t is always a suffix of head, so len-diff is its start)
        let start = match t.split_whitespace().next() {
            Some(tok) if tok.parse::<ItemId>().is_ok() => head_len - t.len() + tok.len(),
            _ => 0,
        };
        let end = head_len.min(start + head[start..].trim_end().len());
        zones.push((off + start, off + end));
        off += line.len();
    }
    zones
}

/// Is `off` inside any `(start, end)` byte range?
fn in_zones(off: usize, zones: &[(usize, usize)]) -> bool {
    zones.iter().any(|(s, e)| off >= *s && off < *e)
}

/// 1-based `(line, col)` for a byte offset — cols count chars.
fn line_col(src: &str, off: usize) -> (usize, usize) {
    let (mut line, mut col) = (1, 1);
    for (i, c) in src.char_indices() {
        if i >= off {
            break;
        }
        if c == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

/// A file queued for the dangling-ref scan — `zones` are the byte
/// ranges worth scanning (a post-frontmatter body, or the title
/// heads of TODO/BACKLOG lines); `self_ids` are the file's own
/// claims (frontmatter id + title/H1 claims) that never flag.
struct ProseRef {
    rel: String,
    src: String,
    zones: Vec<(usize, usize)>,
    self_ids: Vec<ItemId>,
}

/// The ids a file claims for itself — frontmatter id plus any
/// title/H1 claim (a renumber candidate's `ADR-0001:` heading is a
/// claim, not a stale reference).
fn self_ids(file: &myco_core::ItemFile) -> Vec<ItemId> {
    let mut v = vec![file.meta.id];
    match title_claim(file) {
        Claim::One(w) if w != file.meta.id => v.push(w),
        Claim::Conflict(x, y) => {
            v.push(x);
            v.push(y);
        }
        _ => {}
    }
    v
}

/// Is `line` a terminal item? `[x]`/`[X]` checkbox, or a
/// `· status:done`/`· status:cancelled` metadata token — both
/// archive into DONE.md, so neither belongs in TODO.md.
fn is_done_line(line: &str) -> bool {
    let t = line.trim_start();
    if ["- [x]", "- [X]", "* [x]", "* [X]", "+ [x]", "+ [X]"]
        .iter()
        .any(|m| t.starts_with(m))
    {
        return true;
    }
    t.split('·')
        .skip(1)
        .any(|p| matches!(p.trim(), "status:done" | "status:cancelled"))
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
pub(crate) struct Located {
    pub(crate) item: Item,
    pub(crate) loc: String,
}

/// `analyze` over the prelude table — the shape tests and
/// table-free callers want.
#[cfg(test)]
fn analyze(wt: &Path) -> Analysis {
    analyze_with(wt, myco_core::kind::prelude_table())
}

/// `analyze` resolved against `kinds` — `[[kinds]]` (P-008) makes
/// the file-spec vocabulary data: dir scans, filename grammar,
/// and "is this kind declared" all read the table.
pub(crate) fn analyze_with(wt: &Path, kinds: &myco_core::KindTable) -> Analysis {
    let mut a = Analysis::default();
    let mut items: Vec<Located> = Vec::new();
    let mut ids: BTreeMap<ItemId, Vec<String>> = BTreeMap::new();
    let mut candidates: Vec<Renum> = Vec::new();
    // files queued for the dangling-ref prose scan — resolved once
    // every live + archived id is known
    let mut prose: Vec<ProseRef> = Vec::new();

    let mut meta = ProjectMeta::new(
        wt.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "project".into()),
    );
    let mut meta_ok = true;
    let mut meta_dirty = false; // missing or drift — needs a write
                                // a doc that could hold item ids but didn't yield them — parse
                                // failure or unreadable — makes the id set incomplete: checks
                                // that assume every id is known (dangling, orphan, renumber)
                                // must stand down or they'll mint false findings and dup ids
    let mut ids_incomplete = false;

    // --- meta.toml -------------------------------------------------
    match read(wt, "meta.toml") {
        Read::Unreadable(m) => {
            a.findings
                .push(Finding::at("meta.toml", "unreadable-file", m, false));
            meta_ok = false;
        }
        Read::Missing => {
            a.findings.push(Finding::at(
                "meta.toml",
                "missing-file",
                "meta.toml missing — `myco init` creates it",
                true,
            ));
            meta_dirty = true;
        }
        Read::Ok(src) => match myco_store_md::parse_meta(&src) {
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
        Read::Unreadable(m) => {
            a.findings
                .push(Finding::at("TODO.md", "unreadable-file", m, false));
            ids_incomplete = true;
        }
        Read::Missing => {
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
        Read::Ok(src) => {
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
                        "{} terminal item(s) — done/cancelled tasks are removed from TODO.md",
                        done.len()
                    ),
                    severity: Severity::Error,
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
                Err(e) => {
                    a.findings.push(Finding::parse(&e, false));
                    ids_incomplete = true;
                }
            }
            // item titles reference other ids in prose ("see ADR-0008")
            prose.push(ProseRef {
                rel: "TODO.md".into(),
                zones: title_zones(&src),
                src,
                self_ids: Vec::new(),
            });
        }
    }

    // --- BACKLOG.md ------------------------------------------------
    match read(wt, "BACKLOG.md") {
        Read::Unreadable(m) => {
            a.findings
                .push(Finding::at("BACKLOG.md", "unreadable-file", m, false));
            ids_incomplete = true;
        }
        Read::Missing => {
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
        Read::Ok(src) => {
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
                Err(e) => {
                    a.findings.push(Finding::parse(&e, false));
                    ids_incomplete = true;
                }
            }
            prose.push(ProseRef {
                rel: "BACKLOG.md".into(),
                zones: title_zones(&src),
                src,
                self_ids: Vec::new(),
            });
        }
    }

    // --- DONE.md ---------------------------------------------------
    // the terminal-task archive — parses like BACKLOG.md but
    // done/cancelled-only. Ids join the ledger set (dup detection + ref
    // resolution) while items skip the live-item checks — an
    // archived `blocks:` may legitimately dangle.
    match read(wt, "DONE.md") {
        Read::Unreadable(m) => {
            a.findings
                .push(Finding::at("DONE.md", "unreadable-file", m, false));
            ids_incomplete = true;
        }
        Read::Missing => {
            a.findings.push(Finding::at(
                "DONE.md",
                "missing-file",
                "DONE.md missing — `myco init` creates it",
                true,
            ));
            if let Ok(text) = myco_store_md::emit_done(&myco_core::DoneDoc::default()) {
                a.fixes.push(Fix::Write {
                    rel: "DONE.md".into(),
                    contents: text,
                });
            }
        }
        Read::Ok(src) => match myco_store_md::parse_done(&src) {
            Ok(doc) => {
                for it in doc.items().flat_map(|i| i.subtree()) {
                    ids.entry(it.id).or_default().push("DONE.md".into());
                }
                if myco_store_md::emit_done(&doc)
                    .map(|e| normalize(&src) != e)
                    .unwrap_or(true)
                {
                    a.findings.push(Finding::at(
                        "DONE.md",
                        "emit-drift",
                        "DONE.md isn't canonical — re-emit",
                        true,
                    ));
                    if let Ok(text) = myco_store_md::emit_done(&doc) {
                        a.fixes.push(Fix::Write {
                            rel: "DONE.md".into(),
                            contents: text,
                        });
                    }
                }
            }
            Err(e) => {
                a.findings.push(Finding::parse(&e, false));
                ids_incomplete = true;
            }
        },
    }

    // --- index.md ---------------------------------------------------
    match read(wt, "index.md") {
        Read::Unreadable(m) => {
            a.findings
                .push(Finding::at("index.md", "unreadable-file", m, false));
        }
        Read::Missing => {
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
        // verbatim freeform — the whole file is scannable prose
        Read::Ok(src) => prose.push(ProseRef {
            rel: "index.md".into(),
            zones: vec![(0, src.len())],
            src,
            self_ids: Vec::new(),
        }),
    }

    // --- item dirs --------------------------------------------------
    // file_backed is all `dir` storage, so item_dir never yields None
    for spec in kinds.file_backed() {
        let kind = spec.id_kind();
        let dir_name = spec.dir_name();
        let dir = wt.join(&dir_name);
        if !dir.is_dir() {
            a.findings.push(Finding::at(
                &dir_name,
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
            let src = match read(wt, &rel) {
                Read::Ok(s) => s,
                Read::Unreadable(m) => {
                    a.findings
                        .push(Finding::at(&rel, "unreadable-file", m, false));
                    ids_incomplete = true;
                    continue;
                }
                Read::Missing => continue,
            };
            match myco_store_md::parse_item_file(&src, spec, &rel) {
                Ok(file) => {
                    let id = file.meta.id;
                    let canonical = myco_store_md::item_filename(spec, &id, &file.meta.title)
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
                        item: file.meta.clone(),
                        loc: rel.clone(),
                    });
                    // the body (post-frontmatter) is freeform prose
                    let start = frontmatter_end(&src);
                    prose.push(ProseRef {
                        rel,
                        zones: vec![(start, src.len())],
                        self_ids: self_ids(&file),
                        src,
                    });
                }
                Err(e) => {
                    // a file that's perfectly valid under another
                    // kind is a rename, not a parse error
                    let moved = frontmatter_id(&src)
                        .filter(|id| id.kind() != kind)
                        .and_then(|id| {
                            let spec2 = kinds.spec_of(id.kind())?;
                            myco_store_md::parse_item_file(&src, spec2, &rel)
                                .ok()
                                .map(|f| (id, f))
                        });
                    match moved {
                        Some((id, file)) => {
                            // parsed under its own spec → file-backed
                            let Some(spec2) = kinds.spec_of(id.kind()) else {
                                continue;
                            };
                            let dir2 = myco_store_md::item_dir(spec2).unwrap_or_default();
                            let to = myco_store_md::item_filename(spec2, &id, &file.meta.title)
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
                                item: file.meta.clone(),
                                loc: rel.clone(),
                            });
                            let start = frontmatter_end(&src);
                            prose.push(ProseRef {
                                rel,
                                zones: vec![(start, src.len())],
                                self_ids: self_ids(&file),
                                src,
                            });
                        }
                        None => {
                            // `---` YAML frontmatter — hand-authored
                            // files reach for the ubiquitous
                            // convention; `--fix` rewrites the block
                            // as `+++` TOML and re-emits canonically
                            let repaired = myco_store_md::repair_frontmatter(&src).and_then(|rs| {
                                myco_store_md::parse_item_file(&rs, spec, &rel)
                                    .ok()
                                    .and_then(|file| {
                                        myco_store_md::emit_item_file(&file)
                                            .ok()
                                            .map(|text| (file, text, rs))
                                    })
                            });
                            match repaired {
                                Some((file, text, rsrc)) => {
                                    let id = file.meta.id;
                                    let canonical =
                                        myco_store_md::item_filename(spec, &id, &file.meta.title)
                                            .map(|f| format!("{dir_name}/{f}"));
                                    let target = canonical.clone().unwrap_or_else(|| rel.clone());
                                    a.findings.push(Finding::at(
                                        &rel,
                                        "frontmatter",
                                        format!(
                                            "{rel} uses `---` frontmatter — \
                                             `--fix` rewrites it as `+++` TOML"
                                        ),
                                        true,
                                    ));
                                    if canonical.as_deref() != Some(rel.as_str()) {
                                        a.findings.push(Finding::at(
                                            &rel,
                                            "filename",
                                            format!(
                                                "{rel} should be {target} \
                                                 (kind/id/slug mismatch)"
                                            ),
                                            true,
                                        ));
                                        a.fixes.push(Fix::Rename {
                                            from: rel.clone(),
                                            to: target.clone(),
                                        });
                                    }
                                    a.fixes.push(Fix::Write {
                                        rel: target.clone(),
                                        contents: text,
                                    });
                                    ids.entry(id).or_default().push(rel.clone());
                                    items.push(Located {
                                        item: file.meta.clone(),
                                        loc: rel.clone(),
                                    });
                                    let start = frontmatter_end(&rsrc);
                                    prose.push(ProseRef {
                                        rel,
                                        zones: vec![(start, rsrc.len())],
                                        self_ids: self_ids(&file),
                                        src: rsrc,
                                    });
                                }
                                None => {
                                    a.findings.push(Finding::parse(&e, false));
                                    ids_incomplete = true;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // archived item files keep their ids — a handoff for an
    // archived item isn't orphaned (filenames only; cold storage
    // isn't parsed or normalized)
    for spec in kinds.file_backed() {
        let dir_name = spec.dir_name();
        let adir = wt.join(&dir_name).join("archive");
        let names: Vec<String> = match std::fs::read_dir(&adir) {
            Ok(rd) => rd
                .flatten()
                .filter(|e| e.path().is_file())
                .filter_map(|e| e.file_name().to_str().map(String::from))
                .collect(),
            Err(_) => Vec::new(),
        };
        for name in names {
            if let Some(id) = myco_store_md::parse_item_filename(spec, &name) {
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
                let src = match read(wt, &rel) {
                    Read::Ok(s) => s,
                    Read::Unreadable(m) => {
                        a.findings
                            .push(Finding::at(&rel, "unreadable-file", m, false));
                        continue;
                    }
                    Read::Missing => continue,
                };
                let fid = match myco_store_md::parse_handoff_file(&src, &rel) {
                    Ok((fid, _)) => fid,
                    Err(e) => {
                        a.findings.push(Finding::parse(&e, false));
                        continue;
                    }
                };
                let canon = myco_store_md::handoff_filename(&fid);
                if !archived && !ids_incomplete && !ids.contains_key(&fid) {
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
                // archived handoffs are scanned too — a cold file
                // may still reference live ids
                let start = frontmatter_end(&src);
                prose.push(ProseRef {
                    rel,
                    zones: vec![(start, src.len())],
                    self_ids: vec![fid],
                    src,
                });
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
            if !ids_incomplete && !ids.contains_key(b) {
                a.findings.push(Finding::at(
                    &l.loc,
                    "dangling-blocks",
                    format!("{} blocks {b} which doesn't exist", l.item.id),
                    false,
                ));
            }
        }
        // `links:` is a declared ref too — same error class
        // (ADR-0002), its own code so reports name the field.
        if !ids_incomplete {
            for lnk in &l.item.links {
                if !ids.contains_key(lnk) {
                    a.findings.push(Finding::at(
                        &l.loc,
                        "dangling-links",
                        format!("{} links {lnk} which doesn't exist", l.item.id),
                        false,
                    ));
                }
            }
        }
    }
    // `supersedes:`/`superseded_by:` are declared refs too —
    // same error class as dangling-blocks (ADR-0002).
    if !ids_incomplete {
        for l in &items {
            for (field, target) in [
                ("supersedes", l.item.supersedes),
                ("superseded_by", l.item.superseded_by),
            ] {
                let Some(target) = target else { continue };
                if !ids.contains_key(&target) {
                    a.findings.push(Finding::at(
                        &l.loc,
                        "dangling-supersede",
                        format!("{} {field} {target} which doesn't exist", l.item.id),
                        false,
                    ));
                }
            }
        }
    }

    // bare `PREFIX-NNNN` mentions in prose — a stale ref is a
    // WARNING, not an error (ADR-0002): doc-storage items are
    // deleted when done, so prose and handoffs that name them
    // dangle by design and forever. `--fix` can't repair it either
    // (was the ref a renumber, another repo, or history?), so it
    // must never fail the gate. Skipped when the id set is
    // incomplete: a ref may resolve to an id hidden in a doc that
    // failed to parse.
    if !ids_incomplete {
        for p in &prose {
            let code = myco_store_md::code_spans(&p.src);
            for (off, id) in id_ref_tokens(&p.src) {
                if p.self_ids.contains(&id)
                    || !in_zones(off, &p.zones)
                    || in_zones(off, &code)
                    || ids.contains_key(&id)
                {
                    continue;
                }
                let (line, col) = line_col(&p.src, off);
                a.findings.push(Finding {
                    file: p.rel.clone(),
                    line,
                    col,
                    code: "dangling-ref",
                    msg: format!("{id} doesn't exist — stale prose reference"),
                    severity: Severity::Warn,
                    fixable: false,
                    rendered: None,
                });
            }
        }
    }

    // --- id-title-mismatch: renumber, repoint refs -------------------
    // A title's claimed number wins the id when free — the repair
    // for ids that outran the numbers their titles still carry.
    // Collisions (two titles want one number, or an item holds it)
    // are reported and skipped for manual resolution. When the id
    // set is incomplete, grants are unsafe — a wanted number may
    // live in a doc that failed to parse — so every candidate is
    // reported for manual renumber instead.
    if !ids_incomplete {
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
            let to = kinds
                .spec_of(w.kind())
                .and_then(|s| {
                    let d = myco_store_md::item_dir(s)?;
                    myco_store_md::item_filename(s, &w, &nf.meta.title).map(|f| format!("{d}/{f}"))
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
                    || l.item.links.iter().any(|x| id_map.contains_key(x))
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
                        kinds
                            .spec_of(l.item.id.kind())
                            .and_then(|s| {
                                let d = myco_store_md::item_dir(s)?;
                                myco_store_md::item_filename(s, &l.item.id, &l.item.title)
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
                if let Read::Ok(src) = read(wt, &hrel) {
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
    } else {
        for cand in &candidates {
            a.findings.push(Finding::at(
                &cand.rel,
                "id-title-mismatch",
                format!(
                    "title says {} but id is {} — renumber skipped: \
                     a ledger file is unreadable or unparsable; fix it first",
                    cand.wanted, cand.current
                ),
                false,
            ));
        }
    }

    // counters ≥ max id per kind; a single meta.toml write covers
    // missing + drift + reconcile
    if meta_ok {
        let mut reg = meta.registry();
        reg.observe_all(ids.keys().copied());
        // undeclared-kind items — an id whose prefix no [[kinds]]
        // entry owns can't be allocated, filed, or filtered
        // sanely; declare it or fix the prefix
        for (id, rels) in &ids {
            if kinds.spec_of(id.kind()).is_none() {
                a.findings.push(Finding::at(
                    &rels[0],
                    "unknown-kind",
                    format!(
                        "{id} uses undeclared kind prefix {:?} — add a [[kinds]] entry or fix the id",
                        id.kind().prefix()
                    ),
                    false,
                ));
            }
        }
        let reconciled = reg.counters();
        if reconciled != meta.next {
            let lagging: Vec<String> = kinds
                .iter()
                .map(|s| s.id_kind())
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

    a.ids = ids;
    a.items = items;
    a
}

// ---------- duplicate-id healing (`myco sync`) -------------------
//
// Two writers minting the same id is the one collision git can't
// resolve: the id lives in filenames, frontmatter, handoff names,
// `blocks:`/`links:` refs and prose. The heal rule is "remote
// wins": the item whose location exists on `origin/<branch>`
// keeps the id — inbound refs stay bound to it — and the local
// item moves to a fresh id, its own files/refs rewritten. Doc-
// storage items (TODO/BACKLOG/DONE lines) match their remote
// side by title; anything ambiguous is reported, never guessed.

/// Ledger docs whose items share one file — loc strings, not
/// paths, so "which line is whose" needs the title lookup.
const DOC_LOCS: &[&str] = &["TODO.md", "BACKLOG.md", "DONE.md"];

/// Does `remote:<loc>` hold *this* item? File kinds answer by
/// path existence; doc kinds by a line carrying both the id and
/// the title (two dup items share the id — the title is the only
/// discriminator).
fn remote_holds(wt: &Path, remote: &str, item: &Item, loc: &str) -> bool {
    if DOC_LOCS.contains(&loc) {
        let Ok(src) = myco_ops::vcs::git_out(wt, &["show", &format!("{remote}:{loc}")]) else {
            return false;
        };
        let idtok = item.id.to_string();
        src.lines()
            .any(|l| l.contains(&idtok) && l.contains(&item.title))
    } else {
        // cat-file -e via git_out — a miss is expected here, and
        // the bool `git()` helper would leak `fatal:` on stderr
        myco_ops::vcs::git_out(wt, &["cat-file", "-e", &format!("{remote}:{loc}")]).is_ok()
    }
}

/// Rewrite a doc item's own id token on its line — the first
/// token after the list marker (`- [ ] T-076 …`). `None` unless
/// exactly one line matches (ambiguous dups are reported, never
/// guessed).
fn repoint_doc_line(src: &str, item: &Item, new: &ItemId) -> Option<String> {
    let idtok = item.id.to_string();
    let mut out = String::with_capacity(src.len());
    let mut changed = false;
    let mut matched = 0usize;
    for line in src.split_inclusive('\n') {
        let body = line.strip_suffix('\n').unwrap_or(line);
        let head_end = body.find('·').unwrap_or(body.len());
        let head = &body[..head_end];
        // the item's own id is the first token after `- [ ]` —
        // `t` stays a suffix of `head`, so its offset is exact
        let mut t = head.trim_start();
        if let Some(r) = t.strip_prefix(['-', '*', '+']) {
            t = r.trim_start();
        }
        for cb in ["[ ]", "[x]", "[X]"] {
            if let Some(r) = t.strip_prefix(cb) {
                t = r.trim_start();
                break;
            }
        }
        let own = t
            .strip_prefix(idtok.as_str())
            .is_some_and(|rest| rest.chars().next().is_none_or(char::is_whitespace))
            && head.contains(&item.title);
        if own {
            matched += 1;
            if matched == 1 {
                let pos = head.len() - t.len();
                let mut l = String::with_capacity(line.len() + 8);
                l.push_str(&line[..pos]);
                l.push_str(&new.to_string());
                l.push_str(&line[pos + idtok.len()..]);
                out.push_str(&l);
                changed = true;
                continue;
            }
        }
        out.push_str(line);
    }
    (changed && matched == 1).then_some(out)
}

/// One pending id move — `old` keeps its inbound refs (the
/// remote item); the local item at `loc` gets `new`.
#[derive(Clone)]
struct IdMove {
    old: ItemId,
    new: ItemId,
    item: Item,
    loc: String,
}

/// Every `*.md` file under `dir` (recursive — `archive/` included),
/// worktree-relative, sorted for determinism.
fn md_files_under(wt: &Path, dir: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_string()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(wt.join(&d)) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                if let Some(n) = p.file_name().and_then(|n| n.to_str()) {
                    stack.push(format!("{d}/{n}"));
                }
            } else if p.extension().is_some_and(|x| x == "md") {
                if let Ok(rel) = p.strip_prefix(wt) {
                    out.push(rel.to_string_lossy().replace('\\', "/"));
                }
            }
        }
    }
    out.sort();
    out
}

/// Append the `renumbered <old> → <new>` audit entry to the healed
/// item's handoff file — created fresh when the id move left none.
/// Runs after `apply_fixes`, so a handoff the rename just landed
/// is the file this extends.
fn renumber_handoff(wt: &Path, m: &IdMove) -> Result<(), String> {
    let rel = format!("handoffs/{}.md", m.new);
    let path = wt.join(&rel);
    let mut entries = if path.is_file() {
        let src = std::fs::read_to_string(&path).map_err(|e| format!("read {rel}: {e}"))?;
        let (fid, entries) =
            myco_store_md::parse_handoff_file(&src, &rel).map_err(|e| e.to_string())?;
        if fid != m.new {
            return Err(format!("{rel} belongs to {fid}, not {}", m.new));
        }
        entries
    } else {
        Vec::new()
    };
    entries.push(myco_core::HandoffEntry {
        date: myco_ops::items::today(),
        actor: ops::resolve_actor(Some(wt), None),
        session: None,
        note: format!("renumbered {} → {}", m.old, m.new),
    });
    let text = myco_store_md::emit_handoff_file(&m.new, &entries).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("write {rel}: {e}"))
}

/// Heal `duplicate-id` findings after a sync: remote-side items
/// keep their ids, local-side items move to fresh ones. Returns
/// `(fixes applied, report lines)` — the report names every move
/// plus anything left for manual resolution. `remote` is a rev
/// like `origin/project`. Inbound `blocks:`/`links:`/supersede
/// refs to the old id rebind to the new one across the repo, and
/// each healed item records a `renumbered <old> → <new>` handoff
/// entry; a ref that can't be rewritten safely is left for
/// `check` to report.
pub(crate) fn renumber_duplicates(
    wt: &Path,
    kinds: &myco_core::KindTable,
    remote: &str,
) -> Result<(usize, Vec<String>), String> {
    let analysis = analyze_with(wt, kinds);
    let mut report = Vec::new();
    let meta_src = match read(wt, "meta.toml") {
        Read::Ok(s) => s,
        _ => return Ok((0, report)),
    };
    let meta = myco_store_md::parse_meta(&meta_src).map_err(|e| e.to_string())?;
    let mut reg = meta.registry();
    reg.observe_all(analysis.ids.keys().copied());

    let mut by_id: BTreeMap<ItemId, Vec<&Located>> = BTreeMap::new();
    for l in &analysis.items {
        by_id.entry(l.item.id).or_default().push(l);
    }

    let mut moves: Vec<IdMove> = Vec::new();
    for (id, locs) in &analysis.ids {
        if locs.len() < 2 {
            continue;
        }
        let Some(cands) = by_id.get(id) else { continue };
        let flags: Vec<bool> = cands
            .iter()
            .map(|l| remote_holds(wt, remote, &l.item, &l.loc))
            .collect();
        let n_remote = flags.iter().filter(|f| **f).count();
        if n_remote > 1 {
            report.push(format!(
                "{id}: duplicate exists on {remote} too — resolve manually"
            ));
            continue;
        }
        for (i, cand) in cands.iter().enumerate() {
            // remote wins; with no remote claimant the first
            // candidate (stable item order) keeps the id
            if flags[i] || (n_remote == 0 && i == 0) {
                continue;
            }
            let new = reg.alloc(id.kind());
            moves.push(IdMove {
                old: *id,
                new,
                item: cand.item.clone(),
                loc: cand.loc.clone(),
            });
        }
    }
    if moves.is_empty() {
        return Ok((0, report));
    }

    let mut fixes: Vec<Fix> = Vec::new();
    let mut path_renames: Vec<(String, String)> = Vec::new();
    let mut applied_moves: Vec<IdMove> = Vec::new();
    for m in &moves {
        let hfrom = format!("handoffs/{}.md", m.old);
        let hto = format!("handoffs/{}.md", m.new);
        // the handoff file follows the item only when the remote
        // side doesn't own one already
        let remote_handoff =
            myco_ops::vcs::git_out(wt, &["cat-file", "-e", &format!("{remote}:{hfrom}")]).is_ok();
        if DOC_LOCS.contains(&m.loc.as_str()) {
            if let Read::Ok(src) = read(wt, &m.loc) {
                match repoint_doc_line(&src, &m.item, &m.new) {
                    Some(text) => {
                        fixes.push(Fix::Write {
                            rel: m.loc.clone(),
                            contents: text,
                        });
                        applied_moves.push(m.clone());
                    }
                    None => report.push(format!(
                        "{} → {}: {} — line match ambiguous, resolve manually",
                        m.old, m.new, m.loc
                    )),
                }
            }
        } else {
            let Some(spec) = kinds.spec_of(m.old.kind()) else {
                report.push(format!("{}: no kind spec — resolve manually", m.old));
                continue;
            };
            let Some(dir) = myco_store_md::item_dir(spec) else {
                report.push(format!("{}: doc-storage kind at {}?", m.old, m.loc));
                continue;
            };
            if let Read::Ok(src) = read(wt, &m.loc) {
                match myco_store_md::parse_item_file(&src, spec, &m.loc) {
                    Ok(mut nf) => {
                        nf.meta.id = m.new;
                        match myco_store_md::emit_item_file(&nf) {
                            Ok(text) => {
                                if let Some(fname) =
                                    myco_store_md::item_filename(spec, &m.new, &nf.meta.title)
                                {
                                    let to = format!("{dir}/{fname}");
                                    fixes.push(Fix::Rename {
                                        from: m.loc.clone(),
                                        to: to.clone(),
                                    });
                                    fixes.push(Fix::Write {
                                        rel: to.clone(),
                                        contents: text,
                                    });
                                    path_renames.push((m.loc.clone(), to));
                                    applied_moves.push(m.clone());
                                }
                            }
                            Err(e) => report.push(format!("{}: emit failed: {e}", m.old)),
                        }
                    }
                    Err(e) => report.push(format!("{}: {} doesn't parse: {e}", m.old, m.loc)),
                }
            }
        }
        if !remote_handoff {
            if let Read::Ok(src) = read(wt, &hfrom) {
                if let Some(text) = repoint_handoff(&src, &m.old, &m.new) {
                    fixes.push(Fix::Rename {
                        from: hfrom,
                        to: hto.clone(),
                    });
                    fixes.push(Fix::Write {
                        rel: hto,
                        contents: text,
                    });
                }
            }
        }
        report.push(format!("{} → {} ({})", m.old, m.new, m.loc));
    }
    if fixes.is_empty() {
        return Ok((0, report));
    }

    // inbound refs follow the moved item: every `blocks:`/`links:`
    // (and supersede) pointer at the old id rebinds to the new
    // one, across the doc ledgers and every item file (live +
    // archive). Refs that meant the *remote* item rebind too —
    // remote-wins can't tell them apart; what can't be rewritten
    // safely is left for `check` to report.
    let refmap: BTreeMap<ItemId, ItemId> = applied_moves.iter().map(|m| (m.old, m.new)).collect();
    if !refmap.is_empty() {
        for doc in DOC_LOCS {
            transform_file(wt, &mut fixes, doc, doc, &|src: &str| {
                repoint_blocks_tokens(src, &refmap)
            });
        }
        for spec in kinds.file_backed() {
            for rel in md_files_under(wt, &spec.dir_name()) {
                // a file the move itself renames has its pending
                // Write at the *new* path — transform that
                let write_rel = path_renames
                    .iter()
                    .find(|(from, _)| *from == rel)
                    .map(|(_, to)| to.clone())
                    .unwrap_or_else(|| rel.clone());
                transform_file(wt, &mut fixes, &rel, &write_rel, &|src: &str| {
                    repoint_frontmatter(src, &refmap)
                });
            }
        }
    }

    // index.md links item files by path — repoint moved paths
    for (from, to) in &path_renames {
        transform_file(wt, &mut fixes, "index.md", "index.md", &|src: &str| {
            src.contains(from.as_str())
                .then(|| src.replace(from.as_str(), to.as_str()))
        });
    }
    // counters reconcile — the fresh ids must be reflected in
    // meta.toml before the commit lands
    let mut m = meta.clone();
    m.next = reg.counters();
    if m.next != meta.next {
        if let Ok(text) = myco_store_md::emit_meta(&m) {
            fixes.push(Fix::Write {
                rel: "meta.toml".into(),
                contents: text,
            });
        }
    }
    let applied = apply_fixes(wt, &fixes)?;
    // the audit trail — every healed item records both ids in a
    // `renumbered` handoff entry; failures report, never abort
    // (the id move itself already landed)
    for m in &applied_moves {
        if let Err(e) = renumber_handoff(wt, m) {
            report.push(format!(
                "{} → {}: renumbered but the handoff entry failed: {e}",
                m.old, m.new
            ));
        }
    }
    Ok((applied, report))
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
            if to.exists() {
                return Err(format!(
                    "rename {} → {} would overwrite an existing file — resolve manually",
                    from.display(),
                    to.display()
                ));
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

/// The finding a non-Ok AGENTS.md state produces — shared by
/// `run_on` and [`check_json`]. `None` for `State::Ok`.
fn agents_md_finding(state: crate::agentmd::State) -> Option<Finding> {
    let msg = match state {
        crate::agentmd::State::Missing => {
            "AGENTS.md missing — `check --fix` creates it with the agent-awareness block"
        }
        crate::agentmd::State::Absent => {
            "AGENTS.md lacks the managed agent-awareness block — `check --fix` appends it"
        }
        crate::agentmd::State::Drift => {
            "AGENTS.md agent-awareness block isn't canonical — `check --fix` rewrites it"
        }
        crate::agentmd::State::Ok => return None,
    };
    Some(Finding::at("AGENTS.md", "agents-md", msg, true))
}

/// Worktree health — the invariant the whole design rests on:
/// "the `project` branch history is the audit trail". A ledger
/// sitting on a detached HEAD (or on a branch nobody pushes) has
/// no audit trail at all, and nothing else in `check` looks at
/// git. `--fix` reattaches; the unpushed case is reported only,
/// because pushing is a network opinion `check` must not have.
fn worktree_findings(worktree: &Path) -> Vec<Finding> {
    use myco_ops::discover::BRANCH;

    let mut out = Vec::new();
    // Only a checkout that IS its own worktree root has a branch of
    // its own. A plain `project/` subdirectory (the legacy in-tree
    // layout) rides the enclosing branch by design — `myco migrate`
    // is the answer there, not a finding on every `check`.
    let is_root = myco_ops::vcs::repo_root(worktree)
        .map(|top| top.canonicalize().ok() == worktree.canonicalize().ok())
        .unwrap_or(false);
    if !is_root {
        return out;
    }
    match myco_ops::vcs::is_detached(worktree) {
        // a detached ledger worktree: every commit is unreferenced
        Some(true) => {
            out.push(Finding::at(
                ".",
                "worktree-detached",
                "project worktree is on a detached HEAD — commits land on no branch, so the \
                 audit trail is unreachable; `check --fix` reattaches it to `project`",
                true,
            ));
            return out;
        }
        // not a checkout (plain-dir ledger) — nothing to say
        None => return out,
        Some(false) => {}
    }
    match myco_ops::vcs::worktree_branch(worktree).as_deref() {
        Some(BRANCH) => {}
        Some(other) => out.push(Finding::warn(
            ".",
            "worktree-branch",
            format!(
                "project dir is a checkout on `{other}`, not `{BRANCH}` — ledger commits ride \
                 `{other}` (legacy in-tree layout; `myco migrate` normalizes it)"
            ),
        )),
        None => {}
    }
    // local-only work: the branch exists but origin's copy is behind.
    // Only claimable when git actually tracks it (a `--single-branch`
    // clone leaves `origin/<branch>` stale forever); no tracked ref =
    // never published, or no remote at all — a repo-level choice, not
    // this verb's business.
    if myco_ops::vcs::tracks_branch(worktree, "origin", BRANCH) {
        let remote = format!("refs/remotes/origin/{BRANCH}");
        if let Some(ahead) = myco_ops::vcs::commits_ahead(worktree, &remote) {
            if ahead > 0 {
                out.push(Finding::warn(
                    ".",
                    "worktree-unpushed",
                    format!(
                        "{ahead} commit(s) on `{BRANCH}` not pushed to `origin/{BRANCH}` — the \
                         audit trail exists on this machine only"
                    ),
                ));
            }
        }
        if let Some(behind) = myco_ops::vcs::commits_behind(worktree, &remote) {
            if behind > 0 {
                out.push(Finding::warn(
                    ".",
                    "worktree-behind",
                    format!(
                        "{behind} commit(s) on `origin/{BRANCH}` not in this ledger — mutating \
                         verbs refuse until `myco sync` rebases onto them"
                    ),
                ));
            }
        }
    }
    out
}

/// The read-only `check` payload — the `--json` report shape
/// without running a fix pass. Served by `myco serve` (the
/// verdict rides in `ok`; there's no exit code on the wire).
pub fn check_json(proj: &Project) -> Result<serde_json::Value, String> {
    let mut analysis = analyze_with(&proj.worktree, &repo_kinds(&proj.root));
    analysis.findings.extend(worktree_findings(&proj.worktree));
    let agents_md = crate::agentmd::block_state(&proj.root);
    if let Some(f) = agents_md_finding(agents_md) {
        analysis.findings.push(f);
    }
    let remaining = &analysis.findings;
    let errors = remaining
        .iter()
        .filter(|f| f.severity == Severity::Error)
        .count();
    let warnings = remaining.len() - errors;
    Ok(json!({
        "ok": errors == 0,
        "worktree": proj.worktree.to_string_lossy(),
        "repo_root": proj.root.to_string_lossy(),
        "agents_md": agents_md.as_str(),
        "errors": remaining.iter().map(|f| json!({
            "file": f.file, "line": f.line, "col": f.col,
            "code": f.code, "msg": f.msg, "fixable": f.fixable,
            "severity": f.severity.as_str(),
        })).collect::<Vec<_>>(),
        "warnings": warnings,
        "fixes_available": analysis.fixes.len(),
        "fixes_applied": 0,
    }))
}

/// `[views]` lint — unparseable config or an invalid spec is a
/// finding (unfixable — the repair is editing myco.toml). Only the
/// repo layer is linted: check output must be identical for every
/// user and CI. Shadowing a builtin is legal (warned at use), not
/// a finding.
fn view_findings(root: &std::path::Path) -> Vec<Finding> {
    match myco_ops::view::validate_repo_views(root, ops::CONFIG) {
        Ok(errs) => errs
            .into_iter()
            .map(|(name, e)| {
                Finding::at(
                    "myco.toml",
                    "view-invalid",
                    format!("view \"{name}\": {e}"),
                    false,
                )
            })
            .collect(),
        Err(e) => vec![Finding::at("myco.toml", "config", e, false)],
    }
}

/// `[verbs]` lint — an invalid config verb is a finding
/// (unfixable — the repair is editing myco.toml). Only the repo
/// layer is linted; shadowing a builtin is legal (warned at use).
fn verb_findings(root: &std::path::Path) -> Vec<Finding> {
    match myco_ops::verb::validate_repo_verbs(root, ops::CONFIG, crate::registry::builtin_refs()) {
        Ok(errs) => errs
            .into_iter()
            .map(|(name, e)| {
                Finding::at(
                    "myco.toml",
                    "verb-invalid",
                    format!("verb \"{name}\": {e}"),
                    false,
                )
            })
            .collect(),
        Err(e) => vec![Finding::at("myco.toml", "config", e, false)],
    }
}

/// `[[kinds]]` for a repo — the repo-layer myco.toml (or
/// `$MYCO_CONFIG`), prelude on absence; a broken config resolves
/// to the prelude here and reports through [`kind_findings`].
pub(crate) fn repo_kinds(root: &std::path::Path) -> myco_core::KindTable {
    let path = std::env::var("MYCO_CONFIG")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| root.join("myco.toml"));
    ops::CONFIG
        .load(Some(&path))
        .ok()
        .and_then(|c| c.kind_table().ok())
        .unwrap_or_else(myco_core::KindTable::prelude)
}

/// `[[kinds]]` lint — an invalid kind table is a finding
/// (unfixable — the repair is editing myco.toml). Repo layer
/// only, like `view_findings`/`verb_findings`.
fn kind_findings(root: &std::path::Path) -> Vec<Finding> {
    let path = std::env::var("MYCO_CONFIG")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| root.join("myco.toml"));
    if !path.exists() {
        return Vec::new();
    }
    match ops::CONFIG.load(Some(&path)) {
        Err(_) => Vec::new(), // reported by the config finding
        Ok(cfg) => match cfg.kind_table() {
            Err(e) => vec![Finding::at("myco.toml", "kinds-invalid", e, false)],
            Ok(_) => Vec::new(),
        },
    }
}

/// The `check` verb. Exit 1 on errors, 0 when clean (or
/// warnings-only).
pub fn h(ctx: &crate::registry::Ctx, a: &crate::registry::Args) -> Result<i32, String> {
    let (fix, json_out, no_commit) = (a.flag("fix"), ctx.json, ctx.no_commit);
    let proj = crate::discover::discover(ctx.dir.as_deref(), ctx.root.as_deref())?;
    run_on(&proj, fix, json_out, no_commit)
}

/// Check on an already-resolved project (migrate reuses it).
pub fn run_on(proj: &Project, fix: bool, json_out: bool, no_commit: bool) -> Result<i32, String> {
    let wt = &proj.worktree;
    // --fix writes the worktree — the exclusive lock keeps the
    // repair loop from interleaving with another verb's
    // load→mutate→save→commit. A plain check still takes the
    // shared read lock so it can't report on a mid-verb ledger.
    let _lock = if fix {
        ops::lock_worktree(wt)?
    } else {
        ops::lock_shared(wt)?
    };
    let kinds = repo_kinds(&proj.root);
    // Worktree health first: a detached ledger must be reattached
    // before anything else can be committed (`commit_all` refuses a
    // detached HEAD), so the repair runs ahead of the fix loop and
    // the loop's writes ride the normal `--fix` commit.
    let health = worktree_findings(wt);
    let mut fixes_applied = 0usize;
    if fix && health.iter().any(|f| f.fixable) {
        myco_ops::vcs::attach_branch(wt, myco_ops::discover::BRANCH)
            .map_err(|e| format!("reattach {}: {e}", wt.display()))?;
        fixes_applied += 1;
    }
    let mut analysis = analyze_with(wt, &kinds);
    if fix {
        // a fix can unlock another (strip done → the file parses →
        // drift shows) — loop until nothing left to repair
        for _ in 0..4 {
            if analysis.fixes.is_empty() {
                break;
            }
            fixes_applied += apply_fixes(wt, &analysis.fixes)?;
            analysis = analyze_with(wt, &kinds);
        }
    }
    // config layer: `[views]` specs in the repo/user myco.toml —
    // lint-only (no fixer); a broken spec fails every verb that
    // loads config, so it surfaces here too
    analysis.findings.extend(view_findings(&proj.root));
    analysis.findings.extend(verb_findings(&proj.root));
    analysis.findings.extend(kind_findings(&proj.root));
    // root-side: the managed AGENTS.md block lives on the checked-
    // out branch, not in the worktree — validated/repaired against
    // proj.root separately.
    let mut agents_md = crate::agentmd::block_state(&proj.root);
    let mut root_repaired = false;
    if agents_md != crate::agentmd::State::Ok {
        if fix {
            crate::agentmd::sync(&proj.root)?;
            root_repaired = ops::commit_paths(
                &proj.root,
                "chore: manage AGENTS.md agent-awareness block",
                &["AGENTS.md"],
                None,
                ops::commits_enabled(no_commit),
            )?;
            agents_md = crate::agentmd::block_state(&proj.root);
            // a repair that didn't take must not report success
            if agents_md != crate::agentmd::State::Ok {
                analysis.findings.extend(agents_md_finding(agents_md));
            }
        } else {
            analysis.findings.extend(agents_md_finding(agents_md));
        }
    }

    // health is evaluated last, after this run's own commits have
    // landed — otherwise `--fix` reports the pre-repair push state
    // and hides the commit it just made
    if fixes_applied > 0 {
        let msg = format!("chore: check --fix ({fixes_applied} repairs)");
        ops::autocommit(wt, &msg, None, ops::commits_enabled(no_commit))?;
    }
    analysis.findings.extend(worktree_findings(wt));
    let remaining: Vec<&Finding> = analysis.findings.iter().collect();
    let fixes_available = analysis.fixes.len();
    let repairs = fixes_applied + usize::from(root_repaired);
    // warnings never fail the gate (ADR-0002): they are findings the
    // ledger cannot fix by design, and a gate that reds on them is a
    // gate everyone learns to ignore
    let errors = remaining
        .iter()
        .filter(|f| f.severity == Severity::Error)
        .count();
    let warnings = remaining.len() - errors;
    let ok = errors == 0;
    if json_out {
        ops::print_json(&json!({
            "ok": ok,
            "worktree": wt.to_string_lossy(),
            "repo_root": proj.root.to_string_lossy(),
            "agents_md": agents_md.as_str(),
            "errors": remaining.iter().map(|f| json!({
                "file": f.file, "line": f.line, "col": f.col,
                "code": f.code, "msg": f.msg, "fixable": f.fixable,
                "severity": f.severity.as_str(),
            })).collect::<Vec<_>>(),
            "warnings": warnings,
            "fixes_available": fixes_available,
            "fixes_applied": repairs,
        }));
    } else {
        for f in &remaining {
            if let Some(rendered) = &f.rendered {
                eprintln!("{rendered}");
            } else {
                eprintln!(
                    "{}:{}:{} [{}] {}: {}",
                    f.file,
                    f.line,
                    f.col,
                    f.code,
                    f.severity.as_str(),
                    f.msg
                );
            }
        }
        if repairs > 0 {
            eprintln!("check --fix: applied {repairs} repairs");
        }
        if !ok {
            eprintln!(
                "check: {errors} error(s), {warnings} warning(s){}",
                if fix || fixes_available == 0 {
                    String::new()
                } else {
                    format!(" — {fixes_available} fixable via `myco check --fix`")
                }
            );
        } else if warnings > 0 {
            eprintln!("check: clean ({warnings} warning(s))");
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
    use myco_core::ItemKind;

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
    fn analyze_repairs_yaml_frontmatter() {
        let d = tempfile::tempdir().unwrap();
        let wt = d.path();
        let meta = ProjectMeta::new("x");
        write(wt, "meta.toml", &myco_store_md::emit_meta(&meta).unwrap());
        write(wt, "TODO.md", "# TODO — x\n");
        write(
            wt,
            "BACKLOG.md",
            &myco_store_md::emit_backlog(&myco_core::BacklogDoc::default()).unwrap(),
        );
        write(wt, "index.md", "# x\n");
        // the shape hand-authored files arrive in — `---` fence,
        // `key: value` scalars
        write(
            wt,
            "proposals/proposal-0001-x.md",
            "---\nid: P-001\ntitle: A proposal\nstatus: open\n---\n\n## Thesis\n\nbody\n",
        );
        let a = analyze(wt);
        assert!(
            a.findings
                .iter()
                .any(|f| f.code == "frontmatter" && f.fixable),
            "{:?}",
            codes(&a)
        );
        // no unfixable parse finding for the repaired file
        assert!(!a
            .findings
            .iter()
            .any(|f| f.code == "parse" && f.file.contains("proposal-0001")));
        // the canonical slug differs → rename, then write `+++`
        assert!(a.fixes.iter().any(|f| matches!(
            f,
            Fix::Rename { from, to }
                if from == "proposals/proposal-0001-x.md"
                    && to == "proposals/proposal-0001-a-proposal.md"
        )));
        assert!(a.fixes.iter().any(|f| matches!(
            f,
            Fix::Write { rel, contents }
                if rel == "proposals/proposal-0001-a-proposal.md"
                    && contents.starts_with("+++\n")
        )));
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

    #[test]
    fn apply_fixes_refuses_rename_over_existing() {
        let d = tempfile::tempdir().unwrap();
        let wt = d.path();
        std::fs::write(wt.join("a.md"), "moved").unwrap();
        std::fs::write(wt.join("b.md"), "keep me").unwrap();
        let fixes = vec![Fix::Rename {
            from: "a.md".into(),
            to: "b.md".into(),
        }];
        let e = apply_fixes(wt, &fixes).unwrap_err();
        assert!(e.contains("would overwrite"), "{e}");
        // the target survives untouched
        assert_eq!(std::fs::read_to_string(wt.join("b.md")).unwrap(), "keep me");
    }

    // ---- unreadable files ----

    #[test]
    fn unreadable_todo_is_not_missing_and_not_scaffolded() {
        let d = tempfile::tempdir().unwrap();
        write_bytes(d.path(), "TODO.md", b"# TODO \xff\xfe binary\n");
        let a = analyze(d.path());
        let f = a
            .findings
            .iter()
            .find(|f| f.file == "TODO.md")
            .expect("TODO finding");
        assert_eq!(f.code, "unreadable-file");
        assert!(!f.fixable);
        // not reported missing, never scaffolded over
        assert!(!a
            .findings
            .iter()
            .any(|f| f.file == "TODO.md" && f.code == "missing-file"));
        assert!(!a
            .fixes
            .iter()
            .any(|f| matches!(f, Fix::Write { rel, .. } if rel == "TODO.md")));
    }

    #[test]
    fn unreadable_item_file_reports() {
        let d = tempfile::tempdir().unwrap();
        write_bytes(d.path(), "issues/issue-0001-x.md", b"\xff\xfe");
        let a = analyze(d.path());
        assert!(a
            .findings
            .iter()
            .any(|f| f.file == "issues/issue-0001-x.md" && f.code == "unreadable-file"));
    }

    #[test]
    fn parse_fail_suppresses_dependent_checks() {
        let d = tempfile::tempdir().unwrap();
        let wt = d.path();
        // TODO.md fails to parse — T-005 may live in it
        write(
            wt,
            "TODO.md",
            "# TODO — x\n\n- [ ] T-001 bad · status:bogus\n",
        );
        // an issue blocking T-005, and T-005's handoff — both would
        // look dangling/orphaned if the id set were trusted
        write(
            wt,
            "issues/issue-0001-x.md",
            "+++\nid = \"I-001\"\ntitle = \"x\"\nstatus = \"open\"\nblocks = [\"T-005\"]\n+++\n\nsee T-005 for details\n",
        );
        write(
            wt,
            "handoffs/T-005.md",
            "+++\nitem = \"T-005\"\n+++\n\n# Handoffs — T-005\n\n## 2026-01-01 · devin\nnote\n",
        );
        let a = analyze(wt);
        assert!(codes(&a).contains(&"parse"), "{:?}", codes(&a));
        for code in ["dangling-blocks", "orphan-handoff", "dangling-ref"] {
            assert!(
                !codes(&a).contains(&code),
                "{code} fired despite incomplete id set"
            );
        }
    }

    // ---- dangling-ref ----

    #[test]
    fn id_ref_tokens_scan() {
        let t = |n| ItemId::new(ItemKind::Task, n);
        let adr = |n| ItemId::new(ItemKind::Adr, n);
        let got: Vec<ItemId> = id_ref_tokens("see T-003 and ADR-0008, (P-2) [PL-001] I-4 x")
            .into_iter()
            .map(|(_, id)| id)
            .collect();
        assert_eq!(
            got,
            vec![
                t(3),
                adr(8),
                ItemId::new(ItemKind::Proposal, 2),
                ItemId::new(ItemKind::Plan, 1),
                ItemId::new(ItemKind::Issue, 4),
            ]
        );
        // non-tokenizing shapes — unknown prefixes, glued words,
        // digits fused into identifiers, missing digits, zero ids
        for no in [
            "FOO-123",
            "XT-001",   // word char before the prefix
            "T-001abc", // digits fused into a word
            "x_T-001",  // `_` is a word char too
            "ADR-",
            "T-000",    // parses to no valid id
            "adr-0001", // lowercase — not an id
            "prefixes T--1",
        ] {
            assert!(id_ref_tokens(no).is_empty(), "{no}");
        }
        // byte offsets land where the token starts
        let v = id_ref_tokens("ab T-042");
        assert_eq!(v, vec![(3, t(42))]);
    }

    #[test]
    fn frontmatter_end_offsets() {
        let src = "+++\nid = \"T-001\"\n+++\n\nbody\n";
        assert_eq!(&src[frontmatter_end(src)..], "\nbody\n");
        assert_eq!(frontmatter_end("no fm\n"), 0);
        assert_eq!(frontmatter_end(""), 0);
        // unclosed fence → 0 (parse finding covers it)
        assert_eq!(frontmatter_end("+++\nid = \"T-001\"\n"), 0);
    }

    #[test]
    fn title_zones_stop_at_dots() {
        let src = "# TODO — x\n\n- [ ] T-001 title · pri:high · note:T-002\n- [x] T-002 done\n- T-003 bare backlog\nnot-an-item ADR-0001 here\n";
        let zones = title_zones(src);
        let texts: Vec<&str> = zones.iter().map(|(s, e)| &src[*s..*e]).collect();
        // markers + the item's own leading id drop out of the zone;
        // `note:`/`pri:` tokens are beyond the `·` cut; non-item
        // lines scan whole
        assert_eq!(
            texts,
            vec![
                "# TODO — x",
                "",
                " title",
                " done",
                " bare backlog",
                "not-an-item ADR-0001 here"
            ],
            "{texts:?}"
        );
        assert!(in_zones(0, &zones) && !in_zones(src.len() - 1, &zones));
    }

    #[test]
    fn line_col_counts() {
        let src = "one\ntwo\nT-003 x\n";
        let off = src.find("T-003").unwrap();
        assert_eq!(line_col(src, off), (3, 1));
        assert_eq!(line_col(src, off + 2), (3, 3));
    }

    /// Minimal scaffold for analyze-level dangling-ref tests.
    fn scaffold(wt: &Path) {
        let meta = ProjectMeta::new("x");
        write(wt, "meta.toml", &myco_store_md::emit_meta(&meta).unwrap());
        write(
            wt,
            "TODO.md",
            &myco_store_md::emit_todo(&myco_core::TodoDoc::new("x")).unwrap(),
        );
        write(
            wt,
            "BACKLOG.md",
            &myco_store_md::emit_backlog(&myco_core::BacklogDoc::default()).unwrap(),
        );
        write(wt, "index.md", "# x\n");
        write(wt, "handoffs/.gitkeep", "");
    }

    fn dangling_refs(a: &Analysis) -> Vec<&Finding> {
        a.findings
            .iter()
            .filter(|f| f.code == "dangling-ref")
            .collect()
    }

    #[test]
    fn dangling_ref_flags_body_mentions() {
        let d = tempfile::tempdir().unwrap();
        let wt = d.path();
        scaffold(wt);
        // a live ADR body mentioning a missing id — flagged with a
        // span; a mention of ADR-0001 itself resolves; `T-0009` in a
        // code fence and `FOO-123` never flag
        write(
            wt,
            "adrs/0001-one.md",
            "+++\nid = \"ADR-0001\"\ntitle = \"ADR-0001: one\"\nstatus = \"open\"\n+++\n\n# ADR-0001: one\n\nsee ADR-0099 and ADR-0001.\n```\nT-0009 fenced\n```\n`I-077 inline`\nFOO-123\n",
        );
        let a = analyze(wt);
        let refs = dangling_refs(&a);
        assert_eq!(refs.len(), 1, "{:?}", codes(&a));
        let f = refs[0];
        assert_eq!(f.file, "adrs/0001-one.md");
        assert!(f.line > 0 && f.col > 0);
        assert!(!f.fixable);
        assert!(f.msg.contains("ADR-0099"), "{}", f.msg);
    }

    #[test]
    fn dangling_ref_self_and_claimed_ids_skip() {
        let d = tempfile::tempdir().unwrap();
        let wt = d.path();
        scaffold(wt);
        // title claims ADR-0005 (missing) — a renumber candidate,
        // not a dangling prose ref
        write(
            wt,
            "adrs/0002-two.md",
            "+++\nid = \"ADR-0002\"\ntitle = \"ADR-0005: five\"\nstatus = \"open\"\n+++\n\n# ADR-0005: five\n\nbody\n",
        );
        let a = analyze(wt);
        assert!(dangling_refs(&a).is_empty(), "{:?}", codes(&a));
        // but it IS an id-title-mismatch candidate
        assert!(codes(&a).contains(&"id-title-mismatch"));
    }

    #[test]
    fn dangling_ref_todo_backlog_titles_and_index() {
        let d = tempfile::tempdir().unwrap();
        let wt = d.path();
        let meta = ProjectMeta::new("x");
        write(wt, "meta.toml", &myco_store_md::emit_meta(&meta).unwrap());
        // a title mentioning T-999 (missing) flags; `note:see T-888`
        // is a metadata token, not a title — unflagged; T-001's own
        // leading id resolves
        write(
            wt,
            "TODO.md",
            "# TODO — x\n\n- [ ] T-001 follow up on T-999 · note:see T-888\n",
        );
        write(wt, "BACKLOG.md", "# Backlog\n\n- T-002 revisit P-007\n");
        write(
            wt,
            "index.md",
            "# x\n\nLinks: [gone](issues/issue-0042-x.md) and I-042.\n",
        );
        write(wt, "handoffs/.gitkeep", "");
        let a = analyze(wt);
        let refs = dangling_refs(&a);
        let at: Vec<(&str, &str)> = refs
            .iter()
            .map(|f| (f.file.as_str(), f.msg.as_str()))
            .collect();
        assert_eq!(refs.len(), 3, "{at:?}");
        assert!(at
            .iter()
            .any(|(f, m)| *f == "TODO.md" && m.contains("T-999")));
        assert!(at
            .iter()
            .any(|(f, m)| *f == "BACKLOG.md" && m.contains("P-007")));
        assert!(at
            .iter()
            .any(|(f, m)| *f == "index.md" && m.contains("I-042")));
    }

    #[test]
    fn dangling_ref_handoffs_live_and_archived() {
        let d = tempfile::tempdir().unwrap();
        let wt = d.path();
        scaffold(wt);
        // T-001 exists; its handoff mentions missing T-009 → flags
        let item = Item::new(ItemId::new(ItemKind::Task, 1), "t1");
        let mut doc = myco_core::TodoDoc::new("x");
        doc.sections.push(myco_core::Section {
            name: None,
            items: vec![item],
        });
        let todo = myco_store_md::emit_todo(&doc).unwrap();
        std::fs::write(wt.join("TODO.md"), &todo).unwrap();
        write(
            wt,
            "handoffs/T-001.md",
            "+++\nitem = \"T-001\"\n+++\n\n# Handoffs — T-001\n\n## 2026-01-01 · devin\ncontinues T-009\n",
        );
        // an archived handoff for the gone T-002 mentioning I-003
        write(
            wt,
            "handoffs/archive/T-002.md",
            "+++\nitem = \"T-002\"\n+++\n\n# Handoffs — T-002\n\n## 2026-01-01 · devin\nsee I-003\n",
        );
        let a = analyze(wt);
        let refs = dangling_refs(&a);
        let files: Vec<&str> = refs.iter().map(|f| f.file.as_str()).collect();
        assert!(files.contains(&"handoffs/T-001.md"), "{files:?}");
        assert!(files.contains(&"handoffs/archive/T-002.md"), "{files:?}");
        // self-refs (the `# Handoffs — <id>` title) never flag
        assert!(refs
            .iter()
            .all(|f| !f.msg.contains("T-001 doesn't exist")
                && !f.msg.contains("T-002 doesn't exist")));
    }

    #[test]
    fn dangling_ref_archived_item_ids_resolve() {
        let d = tempfile::tempdir().unwrap();
        let wt = d.path();
        scaffold(wt);
        // ADR-0007 lives in cold storage — a live ADR referencing it
        // is NOT stale
        write(
            wt,
            "adrs/archive/0007-old.md",
            "+++\nid = \"ADR-0007\"\ntitle = \"old\"\nstatus = \"done\"\n+++\n",
        );
        write(
            wt,
            "adrs/0001-one.md",
            "+++\nid = \"ADR-0001\"\ntitle = \"one\"\nstatus = \"open\"\n+++\n\nsupersedes ADR-0007.\n",
        );
        let a = analyze(wt);
        assert!(dangling_refs(&a).is_empty(), "{:?}", codes(&a));
    }
}
