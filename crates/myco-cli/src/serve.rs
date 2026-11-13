//! `myco serve` — a sequential request/response verb surface for
//! embedding: an orchestrator (peregrin) spawns one long-lived
//! process per repo instead of one spawn per verb. A single
//! process serializes writes — no `index.lock` races — and the
//! store is re-opened per request, so the served process always
//! sees on-disk truth.
//!
//! Two abstractions keep the surface honest: [`ServeTransport`]
//! moves one raw line in and out (stdio first; a unix socket can
//! slot in untouched), and [`WireProtocol`] frames requests and
//! responses (JSON-RPC 2.0 first — the default, not the spec).
//! [`Server::dispatch`] is the verb surface itself and knows
//! neither. stdin IS the wire — no served method reads content
//! from it. `myco batch` drives the identical surface one-shot
//! through [`Server::handle_line`] — NDJSON in, NDJSON out.

use std::io::{BufRead, Write};
use std::path::PathBuf;

use myco_core::{
    eval, eval_scoped, Actor, GroupAxis, Item, ItemId, ItemKind, Priority, Scoped, SortKey, Status,
    ViewFormat, ViewSource, ViewSpec,
};
use myco_ops::view::Origin;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    backlog, check, continuity, digest, itemcmd, items, movecmd, next, ops, reap, report, schema,
    status, todo,
};

// ---------------------------------------------------------------- wire

/// JSON-RPC 2.0 error codes served on the wire.
pub const PARSE_ERROR: i64 = -32700;
/// The envelope decoded but isn't a callable request (also a
/// missing `id` — notifications aren't served).
pub const INVALID_REQUEST: i64 = -32600;
/// The method isn't on the v1 surface.
pub const METHOD_NOT_FOUND: i64 = -32601;
/// `params` didn't decode into the method's shape.
pub const INVALID_PARAMS: i64 = -32602;
/// The verb itself failed.
pub const VERB_ERROR: i64 = -32000;
/// A status-machine rejection — `data` carries
/// `{from, to, valid: []}`.
pub const INVALID_TRANSITION: i64 = -32001;

/// One decoded request — `id` is echoed back verbatim so callers
/// correlate responses on a shared pipe.
#[derive(Debug)]
pub struct Request {
    /// Wire-level correlation id.
    pub id: Value,
    /// The method name (`todo.add`, `status`, …).
    pub method: String,
    /// Method params (`null` when absent).
    pub params: Value,
}

/// One response to encode.
pub enum Response {
    /// The method's payload.
    Result {
        /// Correlation id echoed from the request.
        id: Value,
        /// The result value.
        result: Value,
    },
    /// A wire or dispatch failure.
    Error {
        /// Correlation id (`null` when the request never decoded).
        id: Value,
        /// One of the `-32xxx` codes.
        code: i64,
        /// Human/agent-readable message.
        message: String,
        /// Optional structured detail (transition targets, …).
        data: Option<Value>,
    },
}

/// A line that never decoded into a [`Request`].
#[derive(Debug)]
pub struct WireError {
    /// The error code.
    pub code: i64,
    /// What went wrong.
    pub message: String,
}

/// Request/response framing — the abstraction point. JSON-RPC
/// 2.0 is the default impl; [`Server::serve`] doesn't know it.
pub trait WireProtocol {
    /// Decode one raw line into a request.
    fn parse(&self, line: &str) -> Result<Request, WireError>;
    /// Encode one response as a single line.
    fn encode(&self, response: &Response) -> String;
}

/// JSON-RPC 2.0 — one request/response object per line.
pub struct JsonRpc2;

impl WireProtocol for JsonRpc2 {
    fn parse(&self, line: &str) -> Result<Request, WireError> {
        let v: Value = serde_json::from_str(line).map_err(|e| WireError {
            code: PARSE_ERROR,
            message: format!("parse error: {e}"),
        })?;
        let invalid = |message: &str| WireError {
            code: INVALID_REQUEST,
            message: message.to_string(),
        };
        let obj = v
            .as_object()
            .ok_or_else(|| invalid("request is not a JSON object"))?;
        let method = obj
            .get("method")
            .and_then(Value::as_str)
            .filter(|m| !m.is_empty())
            .ok_or_else(|| invalid("missing or invalid `method`"))?
            .to_string();
        let id = obj
            .get("id")
            .filter(|id| !id.is_null())
            .cloned()
            .ok_or_else(|| invalid("missing `id` — notifications aren't served"))?;
        let params = obj.get("params").cloned().unwrap_or(Value::Null);
        Ok(Request { id, method, params })
    }

    fn encode(&self, response: &Response) -> String {
        match response {
            Response::Result { id, result } => {
                json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string()
            }
            Response::Error {
                id,
                code,
                message,
                data,
            } => {
                let mut e = json!({"code": code, "message": message});
                if let Some(d) = data {
                    e["data"] = d.clone();
                }
                json!({"jsonrpc": "2.0", "id": id, "error": e}).to_string()
            }
        }
    }
}

// ---------------------------------------------------------------- transport

/// One request line in, one response line out — the transport
/// abstraction.
pub trait ServeTransport {
    /// The next raw request line — `Ok(None)` on clean EOF.
    fn read_request(&mut self) -> Result<Option<String>, String>;
    /// Write one raw response line (and flush).
    fn write_response(&mut self, line: &str) -> Result<(), String>;
}

/// stdin/stdout — `BufRead` lines in, `writeln!` + flush out.
pub struct StdioTransport<R, W> {
    reader: R,
    writer: W,
}

impl<R, W> StdioTransport<R, W> {
    /// Wrap a reader/writer pair.
    pub fn new(reader: R, writer: W) -> Self {
        StdioTransport { reader, writer }
    }
}

impl<R: BufRead, W: Write> ServeTransport for StdioTransport<R, W> {
    fn read_request(&mut self) -> Result<Option<String>, String> {
        let mut line = String::new();
        let n = self
            .reader
            .read_line(&mut line)
            .map_err(|e| format!("read request: {e}"))?;
        if n == 0 {
            return Ok(None);
        }
        Ok(Some(line.trim_end_matches(['\n', '\r']).to_string()))
    }

    fn write_response(&mut self, line: &str) -> Result<(), String> {
        writeln!(self.writer, "{line}")
            .and_then(|()| self.writer.flush())
            .map_err(|e| format!("write response: {e}"))
    }
}

// ---------------------------------------------------------------- params

/// `view`/`next`'s `spec` wire object — a partial `ViewSpec`
/// merged over a base (each set field replaces, unset keeps).
/// `deny_unknown_fields` so a typo'd key is -32602, not a silent
/// no-op. Decoded out of the shared `Args` bag
/// ([`crate::registry::Args::spec`]).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpecPatch {
    filter: Option<FilterPatch>,
    source: Option<ViewSource>,
    group_by: Option<GroupAxis>,
    sort: Option<Vec<SortKey>>,
    limit: Option<u32>,
    format: Option<ViewFormat>,
}

/// A partial `ViewFilter` — same set-replaces/absent-keeps rule.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FilterPatch {
    kind: Option<Vec<String>>,
    status: Option<Vec<Status>>,
    #[serde(default, deserialize_with = "de_opt_priority")]
    pri_min: Option<Priority>,
    #[serde(default, deserialize_with = "de_opt_priority")]
    pri_max: Option<Priority>,
    label: Option<Vec<String>>,
    owner: Option<Vec<Actor>>,
    claimed: Option<bool>,
    epic: Option<String>,
    milestone: Option<String>,
    #[serde(default, deserialize_with = "de_opt_date")]
    due: Option<time::Date>,
    estimate: Option<u32>,
    cycle: Option<String>,
    superseded: Option<bool>,
}

/// `pri_min`/`pri_max` wire values take the same lenient read as
/// `[views]` TOML — `crit` and `critical` both parse.
fn de_opt_priority<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Priority>, D::Error> {
    Option::<String>::deserialize(d)?
        .map(|s| s.parse().map_err(serde::de::Error::custom))
        .transpose()
}

/// `due` wire values are `"YYYY-MM-DD"` strings — the same read
/// `due:` tokens and `[views]` TOML accept.
fn de_opt_date<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<time::Date>, D::Error> {
    Option::<String>::deserialize(d)?
        .map(|s| myco_core::date::parse(&s).map_err(serde::de::Error::custom))
        .transpose()
}

impl SpecPatch {
    /// Fold set fields over `base` — a set field replaces, an
    /// unset one keeps the base's value (`Option` scalars can be
    /// set but never cleared, matching the CLI flags).
    fn apply(&self, base: &mut ViewSpec) {
        if let Some(p) = &self.filter {
            let f = &mut base.filter;
            if let Some(v) = &p.kind {
                f.kind = v.clone();
            }
            if let Some(v) = &p.status {
                f.status = v.clone();
            }
            if let Some(v) = p.pri_min {
                f.pri_min = Some(v);
            }
            if let Some(v) = p.pri_max {
                f.pri_max = Some(v);
            }
            if let Some(v) = &p.label {
                f.label = v.clone();
            }
            if let Some(v) = &p.owner {
                f.owner = v.clone();
            }
            if let Some(v) = p.claimed {
                f.claimed = Some(v);
            }
            if let Some(v) = &p.epic {
                f.epic = Some(v.clone());
            }
            if let Some(v) = &p.milestone {
                f.milestone = Some(v.clone());
            }
            if let Some(v) = p.due {
                f.due = Some(v);
            }
            if let Some(v) = p.estimate {
                f.estimate = Some(v);
            }
            if let Some(v) = &p.cycle {
                f.cycle = Some(v.clone());
            }
            if let Some(v) = p.superseded {
                f.superseded = Some(v);
            }
        }
        if let Some(s) = self.source {
            base.source = s;
        }
        if let Some(g) = self.group_by {
            base.group_by = Some(g);
        }
        if let Some(s) = &self.sort {
            base.sort = s.clone();
        }
        if let Some(n) = self.limit {
            base.limit = Some(n);
        }
        if let Some(fmt) = self.format {
            base.format = fmt;
        }
    }
}

// ---------------------------------------------------------------- dispatch

/// Dispatch failure — [`Server::respond`] maps these to wire
/// codes; dispatch itself knows no wire.
#[derive(Debug)]
pub enum Fail {
    /// The method isn't served.
    UnknownMethod,
    /// Params didn't decode — -32602.
    BadParams(String),
    /// The verb failed — -32000.
    Verb(String),
    /// Status-machine rejection — -32001 + `data`.
    Transition {
        /// The item being set.
        id: ItemId,
        /// Current status.
        from: Status,
        /// Requested status.
        to: Status,
    },
}

impl From<String> for Fail {
    fn from(s: String) -> Fail {
        Fail::Verb(s)
    }
}

/// The served surface — discovery flags + `--no-commit`, shared
/// across requests. Nothing caches store state: every method
/// re-opens the store.
pub struct Server {
    dir: Option<PathBuf>,
    root: Option<PathBuf>,
    no_commit: bool,
}

impl Server {
    /// A server over the caller's discovery context.
    pub fn new(dir: Option<PathBuf>, root: Option<PathBuf>, no_commit: bool) -> Self {
        Server {
            dir,
            root,
            no_commit,
        }
    }

    /// The `--dir` discovery flag.
    pub(crate) fn dir(&self) -> Option<&std::path::Path> {
        self.dir.as_deref()
    }

    /// The `--root` discovery flag.
    pub(crate) fn root(&self) -> Option<&std::path::Path> {
        self.root.as_deref()
    }

    /// The `--no-commit` flag — verbs that commit outside the
    /// served repo (`item.move`'s second worktree) need it raw;
    /// [`Server::commit`] already folds it in for the common case.
    pub(crate) fn no_commit(&self) -> bool {
        self.no_commit
    }

    /// Exclusive-locked store — mutating methods.
    fn open(&self) -> Result<(crate::discover::Project, items::LockedStore), Fail> {
        items::open(self.dir.as_deref(), self.root.as_deref()).map_err(Fail::Verb)
    }

    /// Shared-locked store — read methods.
    fn open_read(&self) -> Result<(crate::discover::Project, items::LockedStore), Fail> {
        items::open_read(self.dir.as_deref(), self.root.as_deref()).map_err(Fail::Verb)
    }

    fn commit(&self, wt: &std::path::Path, msg: &str, actor: Option<&Actor>) -> Result<bool, Fail> {
        items::commit(wt, msg, actor, self.no_commit).map_err(Fail::Verb)
    }

    /// The request loop — wire- and transport-agnostic: read one
    /// request, write one response, until EOF (`Ok(0)`).
    pub fn serve<T: ServeTransport, W: WireProtocol>(
        &self,
        transport: &mut T,
        wire: &W,
    ) -> Result<i32, String> {
        while let Some(line) = transport.read_request()? {
            if let Some(response) = self.handle_line(wire, &line) {
                transport.write_response(&response)?;
            }
        }
        Ok(0)
    }

    /// One raw request line → one encoded response line — `None`
    /// for blank lines (skipped, not an error). `myco batch`
    /// drives this same path for one-shot NDJSON runs.
    pub fn handle_line<W: WireProtocol>(&self, wire: &W, line: &str) -> Option<String> {
        if line.trim().is_empty() {
            return None;
        }
        let response = match wire.parse(line) {
            Ok(req) => self.respond(req),
            Err(e) => Response::Error {
                id: Value::Null,
                code: e.code,
                message: e.message,
                data: None,
            },
        };
        Some(wire.encode(&response))
    }

    fn respond(&self, req: Request) -> Response {
        match self.dispatch(&req.method, &req.params) {
            Ok(result) => Response::Result { id: req.id, result },
            Err(fail) => {
                let (code, message, data) = match fail {
                    Fail::UnknownMethod => (
                        METHOD_NOT_FOUND,
                        format!("unknown method {:?}", req.method),
                        None,
                    ),
                    Fail::BadParams(m) => (INVALID_PARAMS, m, None),
                    Fail::Verb(m) => (VERB_ERROR, m, None),
                    Fail::Transition { id, from, to } => (
                        INVALID_TRANSITION,
                        format!("{id}: invalid status transition {from} → {to}"),
                        Some(json!({
                            "from": from.token(),
                            "to": to.token(),
                            "valid": from
                                .transitions()
                                .iter()
                                .map(|s| s.token())
                                .collect::<Vec<_>>(),
                        })),
                    ),
                };
                Response::Error {
                    id: req.id,
                    code,
                    message,
                    data,
                }
            }
        }
    }

    /// The config root for `[verbs]`/`[views]` resolution —
    /// discovery's repo root, else `--root`/cwd.
    pub(crate) fn cfg_root(&self) -> PathBuf {
        crate::discover::repo_root_for(self.dir.as_deref(), self.root.as_deref())
            .ok()
            .or_else(|| self.root.clone())
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."))
    }

    /// `initialize`'s method table — the registry's wire verbs
    /// ∪ this repo's config verbs (per call, like view config).
    fn methods(&self) -> Vec<String> {
        let mut out: Vec<String> = crate::registry::served_names()
            .iter()
            .map(|s| s.to_string())
            .collect();
        if let Ok((verbs, _)) = myco_ops::verb::named_verbs_lenient(
            &self.cfg_root(),
            crate::ops::CONFIG,
            crate::registry::builtin_refs(),
        ) {
            out.extend(verbs.iter().map(|v| v.name.clone()));
        }
        out
    }

    /// The verb surface — `method` + `params` → a JSON payload,
    /// transport- and wire-agnostic. One dispatch path: the
    /// registry's wire handler, then this repo's config verbs.
    fn dispatch(&self, method: &str, params: &Value) -> Result<Value, Fail> {
        if let Some(v) = crate::registry::find(method) {
            let Some(w) = v.wire else {
                return Err(Fail::UnknownMethod);
            };
            let a = crate::registry::Args::from_json(v, params)
                .map_err(|e| Fail::BadParams(format!("invalid params: {e}")))?;
            return w(self, &a);
        }
        crate::cfgverb::wire(self, method, params)
    }
}

/// The `initialize` handshake — protocol caps + the method table.
pub(crate) fn w_initialize(s: &Server, _a: &crate::registry::Args) -> Result<Value, Fail> {
    Ok(json!({
        "protocol": 1,
        "wire": "jsonrpc-2.0",
        "transport": "stdio",
        "server": {"name": "myco", "version": env!("CARGO_PKG_VERSION")},
        "methods": s.methods(),
    }))
}

/// `manifest` on the wire.
pub(crate) fn w_manifest(_s: &Server, _a: &crate::registry::Args) -> Result<Value, Fail> {
    Ok(crate::manifest())
}

/// `schema` on the wire.
pub(crate) fn w_schema(_s: &Server, _a: &crate::registry::Args) -> Result<Value, Fail> {
    Ok(schema::document())
}

/// `status` — `{prompt}` picks the digest or the prompt block.
/// `--prompt` marks items owned by the resolved actor (`⚑`) so an
/// agent sees its own claims at a glance.
pub(crate) fn w_status(s: &Server, a: &crate::registry::Args) -> Result<Value, Fail> {
    let (proj, store) = s.open_read()?;
    if a.flag("prompt") {
        let me = ops::resolve_actor(Some(&proj.worktree), None);
        status::prompt_json(&store, Some(&me)).map_err(Fail::Verb)
    } else {
        status::status_json(&store).map_err(Fail::Verb)
    }
}

/// `report` on the wire.
pub(crate) fn w_report(s: &Server, _a: &crate::registry::Args) -> Result<Value, Fail> {
    let (proj, store) = s.open_read()?;
    report::report_json(&store, &proj.worktree).map_err(Fail::Verb)
}

/// `check` on the wire — the read-only payload (no `--fix`).
pub(crate) fn w_check(s: &Server, _a: &crate::registry::Args) -> Result<Value, Fail> {
    let (proj, _store) = s.open_read()?;
    check::check_json(&proj).map_err(Fail::Verb)
}

/// `undo` — `{force?}`; rewinds the project tip one commit. No
/// store needed — the reset is pure git.
pub(crate) fn w_undo(s: &Server, a: &crate::registry::Args) -> Result<Value, Fail> {
    let force = a.flag("force");
    let proj = crate::discover::discover(s.dir(), s.root()).map_err(Fail::Verb)?;
    if a.flag("list") {
        let entries = crate::ops::history_list(&proj.worktree);
        return Ok(json!({
            "redoable": entries.iter().map(|e| json!({
                "hash": e.target, "subject": e.subject, "date": e.date,
            })).collect::<Vec<_>>(),
        }));
    }
    let _lock = crate::ops::lock_worktree(&proj.worktree).map_err(Fail::Verb)?;
    let out = crate::ops::undo_last(&proj.worktree, force).map_err(Fail::Verb)?;
    Ok(json!({
        "undone": {"hash": out.hash, "subject": out.subject, "date": out.date},
        "warnings": out.warnings,
    }))
}

/// `redo` — restores the commit the last undo moved away from.
/// No store needed — the reset is pure git.
pub(crate) fn w_redo(s: &Server, _a: &crate::registry::Args) -> Result<Value, Fail> {
    let proj = crate::discover::discover(s.dir(), s.root()).map_err(Fail::Verb)?;
    let _lock = crate::ops::lock_worktree(&proj.worktree).map_err(Fail::Verb)?;
    let out = crate::ops::redo_last(&proj.worktree).map_err(Fail::Verb)?;
    let remaining = crate::ops::redo_depth(&proj.worktree);
    Ok(json!({
        "redone": {"hash": out.hash, "subject": out.subject, "date": out.date},
        "remaining": remaining,
        "warnings": out.warnings,
    }))
}

/// `item.show` — `{id}` (a single id per request).
pub(crate) fn w_item_show(s: &Server, a: &crate::registry::Args) -> Result<Value, Fail> {
    let id = a.one::<ItemId>("id").map_err(Fail::BadParams)?;
    let (_proj, store) = s.open_read()?;
    itemcmd::show_json(&store, id).map_err(Fail::Verb)
}

/// `resume` — `{id, last}`.
pub(crate) fn w_resume(s: &Server, a: &crate::registry::Args) -> Result<Value, Fail> {
    let id = a.one::<ItemId>("id").map_err(Fail::BadParams)?;
    let last: Option<usize> = a.parsed("last").map_err(Fail::BadParams)?;
    let (_proj, store) = s.open_read()?;
    continuity::resume_json(&store, id, last).map_err(Fail::Verb)
}

/// `todo.list` / `backlog.list` — the shared list filters.
pub(crate) fn w_todo_list(s: &Server, a: &crate::registry::Args) -> Result<Value, Fail> {
    w_doc_list(s, a, false)
}

/// `backlog.list`.
pub(crate) fn w_backlog_list(s: &Server, a: &crate::registry::Args) -> Result<Value, Fail> {
    w_doc_list(s, a, true)
}

fn w_doc_list(s: &Server, a: &crate::registry::Args, backlog: bool) -> Result<Value, Fail> {
    let f = items::list_args(a).map_err(Fail::BadParams)?;
    let (_proj, store) = s.open_read()?;
    let found = todo::list_items(&store, backlog, &f)?;
    Ok(items_json(&found))
}

/// `done.list` — the DONE.md archive, same list filters.
pub(crate) fn w_done_list(s: &Server, a: &crate::registry::Args) -> Result<Value, Fail> {
    let f = items::list_args(a).map_err(Fail::BadParams)?;
    let (_proj, store) = s.open_read()?;
    let all = crate::done::done_items(&store)?;
    let found: Vec<Item> = myco_core::tree_view(&all)
        .into_iter()
        .filter(|(_, i)| f.matches(i))
        .take(f.take())
        .map(|(_, i)| i.clone())
        .collect();
    Ok(items_json(&found))
}

/// `done.remove` — delete an archived task subtree from DONE.md.
pub(crate) fn w_done_remove(s: &Server, a: &crate::registry::Args) -> Result<Value, Fail> {
    let id = a.req::<ItemId>("id").map_err(Fail::BadParams)?;
    let (proj, store) = s.open()?;
    let item = crate::done::remove_item(&store, id).map_err(Fail::Verb)?;
    let committed = s.commit(&proj.worktree, &format!("chore: remove {id}"), None)?;
    Ok(json!({"item": item, "file": "DONE.md", "committed": committed}))
}

/// `issue.list`.
pub(crate) fn w_issue_list(s: &Server, a: &crate::registry::Args) -> Result<Value, Fail> {
    w_file_list(s, a, ItemKind::Issue)
}

/// `proposal.list`.
pub(crate) fn w_proposal_list(s: &Server, a: &crate::registry::Args) -> Result<Value, Fail> {
    w_file_list(s, a, ItemKind::Proposal)
}

/// `plan.list`.
pub(crate) fn w_plan_list(s: &Server, a: &crate::registry::Args) -> Result<Value, Fail> {
    w_file_list(s, a, ItemKind::Plan)
}

/// `adr.list`.
pub(crate) fn w_adr_list(s: &Server, a: &crate::registry::Args) -> Result<Value, Fail> {
    w_file_list(s, a, ItemKind::Adr)
}

fn w_file_list(s: &Server, a: &crate::registry::Args, kind: ItemKind) -> Result<Value, Fail> {
    let f = items::list_args(a).map_err(Fail::BadParams)?;
    let (_proj, store) = s.open_read()?;
    let found = items::query_files(&store, kind, a.flag("archived"), &f).map_err(Fail::Verb)?;
    Ok(items::marked_json(&found))
}

/// `todo.add`.
pub(crate) fn w_todo_add(s: &Server, a: &crate::registry::Args) -> Result<Value, Fail> {
    let title = a.text("title").map_err(Fail::BadParams)?.to_string();
    let section = a.opt("section").map(str::to_string);
    let parent: Option<ItemId> = a.parsed("parent").map_err(Fail::BadParams)?;
    let meta = items::meta_args(a).map_err(Fail::BadParams)?;
    let (proj, store) = s.open()?;
    let (item, file) =
        todo::add_item(&store, &title, section.as_deref(), parent, &meta).map_err(Fail::Verb)?;
    let committed = s.commit(
        &proj.worktree,
        &format!("chore: add {}", item.id),
        meta.owner.as_ref(),
    )?;
    Ok(json!({"item": item, "file": file, "parent": parent, "committed": committed}))
}

/// `todo.done`.
pub(crate) fn w_todo_done(s: &Server, a: &crate::registry::Args) -> Result<Value, Fail> {
    let id = a.req::<ItemId>("id").map_err(Fail::BadParams)?;
    let (proj, store) = s.open()?;
    let item = todo::done_item(&store, id).map_err(Fail::Verb)?;
    let committed = s.commit(&proj.worktree, &format!("chore: done {id}"), None)?;
    Ok(json!({"item": item, "committed": committed}))
}

/// `todo.cancel`.
pub(crate) fn w_todo_cancel(s: &Server, a: &crate::registry::Args) -> Result<Value, Fail> {
    let id = a.req::<ItemId>("id").map_err(Fail::BadParams)?;
    let (proj, store) = s.open()?;
    let item = todo::cancel_item(&store, id).map_err(Fail::Verb)?;
    let committed = s.commit(&proj.worktree, &format!("chore: cancel {id}"), None)?;
    Ok(json!({"item": item, "file": "DONE.md", "committed": committed}))
}

/// `todo.defer`.
pub(crate) fn w_todo_defer(s: &Server, a: &crate::registry::Args) -> Result<Value, Fail> {
    let id = a.req::<ItemId>("id").map_err(Fail::BadParams)?;
    let (proj, store) = s.open()?;
    let item = todo::defer_item(&store, id).map_err(Fail::Verb)?;
    let committed = s.commit(&proj.worktree, &format!("chore: defer {id}"), None)?;
    Ok(json!({"item": item, "file": "BACKLOG.md", "committed": committed}))
}

/// `backlog.add`.
pub(crate) fn w_backlog_add(s: &Server, a: &crate::registry::Args) -> Result<Value, Fail> {
    let title = a.text("title").map_err(Fail::BadParams)?.to_string();
    let section = a.opt("section").map(str::to_string);
    let meta = items::meta_args(a).map_err(Fail::BadParams)?;
    let (proj, store) = s.open()?;
    let item = backlog::add_item(&store, &title, section.as_deref(), &meta).map_err(Fail::Verb)?;
    let committed = s.commit(
        &proj.worktree,
        &format!("chore: add {}", item.id),
        meta.owner.as_ref(),
    )?;
    Ok(json!({"item": item, "file": "BACKLOG.md", "committed": committed}))
}

/// `backlog.promote`.
pub(crate) fn w_backlog_promote(s: &Server, a: &crate::registry::Args) -> Result<Value, Fail> {
    let id = a.req::<ItemId>("id").map_err(Fail::BadParams)?;
    let (proj, store) = s.open()?;
    let item = backlog::promote_item(&store, id).map_err(Fail::Verb)?;
    let committed = s.commit(&proj.worktree, &format!("chore: promote {id}"), None)?;
    Ok(json!({"item": item, "file": "TODO.md", "committed": committed}))
}

/// `item.set` — `status` goes through the transition machine
/// (single step, typed -32001 on rejection); `""` clears
/// `epic`/`milestone`.
pub(crate) fn w_item_set(s: &Server, a: &crate::registry::Args) -> Result<Value, Fail> {
    let id = a.req::<ItemId>("id").map_err(Fail::BadParams)?;
    let flags = itemcmd::set_flags(a).map_err(Fail::BadParams)?;
    let session = a.opt("session").map(str::to_string);
    if flags.is_noop() && session.is_none() {
        return Err(Fail::Verb(
            "item set: nothing to change — pass at least one field".into(),
        ));
    }
    let (proj, store) = s.open()?;
    // pre-check the status step so the wire gets the typed
    // -32001 + valid targets, not a string
    if let Some(to) = flags.status {
        let Some((item, _, _)) = items::find_item(&store, id).map_err(Fail::Verb)? else {
            return Err(Fail::Verb(format!("{id}: no such item")));
        };
        item.status
            .transition_to(to)
            .map_err(|_| Fail::Transition {
                id,
                from: item.status,
                to,
            })?;
    }
    let me = ops::resolve_actor(Some(&proj.worktree), flags.owner.as_ref());
    let outcome = itemcmd::set_item(&store, id, &flags, session, &me).map_err(Fail::Verb)?;
    let committed = s.commit(&proj.worktree, &format!("chore: set {id}"), Some(&me))?;
    let mut payload =
        json!({"item": outcome.item, "location": outcome.location, "committed": committed});
    if let Some(entry) = outcome.claim {
        payload["claim"] = json!(entry);
    }
    if let Some(from) = outcome.taken_from {
        payload["taken_from"] = json!(from);
    }
    Ok(payload)
}

/// `item.move` — `{id, to?, note?, actor?, dry_run?, finish?}`;
/// the destination is resolved and committed like the CLI path
/// (two worktrees, two autocommits — both reported in the
/// payload). `dry_run` previews the plan with no writes;
/// `finish` completes a partial move off its `moved to` pointer.
pub(crate) fn w_item_move(s: &Server, a: &crate::registry::Args) -> Result<Value, Fail> {
    let id = a.req::<ItemId>("id").map_err(Fail::BadParams)?;
    let to = a.opt("to").map(str::to_string);
    let note = a.opt("note").map(str::to_string);
    let actor: Option<Actor> = a.parsed("actor").map_err(Fail::BadParams)?;
    let finish = a.flag("finish");
    let dry_run = a.flag("dry-run");
    if finish && (dry_run || to.is_some()) {
        return Err(Fail::BadParams(
            "item move: `finish` takes no `to`/`dry_run`".into(),
        ));
    }
    if !finish && to.is_none() {
        return Err(Fail::BadParams(
            "item move: `to` is required (or `finish`)".into(),
        ));
    }
    if dry_run {
        let p = movecmd::move_plan(
            s.dir(),
            s.root(),
            id,
            to.as_deref().unwrap_or_default(),
            note.as_deref(),
        )
        .map_err(Fail::Verb)?;
        return Ok(movecmd::plan_json(&p));
    }
    let m = if finish {
        movecmd::finish_move(s.dir(), s.root(), id, actor, s.no_commit()).map_err(Fail::Verb)?
    } else {
        movecmd::do_move(
            s.dir(),
            s.root(),
            id,
            to.as_deref().unwrap_or_default(),
            note.as_deref(),
            actor,
            s.no_commit(),
        )
        .map_err(Fail::Verb)?
    };
    Ok(movecmd::moved_json(&m))
}

/// `item.reorder` — `{id, delta}`; sibling swaps inside the
/// item's ledger doc (the TUI's `Alt+↑/↓` — `Ledger::move_within`).
pub(crate) fn w_item_reorder(s: &Server, a: &crate::registry::Args) -> Result<Value, Fail> {
    let id = a.req::<ItemId>("id").map_err(Fail::BadParams)?;
    let delta = a.req::<isize>("delta").map_err(Fail::BadParams)?;
    let (proj, store) = s.open()?;
    let (item, file, moved) = itemcmd::reorder_item(&store, id, delta).map_err(Fail::Verb)?;
    let committed = s.commit(&proj.worktree, &format!("chore: reorder {id}"), None)?;
    Ok(json!({
        "item": item, "file": file, "delta": delta, "moved": moved,
        "committed": committed,
    }))
}

/// `handoff` — `note "-"` reads stdin, and stdin IS the wire.
pub(crate) fn w_handoff(s: &Server, a: &crate::registry::Args) -> Result<Value, Fail> {
    let id = a.req::<ItemId>("id").map_err(Fail::BadParams)?;
    let note = a.text("note").map_err(Fail::BadParams)?.to_string();
    if note == "-" {
        return Err(Fail::BadParams(
            "note \"-\" reads stdin — stdin is the wire; pass the note inline".into(),
        ));
    }
    let actor: Option<Actor> = a.parsed("actor").map_err(Fail::BadParams)?;
    let session = a.opt("session").map(str::to_string);
    let (proj, store) = s.open()?;
    let me = ops::resolve_actor(Some(&proj.worktree), actor.as_ref());
    let (entry, rel) = continuity::record_handoff(&store, &proj.worktree, id, note, &me, session)
        .map_err(Fail::Verb)?;
    let committed = s.commit(
        &proj.worktree,
        &format!("chore: handoff {id}"),
        Some(&entry.actor),
    )?;
    Ok(json!({"id": id.to_string(), "path": rel, "entry": entry, "committed": committed}))
}

/// `interrupt` — `{id}` reverts one item (`session` asserts the
/// claim tag); `{session}` without `id` is the teardown — every
/// item claimed under the session reverts, one commit, per-item
/// errors collected in `errors[]`.
pub(crate) fn w_interrupt(s: &Server, a: &crate::registry::Args) -> Result<Value, Fail> {
    let id: Option<ItemId> = a.parsed("id").map_err(Fail::BadParams)?;
    let note = a.opt("note").map(str::to_string);
    let actor: Option<Actor> = a.parsed("actor").map_err(Fail::BadParams)?;
    let session = a.opt("session").map(str::to_string);
    let (proj, store) = s.open()?;
    let me = ops::resolve_actor(Some(&proj.worktree), actor.as_ref());
    match (id, session) {
        (Some(id), session) => {
            let entry = continuity::interrupt_item(&store, id, note.as_deref(), &me, session)
                .map_err(Fail::Verb)?;
            let committed = s.commit(
                &proj.worktree,
                &format!("chore: interrupt {id}"),
                Some(&entry.actor),
            )?;
            Ok(
                json!({"id": id.to_string(), "status": "open", "entry": entry, "committed": committed}),
            )
        }
        (None, Some(session)) => {
            let out = continuity::interrupt_session(&store, &session, note.as_deref(), &me)
                .map_err(Fail::Verb)?;
            let committed = s.commit(
                &proj.worktree,
                &format!("chore: interrupt session {session}"),
                Some(&me),
            )?;
            Ok(json!({
                "session": session,
                "interrupted": out.interrupted.iter().map(|(id, e)| {
                    json!({"id": id.to_string(), "status": "open", "entry": e})
                }).collect::<Vec<_>>(),
                "errors": out.errors.iter().map(|(id, e)| {
                    json!({"id": id.to_string(), "error": e})
                }).collect::<Vec<_>>(),
                "committed": committed,
            }))
        }
        (None, None) => Err(Fail::BadParams(
            "missing `id` — pass an item id, or a `session` to interrupt \
             everything it claimed"
                .into(),
        )),
    }
}

/// `reap` — `{idle_days?, session?, actor?, dry_run?}` — the bulk
/// stale-claim sweep: every live `claimed`/`in-progress` item idle
/// past the threshold reverts to `open` in ONE commit. `dry_run`
/// plans with zero writes; per-item failures land in `errors[]`
/// without aborting the sweep.
pub(crate) fn w_reap(s: &Server, a: &crate::registry::Args) -> Result<Value, Fail> {
    let idle_days = reap::idle_threshold(a).map_err(Fail::BadParams)?;
    let session = a.opt("session").map(str::to_string);
    let actor: Option<Actor> = a.parsed("actor").map_err(Fail::BadParams)?;
    let dry_run = a.flag("dry-run");
    // dry_run reads through the shared lock — zero writes either way
    let (proj, store) = if dry_run { s.open_read()? } else { s.open()? };
    let me = ops::resolve_actor(Some(&proj.worktree), actor.as_ref());
    let p = reap::plan(&store, session.as_deref(), idle_days).map_err(Fail::Verb)?;
    if dry_run {
        return Ok(reap::payload(&p.hits, &p.errors, true, false));
    }
    let (reaped, errors) = reap::apply(&store, &p.hits, &me);
    let mut all_errors = p.errors;
    all_errors.extend(errors);
    let hits: Vec<reap::ReapHit> = reaped.iter().map(|r| r.hit.clone()).collect();
    let committed = if reaped.is_empty() {
        false
    } else {
        s.commit(
            &proj.worktree,
            &format!("chore: reap {} stale claims", reaped.len()),
            Some(&me),
        )?
    };
    Ok(reap::payload(&hits, &all_errors, false, committed))
}

/// `next` — `{spec}` merges over the `next` named view; `limit`
/// pinned at 1. `{"item": null}` when the backlog is clear.
pub(crate) fn w_next(s: &Server, a: &crate::registry::Args) -> Result<Value, Fail> {
    let patch = a.spec("spec").map_err(Fail::BadParams)?;
    let (proj, store) = s.open_read()?;
    let (view, _warnings) = myco_ops::view::resolve_lenient(&proj.root, "next", crate::ops::CONFIG)
        .map_err(Fail::Verb)?;
    let mut spec = view.spec;
    if let Some(patch) = &patch {
        patch.apply(&mut spec);
    }
    spec.limit = Some(1);
    spec.validate().map_err(Fail::Verb)?;
    let scan = digest::scan(&store).map_err(Fail::Verb)?;
    Ok(json!({"item": next::pick(&scan, &spec, myco_store::Store::kinds(&store))}))
}

/// `view` — `{name}` resolves a named view (builtin preset or
/// `[views]` config); `{spec}` patches over it (or over the
/// default spec when `name` is absent). Neither lists views.
pub(crate) fn w_view(s: &Server, a: &crate::registry::Args) -> Result<Value, Fail> {
    let name = a.opt("name").map(str::to_string);
    let patch = a.spec("spec").map_err(Fail::BadParams)?;
    let (proj, store) = s.open_read()?;
    match (name.as_deref(), patch.as_ref()) {
        // bare — enumerate the defined views
        (None, None) => {
            let (views, warnings) =
                myco_ops::view::named_views(&proj.root, crate::ops::CONFIG).map_err(Fail::Verb)?;
            Ok(json!({
                "views": views
                    .iter()
                    .map(|v| json!({
                        "name": v.name,
                        "origin": match v.origin {
                            Origin::Builtin => "builtin",
                            Origin::Config => "config",
                        },
                        "spec": v.spec,
                    }))
                    .collect::<Vec<_>>(),
                "warnings": warnings,
            }))
        }
        (name, patch) => {
            let (mut spec, warnings) = match name {
                Some(n) => {
                    let (v, w) = myco_ops::view::resolve(&proj.root, n, crate::ops::CONFIG)
                        .map_err(Fail::Verb)?;
                    (v.spec, w)
                }
                None => (ViewSpec::default(), Vec::new()),
            };
            if let Some(patch) = patch {
                patch.apply(&mut spec);
            }
            spec.validate().map_err(Fail::Verb)?;
            let mut warnings = warnings;
            // `repos` emits per-repo rows, not item groups — the
            // same emit the CLI's repos table renders
            if spec.format == ViewFormat::Repos {
                let (rows, repo_warnings) = match spec.source {
                    ViewSource::Index => {
                        let (v, w) = crate::view::index_universe().map_err(Fail::Verb)?;
                        (v.iter().map(crate::view::repo_row).collect::<Vec<_>>(), w)
                    }
                    ViewSource::Repo => {
                        let scan = digest::scan(&store).map_err(Fail::Verb)?;
                        (vec![crate::view::repo_row_local(&proj, &scan)], Vec::new())
                    }
                };
                warnings.extend(repo_warnings);
                return Ok(json!({
                    "view": name,
                    "spec": spec,
                    "repos": rows.iter().map(crate::view::row_json).collect::<Vec<_>>(),
                    "warnings": warnings,
                }));
            }
            // the universe owners — `groups` borrows item refs
            // out of them
            let (interrupted, scan, views) = match spec.source {
                ViewSource::Repo => {
                    let s = digest::scan(&store).map_err(Fail::Verb)?;
                    (
                        continuity::interrupted_set(&store, s.live()).map_err(Fail::Verb)?,
                        Some(s),
                        Vec::new(),
                    )
                }
                // `source = "index"` unions the registered repos'
                // snapshots, repo-tagged; snapshots carry no
                // handoffs — the set stays empty
                ViewSource::Index => {
                    let (v, repo_warnings) = crate::view::index_universe().map_err(Fail::Verb)?;
                    warnings.extend(repo_warnings);
                    (std::collections::BTreeSet::new(), None, v)
                }
            };
            // top-level items only — children ride with their parent
            let groups = match spec.source {
                ViewSource::Repo => eval(
                    scan.iter()
                        .flat_map(|s| s.items.iter().filter(|i| !s.archived.contains(&i.id))),
                    &spec,
                    myco_store::Store::kinds(&store),
                ),
                ViewSource::Index => eval_scoped(
                    views.iter().flat_map(|v| {
                        v.snap.iter().flat_map(|s| {
                            s.items
                                .iter()
                                .filter(|i| !s.archived.contains(&i.id))
                                .map(move |i| Scoped::tagged(s.name.as_str(), i))
                        })
                    }),
                    &spec,
                    myco_core::kind::prelude_table(),
                ),
            };
            Ok(json!({
                "view": name,
                "spec": spec,
                "groups": groups
                    .iter()
                    .map(|g| json!({"label": g.label, "items": g.items}))
                    .collect::<Vec<_>>(),
                "interrupted": interrupted
                    .iter()
                    .map(|i| i.to_string())
                    .collect::<Vec<_>>(),
                "warnings": warnings,
            }))
        }
    }
}

/// The `serve` verb itself — stdio transport, JSON-RPC wire.
pub(crate) fn h_serve(
    ctx: &crate::registry::Ctx,
    _a: &crate::registry::Args,
) -> Result<i32, String> {
    run(ctx.dir.clone(), ctx.root.clone(), ctx.no_commit)
}

/// `Vec<Item>` → a JSON array of item objects.
fn items_json(items: &[myco_core::Item]) -> Value {
    json!(items
        .iter()
        .map(|i| serde_json::to_value(i).unwrap_or(json!(null)))
        .collect::<Vec<_>>())
}

/// The `serve` verb — stdio transport, JSON-RPC 2.0 wire, EOF
/// exits 0.
pub fn run(dir: Option<PathBuf>, root: Option<PathBuf>, no_commit: bool) -> Result<i32, String> {
    let server = Server::new(dir, root, no_commit);
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut transport = StdioTransport::new(stdin.lock(), stdout.lock());
    server.serve(&mut transport, &JsonRpc2)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// A scaffolded project dir (no git needed — discovery
    /// accepts `--dir` verbatim and `no_commit` skips commits).
    fn fixture() -> (tempfile::TempDir, Server) {
        let d = tempfile::tempdir().unwrap();
        let root = camino::Utf8PathBuf::from_path_buf(d.path().to_path_buf()).unwrap();
        let store = myco_store_md::MdStore::new(root);
        crate::ops::scaffold(&store, d.path(), "p").unwrap();
        let s = Server::new(Some(d.path().to_path_buf()), None, true);
        (d, s)
    }

    /// A scaffolded worktree under an explicit root — `dir` =
    /// `<d>/project`, `root` = `d`, so repo-layer `myco.toml`
    /// views resolve hermetically inside the tempdir.
    fn rooted_fixture() -> (tempfile::TempDir, Server) {
        let d = tempfile::tempdir().unwrap();
        let wt = d.path().join("project");
        std::fs::create_dir(&wt).unwrap();
        let root = camino::Utf8PathBuf::from_path_buf(wt.clone()).unwrap();
        let store = myco_store_md::MdStore::new(root);
        crate::ops::scaffold(&store, &wt, "p").unwrap();
        let s = Server::new(Some(wt), Some(d.path().to_path_buf()), true);
        (d, s)
    }

    /// Pin config discovery to nowhere — user-layer views can't
    /// leak into `resolve`/`named_views` mid-test.
    fn no_user_config<T>(f: impl FnOnce() -> T) -> T {
        temp_env::with_vars(
            [
                ("MYCO_CONFIG", None::<&str>),
                ("XDG_CONFIG_HOME", Some("/nonexistent-xdg")),
                ("HOME", Some("/nonexistent-home")),
            ],
            f,
        )
    }

    fn call(s: &Server, method: &str, params: Value) -> Result<Value, Fail> {
        s.dispatch(method, &params)
    }

    #[test]
    fn jsonrpc_parse_valid_and_errors() {
        let w = JsonRpc2;
        let r = w
            .parse(r#"{"jsonrpc":"2.0","id":7,"method":"status","params":{"prompt":true}}"#)
            .unwrap();
        assert_eq!(r.id, json!(7));
        assert_eq!(r.method, "status");
        assert_eq!(r.params["prompt"], true);
        // params optional
        let r = w.parse(r#"{"id":"a","method":"manifest"}"#).unwrap();
        assert_eq!(r.id, json!("a"));
        assert!(r.params.is_null());
        // not json → -32700
        assert_eq!(w.parse("hello").unwrap_err().code, PARSE_ERROR);
        // non-object → -32600
        assert_eq!(w.parse("[1,2]").unwrap_err().code, INVALID_REQUEST);
        // missing id → -32600 (notifications aren't served)
        assert_eq!(
            w.parse(r#"{"method":"status"}"#).unwrap_err().code,
            INVALID_REQUEST
        );
        // missing method → -32600
        assert_eq!(w.parse(r#"{"id":1}"#).unwrap_err().code, INVALID_REQUEST);
    }

    #[test]
    fn jsonrpc_encode_roundtrip() {
        let w = JsonRpc2;
        let line = w.encode(&Response::Result {
            id: json!("x"),
            result: json!({"ok": true}),
        });
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["id"], "x");
        assert_eq!(v["result"]["ok"], true);
        assert!(!line.contains('\n'));
        // error with data
        let line = w.encode(&Response::Error {
            id: Value::Null,
            code: INVALID_TRANSITION,
            message: "nope".into(),
            data: Some(json!({"valid": ["open"]})),
        });
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["id"], Value::Null);
        assert_eq!(v["error"]["code"], INVALID_TRANSITION);
        assert_eq!(v["error"]["data"]["valid"], json!(["open"]));
    }

    #[test]
    fn serve_loop_over_memory_transport() {
        let (_d, s) = fixture();
        let input = b"{\"id\":1,\"method\":\"manifest\"}\ngarbage\n{\"id\":2,\"method\":\"todo.add\",\"params\":{\"title\":\"x\"}}\n";
        let mut out = Vec::new();
        {
            let mut t = StdioTransport::new(&input[..], &mut out);
            assert_eq!(s.serve(&mut t, &JsonRpc2).unwrap(), 0);
        }
        let lines: Vec<Value> = String::from_utf8(out)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0]["id"], 1);
        assert_eq!(lines[0]["result"]["name"], "myco");
        // a parse error doesn't kill the loop
        assert_eq!(lines[1]["error"]["code"], PARSE_ERROR);
        assert_eq!(lines[1]["id"], Value::Null);
        assert_eq!(lines[2]["result"]["item"]["id"], "T-001");
        // EOF → clean exit (serve returned already)
    }

    #[test]
    fn dispatch_reads_and_writes() {
        let (_d, s) = fixture();
        // reads on an empty ledger
        let v = call(&s, "status", Value::Null).unwrap();
        assert_eq!(v["repo"], "p");
        assert_eq!(v["counts"]["tasks"]["open"], 0);
        let v = call(&s, "check", Value::Null).unwrap();
        assert!(v["errors"].is_array());
        // write + read-back through fresh store opens
        let v = call(&s, "todo.add", json!({"title": "first", "pri": "high"})).unwrap();
        assert_eq!(v["item"]["id"], "T-001");
        assert_eq!(v["committed"], false); // no_commit
        let v = call(&s, "todo.list", Value::Null).unwrap();
        assert_eq!(v[0]["id"], "T-001");
        assert_eq!(v[0]["priority"], "high");
        let v = call(&s, "status", json!({"prompt": true})).unwrap();
        assert!(v["prompt"].as_str().unwrap().contains("T-001"));
        // item.set — legal step, then an invalid one → -32001 shape
        let v = call(
            &s,
            "item.set",
            json!({"id": "T-001", "status": "in-progress"}),
        )
        .unwrap();
        assert_eq!(v["item"]["status"], "in-progress");
        match call(&s, "item.set", json!({"id": "T-001", "status": "claimed"})) {
            Err(Fail::Transition { id, from, to }) => {
                assert_eq!(id.to_string(), "T-001");
                assert_eq!(from, Status::InProgress);
                assert_eq!(to, Status::Claimed);
            }
            _ => panic!("expected transition failure"),
        }
        // interrupt a live item → open + marker
        let v = call(&s, "interrupt", json!({"id": "T-001"})).unwrap();
        assert_eq!(v["status"], "open");
        // todo.done removes the line
        call(
            &s,
            "item.set",
            json!({"id": "T-001", "status": "in-progress"}),
        )
        .unwrap();
        let v = call(&s, "todo.done", json!({"id": "T-001"})).unwrap();
        assert_eq!(v["item"]["status"], "done");
        assert_eq!(
            call(&s, "todo.list", Value::Null)
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            0
        );
        // unknown + bad params
        assert!(matches!(
            call(&s, "nope", Value::Null),
            Err(Fail::UnknownMethod)
        ));
        assert!(matches!(
            call(&s, "todo.add", json!({})),
            Err(Fail::BadParams(_))
        ));
    }

    #[test]
    fn item_show_carries_kind_grouped_related() {
        let (_d, s) = fixture();
        call(&s, "todo.add", json!({"title": "alpha"})).unwrap();
        call(&s, "todo.add", json!({"title": "beta — see T-001"})).unwrap();
        call(&s, "item.set", json!({"id": "T-001", "blocks": "T-002"})).unwrap();
        call(&s, "handoff", json!({"id": "T-001", "note": "wip"})).unwrap();

        // no `index add`, no MYCO_HOME — related is derived live
        // from the ledger, so an unregistered repo answers fully
        let v = call(&s, "item.show", json!({"id": "T-001"})).unwrap();
        assert_eq!(
            v["related"]["handoff"],
            json!([{"id": "T-001", "title": "alpha"}])
        );
        assert_eq!(
            v["related"]["blocks"],
            json!([{"id": "T-002", "title": "beta — see T-001"}])
        );
        // T-002's title mentions T-001 → mirrored incoming edge
        assert_eq!(
            v["related"]["referenced_by"],
            json!([{"id": "T-002", "title": "beta — see T-001"}])
        );

        let v = call(&s, "item.show", json!({"id": "T-002"})).unwrap();
        assert_eq!(v["related"]["blocked_by"][0]["id"], "T-001");
        assert_eq!(v["related"]["references"][0]["id"], "T-001");

        // no edges → an empty object, not an absent key
        call(&s, "todo.add", json!({"title": "lonely"})).unwrap();
        let v = call(&s, "item.show", json!({"id": "T-003"})).unwrap();
        assert_eq!(v["related"], json!({}));

        // batch drives the identical dispatch — one line through
        // handle_line proves related-ness rides it too
        let line = s
            .handle_line(
                &JsonRpc2,
                &json!({"id": 1, "method": "item.show", "params": {"id": "T-002"}}).to_string(),
            )
            .unwrap();
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["result"]["related"]["blocked_by"][0]["id"], "T-001");
    }

    #[test]
    fn session_teardown_on_the_wire() {
        let (_d, s) = fixture();
        call(&s, "todo.add", json!({"title": "s1 work"})).unwrap();
        call(&s, "todo.add", json!({"title": "s2 work"})).unwrap();
        call(&s, "todo.add", json!({"title": "sessionless"})).unwrap();
        // claim through the wire item.set — session tags the claim
        let v = call(
            &s,
            "item.set",
            json!({"id": "T-001", "status": "in-progress", "session": "s1"}),
        )
        .unwrap();
        assert_eq!(v["claim"]["session"], "s1");
        assert_eq!(v["claim"]["note"], "claimed");
        call(
            &s,
            "item.set",
            json!({"id": "T-002", "status": "claimed", "owner": "devin", "session": "s2"}),
        )
        .unwrap();
        call(
            &s,
            "item.set",
            json!({"id": "T-003", "status": "in-progress"}),
        )
        .unwrap();

        // single-id interrupt with a foreign session → verb error,
        // nothing reverts
        match call(&s, "interrupt", json!({"id": "T-001", "session": "s2"})) {
            Err(Fail::Verb(m)) => assert!(m.contains("claimed under session s1"), "{m}"),
            other => panic!("expected verb failure: {other:?}"),
        }
        let v = call(&s, "todo.list", Value::Null).unwrap();
        assert_eq!(v[0]["status"], "in-progress");

        // the teardown — only s1's item reverts
        let v = call(&s, "interrupt", json!({"session": "s1"})).unwrap();
        assert_eq!(v["session"], "s1");
        let ids: Vec<&str> = v["interrupted"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["T-001"]);
        assert_eq!(v["errors"], json!([]));
        assert_eq!(v["interrupted"][0]["entry"]["session"], "s1");
        // s2 + sessionless claims untouched (the revert re-lands
        // T-001 at the section tail — sort before comparing)
        let v = call(&s, "todo.list", Value::Null).unwrap();
        let mut statuses: Vec<(&str, &str)> = v
            .as_array()
            .unwrap()
            .iter()
            .map(|i| (i["id"].as_str().unwrap(), i["status"].as_str().unwrap()))
            .collect();
        statuses.sort();
        assert_eq!(
            statuses,
            [
                ("T-001", "open"),
                ("T-002", "claimed"),
                ("T-003", "in-progress")
            ]
        );
        // no id and no session → -32602
        assert!(matches!(
            call(&s, "interrupt", json!({})),
            Err(Fail::BadParams(_))
        ));
    }

    #[test]
    fn reap_on_the_wire() {
        let (d, s) = fixture();
        call(&s, "todo.add", json!({"title": "stale wip"})).unwrap();
        call(
            &s,
            "item.set",
            json!({"id": "T-001", "status": "in-progress"}),
        )
        .unwrap();
        call(&s, "todo.add", json!({"title": "fresh wip"})).unwrap();
        call(
            &s,
            "item.set",
            json!({"id": "T-002", "status": "in-progress"}),
        )
        .unwrap();
        // backdate T-001's `created:` — T-002 stays fresh
        let p = d.path().join("TODO.md");
        let text = std::fs::read_to_string(&p).unwrap();
        let mut out = String::with_capacity(text.len());
        for line in text.lines() {
            if line.split_whitespace().any(|t| t == "T-001") {
                let pos = line.find("created:").unwrap() + "created:".len();
                out.push_str(&line[..pos]);
                out.push_str("2020-01-01");
                out.push_str(&line[pos + 10..]);
            } else {
                out.push_str(line);
            }
            out.push('\n');
        }
        std::fs::write(&p, out).unwrap();

        // dry_run plans with zero writes — same payload shape
        let v = call(&s, "reap", json!({"dry-run": true})).unwrap();
        assert_eq!(v["dry_run"], true);
        assert_eq!(v["committed"], false);
        assert_eq!(v["reaped"][0]["id"], "T-001");
        assert!(v["reaped"][0]["idle_days"].as_i64().unwrap() > 100);
        // the real sweep — no_commit server → committed false,
        // but the status still reverted on disk
        let v = call(&s, "reap", json!({"idle-days": 3})).unwrap();
        assert_eq!(v["dry_run"], false);
        assert_eq!(v["committed"], false);
        assert_eq!(v["reaped"].as_array().unwrap().len(), 1);
        assert_eq!(v["errors"], json!([]));
        let v = call(&s, "todo.list", Value::Null).unwrap();
        let mut statuses: Vec<(&str, &str)> = v
            .as_array()
            .unwrap()
            .iter()
            .map(|i| (i["id"].as_str().unwrap(), i["status"].as_str().unwrap()))
            .collect();
        statuses.sort();
        assert_eq!(statuses, [("T-001", "open"), ("T-002", "in-progress")]);
        // swept clean → empty plan, still a clean payload
        let v = call(&s, "reap", json!({})).unwrap();
        assert_eq!(v["reaped"], json!([]));
        assert_eq!(v["committed"], false);
    }

    #[test]
    fn stdio_transport_lines_and_eof() {
        let input = b"one\n\r\nthree";
        let mut out = Vec::new();
        let mut t = StdioTransport::new(&input[..], &mut out);
        assert_eq!(t.read_request().unwrap().as_deref(), Some("one"));
        assert_eq!(t.read_request().unwrap().as_deref(), Some(""));
        assert_eq!(t.read_request().unwrap().as_deref(), Some("three"));
        assert_eq!(t.read_request().unwrap(), None);
        t.write_response("resp").unwrap();
        assert_eq!(out, b"resp\n");
    }

    #[test]
    fn view_named_adhoc_list_and_errors() {
        let (_d, s) = rooted_fixture();
        no_user_config(|| {
            // list mode — `{}` enumerates the defined views
            let v = call(&s, "view", json!({})).unwrap();
            let names: Vec<&str> = v["views"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x["name"].as_str().unwrap())
                .collect();
            assert_eq!(
                names,
                [
                    "board",
                    "open",
                    "epic",
                    "milestone",
                    "next",
                    "org-board",
                    "org-status"
                ]
            );
            assert_eq!(v["views"][0]["origin"], "builtin");
            assert!(v["views"][0]["spec"].is_object());
            assert_eq!(v["warnings"], json!([]));

            call(&s, "todo.add", json!({"title": "a"})).unwrap();
            call(&s, "todo.add", json!({"title": "b", "pri": "high"})).unwrap();

            // named view — `open` selects open items, ungrouped
            let v = call(&s, "view", json!({"name": "open"})).unwrap();
            assert_eq!(v["view"], "open");
            assert_eq!(v["spec"]["filter"]["status"], json!(["open"]));
            let items = v["groups"][0]["items"].as_array().unwrap();
            assert_eq!(items.len(), 2);
            assert_eq!(items[0]["id"], "T-002"); // pri,id sort
            assert_eq!(v["interrupted"], json!([]));
            assert_eq!(v["warnings"], json!([]));

            // ad-hoc spec — group_by yields labeled buckets
            let v = call(
                &s,
                "view",
                json!({"spec": {"filter": {"kind": ["task"]}, "group_by": "priority"}}),
            )
            .unwrap();
            assert_eq!(v["view"], Value::Null);
            let groups = v["groups"].as_array().unwrap();
            assert_eq!(groups[0]["label"], "critical");
            assert_eq!(
                groups.iter().find(|g| g["label"] == "high").unwrap()["items"][0]["id"],
                "T-002"
            );

            // spec merges over the named view — the set field
            // replaces, unset keep the named spec's values
            let v = call(
                &s,
                "view",
                json!({"name": "open", "spec": {"filter": {"status": ["done"]}}}),
            )
            .unwrap();
            let total: usize = v["groups"]
                .as_array()
                .unwrap()
                .iter()
                .map(|g| g["items"].as_array().unwrap().len())
                .sum();
            assert_eq!(total, 0, "done filter over `open`'s spec");

            // unknown name → verb error naming the defined set
            match call(&s, "view", json!({"name": "nope"})) {
                Err(Fail::Verb(m)) => assert!(m.contains("open"), "{m}"),
                other => panic!("expected verb failure: {other:?}"),
            }
            // malformed spec shape → -32602; unknown key → -32602
            assert!(matches!(
                call(&s, "view", json!({"spec": {"group_by": "wat"}})),
                Err(Fail::BadParams(_))
            ));
            assert!(matches!(
                call(&s, "view", json!({"spec": {"nonsense": 1}})),
                Err(Fail::BadParams(_))
            ));
            // an invalid-but-decodable spec → verb error
            assert!(matches!(
                call(
                    &s,
                    "view",
                    json!({"spec": {"filter": {"claimed": true, "owner": ["devin"]}}})
                ),
                Err(Fail::Verb(_))
            ));
        });
    }

    #[test]
    fn view_resolves_repo_config_views() {
        let (d, s) = rooted_fixture();
        std::fs::write(
            d.path().join("myco.toml"),
            "[views.hot]\nfilter = { pri_min = \"high\" }\n",
        )
        .unwrap();
        no_user_config(|| {
            call(&s, "todo.add", json!({"title": "cold"})).unwrap();
            call(&s, "todo.add", json!({"title": "hot", "pri": "critical"})).unwrap();
            let v = call(&s, "view", json!({"name": "hot"})).unwrap();
            let items = v["groups"][0]["items"].as_array().unwrap();
            assert_eq!(items.len(), 1);
            assert_eq!(items[0]["id"], "T-002");
            // list reports the config origin too
            let v = call(&s, "view", json!({})).unwrap();
            let hot = v["views"]
                .as_array()
                .unwrap()
                .iter()
                .find(|x| x["name"] == "hot")
                .unwrap();
            assert_eq!(hot["origin"], "config");
        });
    }

    #[test]
    fn view_spec_accepts_priority_tokens_like_toml() {
        let (_d, s) = rooted_fixture();
        no_user_config(|| {
            call(&s, "todo.add", json!({"title": "hot", "pri": "high"})).unwrap();
            call(&s, "todo.add", json!({"title": "cold"})).unwrap();
            // "crit" (token) parses like "critical" (serde name)
            let v = call(
                &s,
                "view",
                json!({"spec": {"filter": {"pri_min": "high", "pri_max": "crit"}}}),
            )
            .unwrap();
            let items = v["groups"][0]["items"].as_array().unwrap();
            assert_eq!(items.len(), 1);
            assert_eq!(items[0]["id"], "T-001");
        });
    }

    #[test]
    fn handle_line_skips_blanks_and_encodes_errors() {
        let (_d, s) = fixture();
        assert_eq!(s.handle_line(&JsonRpc2, ""), None);
        assert_eq!(s.handle_line(&JsonRpc2, "   "), None);
        let bad = s.handle_line(&JsonRpc2, "not json").unwrap();
        let v: Value = serde_json::from_str(&bad).unwrap();
        assert_eq!(v["id"], Value::Null);
        assert_eq!(v["error"]["code"], PARSE_ERROR);
    }
}
