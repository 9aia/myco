//! Item files — one md file per issue/proposal/adr/plan:
//!
//! ```text
//! issues/issue-NNNN-slug.md    proposals/proposal-NNNN-slug.md
//! adrs/NNNN-slug.md            plans/pl-NNNN-slug.md
//! ```
//!
//! `NNNN` = id number padded to 4 digits. TOML frontmatter in
//! `+++` fences (id, title, status, created, owner, priority,
//! labels, blocks, links, note, superseded_by, supersedes, epic,
//! milestone, due, estimate, cycle, done — extendable: unknown keys are
//! ignored on read for forward-compat); body = free-form markdown
//! (verbatim; kind templates are `myco-template`'s job).

use myco_core::{date, Actor, Item, ItemFile, ItemId, KindSpec, Priority, Status, Storage};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use crate::error::{EmitError, ParseError};
use crate::{line, mdx};

/// The `project/` subdirectory a file-backed kind lives in —
/// the spec's `dir` (`issues`, `spikes`); `doc`-storage kinds
/// have no files (`None`).
pub fn item_dir(spec: &KindSpec) -> Option<String> {
    (spec.storage == Storage::Dir).then(|| spec.dir_name())
}

/// Filename prefix before the number — `issue`, `proposal`, `pl`,
/// the spec's `file_prefix` for user kinds, or empty for ADRs
/// (`adrs/0004-slug.md`).
fn file_prefix(spec: &KindSpec) -> Option<String> {
    (spec.storage == Storage::Dir).then(|| spec.file_prefix())
}

/// Canonical filename for an item — `issue-0003-memory-leak.md`,
/// `0004-hybrid-store.md`. Slug = `slug::slugify(title)`,
/// `"untitled"` when the title slugifies to nothing.
pub fn item_filename(spec: &KindSpec, id: &ItemId, title: &str) -> Option<String> {
    let prefix = file_prefix(spec)?;
    let slug = {
        let s = slug::slugify(title);
        if s.is_empty() {
            "untitled".to_string()
        } else {
            s
        }
    };
    Some(if prefix.is_empty() {
        format!("{:04}-{slug}.md", id.number())
    } else {
        format!("{prefix}-{:04}-{slug}.md", id.number())
    })
}

/// `issues/issue-0003-x.md` → `I-003`; `None` when the filename
/// doesn't fit the kind's pattern. The number must fit `u32` —
/// the same bound [`ItemId`]'s own parse applies, so anything
/// [`item_filename`] can emit parses back (id numbers are `u32`,
/// i.e. up to 10 digits).
pub fn parse_item_filename(spec: &KindSpec, filename: &str) -> Option<ItemId> {
    let prefix = file_prefix(spec)?;
    let stem = filename.strip_suffix(".md")?;
    let rest = if prefix.is_empty() {
        stem
    } else {
        stem.strip_prefix(prefix.as_str())?.strip_prefix('-')?
    };
    let (digits, slug) = rest.split_once('-')?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) || slug.is_empty() {
        return None;
    }
    let number: u32 = digits.parse().ok()?;
    if number == 0 {
        return None;
    }
    Some(ItemId::new(spec.id_kind(), number))
}

/// Deepest line the `frontmatter_id` probe will read — real
/// frontmatter is ~10 keys; past this the `+++` block is
/// certainly unclosed (or the file is pathological).
const FRONTMATTER_PROBE_MAX: usize = 128;

/// `id = "…"` pulled out of a *closed* `+++` frontmatter block —
/// the cheap probe for when the strict parse isn't needed (or
/// already failed). `None` for files with no `+++` fence, no
/// closing fence within [`FRONTMATTER_PROBE_MAX`] lines, or no
/// parseable `id` line — including frontmatter-less files that
/// merely *borrow* an id number via their filename. The close
/// requirement is what keeps a body's `id = "…"` (a code block,
/// a quoted example) from posing as frontmatter.
pub fn frontmatter_id(src: &str) -> Option<ItemId> {
    let mut lines = src.lines();
    if lines.next()?.trim() != "+++" {
        return None;
    }
    let mut found = None;
    for l in lines.take(FRONTMATTER_PROBE_MAX) {
        let t = l.trim();
        // the close ends the block — an id only counts when the
        // block actually closed (else a body's `id = "…"` would
        // pose as frontmatter)
        if t == "+++" {
            return found;
        }
        if found.is_none() {
            if let Some(rest) = t.strip_prefix("id") {
                if let Some(v) = rest.trim_start().strip_prefix('=') {
                    if let Ok(id) = v.trim().trim_matches('"').parse::<ItemId>() {
                        found = Some(id);
                    }
                }
            }
        }
    }
    None // window exhausted — never closed
}

/// Deepest line [`repair_frontmatter`] will read — same bound as
/// [`FRONTMATTER_PROBE_MAX`]; real frontmatter is ~10 keys.
const REPAIR_MAX: usize = FRONTMATTER_PROBE_MAX;

/// Strip one layer of `'`/`"` quoting a hand-written YAML scalar
/// carries.
fn unquote(s: &str) -> &str {
    let b = s.as_bytes();
    if b.len() >= 2
        && ((b[0] == b'"' && b[b.len() - 1] == b'"') || (b[0] == b'\'' && b[b.len() - 1] == b'\''))
    {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

/// Escape a string for a TOML basic string.
fn toml_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// `key` is a TOML bare key — letter/`_` then alnum/`_`/`-`.
fn is_bare_key(key: &str) -> bool {
    let mut chars = key.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Rewrite a leading `---` YAML frontmatter block as the canonical
/// `+++` TOML fence — the repair half of [`parse_item_file`]'s
/// strictness. Agents hand-authoring item files reach for the
/// ubiquitous `---` + `key: value` convention; this converts the
/// simple subset — top-level `key: scalar`, `key: [a, b]` inline
/// lists, `#` comments, blank lines — and returns the source with
/// a `+++` fence, still *unvalidated*: callers re-parse. `None`
/// when the file doesn't open with a closed `---` block, the block
/// exceeds [`REPAIR_MAX`] lines, or a frontmatter line isn't
/// `key: value` at column 0 (nested maps, `- ` list items — shapes
/// this doesn't claim to understand).
pub fn repair_frontmatter(src: &str) -> Option<String> {
    let mut lines = src.split_inclusive('\n');
    let first = lines.next()?;
    if first.trim_end() != "---" {
        return None;
    }
    let fm_start = first.len();
    let mut off = fm_start;
    let mut close = None;
    for line in lines.by_ref().take(REPAIR_MAX) {
        if line.trim_end() == "---" {
            close = Some((off, off + line.len()));
            break;
        }
        off += line.len();
    }
    let (fm_end, body_start) = close?;
    let mut toml = String::new();
    for raw in src[fm_start..fm_end].lines() {
        // indented lines are nested YAML — out of scope
        if raw.starts_with(|c: char| c.is_whitespace()) {
            if !raw.trim().is_empty() {
                return None;
            }
            continue;
        }
        let l = raw.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        let (key, val) = l.split_once(':')?;
        if !is_bare_key(key) {
            return None;
        }
        let val = val.trim();
        if val.starts_with('[') && val.ends_with(']') {
            let inner = &val[1..val.len() - 1];
            let items: Vec<String> = inner
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| format!("\"{}\"", toml_escape(unquote(s))))
                .collect();
            toml.push_str(&format!("{} = [{}]\n", key, items.join(", ")));
        } else {
            let v = unquote(val);
            // a bare u32 (`estimate: 3`) — everything else is a
            // TOML string, which every stringly-typed frontmatter
            // field (and `de_opt_created`'s date forms) accepts
            if !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()) {
                toml.push_str(&format!("{key} = {v}\n"));
            } else {
                toml.push_str(&format!("{key} = \"{}\"\n", toml_escape(v)));
            }
        }
    }
    let mut out = String::from("+++\n");
    out.push_str(&toml);
    out.push_str("+++\n");
    out.push_str(&src[body_start..]);
    Some(out)
}

/// Frontmatter DTO — stringly-typed so bad values point at the
/// frontmatter block, not toml's internals.
#[derive(Debug, Deserialize)]
struct FrontmatterIn {
    id: String,
    title: String,
    status: Status,
    #[serde(default, deserialize_with = "de_opt_created")]
    created: Option<String>,
    owner: Option<String>,
    #[serde(default, deserialize_with = "de_opt_priority")]
    priority: Option<Priority>,
    #[serde(default)]
    labels: Vec<String>,
    #[serde(default)]
    blocks: Vec<String>,
    #[serde(default)]
    links: Vec<String>,
    note: Option<String>,
    superseded_by: Option<String>,
    supersedes: Option<String>,
    epic: Option<String>,
    milestone: Option<String>,
    #[serde(default, deserialize_with = "de_opt_created")]
    due: Option<String>,
    estimate: Option<u32>,
    cycle: Option<String>,
    #[serde(default, deserialize_with = "de_opt_created")]
    done: Option<String>,
}

/// `priority` reads like the line grammar — `low|med|high|crit`,
/// the spelling emit writes — plus the serde full names
/// (`medium`, `critical`) older files and hand-edits carry, via
/// [`Priority`]'s `FromStr` (lenient read, strict canonical
/// write). Deriving `Priority` would accept full names only.
fn de_opt_priority<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Priority>, D::Error> {
    Option::<String>::deserialize(d)?
        .map(|s| s.parse().map_err(serde::de::Error::custom))
        .transpose()
}

/// `created` accepts both the quoted string emit writes
/// (`created = "2026-02-01"`) and a bare TOML date
/// (`created = 2026-02-01`) — hand-edited files use the native
/// form. Either way the value lands as text for [`date::parse`]
/// to validate.
fn de_opt_created<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    match Option::<toml::Value>::deserialize(d)? {
        None => Ok(None),
        Some(toml::Value::String(s)) => Ok(Some(s)),
        Some(toml::Value::Datetime(dt)) => Ok(Some(dt.to_string())),
        Some(v) => Err(serde::de::Error::custom(format!(
            "expected a `YYYY-MM-DD` date, got {v}"
        ))),
    }
}

/// Emit shape — field order IS the frontmatter key order.
#[derive(Serialize)]
struct FrontmatterOut<'a> {
    id: String,
    title: &'a str,
    status: Status,
    #[serde(skip_serializing_if = "Option::is_none")]
    created: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    owner: Option<String>,
    /// The `pri:` token spelling (`med`, `crit`) — frontmatter is
    /// line-grammar vocabulary, not serde's full names (I-032).
    /// Read stays lenient (`medium`/`critical` parse), so any
    /// rewrite normalizes them to tokens.
    priority: &'a str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    labels: Vec<&'a str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    blocks: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    links: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    superseded_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    supersedes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    epic: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    milestone: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    due: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    estimate: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cycle: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    done: Option<String>,
}

/// Parse an item file's source. `spec` declares the kind the
/// directory the file was found in belongs to — an id whose
/// prefix isn't the spec's (`P-002` inside `issues/`) is an
/// error.
pub fn parse_item_file(src: &str, spec: &KindSpec, path: &str) -> Result<ItemFile, ParseError> {
    let src = mdx::normalize(src);
    let at = |off: usize, len: usize, m: String| ParseError::at(path, src.clone(), off, len, m);
    let whole = |m: &str| ParseError::whole_file(path, src.clone(), m);
    let mut lines = src.split_inclusive('\n');
    let first = lines.next().map(str::trim_end);
    if first != Some("+++") {
        // `---` is the YAML fence hand-authored files reach for —
        // name the repair so the reader isn't stuck on the spec
        let msg = if first == Some("---") {
            "expected `+++` TOML frontmatter fence on line 1 — `---` YAML \
             frontmatter isn't the file spec; `myco check --fix` rewrites it"
        } else {
            "expected `+++` TOML frontmatter fence on line 1"
        };
        return Err(whole(msg));
    }
    // find the closing `+++` line (byte offsets into src)
    let fm_start = "+++\n".len();
    let mut fm_end = None;
    let mut off = fm_start;
    for line in lines {
        if line.trim_end() == "+++" {
            fm_end = Some((off, off + line.len()));
            break;
        }
        off += line.len();
    }
    let Some((fm_end, fence_end)) = fm_end else {
        return Err(whole("unclosed `+++` frontmatter fence"));
    };
    let fm_src = &src[fm_start..fm_end];
    let fm: FrontmatterIn = toml::from_str(fm_src).map_err(|e| {
        let (o, l) = e
            .span()
            .map(|s| (fm_start + s.start, s.end.saturating_sub(s.start)))
            .unwrap_or((fm_start, fm_end - fm_start));
        at(o, l, e.to_string())
    })?;
    // DTO → Item; value errors point at the whole frontmatter block.
    let fm_err = |m: String| at(fm_start, fm_end - fm_start, m);
    let id: ItemId = fm
        .id
        .parse()
        .map_err(|e: myco_core::ParseItemIdError| fm_err(e.to_string()))?;
    if id.kind() != spec.id_kind() {
        return Err(fm_err(format!(
            "id {id} doesn't belong in this file (expected a {} id)",
            spec.name
        )));
    }
    // filename ↔ frontmatter: a canonical filename's number must
    // agree with the id the file claims — `issue-0003-x.md`
    // holding `I-009` reserves 3 in the id scan while claiming 9,
    // which would let the allocator reissue a live id. A name
    // that doesn't fit the kind's pattern carries no number to
    // check against (that's a filename finding, not this error).
    let basename = std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path);
    if let Some(fname_id) = parse_item_filename(spec, basename) {
        if fname_id != id {
            return Err(fm_err(format!(
                "filename says {fname_id} but frontmatter id is {id}"
            )));
        }
    }
    let owner = fm
        .owner
        .map(|o| Actor::new(&o).map_err(|e| fm_err(e.to_string())))
        .transpose()?;
    let created = fm
        .created
        .map(|c| date::parse(&c).map_err(|e| fm_err(format!("bad created date — {e}"))))
        .transpose()?;
    let mut labels = BTreeSet::new();
    for l in fm.labels {
        if !myco_core::is_valid_label(&l) {
            return Err(fm_err(format!("invalid label {l:?}")));
        }
        labels.insert(l);
    }
    let mut blocks = BTreeSet::new();
    for b in fm.blocks {
        blocks.insert(
            b.parse::<ItemId>()
                .map_err(|e: myco_core::ParseItemIdError| fm_err(e.to_string()))?,
        );
    }
    let mut links = BTreeSet::new();
    for l in fm.links {
        links.insert(
            l.parse::<ItemId>()
                .map_err(|e: myco_core::ParseItemIdError| fm_err(e.to_string()))?,
        );
    }
    let mut item = Item::new(id, fm.title);
    item.status = fm.status;
    item.priority = fm.priority.unwrap_or_default();
    item.owner = owner;
    item.labels = labels;
    item.blocks = blocks;
    item.links = links;
    item.created = created;
    item.note = fm.note;
    item.superseded_by = fm
        .superseded_by
        .map(|s| {
            s.parse::<ItemId>()
                .map_err(|e: myco_core::ParseItemIdError| fm_err(e.to_string()))
        })
        .transpose()?;
    item.supersedes = fm
        .supersedes
        .map(|s| {
            s.parse::<ItemId>()
                .map_err(|e: myco_core::ParseItemIdError| fm_err(e.to_string()))
        })
        .transpose()?;
    for (key, name) in [
        ("epic", &fm.epic),
        ("milestone", &fm.milestone),
        ("cycle", &fm.cycle),
    ] {
        if let Some(name) = name {
            if !myco_core::is_valid_group_name(name) {
                return Err(fm_err(format!("invalid {key} name {name:?}")));
            }
        }
    }
    item.epic = fm.epic;
    item.milestone = fm.milestone;
    item.due = fm
        .due
        .map(|d| date::parse(&d).map_err(|e| fm_err(format!("bad due date — {e}"))))
        .transpose()?;
    if let Some(e) = fm.estimate {
        if e > myco_core::MAX_ESTIMATE {
            return Err(fm_err(format!(
                "estimate {e} exceeds max {}",
                myco_core::MAX_ESTIMATE
            )));
        }
        item.estimate = Some(e);
    }
    item.cycle = fm.cycle;
    item.done = fm
        .done
        .map(|d| date::parse(&d).map_err(|e| fm_err(format!("bad done date — {e}"))))
        .transpose()?;
    if item.blocks.contains(&id)
        || item.links.contains(&id)
        || item.supersedes == Some(id)
        || item.superseded_by == Some(id)
    {
        return Err(fm_err(format!(
            "{id} can't block, link, or supersede itself"
        )));
    }
    // body: everything after the closing fence, canonicalized —
    // leading blank lines go, the first content line's own
    // indentation survives (an indented code block stays a code
    // block)
    let file = ItemFile {
        meta: item,
        body: myco_core::canonical_body(&src[fence_end..]).to_string(),
    };
    // parse-safe: the body must be markdown the parser accepts
    if !file.body.is_empty() {
        mdx::parse(&file.body).map_err(|m| at(fence_end, src.len() - fence_end, m))?;
    }
    Ok(file)
}

/// Canonical item file — `+++\n<toml>+++\n\n<body>\n`. The model
/// is validated first ([`line::check_item`] — the same validity
/// source as the line grammar) so emit can't write a file parse
/// would reject; children are an error, never silently dropped.
pub fn emit_item_file(file: &ItemFile) -> Result<String, EmitError> {
    line::check_item(&file.meta, "item file")?;
    if !file.meta.children.is_empty() {
        return Err(EmitError::new(
            "item file",
            format!(
                "{} carries children — sub-items only exist in TODO.md/BACKLOG.md",
                file.meta.id
            ),
        ));
    }
    let fm = FrontmatterOut {
        id: file.meta.id.to_string(),
        title: &file.meta.title,
        status: file.meta.status,
        created: file.meta.created.map(date::render),
        owner: file.meta.owner.as_ref().map(ToString::to_string),
        priority: file.meta.priority.token(),
        labels: file.meta.labels.iter().map(String::as_str).collect(),
        blocks: file.meta.blocks.iter().map(ItemId::to_string).collect(),
        links: file.meta.links.iter().map(ItemId::to_string).collect(),
        note: file.meta.note.as_deref(),
        superseded_by: file.meta.superseded_by.map(|i| i.to_string()),
        supersedes: file.meta.supersedes.map(|i| i.to_string()),
        epic: file.meta.epic.as_deref(),
        milestone: file.meta.milestone.as_deref(),
        due: file.meta.due.map(date::render),
        estimate: file.meta.estimate,
        cycle: file.meta.cycle.as_deref(),
        done: file.meta.done.map(date::render),
    };
    let toml = toml::to_string(&fm).map_err(|e| EmitError::new("item file", e.to_string()))?;
    if file.body != myco_core::canonical_body(&file.body) {
        // a body parse can't reproduce byte-for-byte would emit
        // then read back as a different model — non-canonical
        // bodies are the caller's bug, not content to reshape
        return Err(EmitError::new(
            "item file",
            "body isn't canonical — leading blank lines or trailing whitespace",
        ));
    }
    let mut out = String::from("+++\n");
    out.push_str(&toml);
    out.push_str("+++\n");
    if !file.body.is_empty() {
        out.push('\n');
        out.push_str(&file.body);
        out.push('\n');
    }
    Ok(out)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use myco_core::{ItemKind, KindTable};

    /// The prelude spec for a builtin kind — tests name it by
    /// its const, the store resolves the spec.
    fn spec(kind: ItemKind) -> KindSpec {
        KindTable::prelude().spec_of(kind).unwrap().clone()
    }

    const ISSUE: &str = "+++\nid = \"I-003\"\ntitle = \"Memory leak\"\nstatus = \"in-progress\"\ncreated = \"2026-02-01\"\nowner = \"devin\"\npriority = \"high\"\nlabels = [\"index\", \"perf\"]\nblocks = [\"T-010\"]\nnote = \"resumed\"\n+++\n\n## Description\n\nBody text.\n";

    #[test]
    fn filename_mapping() {
        assert_eq!(
            item_filename(
                &spec(ItemKind::Issue),
                &ItemId::new(ItemKind::Issue, 3),
                "Memory leak!"
            ),
            Some("issue-0003-memory-leak.md".into())
        );
        assert_eq!(
            item_filename(
                &spec(ItemKind::Adr),
                &ItemId::new(ItemKind::Adr, 4),
                "Hybrid store"
            ),
            Some("0004-hybrid-store.md".into())
        );
        assert_eq!(
            item_filename(
                &spec(ItemKind::Plan),
                &ItemId::new(ItemKind::Plan, 2),
                "v2 phases"
            ),
            Some("pl-0002-v2-phases.md".into())
        );
        assert_eq!(
            item_filename(
                &spec(ItemKind::Proposal),
                &ItemId::new(ItemKind::Proposal, 7),
                ""
            ),
            Some("proposal-0007-untitled.md".into())
        );
        assert_eq!(
            item_filename(&spec(ItemKind::Task), &ItemId::new(ItemKind::Task, 1), "x"),
            None
        );
    }

    #[test]
    fn filename_parse() {
        assert_eq!(
            parse_item_filename(&spec(ItemKind::Issue), "issue-0003-memory-leak.md"),
            Some(ItemId::new(ItemKind::Issue, 3))
        );
        assert_eq!(
            parse_item_filename(&spec(ItemKind::Adr), "0004-hybrid.md"),
            Some(ItemId::new(ItemKind::Adr, 4))
        );
        assert_eq!(
            parse_item_filename(&spec(ItemKind::Plan), "pl-0002-x.md"),
            Some(ItemId::new(ItemKind::Plan, 2))
        );
        for (kind, name) in [
            (ItemKind::Task, "issue-0003-x.md"),
            (ItemKind::Issue, "issue-x-y.md"),
            (ItemKind::Issue, "issue-0003.md"),
            (ItemKind::Issue, "issue-0000-x.md"),
            (ItemKind::Issue, "issue-0003-.md"),
            (ItemKind::Issue, "issue-0003-x.txt"),
            (ItemKind::Issue, "proposal-0003-x.md"),
            (ItemKind::Adr, "adr-0004-x.md"),
            (ItemKind::Adr, "0004.md"),
            (ItemKind::Issue, "issue-0003x-y.md"),
            // past u32 the number can't name an item
            (ItemKind::Issue, "issue-99999999999-x.md"),
        ] {
            assert_eq!(parse_item_filename(&spec(kind), name), None, "{name}");
        }
        // the number bound is u32 — same as ItemId's own parse —
        // so every filename item_filename can emit parses back
        // (the old 9-digit cap rejected 10-digit u32s, I-021)
        assert_eq!(
            parse_item_filename(&spec(ItemKind::Issue), "issue-4294967295-x.md"),
            Some(ItemId::new(ItemKind::Issue, u32::MAX))
        );
        assert_eq!(
            parse_item_filename(&spec(ItemKind::Issue), "issue-0000000003-x.md"),
            Some(ItemId::new(ItemKind::Issue, 3))
        );
    }

    #[test]
    fn roundtrip() {
        let file = parse_item_file(
            ISSUE,
            &spec(ItemKind::Issue),
            "issues/issue-0003-memory-leak.md",
        )
        .unwrap();
        assert_eq!(file.meta.id, ItemId::new(ItemKind::Issue, 3));
        assert_eq!(file.meta.status, Status::InProgress);
        assert_eq!(file.meta.priority, Priority::High);
        assert_eq!(file.meta.owner.as_ref().unwrap().as_str(), "devin");
        assert_eq!(file.meta.note.as_deref(), Some("resumed"));
        assert!(file.meta.labels.contains("perf"));
        assert!(file.meta.blocks.contains(&ItemId::new(ItemKind::Task, 10)));
        assert_eq!(file.body, "## Description\n\nBody text.");
        assert_eq!(emit_item_file(&file).unwrap(), ISSUE);
    }

    #[test]
    fn supersede_keys_roundtrip() {
        let src = "+++\nid = \"ADR-0002\"\ntitle = \"old\"\nstatus = \"done\"\npriority = \"med\"\nsuperseded_by = \"ADR-0003\"\nsupersedes = \"ADR-0001\"\n+++\n";
        let file = parse_item_file(src, &spec(ItemKind::Adr), "adrs/0002-old.md").unwrap();
        assert_eq!(file.meta.superseded_by, Some(ItemId::new(ItemKind::Adr, 3)));
        assert_eq!(file.meta.supersedes, Some(ItemId::new(ItemKind::Adr, 1)));
        assert_eq!(emit_item_file(&file).unwrap(), src);
        // bad id values are frontmatter errors
        let bad = "+++\nid = \"ADR-0001\"\ntitle = \"x\"\nstatus = \"open\"\nsuperseded_by = \"nope\"\n+++\n";
        assert!(parse_item_file(bad, &spec(ItemKind::Adr), "x.md").is_err());
    }

    #[test]
    fn links_roundtrip_and_self_reject() {
        let src = "+++\nid = \"T-001\"\ntitle = \"x\"\nstatus = \"open\"\npriority = \"med\"\nlinks = [\"P-013\", \"PL-001\"]\n+++\n";
        let file = parse_item_file(src, &spec(ItemKind::Task), "x.md").unwrap();
        assert!(file.meta.links.contains(&ItemId::new(ItemKind::Plan, 1)));
        assert!(file
            .meta
            .links
            .contains(&ItemId::new(ItemKind::Proposal, 13)));
        assert_eq!(emit_item_file(&file).unwrap(), src);
        // a self-link is a frontmatter error
        let bad =
            "+++\nid = \"I-001\"\ntitle = \"x\"\nstatus = \"open\"\nlinks = [\"I-001\"]\n+++\n";
        assert!(parse_item_file(bad, &spec(ItemKind::Issue), "x.md").is_err());
        // a bad id value is too
        let bad =
            "+++\nid = \"I-001\"\ntitle = \"x\"\nstatus = \"open\"\nlinks = [\"nope\"]\n+++\n";
        assert!(parse_item_file(bad, &spec(ItemKind::Issue), "x.md").is_err());
    }

    #[test]
    fn epic_milestone_roundtrip() {
        let src = "+++\nid = \"I-001\"\ntitle = \"x\"\nstatus = \"open\"\npriority = \"med\"\nepic = \"v2\"\nmilestone = \"1.0\"\n+++\n";
        let file = parse_item_file(src, &spec(ItemKind::Issue), "issues/issue-0001-x.md").unwrap();
        assert_eq!(file.meta.epic.as_deref(), Some("v2"));
        assert_eq!(file.meta.milestone.as_deref(), Some("1.0"));
        assert_eq!(emit_item_file(&file).unwrap(), src);
        // invalid group names are frontmatter errors
        for bad in ["epic = \"has space\"", "milestone = \"a/b\""] {
            let s = format!("+++\nid = \"I-001\"\ntitle = \"x\"\nstatus = \"open\"\n{bad}\n+++\n");
            assert!(
                parse_item_file(&s, &spec(ItemKind::Issue), "x.md").is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn scheduling_keys_roundtrip() {
        let src = "+++\nid = \"I-001\"\ntitle = \"x\"\nstatus = \"open\"\npriority = \"med\"\ndue = \"2026-12-01\"\nestimate = 3\ncycle = \"sprint-12\"\n+++\n";
        let file = parse_item_file(src, &spec(ItemKind::Issue), "issues/issue-0001-x.md").unwrap();
        assert_eq!(file.meta.due, Some(date::parse("2026-12-01").unwrap()));
        assert_eq!(file.meta.estimate, Some(3));
        assert_eq!(file.meta.cycle.as_deref(), Some("sprint-12"));
        assert_eq!(emit_item_file(&file).unwrap(), src);
        // bare TOML dates read like `created` does; emit canonicalizes
        let src = "+++\nid = \"I-001\"\ntitle = \"x\"\nstatus = \"open\"\ndue = 2026-12-01\n+++\n";
        let file = parse_item_file(src, &spec(ItemKind::Issue), "issues/x.md").unwrap();
        assert_eq!(file.meta.due, Some(date::parse("2026-12-01").unwrap()));
        assert!(emit_item_file(&file)
            .unwrap()
            .contains("due = \"2026-12-01\""));
        // bad values are frontmatter errors
        for bad in [
            "due = \"tomorrow\"",
            "due = 2026-13-40",
            "estimate = \"3\"",
            "estimate = 1001",
            "estimate = -1",
            "cycle = \"has space\"",
        ] {
            let s = format!("+++\nid = \"I-001\"\ntitle = \"x\"\nstatus = \"open\"\n{bad}\n+++\n");
            assert!(
                parse_item_file(&s, &spec(ItemKind::Issue), "x.md").is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn frontmatter_id_needs_a_closed_block() {
        let i1 = Some(ItemId::new(ItemKind::Issue, 1));
        // the closed block yields its id
        assert_eq!(frontmatter_id("+++\nid = \"I-1\"\n+++\n"), i1);
        // id lines after the close are body, not frontmatter
        assert_eq!(
            frontmatter_id("+++\ntitle = \"x\"\n+++\nid = \"I-1\"\n"),
            None
        );
        // an unclosed block never counts — a body/code `id = "…"`
        // can't pose as frontmatter
        assert_eq!(
            frontmatter_id("+++\ntitle = \"x\"\n\nbody\nid = \"I-1\"\n"),
            None
        );
        assert_eq!(frontmatter_id("+++\nid = \"I-1\"\n"), None);
        // …and the scan is bounded even when the "block" is huge
        let mut src = "+++\n".to_string();
        for _ in 0..(FRONTMATTER_PROBE_MAX + 10) {
            src.push_str("title = \"x\"\n");
        }
        src.push_str("id = \"I-1\"\n+++\n");
        assert_eq!(frontmatter_id(&src), None);
    }

    #[test]
    fn repair_frontmatter_rewrites_yaml_fence() {
        // the shape hand-authored files arrive in — `---` fence,
        // `key: value` scalars, an inline list, comments
        let src = "---\n\
                   id: I-003\n\
                   title: Memory leak\n\
                   status: in-progress\n\
                   created: 2026-02-01\n\
                   owner: devin\n\
                   priority: high\n\
                   labels: [index, perf]\n\
                   blocks: [T-010]\n\
                   note: resumed\n\
                   # a comment\n\
                   ---\n\
                   \n\
                   ## Description\n\
                   \n\
                   Body text.\n";
        let repaired = repair_frontmatter(src).unwrap();
        assert!(repaired.starts_with("+++\n"), "{repaired}");
        assert!(repaired.contains("id = \"I-003\""), "{repaired}");
        assert!(
            repaired.contains("labels = [\"index\", \"perf\"]"),
            "{repaired}"
        );
        assert!(!repaired.contains("---\n# a comment"), "{repaired}");
        // and it parses + re-emits canonically
        let file = parse_item_file(&repaired, &spec(ItemKind::Issue), "issues/x.md").unwrap();
        assert_eq!(file.meta.id, ItemId::new(ItemKind::Issue, 3));
        assert_eq!(file.meta.status, Status::InProgress);
        assert_eq!(file.meta.priority, Priority::High);
        assert!(file.meta.labels.contains("perf"));
        assert!(file.meta.blocks.contains(&ItemId::new(ItemKind::Task, 10)));
        assert_eq!(file.body, "## Description\n\nBody text.");
        assert_eq!(emit_item_file(&file).unwrap(), ISSUE);
    }

    #[test]
    fn repair_frontmatter_declines_what_it_cant_read() {
        // not a `---` file at all
        assert_eq!(repair_frontmatter("+++\nid = \"I-1\"\n+++\n"), None);
        assert_eq!(repair_frontmatter("no fence\n"), None);
        // unclosed block
        assert_eq!(repair_frontmatter("---\nid: I-1\n"), None);
        // a `- ` list item isn't `key: value`
        assert_eq!(repair_frontmatter("---\nlabels:\n  - a\n---\nbody\n"), None);
        // indented (nested) YAML is out of scope
        assert_eq!(
            repair_frontmatter("---\nid: I-1\n  nested: x\n---\nbody\n"),
            None
        );
        // over the probe bound
        let mut src = "---\n".to_string();
        for _ in 0..(REPAIR_MAX + 5) {
            src.push_str("k: v\n");
        }
        src.push_str("---\nbody\n");
        assert_eq!(repair_frontmatter(&src), None);
    }

    #[test]
    fn frontmatter_normalizes_full_names() {
        // serde's full names and bare TOML dates — the shapes a
        // hand-edit (or an older myco) produces — read as their
        // canonical values (I-021, I-032); emit rewrites the
        // strict `pri:`-token / quoted-date forms
        let src = "+++\nid = \"I-001\"\ntitle = \"x\"\nstatus = \"open\"\npriority = \"medium\"\ncreated = 2026-02-01\n+++\n";
        let file = parse_item_file(src, &spec(ItemKind::Issue), "issues/x.md").unwrap();
        assert_eq!(file.meta.priority, Priority::Medium);
        assert_eq!(file.meta.created, Some(date::parse("2026-02-01").unwrap()));
        let out = emit_item_file(&file).unwrap();
        assert!(out.contains("priority = \"med\""));
        assert!(out.contains("created = \"2026-02-01\""));
        // `critical` normalizes to `crit` the same way
        let src =
            "+++\nid = \"I-001\"\ntitle = \"x\"\nstatus = \"open\"\npriority = \"critical\"\n+++\n";
        let file = parse_item_file(src, &spec(ItemKind::Issue), "issues/x.md").unwrap();
        assert_eq!(file.meta.priority, Priority::Critical);
        assert!(emit_item_file(&file)
            .unwrap()
            .contains("priority = \"crit\""));
        // token spellings round-trip byte-identical
        let src =
            "+++\nid = \"I-001\"\ntitle = \"x\"\nstatus = \"open\"\npriority = \"med\"\n+++\n";
        let file = parse_item_file(src, &spec(ItemKind::Issue), "issues/x.md").unwrap();
        assert_eq!(emit_item_file(&file).unwrap(), src);
        // a datetime-with-time is still a bad `YYYY-MM-DD` date
        let bad = "+++\nid = \"I-001\"\ntitle = \"x\"\nstatus = \"open\"\ncreated = 2026-02-01T10:00:00Z\n+++\n";
        assert!(parse_item_file(bad, &spec(ItemKind::Issue), "x.md").is_err());
        // …and a wrong-typed value errors, not silently coerces
        let bad = "+++\nid = \"I-001\"\ntitle = \"x\"\nstatus = \"open\"\ncreated = 42\n+++\n";
        assert!(parse_item_file(bad, &spec(ItemKind::Issue), "x.md").is_err());
    }

    #[test]
    fn self_reference_rejected() {
        // an item can't block or supersede itself — frontmatter
        // and line grammar reject it alike (I-021)
        for src in [
            "+++\nid = \"I-001\"\ntitle = \"x\"\nstatus = \"open\"\nblocks = [\"I-001\"]\n+++\n",
            "+++\nid = \"ADR-001\"\ntitle = \"x\"\nstatus = \"open\"\nsupersedes = \"ADR-001\"\n+++\n",
            "+++\nid = \"ADR-001\"\ntitle = \"x\"\nstatus = \"open\"\nsuperseded_by = \"ADR-001\"\n+++\n",
        ] {
            let kind = if src.contains("ADR") {
                ItemKind::Adr
            } else {
                ItemKind::Issue
            };
            assert!(
                parse_item_file(src, &spec(kind), "x.md").is_err(),
                "{src:?}"
            );
        }
    }

    #[test]
    fn body_keeps_leading_indented_code() {
        // a body that opens with an indented code block must not
        // lose its indent to `trim()` — that would demote it to a
        // paragraph on read (I-021)
        let src = "+++\nid = \"I-001\"\ntitle = \"x\"\nstatus = \"open\"\npriority = \"med\"\n+++\n\n    fn main() {}\n\npara\n";
        let file = parse_item_file(src, &spec(ItemKind::Issue), "issues/x.md").unwrap();
        assert_eq!(file.body, "    fn main() {}\n\npara");
        assert_eq!(emit_item_file(&file).unwrap(), src);
    }

    #[test]
    fn emit_rejects_noncanonical_body() {
        // a hand-built body parse can't reproduce byte-for-byte —
        // emit used to silently `trim_end`, breaking the
        // `parse(emit(m)) == m` fixed point (I-021)
        let good = ItemFile::new(Item::new(ItemId::new(ItemKind::Issue, 1), "x"));
        for body in ["  body  ", "\n\nbody", "body\n\n"] {
            let mut f = good.clone();
            f.body = body.into();
            assert!(emit_item_file(&f).is_err(), "{body:?}");
        }
    }

    #[test]
    fn minimal_frontmatter() {
        let src = "+++\nid = \"P-002\"\ntitle = \"t\"\nstatus = \"open\"\n+++\n";
        let file = parse_item_file(src, &spec(ItemKind::Proposal), "proposals/x.md").unwrap();
        assert_eq!(file.meta.priority, Priority::Medium);
        assert!(file.body.is_empty());
        let out = emit_item_file(&file).unwrap();
        assert!(out.contains("priority = \"med\""));
        assert!(!out.contains("owner"));
        assert_eq!(
            parse_item_file(&out, &spec(ItemKind::Proposal), "p.md").unwrap(),
            file
        );
    }

    #[test]
    fn parse_errors() {
        for (src, why) in [
            ("no fence\n", "no +++"),
            ("+++\nid = \"I-001\"\n", "unclosed"),
            ("+++\n+++\n", "missing keys"),
            (
                "+++\nid = \"x\"\ntitle = \"t\"\nstatus = \"open\"\n+++\n",
                "bad id",
            ),
            (
                "+++\nid = \"I-001\"\ntitle = \"t\"\nstatus = \"bogus\"\n+++\n",
                "bad status",
            ),
            (
                "+++\nid = \"I-001\"\ntitle = \"t\"\nstatus = \"open\"\nowner = \"a b\"\n+++\n",
                "bad actor",
            ),
            (
                "+++\nid = \"I-001\"\ntitle = \"t\"\nstatus = \"open\"\ncreated = \"soon\"\n+++\n",
                "bad date",
            ),
            (
                "+++\nid = \"I-001\"\ntitle = \"t\"\nstatus = \"open\"\nlabels = [\"a b\"]\n+++\n",
                "bad label",
            ),
            (
                "+++\nid = \"I-001\"\ntitle = \"t\"\nstatus = \"open\"\nblocks = [\"x\"]\n+++\n",
                "bad blocks",
            ),
            (
                "+++\nid = \"P-001\"\ntitle = \"t\"\nstatus = \"open\"\n+++\n",
                "wrong kind",
            ),
            (
                "+++\nid = \"I-001\"\ntitle = \"t\"\nstatus = \"open\"\nwho = \"?\"\n+++\n",
                "unknown key ok?",
            ),
        ] {
            let r = parse_item_file(src, &spec(ItemKind::Issue), "issues/x.md");
            assert!(r.is_err() == (why != "unknown key ok?"), "{why}: {src:?}");
        }
    }

    #[test]
    fn filename_id_must_match_frontmatter() {
        // `issue-0003-x.md` claiming `I-009` — the scan would
        // reserve 3 while the file holds 9; that's corruption, not
        // a rename waiting to happen
        let src = "+++\nid = \"I-009\"\ntitle = \"x\"\nstatus = \"open\"\n+++\n";
        assert!(parse_item_file(src, &spec(ItemKind::Issue), "issues/issue-0003-x.md").is_err());
        // agreement parses; a name carrying no number isn't checked
        let src = "+++\nid = \"I-003\"\ntitle = \"x\"\nstatus = \"open\"\n+++\n";
        assert!(parse_item_file(src, &spec(ItemKind::Issue), "issues/issue-0003-x.md").is_ok());
        assert!(parse_item_file(src, &spec(ItemKind::Issue), "issues/x.md").is_ok());
        // kind mismatch stays a kind error (checked first)
        let src = "+++\nid = \"P-002\"\ntitle = \"x\"\nstatus = \"open\"\n+++\n";
        assert!(parse_item_file(src, &spec(ItemKind::Issue), "issues/issue-0002-x.md").is_err());
        // but parses under its own kind even inside issues/ — the
        // filename's `issue` prefix claims no proposal number
        assert!(parse_item_file(src, &spec(ItemKind::Proposal), "issues/issue-0002-x.md").is_ok());
    }

    #[test]
    fn emit_validates_the_model() {
        let good = ItemFile::new(Item::new(ItemId::new(ItemKind::Issue, 1), "x"));
        assert!(emit_item_file(&good).is_ok());
        // an invalid label — parse would reject the emitted file
        let mut bad = good.clone();
        bad.meta.labels.insert("bad label".into());
        assert!(emit_item_file(&bad).is_err());
        // invalid group names too
        let mut bad = good.clone();
        bad.meta.epic = Some("bad epic".into());
        assert!(emit_item_file(&bad).is_err());
        bad.meta.epic = None;
        bad.meta.milestone = Some("bad/ms".into());
        assert!(emit_item_file(&bad).is_err());
        // an empty/unemittable note
        let mut bad = good.clone();
        bad.meta.note = Some(String::new());
        assert!(emit_item_file(&bad).is_err());
        // children are an error, never silently dropped
        let mut bad = good;
        bad.meta
            .children
            .push(Item::new(ItemId::new(ItemKind::Task, 1), "sub"));
        let e = emit_item_file(&bad).unwrap_err();
        assert!(e.to_string().contains("children"), "{e}");
    }

    #[test]
    fn unknown_keys_tolerated() {
        // forward-compat: files written by a newer myco still load
        let src =
            "+++\nid = \"I-001\"\ntitle = \"t\"\nstatus = \"open\"\nfuture = 42\n+++\n\nbody\n";
        let file = parse_item_file(src, &spec(ItemKind::Issue), "issues/x.md").unwrap();
        assert_eq!(file.body, "body");
    }

    #[test]
    fn crlf_body_and_crlf_file() {
        let src = ISSUE.replace('\n', "\r\n");
        let file = parse_item_file(&src, &spec(ItemKind::Issue), "x.md").unwrap();
        assert_eq!(file.body, "## Description\n\nBody text.");
    }
}
