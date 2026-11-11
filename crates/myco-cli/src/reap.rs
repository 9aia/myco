//! `myco reap` — the bulk stale-claim reaper. A scheduler
//! (peregrin campaign, a cron'd `myco batch`) calls it
//! periodically: every live `claimed`/`in-progress` item whose
//! last activity — the latest handoff entry's date, else
//! `created` — is older than `--idle-days` (default
//! [`report::STALE_CLAIM_DAYS`], the same threshold `report`'s
//! stale-claims finding uses) reverts to `open` with an
//! `interrupted` handoff — exactly `myco interrupt <ID>`, batched:
//! per-item failures are collected, the rest still land, and the
//! whole sweep is ONE commit (`chore: reap N stale claims`).
//! `--session <id>` scopes to items whose claim tag names that
//! session (the [`continuity::claim_session`] rule `interrupt
//! --session` uses); `--dry-run` lists the sweep with zero writes.

use myco_core::{Actor, HandoffEntry, ItemId, Status};
use myco_store::Store;
use serde_json::{json, Value};

use crate::{continuity, digest, items, ops, registry, report};

/// One reap candidate — everything the report needs, captured at
/// scan time so the JSON/human output survives the mutations.
#[derive(Debug, Clone)]
pub struct ReapHit {
    /// The item.
    pub id: ItemId,
    /// Title at scan time.
    pub title: String,
    /// Owner at scan time (`null` in JSON when unowned).
    pub owner: Option<Actor>,
    /// The session holding the item's claim tag, when one exists.
    pub session: Option<String>,
    /// Whole days since last activity.
    pub idle_days: i64,
    /// Status at scan time (`claimed` | `in-progress`).
    pub status: Status,
}

/// A hit plus the `interrupted` marker entry `apply` wrote.
pub struct Reaped {
    /// The candidate as scanned.
    pub hit: ReapHit,
    /// The marker handoff entry.
    pub entry: HandoffEntry,
}

/// The sweep plan — candidates plus per-item scan errors.
#[derive(Debug, Default)]
pub struct ReapPlan {
    /// Matching items, in ledger order.
    pub hits: Vec<ReapHit>,
    /// `(id, error)` — an unreadable handoff log or claim-tag
    /// read reports per item and doesn't abort the sweep (same
    /// rule as `interrupt --session`).
    pub errors: Vec<(ItemId, String)>,
}

/// `--idle-days` → the threshold — absent means the report's
/// stale-claim constant; `Val::Count` already bounds it to `u64`,
/// so the `i64` conversion only fails past `i64::MAX`.
pub fn idle_threshold(a: &registry::Args) -> Result<i64, String> {
    match a.parsed::<u64>("idle-days")? {
        Some(n) => i64::try_from(n).map_err(|_| "--idle-days is too large".to_string()),
        None => Ok(report::STALE_CLAIM_DAYS),
    }
}

/// The scan half — live `claimed`/`in-progress` items whose last
/// activity is older than `idle_days`, optionally scoped to one
/// claim session. Pure read: no writes, safe for `--dry-run`.
pub fn plan(store: &impl Store, session: Option<&str>, idle_days: i64) -> Result<ReapPlan, String> {
    if let Some(s) = session {
        continuity::validate_session(s)?;
    }
    let scan = digest::scan(store)?;
    let today = items::today();
    let mut out = ReapPlan::default();
    for i in scan.live() {
        if !matches!(i.status, Status::Claimed | Status::InProgress) {
            continue;
        }
        let last = match store.read_handoffs(i.id) {
            Ok(entries) => entries.last().map(|e| e.date).or(i.created),
            Err(e) => {
                out.errors.push((i.id, e.to_string()));
                continue;
            }
        };
        let Some(last) = last else { continue };
        let idle = (today - last).whole_days();
        if idle <= idle_days {
            continue;
        }
        let sess = match continuity::claim_session(store, i.id) {
            Ok(s) => s,
            Err(e) => {
                out.errors.push((i.id, e));
                continue;
            }
        };
        if let Some(want) = session {
            if sess.as_deref() != Some(want) {
                continue;
            }
        }
        out.hits.push(ReapHit {
            id: i.id,
            title: i.title.clone(),
            owner: i.owner.clone(),
            session: sess,
            idle_days: idle,
            status: i.status,
        });
    }
    Ok(out)
}

/// The mutating half — each hit through [`continuity::interrupt_item`]
/// (the same status revert + `interrupted` marker), the item's own
/// claim session carried through so the marker stays tagged and a
/// claim that moved between plan and apply can't be clobbered.
/// Per-item failures are collected — the rest still land. Caller
/// commits once for the whole sweep.
pub fn apply(
    store: &impl Store,
    hits: &[ReapHit],
    actor: &Actor,
) -> (Vec<Reaped>, Vec<(ItemId, String)>) {
    let mut reaped = Vec::new();
    let mut errors = Vec::new();
    for hit in hits {
        let note = format!("reaped after {} idle days", hit.idle_days);
        match continuity::interrupt_item(store, hit.id, Some(&note), actor, hit.session.clone()) {
            Ok(entry) => reaped.push(Reaped {
                hit: hit.clone(),
                entry,
            }),
            Err(e) => errors.push((hit.id, e)),
        }
    }
    (reaped, errors)
}

fn hit_json(h: &ReapHit) -> Value {
    json!({
        "id": h.id.to_string(),
        "title": h.title,
        "owner": h.owner.as_ref().map(Actor::as_str),
        "session": h.session,
        "idle_days": h.idle_days,
    })
}

/// The `--json`/wire payload — `{"reaped": [{id,title,owner,
/// session,idle_days}], "errors": [{id,error}], "dry_run": bool,
/// "committed": bool}`.
pub fn payload(
    hits: &[ReapHit],
    errors: &[(ItemId, String)],
    dry_run: bool,
    committed: bool,
) -> Value {
    json!({
        "reaped": hits.iter().map(hit_json).collect::<Vec<_>>(),
        "errors": errors.iter().map(|(id, e)| {
            json!({"id": id.to_string(), "error": e})
        }).collect::<Vec<_>>(),
        "dry_run": dry_run,
        "committed": committed,
    })
}

/// Shared result rendering — the `--json` payload or the human
/// block; per-item errors go to stderr either way. Returns the
/// exit code (1 when any item errored — the reverts still
/// committed, same rule as `interrupt --session`).
pub fn report(
    hits: &[ReapHit],
    errors: &[(ItemId, String)],
    idle_days: i64,
    dry_run: bool,
    committed: bool,
    json_out: bool,
) -> i32 {
    if json_out {
        ops::print_json(&payload(hits, errors, dry_run, committed));
    } else {
        if hits.is_empty() {
            println!("nothing to reap — no claimed/in-progress items idle >{idle_days}d");
        } else {
            let verb = if dry_run { "would reap" } else { "reaped" };
            println!(
                "{verb} {} stale claim(s) → open (idle >{idle_days}d)",
                hits.len()
            );
            for h in hits {
                let sess = h
                    .session
                    .as_deref()
                    .map_or_else(String::new, |s| format!(" · session:{s}"));
                println!("  {} · {}d idle{} · {}", h.id, h.idle_days, sess, h.title);
            }
        }
        for (id, e) in errors {
            eprintln!("myco: error: {id}: {e}");
        }
    }
    if errors.is_empty() {
        0
    } else {
        1
    }
}

/// The `reap` verb — scheduler-facing bulk interrupt. One commit
/// covers the whole sweep; zero matches is a clean no-op (no
/// commit, `committed: false`).
pub fn h_reap(ctx: &registry::Ctx, a: &registry::Args) -> Result<i32, String> {
    let idle_days = idle_threshold(a)?;
    let session = a.opt("session").map(str::to_string);
    let dry_run = a.flag("dry-run");
    let explicit: Option<Actor> = a.parsed("actor")?;
    // dry-run needs no write lock or sync — the shared read path
    let (proj, store) = if dry_run {
        items::open_read(ctx.dir.as_deref(), ctx.root.as_deref())?
    } else {
        items::open(ctx.dir.as_deref(), ctx.root.as_deref())?
    };
    let me = ops::resolve_actor(Some(&proj.worktree), explicit.as_ref());
    let p = plan(&store, session.as_deref(), idle_days)?;
    if dry_run {
        return Ok(report(&p.hits, &p.errors, idle_days, true, false, ctx.json));
    }
    let (reaped, errors) = apply(&store, &p.hits, &me);
    let mut all_errors = p.errors;
    all_errors.extend(errors);
    let hits: Vec<ReapHit> = reaped.iter().map(|r| r.hit.clone()).collect();
    let committed = if reaped.is_empty() {
        false
    } else {
        items::commit(
            &proj.worktree,
            &format!("chore: reap {} stale claims", reaped.len()),
            Some(&me),
            ctx.no_commit,
        )?
    };
    Ok(report(
        &hits,
        &all_errors,
        idle_days,
        false,
        committed,
        ctx.json,
    ))
}
