//! `myco handoff` / `myco resume` / `myco interrupt` — the
//! continuity trio. At session end `handoff` appends a
//! `## <today> · <actor>` entry plus note to `handoffs/<ID>.md`
//! (append-only, one file per item, uniform across kinds). At
//! session start `resume` emits the minimal restart context — the
//! item, its `blocks:` dep titles, the latest handoff entries,
//! the open checklist — sized for a prompt, the single-item
//! counterpart of `status --prompt`. `interrupt` is the abnormal
//! end: a work-state item (claimed/in-progress/blocked) reverts
//! to `open` and gets an `interrupted` handoff in one commit.
//!
//! sepia owns session lifecycle; it calls these verbs from hooks.
//! `--session <id>` is the hook contract: `handoff`/`interrupt`
//! tag their entry with it, and the claiming verbs
//! (`next --claim`, `item set`) record it as a `claimed`
//! handoff entry — the tag lives in the entry heading
//! (`session:<id>`), no item field. An item is "claimed under"
//! a session when its latest session-tagged handoff entry names
//! it ([`claim_session`]); `interrupt --session <id>` (no item
//! id) is the teardown — every live work-state item that session
//! last tagged reverts in one commit, so a dying session
//! releases exactly its own work.

use std::collections::BTreeSet;

use myco_core::{is_valid_session, Actor, HandoffEntry, ItemId, Status};
use myco_store::Store;
use serde_json::json;

use crate::{items, ops};

/// `--actor` fallback — the local CLI wrote it. Also `next
/// --claim`'s owner default and the `Actor:` commit trailer's.
pub(crate) use myco_ops::ops::DEFAULT_ACTOR;

/// Handoff entries shown by default (`--last` overrides).
const DEFAULT_LAST: usize = 10;

/// The note prefix `interrupt` writes — also the derivation rule:
/// an item "was interrupted" when its latest handoff note starts
/// with this word. No dedicated status — the event lives in the
/// append-only log; status/board/report surfaces derive the flag.
pub const INTERRUPT_NOTE: &str = "interrupted";

/// The note the claim-side verbs write when `--session` tags a
/// claim (`next --claim`, `item set` on a work-state item) — the
/// `session:<id>` heading is the tag `interrupt --session`
/// filters on; the note is for humans reading the log.
pub const CLAIM_NOTE: &str = "claimed";

/// `--session` values must fit the `session:<id>` heading token.
fn validate_session(s: &str) -> Result<(), String> {
    if !is_valid_session(s) {
        return Err(format!(
            "invalid --session {s:?} — sessions match `[A-Za-z0-9._:-]+`"
        ));
    }
    Ok(())
}

/// Which session holds the item's claim — the `session:` of its
/// latest handoff entry that carries one. An untagged entry is
/// anonymous: it neither claims nor transfers; another session's
/// tagged write does. `None` = no session ever declared one.
///
/// Known edge: an item claimed under `s`, interrupted, then
/// re-claimed *without* `--session` still reads as `s`'s — an
/// untagged claim leaves no tag to supersede. Session-aware
/// callers tag every claim (`--session`) and the rule stays
/// exact.
pub fn claim_session(store: &impl Store, id: ItemId) -> Result<Option<String>, String> {
    let entries = store.read_handoffs(id).map_err(|e| e.to_string())?;
    Ok(entries.iter().rev().find_map(|e| e.session.clone()))
}

/// An item's writable pre-image — the line ledger plus its item
/// file when file-backed — snapshotted so a paired write (status
/// revert + marker entry; claim mutation + tag entry) can roll
/// the first half back when the second fails instead of leaving
/// it on disk for the *next* verb's autocommit to sweep up under
/// its own message.
struct Preimage {
    ledger: myco_core::Ledger,
    file: Option<myco_core::ItemFile>,
}

fn preimage(store: &impl Store, id: ItemId) -> Result<Preimage, String> {
    let ledger = items::load_ledger(store)?;
    let file = match store.item(id) {
        Ok(f) => Some(f),
        Err(myco_store::Error::NotFound { .. }) => None,
        Err(e) => return Err(e.to_string()),
    };
    Ok(Preimage { ledger, file })
}

/// Best-effort restore of a [`Preimage`] — returns the legs that
/// wouldn't write so the caller can report what stuck.
fn restore(store: &impl Store, pre: &Preimage) -> Vec<String> {
    let mut stuck = Vec::new();
    if let Err(r) = items::save_ledger(store, &pre.ledger) {
        stuck.push(format!("ledger: {r}"));
    }
    if let Some(f) = &pre.file {
        if let Err(r) = store.save_item(f) {
            stuck.push(format!("item file: {r}"));
        }
    }
    stuck
}

/// Did the item's latest handoff mark it interrupted? A read
/// failure propagates — a malformed handoff file is a finding,
/// not a silent "not interrupted".
pub fn is_interrupted(store: &impl Store, id: ItemId) -> Result<bool, String> {
    let entries = store.read_handoffs(id).map_err(|e| e.to_string())?;
    Ok(entries
        .last()
        .is_some_and(|e| e.note.trim_start().starts_with(INTERRUPT_NOTE)))
}

/// The interrupted subset of `items` — for status/board markers.
/// The first unreadable handoff file fails the set.
pub fn interrupted_set<'a>(
    store: &impl Store,
    items: impl Iterator<Item = &'a myco_core::Item>,
) -> Result<BTreeSet<ItemId>, String> {
    items
        .map(|i| is_interrupted(store, i.id).map(|hit| (i.id, hit)))
        .filter_map(|r| match r {
            Ok((id, true)) => Some(Ok(id)),
            Ok(_) => None,
            Err(e) => Some(Err(e)),
        })
        .collect()
}

/// `--note` inline, or `-` for stdin.
fn read_note(note: &str) -> Result<String, String> {
    let note = if note == "-" {
        use std::io::Read;
        let mut s = String::new();
        std::io::stdin()
            .read_to_string(&mut s)
            .map_err(|e| format!("read --note from stdin: {e}"))?;
        s
    } else {
        note.to_string()
    };
    let note = note.trim().to_string();
    if note.is_empty() {
        return Err("handoff: --note is empty".into());
    }
    Ok(note)
}

/// The handoff write — the item must exist somewhere in the
/// ledger (any kind, live or archived). Validates the note and
/// the session id, appends the entry, and returns it plus the
/// worktree-relative path (the file lands wherever it already
/// lived). Caller commits.
pub fn record_handoff(
    store: &impl Store,
    worktree: &std::path::Path,
    id: ItemId,
    note: String,
    actor: Option<Actor>,
    session: Option<String>,
) -> Result<(HandoffEntry, String), String> {
    if items::find_item(store, id)?.is_none() {
        return Err(format!("{id}: no such item — handoffs attach to items"));
    }
    let note = note.trim().to_string();
    if note.is_empty() {
        return Err("handoff: note is empty".into());
    }
    if let Some(s) = &session {
        validate_session(s)?;
    }
    let entry = HandoffEntry {
        date: items::today(),
        actor: match actor {
            Some(a) => a,
            None => Actor::new(DEFAULT_ACTOR).map_err(|e| e.to_string())?,
        },
        session,
        note,
    };
    store
        .append_handoff(id, &entry)
        .map_err(|e| e.to_string())?;
    // the file lands wherever it already lived
    let live = format!("handoffs/{id}.md");
    let rel = if worktree.join(&live).exists() {
        live
    } else {
        format!("handoffs/archive/{id}.md")
    };
    Ok((entry, rel))
}

/// The `handoff` verb — `-` reads the note from stdin.
pub fn h_handoff(ctx: &crate::registry::Ctx, a: &crate::registry::Args) -> Result<i32, String> {
    let (proj, store) = items::open(ctx.dir.as_deref(), ctx.root.as_deref())?;
    let (entry, rel) = record_handoff(
        &store,
        &proj.worktree,
        a.req::<ItemId>("id")?,
        read_note(a.text("note")?)?,
        a.parsed("actor")?,
        a.opt("session").map(str::to_string),
    )?;
    let id = a.req::<ItemId>("id")?;
    let committed = items::commit(
        &proj.worktree,
        &format!("chore: handoff {id}"),
        Some(&entry.actor),
        ctx.no_commit,
    )?;
    items::report(
        json!({"id": id.to_string(), "path": rel, "entry": entry}),
        committed,
        ctx.json,
        &format!("handoff {id} → {rel}"),
    )
}

/// The interrupt write — a session ended with work still
/// claimed/in-progress/blocked. Reverts the item to `open`
/// through the shared `item set` mutation path (relocation rules
/// included) and appends the `interrupted` marker entry. Caller
/// commits — one commit covers both writes.
///
/// `session` scopes as well as tags: when passed, the item's
/// claim must be held by that session ([`claim_session`]) —
/// `interrupt <id> --session <s>` can't clobber a claim it
/// doesn't own. Without `session` the interrupt is unconditional
/// (the escape hatch).
pub fn interrupt_item(
    store: &impl Store,
    id: ItemId,
    note: Option<&str>,
    actor: Option<Actor>,
    session: Option<String>,
) -> Result<HandoffEntry, String> {
    let Some((item, _, _)) = items::find_item(store, id)? else {
        return Err(format!("{id}: no such item"));
    };
    match item.status {
        Status::Claimed | Status::InProgress | Status::Blocked => {}
        s => {
            return Err(format!(
                "{id}: status is {s} — nothing to interrupt \
                 (only claimed/in-progress/blocked revert to open)"
            ));
        }
    }
    if let Some(s) = &session {
        validate_session(s)?;
        match claim_session(store, id)? {
            Some(held) if held == *s => {}
            held => {
                let who = held.map_or_else(|| "no session".into(), |h| format!("session {h}"));
                return Err(format!(
                    "{id}: claimed under {who}, not session {s} — \
                     drop --session to interrupt anyway"
                ));
            }
        }
    }
    let note = match note.map(str::trim).filter(|n| !n.is_empty()) {
        Some(extra) => format!("{INTERRUPT_NOTE} — {extra}"),
        None => INTERRUPT_NOTE.to_string(),
    };
    let entry = HandoffEntry {
        date: items::today(),
        actor: match actor {
            Some(a) => a,
            None => Actor::new(DEFAULT_ACTOR).map_err(|e| e.to_string())?,
        },
        session,
        note,
    };
    // two writes share the caller's one commit — snapshot the
    // pre-image so a failed handoff append can roll the status
    // revert back
    let pre = preimage(store, id)?;
    // revert through the shared mutation path (relocation rules
    // included), then the marker handoff
    crate::itemcmd::mutate_item(store, id, &crate::itemcmd::SetFlags::status(Status::Open))?;
    if let Err(e) = store.append_handoff(id, &entry) {
        let stuck = restore(store, &pre);
        return Err(if stuck.is_empty() {
            format!("{id}: handoff append failed ({e}) — status revert rolled back")
        } else {
            format!(
                "{id}: handoff append failed ({e}) — status revert left on disk \
                 (rollback: {})",
                stuck.join("; ")
            )
        });
    }
    Ok(entry)
}

/// The claim-side write behind `--session` on the claiming verbs
/// (`next --claim`, `item set`): apply `flags` through the shared
/// mutation path and record the claim as a `claimed` handoff
/// entry tagged with the session — the tag
/// [`claim_session`]/`interrupt --session` read. The item must
/// land (or already sit) in a work state — `--session` tags a
/// claim, and an open item has none. Caller commits — one commit
/// covers both writes; a failed tag rolls the mutation back.
/// `actor` names the claimer (falls back to the item's
/// post-mutation owner, then `cli`).
pub fn tag_claim(
    store: &impl Store,
    id: ItemId,
    flags: &crate::itemcmd::SetFlags,
    actor: Option<Actor>,
    session: &str,
) -> Result<(Option<myco_core::Item>, String, HandoffEntry), String> {
    let Some((item, _, _)) = items::find_item(store, id)? else {
        return Err(format!("{id}: no such item"));
    };
    let post = flags.status.unwrap_or(item.status);
    if !matches!(post, Status::Claimed | Status::InProgress | Status::Blocked) {
        return Err(format!(
            "{id}: --session tags a claim — the item would be {post}, which has no claim"
        ));
    }
    validate_session(session)?;
    let pre = preimage(store, id)?;
    let (item, location) = crate::itemcmd::mutate_item(store, id, flags)?;
    let entry = HandoffEntry {
        date: items::today(),
        actor: actor
            .or_else(|| item.as_ref().and_then(|i| i.owner.clone()))
            .unwrap_or(Actor::new(DEFAULT_ACTOR).map_err(|e| e.to_string())?),
        session: Some(session.to_string()),
        note: CLAIM_NOTE.to_string(),
    };
    if let Err(e) = store.append_handoff(id, &entry) {
        let stuck = restore(store, &pre);
        return Err(if stuck.is_empty() {
            format!("{id}: claim tag append failed ({e}) — mutation rolled back")
        } else {
            format!(
                "{id}: claim tag append failed ({e}) — mutation left on disk \
                 (rollback: {})",
                stuck.join("; ")
            )
        });
    }
    Ok((item, location, entry))
}

/// A session teardown's outcome — every item that reverted plus
/// the per-item failures the run walked past.
pub struct SessionInterrupts {
    /// `(id, marker entry)` per reverted item, in ledger order.
    pub interrupted: Vec<(ItemId, HandoffEntry)>,
    /// `(id, error)` per failure — collected, not fatal: the
    /// items that did revert still commit.
    pub errors: Vec<(ItemId, String)>,
}

/// `interrupt --session <s>` — the bulk teardown. Every live
/// work-state item whose claim [`claim_session`] reads as
/// `session`'s reverts + marks. Caller commits once for the whole
/// sweep — a malformed handoff or failed revert reports per item
/// and doesn't abort the rest.
pub fn interrupt_session(
    store: &impl Store,
    session: &str,
    note: Option<&str>,
    actor: Option<Actor>,
) -> Result<SessionInterrupts, String> {
    validate_session(session)?;
    let scan = crate::digest::scan(store)?;
    let candidates: Vec<ItemId> = scan
        .live()
        .filter(|i| {
            matches!(
                i.status,
                Status::Claimed | Status::InProgress | Status::Blocked
            )
        })
        .map(|i| i.id)
        .collect();
    let mut out = SessionInterrupts {
        interrupted: Vec::new(),
        errors: Vec::new(),
    };
    for id in candidates {
        match claim_session(store, id) {
            Ok(Some(held)) if held == session => {
                match interrupt_item(store, id, note, actor.clone(), Some(session.to_string())) {
                    Ok(entry) => out.interrupted.push((id, entry)),
                    Err(e) => out.errors.push((id, e)),
                }
            }
            Ok(_) => {}
            Err(e) => out.errors.push((id, e)),
        }
    }
    Ok(out)
}

/// The bulk report — `{session, interrupted[], errors[],
/// committed}` for `--json`, one line plus per-item stderr
/// errors for humans. Returns the exit code (1 when any item
/// failed — the reverts still committed).
fn session_interrupt_report(
    session: &str,
    out: &SessionInterrupts,
    committed: bool,
    json_out: bool,
) -> i32 {
    if json_out {
        ops::print_json(&json!({
            "session": session,
            "interrupted": out.interrupted.iter().map(|(id, e)| {
                json!({"id": id.to_string(), "status": "open", "entry": e})
            }).collect::<Vec<_>>(),
            "errors": out.errors.iter().map(|(id, e)| {
                json!({"id": id.to_string(), "error": e})
            }).collect::<Vec<_>>(),
            "committed": committed,
        }));
    } else {
        let ids: Vec<String> = out
            .interrupted
            .iter()
            .map(|(id, _)| id.to_string())
            .collect();
        if ids.is_empty() {
            println!("nothing claimed under session {session}");
        } else {
            println!("interrupted {} → open (session {session})", ids.join(", "));
        }
        for (id, e) in &out.errors {
            eprintln!("myco: error: {id}: {e}");
        }
    }
    if out.errors.is_empty() {
        0
    } else {
        1
    }
}

/// The `interrupt` verb — called by whoever owns session
/// lifecycle (sepia hooks, CLI session-end hooks); myco never
/// detects sessions itself. With an id it reverts that one item
/// (`--session` asserts the claim is that session's); without an
/// id, `--session <id>` is the teardown — every item claimed
/// under the session reverts in one commit.
pub fn h_interrupt(ctx: &crate::registry::Ctx, a: &crate::registry::Args) -> Result<i32, String> {
    let (proj, store) = items::open(ctx.dir.as_deref(), ctx.root.as_deref())?;
    let id: Option<ItemId> = a.parsed("id")?;
    let session = a.opt("session").map(str::to_string);
    match (id, session) {
        (Some(id), session) => {
            let entry = interrupt_item(&store, id, a.opt("note"), a.parsed("actor")?, session)?;
            let committed = items::commit(
                &proj.worktree,
                &format!("chore: interrupt {id}"),
                Some(&entry.actor),
                ctx.no_commit,
            )?;
            items::report(
                json!({"id": id.to_string(), "status": "open", "entry": entry}),
                committed,
                ctx.json,
                &format!("interrupted {id} → open"),
            )
        }
        (None, Some(session)) => {
            let actor: Option<Actor> = a.parsed("actor")?;
            let out = interrupt_session(&store, &session, a.opt("note"), actor.clone())?;
            let committed = items::commit(
                &proj.worktree,
                &format!("chore: interrupt session {session}"),
                actor.as_ref(),
                ctx.no_commit,
            )?;
            Ok(session_interrupt_report(
                &session, &out, committed, ctx.json,
            ))
        }
        (None, None) => Err(
            "interrupt: pass an item id, or --session <id> to interrupt \
             everything a session claimed"
                .into(),
        ),
    }
}

/// Unchecked `- [ ]` checkbox items in a markdown body — the open
/// checklist `resume` surfaces (issue bodies, plan phases).
fn open_checklist(body: &str) -> Vec<String> {
    body.lines()
        .filter_map(|l| {
            let t = l.trim_start();
            for m in ["- [ ]", "* [ ]", "+ [ ]"] {
                if let Some(rest) = t.strip_prefix(m) {
                    if rest.is_empty() || rest.starts_with(char::is_whitespace) {
                        return Some(rest.trim().to_string());
                    }
                }
            }
            None
        })
        .collect()
}

/// The gathered `resume` context — the typed form behind
/// [`resume_json`] and the human block.
struct ResumeData {
    item: myco_core::Item,
    location: String,
    blocks_titles: Vec<serde_json::Value>,
    links_titles: Vec<serde_json::Value>,
    shown: Vec<HandoffEntry>,
    total: usize,
    checklist: Vec<String>,
}

/// Gather the resume context — the item, `blocks:`/`links:` dep
/// titles, the latest handoff entries, the open checklist.
fn resume_data(store: &impl Store, id: ItemId, last: Option<usize>) -> Result<ResumeData, String> {
    let Some((item, location, body)) = items::find_item(store, id)? else {
        return Err(format!("{id}: no such item"));
    };
    let dep_titles = |ids: &std::collections::BTreeSet<myco_core::ItemId>| {
        ids.iter()
            .map(|b| {
                let title = items::find_item(store, *b)?.map(|(i, _, _)| i.title);
                Ok(json!({"id": b.to_string(), "title": title}))
            })
            .collect::<Result<Vec<_>, String>>()
    };
    let blocks_titles = dep_titles(&item.blocks)?;
    let links_titles = dep_titles(&item.links)?;
    let entries = store.read_handoffs(id).map_err(|e| e.to_string())?;
    let total = entries.len();
    let n = last.unwrap_or(DEFAULT_LAST);
    let shown: Vec<HandoffEntry> = entries
        .iter()
        .rev()
        .take(n)
        .cloned()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let checklist = body.as_deref().map(open_checklist).unwrap_or_default();
    Ok(ResumeData {
        item,
        location,
        blocks_titles,
        links_titles,
        shown,
        total,
        checklist,
    })
}

/// `ResumeData` → the `{item, location, blocks_titles, links_titles,
/// handoffs[], open_checklist}` payload shape.
fn resume_value(d: &ResumeData) -> serde_json::Value {
    json!({
        "item": d.item,
        "location": d.location,
        "blocks_titles": d.blocks_titles,
        "links_titles": d.links_titles,
        "handoffs": d.shown,
        "handoffs_total": d.total,
        "open_checklist": d.checklist,
    })
}

/// The resume payload — `{item, location, blocks_titles,
/// links_titles, handoffs[], open_checklist}` — shared by
/// `--json` and `myco serve`.
pub fn resume_json(
    store: &impl Store,
    id: ItemId,
    last: Option<usize>,
) -> Result<serde_json::Value, String> {
    Ok(resume_value(&resume_data(store, id, last)?))
}

/// One item's prompt-ready block — the compact item line first,
/// sections only when they carry context. Identical for single-
/// and multi-id output (blocks join on a blank line).
fn resume_block(d: &ResumeData) -> String {
    let mut out = format!("{}\nin: {}", items::compact(&d.item), d.location);
    if !d.blocks_titles.is_empty() {
        out.push_str("\nblocks:");
        for b in &d.blocks_titles {
            out.push_str(&format!(
                "\n  {} {}",
                b["id"].as_str().unwrap_or_default(),
                b["title"].as_str().unwrap_or("(missing)")
            ));
        }
    }
    if !d.links_titles.is_empty() {
        out.push_str("\nlinks:");
        for l in &d.links_titles {
            out.push_str(&format!(
                "\n  {} {}",
                l["id"].as_str().unwrap_or_default(),
                l["title"].as_str().unwrap_or("(missing)")
            ));
        }
    }
    if !d.shown.is_empty() {
        out.push_str("\nhandoffs:");
        for e in &d.shown {
            let mut head = format!("{} · {}", e.date, e.actor);
            if let Some(s) = &e.session {
                head.push_str(&format!(" · session:{s}"));
            }
            out.push_str(&format!("\n{head}"));
            for line in e.note.lines() {
                out.push_str(&format!("\n  {line}"));
            }
            out.push('\n');
        }
        if d.total > d.shown.len() {
            out.push_str(&format!("\n… +{} earlier entries", d.total - d.shown.len()));
        }
    }
    if !d.checklist.is_empty() {
        out.push_str("\nopen checklist:");
        for c in &d.checklist {
            out.push_str(&format!("\n  - [ ] {c}"));
        }
    }
    out
}

/// The `resume` verb — 1..n ids, one discover + store open.
/// `--json` is the single-id payload for one id, else the
/// `[{id, ok, payload|error}]` batch array. Human output is the
/// prompt block per id in argument order; a bad id reports to
/// stderr and exits 1 without aborting the rest.
/// The `resume` verb — 1..n ids, one discover + store open.
pub fn h_resume(ctx: &crate::registry::Ctx, a: &crate::registry::Args) -> Result<i32, String> {
    let ids = a.many_parsed::<ItemId>("id")?;
    let last: Option<usize> = a.parsed("last")?;
    let json_out = ctx.json;
    let (_proj, store) = items::open_read(ctx.dir.as_deref(), ctx.root.as_deref())?;
    if ids.len() == 1 {
        let d = resume_data(&store, ids[0], last)?;
        if json_out {
            ops::print_json(&resume_value(&d));
        } else {
            println!("{}", resume_block(&d));
        }
        return Ok(0);
    }
    if json_out {
        let (entries, failed) = items::multi_json(&ids, |id| resume_json(&store, id, last));
        ops::print_json(&entries);
        return Ok(if failed { 1 } else { 0 });
    }
    let mut blocks = Vec::new();
    let mut failed = false;
    for id in ids {
        match resume_data(&store, id, last) {
            Ok(d) => blocks.push(resume_block(&d)),
            Err(e) => {
                failed = true;
                eprintln!("myco: error: {e}");
            }
        }
    }
    if !blocks.is_empty() {
        println!("{}", blocks.join("\n\n"));
    }
    Ok(if failed { 1 } else { 0 })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use myco_core::{Item, ItemKind, Section};
    use myco_store_md::MdStore;

    #[test]
    fn checklist_extraction() {
        let body = "## Checklist\n\n- [ ] do a\n  - [x] done\n* [ ] star\n+ [ ] plus\n- [ ]not-a-box\n- plain\n\n- [ ]\n";
        assert_eq!(open_checklist(body), vec!["do a", "star", "plus", ""]);
        assert!(open_checklist("no items\n").is_empty());
    }

    /// A scaffolded worktree store — `meta.toml` + TODO.md +
    /// BACKLOG.md, no git (verbs write, no commit layer here).
    fn fixture() -> (tempfile::TempDir, MdStore) {
        let d = tempfile::tempdir().unwrap();
        let root = camino::Utf8PathBuf::from_path_buf(d.path().to_path_buf()).unwrap();
        let store = MdStore::new(root);
        crate::ops::scaffold(&store, d.path(), "p").unwrap();
        (d, store)
    }

    fn entry(session: Option<&str>, note: &str) -> HandoffEntry {
        HandoffEntry {
            date: items::today(),
            actor: Actor::new("devin").unwrap(),
            session: session.map(str::to_string),
            note: note.into(),
        }
    }

    fn task(store: &MdStore, n: u32, status: Status) -> ItemId {
        let id = ItemId::new(ItemKind::Task, n);
        let mut todo = store.todo().unwrap();
        if todo.sections.is_empty() {
            todo.sections.push(Section {
                name: None,
                items: Vec::new(),
            });
        }
        todo.sections[0]
            .items
            .push(Item::new(id, format!("t{n}")).status(status));
        store.save_todo(&todo).unwrap();
        id
    }

    #[test]
    fn claim_session_is_the_latest_tagged_entry() {
        let (_d, store) = fixture();
        let id = ItemId::new(ItemKind::Task, 1);
        // no log at all → no session
        assert_eq!(claim_session(&store, id).unwrap(), None);
        store
            .append_handoff(id, &entry(Some("s1"), "claimed"))
            .unwrap();
        assert_eq!(claim_session(&store, id).unwrap().as_deref(), Some("s1"));
        // an untagged entry is anonymous — doesn't transfer
        store.append_handoff(id, &entry(None, "wip")).unwrap();
        assert_eq!(claim_session(&store, id).unwrap().as_deref(), Some("s1"));
        // another session's tagged write takes it
        store
            .append_handoff(id, &entry(Some("s2"), "claimed"))
            .unwrap();
        assert_eq!(claim_session(&store, id).unwrap().as_deref(), Some("s2"));
    }

    #[test]
    fn interrupt_scopes_to_the_claim_session() {
        let (_d, store) = fixture();
        let id = task(&store, 1, Status::Claimed);
        store
            .append_handoff(id, &entry(Some("s1"), "claimed"))
            .unwrap();
        // another session can't interrupt it
        let err = interrupt_item(&store, id, None, None, Some("s2".into())).unwrap_err();
        assert!(err.contains("claimed under session s1"), "{err}");
        assert_eq!(
            store.todo().unwrap().items().next().unwrap().status,
            Status::Claimed
        );
        // its own session can
        let e = interrupt_item(&store, id, None, None, Some("s1".into())).unwrap();
        assert_eq!(e.session.as_deref(), Some("s1"));
        assert_eq!(
            store.todo().unwrap().items().next().unwrap().status,
            Status::Open
        );
        // untagged interrupt stays unconditional
        let id2 = task(&store, 2, Status::InProgress);
        interrupt_item(&store, id2, None, None, None).unwrap();
    }

    #[test]
    fn tag_claim_mutation_and_entry_commit_together() {
        let (_d, store) = fixture();
        let id = task(&store, 1, Status::Open);
        let flags = crate::itemcmd::SetFlags {
            status: Some(Status::Claimed),
            owner: Some(Actor::new("devin").unwrap()),
            ..Default::default()
        };
        let (item, _loc, e) = tag_claim(&store, id, &flags, None, "s9").unwrap();
        assert_eq!(item.unwrap().status, Status::Claimed);
        assert_eq!(e.note, CLAIM_NOTE);
        assert_eq!(e.session.as_deref(), Some("s9"));
        // the claimer is the post-mutation owner
        assert_eq!(e.actor.as_str(), "devin");
        assert_eq!(claim_session(&store, id).unwrap().as_deref(), Some("s9"));
        // open items have no claim to tag
        let id2 = task(&store, 2, Status::Open);
        let flags = crate::itemcmd::SetFlags::default();
        assert!(tag_claim(&store, id2, &flags, None, "s9").is_err());
    }

    #[test]
    fn interrupt_session_reverts_only_its_own() {
        let (_d, store) = fixture();
        // s1 claims T-1, s2 claims T-2, T-3 is worked session-less
        let t1 = task(&store, 1, Status::InProgress);
        let t2 = task(&store, 2, Status::Claimed);
        let t3 = task(&store, 3, Status::InProgress);
        store
            .append_handoff(t1, &entry(Some("s1"), "claimed"))
            .unwrap();
        store
            .append_handoff(t2, &entry(Some("s2"), "claimed"))
            .unwrap();
        let out = interrupt_session(&store, "s1", None, None).unwrap();
        assert_eq!(out.interrupted.len(), 1);
        assert_eq!(out.interrupted[0].0, t1);
        assert!(out.errors.is_empty());
        let todo = store.todo().unwrap();
        let status_of = |id| todo.items().find(|i| i.id == id).map(|i| i.status).unwrap();
        assert_eq!(status_of(t1), Status::Open);
        assert_eq!(status_of(t2), Status::Claimed);
        assert_eq!(status_of(t3), Status::InProgress);
        // s2's teardown lands the rest, t3 untouched
        let out = interrupt_session(&store, "s2", None, None).unwrap();
        assert_eq!(out.interrupted.len(), 1);
        assert_eq!(out.interrupted[0].0, t2);
    }
}
