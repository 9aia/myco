//! Shared plumbing for the item verbs — the clap flag structs
//! (`--pri/--owner/…`, `--status/--label/--limit`, `--body/--text`)
//! that parse into `myco_ops::items` / `myco-core` types. The ops
//! layer itself is re-exported so call sites read `items::X` for
//! either.

use std::path::Path;

use myco_core::{ItemId, NewItemMeta};
use myco_store_md::MdStore;

pub use myco_ops::items::*;

/// The store the CLI's verbs share — `myco_ops`'s lock-holding
/// handle specialized to `MdStore` (the composition root names
/// the impl).
pub type LockedStore = myco_ops::items::LockedStore<MdStore>;

/// [`myco_ops::items::open_with`] wired to `MdStore` — verb call
/// sites stay two-arg.
pub fn open(
    dir: Option<&Path>,
    root: Option<&Path>,
) -> Result<(myco_ops::discover::Project, LockedStore), String> {
    myco_ops::items::open_with(dir, root, crate::ops::CONFIG, crate::ops::store_at)
}

/// [`myco_ops::items::open_read_with`] wired to `MdStore`.
pub fn open_read(
    dir: Option<&Path>,
    root: Option<&Path>,
) -> Result<(myco_ops::discover::Project, LockedStore), String> {
    myco_ops::items::open_read_with(dir, root, crate::ops::CONFIG, crate::ops::store_at)
}

/// `--pri/--owner/--label/--blocks/--epic/--milestone` → the
/// `NewItemMeta` every `add`/`new` verb applies.
pub fn meta_args(a: &crate::registry::Args) -> Result<NewItemMeta, String> {
    Ok(NewItemMeta {
        priority: a.parsed("pri")?,
        owner: a.parsed("owner")?,
        labels: a.many("label"),
        blocks: a.many_parsed("blocks")?,
        epic: a.opt("epic").map(str::to_string),
        milestone: a.opt("milestone").map(str::to_string),
    })
}

/// `--status/--owner/--label/--limit` → the `Filter` every `list`
/// verb shares.
pub fn list_args(a: &crate::registry::Args) -> Result<Filter, String> {
    Ok(Filter {
        status: a.parsed("status")?,
        owner: a.opt("owner").map(str::to_string),
        labels: a.many("label"),
        limit: a.parsed("limit")?,
    })
}

/// `--body FILE|-` reads a file or stdin, `--text` is inline —
/// the supplied body; `None` → the caller uses the template.
pub fn body_args(a: &crate::registry::Args) -> Result<Option<String>, String> {
    if let Some(t) = a.opt("text") {
        return Ok(Some(t.to_string()));
    }
    match a.opt("body") {
        None => Ok(None),
        Some("-") => {
            use std::io::Read;
            let mut s = String::new();
            std::io::stdin()
                .read_to_string(&mut s)
                .map_err(|e| format!("read --body from stdin: {e}"))?;
            Ok(Some(s))
        }
        Some(path) => std::fs::read_to_string(path)
            .map(Some)
            .map_err(|e| format!("read --body {path}: {e}")),
    }
}

/// `[{id, ok, payload|error}]` — the multi-id `--json` shape
/// (P-004): one entry per id in argument order; a bad or
/// missing id reports inline (`ok: false` + `error`) instead of
/// aborting the batch. Returns the array plus whether any id
/// failed — the caller maps that to a nonzero exit.
pub(crate) fn multi_json(
    ids: &[ItemId],
    f: impl Fn(ItemId) -> Result<serde_json::Value, String>,
) -> (serde_json::Value, bool) {
    let mut failed = false;
    let entries: Vec<serde_json::Value> = ids
        .iter()
        .map(|id| match f(*id) {
            Ok(payload) => {
                serde_json::json!({"id": id.to_string(), "ok": true, "payload": payload})
            }
            Err(error) => {
                failed = true;
                serde_json::json!({"id": id.to_string(), "ok": false, "error": error})
            }
        })
        .collect();
    (serde_json::json!(entries), failed)
}
