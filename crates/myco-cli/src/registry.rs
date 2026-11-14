//! The single verb registry (P-007) — one [`VERBS`] table drives
//! all three front-ends: the clap command tree ([`command`],
//! builder mode, no derive), `manifest().verbs`/`implemented`
//! ([`manifest_verbs`]), and serve/batch dispatch
//! ([`crate::serve::Server::dispatch`]). A verb can't exist on one
//! surface and not the others: `cli`/`wire` handlers share one
//! `params` schema — the wire schema and the CLI surface are one
//! declaration decoded into the same [`Args`] bag.
//!
//! Config-declared verbs (`[verbs]` in `myco.toml`) aren't in the
//! table — they're data, resolved per call through
//! `myco_ops::verb` and executed by `cfgverb`. Builtins always
//! win a name collision.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::OnceLock;

use clap::{Arg, ArgAction, ArgMatches, Command};
use serde_json::Value;

use crate::serve;
use myco_ops::verb::BuiltinVerb;

// ---------------------------------------------------------------- params

/// Which front-ends a param applies to — the CLI filter flags
/// are `Cli`-only, `view`/`next`'s `spec` patch is `Wire`-only,
/// everything else is `Both`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum On {
    /// CLI flag + wire key.
    Both,
    /// CLI only.
    Cli,
    /// Wire only.
    Wire,
}

/// How a param is taken on the CLI — wire params decode by name
/// regardless (positionals are required keys).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PKind {
    /// A positional argument.
    Pos,
    /// A `--flag` boolean.
    Flag,
    /// A `--opt <value>`.
    Opt,
}

/// The value type — drives the clap validator and the wire-side
/// parse. `Spec` is wire-only: a `SpecPatch` object merged into a
/// `ViewSpec` (`view`/`next`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Val {
    /// Free text.
    Str,
    /// A filesystem path.
    Path,
    /// An `ItemId` (`T-001`, `ADR-0004`).
    Id,
    /// A status token (`in-progress`).
    Status,
    /// A priority (`low|med|high|crit`, names too).
    Pri,
    /// An actor id.
    Actor,
    /// A non-negative integer.
    Count,
    /// A sort key (`pri`, `-created`).
    Sort,
    /// An item kind (`task|issue|proposal|adr|plan`).
    Kind,
    /// A view source (`repo|index`).
    Source,
    /// A group axis (`status|priority|kind|owner|epic|milestone|repo|cycle`).
    Axis,
    /// A view format (`list|board|repos|timeline|stats|burnup`).
    Format,
    /// A wire-only `ViewSpec` patch object.
    Spec,
}

/// One declared param — CLI flag/positional *and* wire key.
/// Fields are `pub` so the table reads as data; the `const`
/// builders keep entries terse.
#[derive(Debug, Clone)]
pub struct Param {
    /// The canonical name — clap gets `--name` (`_` → `-`), the
    /// wire gets the `"name"` key.
    pub name: &'static str,
    /// Help text.
    pub help: &'static str,
    /// Pos / Flag / Opt.
    pub kind: PKind,
    /// The value type.
    pub val: Val,
    /// Which front-ends see it.
    pub on: On,
    /// Repeatable / multi-value (CLI `num_args`, wire array).
    pub multi: bool,
    /// Comma-separated values also accepted (CLI).
    pub delimited: bool,
    /// Values may start with `-` (CLI).
    pub hyphen: bool,
    /// Positionals are required by default; opts/flags aren't.
    pub required: bool,
    /// A baked default (`--to backlog`).
    pub default: Option<&'static str>,
    /// A short flag (`-m`).
    pub short: Option<char>,
    /// A visible alias (`--priority` for `--pri`).
    pub alias: Option<&'static str>,
    /// Mutually exclusive param name.
    pub conflicts: Option<&'static str>,
    /// `value_name` in help.
    pub vname: Option<&'static str>,
}

const fn p(name: &'static str, kind: PKind, val: Val, help: &'static str) -> Param {
    Param {
        name,
        help,
        kind,
        val,
        on: On::Both,
        multi: false,
        delimited: false,
        hyphen: false,
        required: matches!(kind, PKind::Pos),
        default: None,
        short: None,
        alias: None,
        conflicts: None,
        vname: None,
    }
}

/// A required positional.
pub const fn pos(name: &'static str, val: Val, help: &'static str) -> Param {
    p(name, PKind::Pos, val, help)
}

/// A `--opt` (optional by default — `.req()` for `--note`).
pub const fn opt(name: &'static str, val: Val, help: &'static str) -> Param {
    p(name, PKind::Opt, val, help)
}

/// A `--flag` boolean.
pub const fn flag(name: &'static str, help: &'static str) -> Param {
    p(name, PKind::Flag, Val::Str, help)
}

impl Param {
    /// Optional (not required) — positionals mostly.
    pub const fn optional(mut self) -> Param {
        self.required = false;
        self
    }
    /// Required — `--note`-style mandatory options.
    pub const fn req(mut self) -> Param {
        self.required = true;
        self
    }
    /// Repeatable/multi-value.
    pub const fn multi(mut self) -> Param {
        self.multi = true;
        self
    }
    /// Comma-separated values also accepted.
    pub const fn delim(mut self) -> Param {
        self.delimited = true;
        self
    }
    /// `allow_hyphen_values`.
    pub const fn hyphen(mut self) -> Param {
        self.hyphen = true;
        self
    }
    /// Baked default value.
    pub const fn default(mut self, v: &'static str) -> Param {
        self.default = Some(v);
        self
    }
    /// A short flag.
    pub const fn short(mut self, c: char) -> Param {
        self.short = Some(c);
        self
    }
    /// A visible long alias.
    pub const fn alias(mut self, a: &'static str) -> Param {
        self.alias = Some(a);
        self
    }
    /// Conflicts with another param.
    pub const fn conflicts(mut self, other: &'static str) -> Param {
        self.conflicts = Some(other);
        self
    }
    /// `value_name` in help output.
    pub const fn vname(mut self, v: &'static str) -> Param {
        self.vname = Some(v);
        self
    }
    /// CLI-only.
    pub const fn cli(mut self) -> Param {
        self.on = On::Cli;
        self
    }
    /// Wire-only.
    pub const fn wire(mut self) -> Param {
        self.on = On::Wire;
        self
    }
}

// ---------------------------------------------------------------- args

/// One decoded param value — strings keep the raw token (typed
/// parses happen in accessors; `Spec` keeps the JSON object).
#[derive(Debug, Clone)]
pub enum ArgVal {
    /// A `--flag` — `true` when present (wire: `"k": true`).
    Flag(bool),
    /// A single value.
    One(String),
    /// A multi value.
    Many(Vec<String>),
    /// A `SpecPatch` JSON object (wire-only).
    Spec(Value),
}

/// The decoded argument bag one handler sees — identical shape
/// whether it came from clap matches, wire JSON params, or an
/// alias's baked argv.
pub struct Args {
    vals: BTreeMap<&'static str, ArgVal>,
}

/// `s` parses as `v`'s type — the clap validator and the wire
/// check share it.
fn validate(v: Val, s: &str) -> Result<(), String> {
    macro_rules! p {
        ($t:ty) => {
            s.parse::<$t>().map(|_| ()).map_err(|e| e.to_string())
        };
    }
    match v {
        Val::Str | Val::Path | Val::Spec => Ok(()),
        Val::Id => p!(myco_core::ItemId),
        Val::Status => p!(myco_core::Status),
        Val::Pri => p!(myco_core::Priority),
        Val::Actor => p!(myco_core::Actor),
        Val::Count => p!(u64),
        Val::Sort => p!(myco_core::SortKey),
        Val::Kind => p!(myco_core::ItemKind),
        Val::Source => p!(myco_core::ViewSource),
        Val::Axis => p!(myco_core::GroupAxis),
        Val::Format => p!(myco_core::ViewFormat),
    }
}

impl Args {
    /// clap `ArgMatches` → the bag (CLI-facing params only).
    pub fn from_matches(v: &Verb, m: &ArgMatches) -> Args {
        let mut vals = BTreeMap::new();
        for p in v.params() {
            if p.on == On::Wire {
                continue;
            }
            match (p.kind, p.multi) {
                (PKind::Flag, _) => vals.insert(p.name, ArgVal::Flag(m.get_flag(p.name))),
                (_, true) => {
                    let many: Vec<String> = m
                        .get_many::<String>(p.name)
                        .map(|vs| vs.cloned().collect())
                        .unwrap_or_default();
                    vals.insert(p.name, ArgVal::Many(many))
                }
                _ => {
                    if let Some(s) = m.get_one::<String>(p.name) {
                        vals.insert(p.name, ArgVal::One(s.clone()))
                    } else {
                        continue;
                    }
                }
            };
        }
        Args { vals }
    }

    /// Wire `params` → the bag (wire-facing params only; unknown
    /// keys and bad values are decode errors — `-32602` at the
    /// caller). `null`/`{}` are both "no params".
    pub fn from_json(v: &Verb, params: &Value) -> Result<Args, String> {
        let mut vals = BTreeMap::new();
        let obj = if params.is_null() {
            serde_json::Map::new()
        } else {
            params
                .as_object()
                .ok_or_else(|| "params must be an object".to_string())?
                .clone()
        };
        for key in obj.keys() {
            if v.params().all(|p| p.on == On::Cli || p.name != key) {
                return Err(format!("unknown param \"{key}\""));
            }
        }
        for p in v.params() {
            if p.on == On::Cli {
                continue;
            }
            let Some(raw) = obj.get(p.name) else {
                if p.required {
                    return Err(format!("missing required param \"{}\"", p.name));
                }
                continue;
            };
            let entry = match (p.kind, p.multi) {
                (PKind::Flag, _) => ArgVal::Flag(
                    raw.as_bool()
                        .ok_or_else(|| format!("\"{}\" must be a boolean", p.name))?,
                ),
                (_, true) => {
                    let items: Vec<String> = match raw {
                        Value::Array(xs) => xs.iter().map(json_scalar).collect::<Result<_, _>>()?,
                        other => vec![json_scalar(other)?],
                    };
                    for s in &items {
                        validate(p.val, s).map_err(|e| format!("\"{}\": {e}", p.name))?;
                    }
                    ArgVal::Many(items)
                }
                _ => {
                    if p.val == Val::Spec {
                        ArgVal::Spec(raw.clone())
                    } else {
                        let s = json_scalar(raw)?;
                        validate(p.val, &s).map_err(|e| format!("\"{}\": {e}", p.name))?;
                        ArgVal::One(s)
                    }
                }
            };
            vals.insert(p.name, entry);
        }
        Ok(Args { vals })
    }

    /// Baked/interactive argv tokens → the bag — the alias path.
    /// Global flags (`--json`, `--no-commit`, `--dir`, `--root`)
    /// fold into `ctx` rather than the bag. `wire` restricts to
    /// wire-facing params: a CLI-only flag in an alias's args is
    /// a dispatch error on the wire, not a silent drop.
    pub fn from_argv(
        v: &Verb,
        tokens: &[String],
        ctx: &mut Ctx,
        wire: bool,
    ) -> Result<Args, String> {
        let mut vals = BTreeMap::new();
        let mut positional = 0usize;
        let mut i = 0;
        let mut no_flags = false;
        let set =
            |vals: &mut BTreeMap<&'static str, ArgVal>, p: &Param, s: &str| -> Result<(), String> {
                validate(p.val, s).map_err(|e| format!("--{}: {e}", p.name))?;
                match (p.kind, p.multi) {
                    (_, true) => match vals.entry(p.name).or_insert_with(|| ArgVal::Many(vec![])) {
                        ArgVal::Many(xs) => xs.extend(s.split(',').map(str::to_string)),
                        _ => unreachable!("multi param bag is Many"),
                    },
                    _ => {
                        vals.insert(p.name, ArgVal::One(s.to_string()));
                    }
                }
                Ok(())
            };
        while i < tokens.len() {
            let t = &tokens[i];
            i += 1;
            if !no_flags && t == "--" {
                no_flags = true;
                continue;
            }
            if !no_flags && t.starts_with("--") {
                let body = &t[2..];
                let (key, inline) = match body.split_once('=') {
                    Some((k, x)) => (k, Some(x.to_string())),
                    None => (body, None),
                };
                // global flags fold into ctx
                match key {
                    "json" => {
                        ctx.json = true;
                        continue;
                    }
                    "no-commit" => {
                        ctx.no_commit = true;
                        continue;
                    }
                    "dir" | "root" => {
                        let val = take_value(key, inline, tokens, &mut i)?;
                        let p = PathBuf::from(val);
                        if key == "dir" {
                            ctx.dir = Some(p);
                        } else {
                            ctx.root = Some(p);
                        }
                        continue;
                    }
                    _ => {}
                }
                let name = key.replace('-', "_");
                let p = v
                    .params()
                    .find(|p| {
                        p.name.replace('_', "-") == key || p.name == name || p.alias == Some(key)
                    })
                    .ok_or_else(|| format!("unknown argument \"--{key}\" for \"{}\"", v.name))?;
                if wire && p.on == On::Cli {
                    return Err(format!(
                        "argument \"--{key}\" isn't serveable — \"{}\" is CLI-only",
                        p.name
                    ));
                }
                if p.kind == PKind::Flag {
                    if let Some(x) = inline {
                        ctx_bool(&x)
                            .map(|b| vals.insert(p.name, ArgVal::Flag(b)))
                            .map_err(|e| format!("--{key}: {e}"))?;
                    } else {
                        vals.insert(p.name, ArgVal::Flag(true));
                    }
                } else {
                    let val = take_value(key, inline, tokens, &mut i)?;
                    set(&mut vals, p, &val)?;
                }
            } else if !no_flags && t.starts_with('-') && t.len() > 1 && !looks_negative(t) {
                // short flag — `-m msg`
                let c = t.chars().nth(1).unwrap_or('-');
                let p = v
                    .params()
                    .find(|p| p.short == Some(c))
                    .ok_or_else(|| format!("unknown argument \"{t}\" for \"{}\"", v.name))?;
                let inline = t.get(2..).map(str::to_string);
                let val = take_value(&c.to_string(), inline, tokens, &mut i)?;
                set(&mut vals, p, &val)?;
            } else {
                // positional — next unfilled pos param
                let pos_params: Vec<&Param> = v.params().filter(|p| p.kind == PKind::Pos).collect();
                let p = pos_params
                    .iter()
                    .enumerate()
                    .find(|(n, _)| *n == positional)
                    .map(|(_, p)| *p)
                    .or_else(|| pos_params.last().filter(|p| p.multi).copied());
                match p {
                    Some(p) => {
                        if !p.multi {
                            positional += 1;
                        } else if positional < pos_params.len() - 1 {
                            positional = pos_params.len() - 1;
                        }
                        set(&mut vals, p, t)?;
                    }
                    None => {
                        return Err(format!(
                            "unexpected argument \"{t}\" — \"{}\" takes no positionals",
                            v.name
                        ))
                    }
                }
            }
        }
        for p in v.params() {
            if p.required && !vals.contains_key(p.name) {
                return Err(format!(
                    "missing required \"{}\" for \"{}\"",
                    p.name, v.name
                ));
            }
            if let Some(d) = p.default {
                vals.entry(p.name)
                    .or_insert_with(|| ArgVal::One(d.to_string()));
            }
        }
        Ok(Args { vals })
    }

    fn get(&self, name: &str) -> Option<&ArgVal> {
        self.vals.get(name)
    }

    /// A `--flag` — `false` when absent.
    pub fn flag(&self, name: &str) -> bool {
        matches!(self.get(name), Some(ArgVal::Flag(true)))
    }

    /// A single string value, when present.
    pub fn opt(&self, name: &str) -> Option<&str> {
        match self.get(name) {
            Some(ArgVal::One(s)) => Some(s),
            _ => None,
        }
    }

    /// A required single string.
    pub fn text(&self, name: &str) -> Result<&str, String> {
        self.opt(name)
            .ok_or_else(|| format!("missing required \"{name}\""))
    }

    /// `opt` parsed as `T`.
    pub fn parsed<T: FromStr>(&self, name: &str) -> Result<Option<T>, String>
    where
        T::Err: std::fmt::Display,
    {
        self.opt(name)
            .map(|s| s.parse().map_err(|e: T::Err| format!("\"{name}\": {e}")))
            .transpose()
    }

    /// `text` parsed as `T`.
    pub fn req<T: FromStr>(&self, name: &str) -> Result<T, String>
    where
        T::Err: std::fmt::Display,
    {
        self.parsed(name)?
            .ok_or_else(|| format!("missing required \"{name}\""))
    }

    /// Every value of a multi param as raw strings.
    pub fn many(&self, name: &str) -> Vec<String> {
        match self.get(name) {
            Some(ArgVal::Many(xs)) => xs.clone(),
            Some(ArgVal::One(s)) => vec![s.clone()],
            _ => Vec::new(),
        }
    }

    /// Every value of a multi param parsed as `T`.
    pub fn many_parsed<T: FromStr>(&self, name: &str) -> Result<Vec<T>, String>
    where
        T::Err: std::fmt::Display,
    {
        self.many(name)
            .iter()
            .map(|s| s.parse().map_err(|e: T::Err| format!("\"{name}\": {e}")))
            .collect()
    }

    /// A single `T` out of a param that may be multi (wire sends
    /// one id even where the CLI takes several).
    pub fn one<T: FromStr>(&self, name: &str) -> Result<T, String>
    where
        T::Err: std::fmt::Display,
    {
        let xs = self.many_parsed::<T>(name)?;
        match xs.len() {
            1 => Ok(xs.into_iter().next().unwrap_or_else(|| unreachable!())),
            0 => Err(format!("missing required \"{name}\"")),
            _ => Err(format!("\"{name}\" takes a single value")),
        }
    }

    /// The `Spec` wire object, when present.
    pub fn spec(&self, name: &str) -> Result<Option<crate::serve::SpecPatch>, String> {
        match self.get(name) {
            Some(ArgVal::Spec(v)) => serde_json::from_value(v.clone())
                .map(Some)
                .map_err(|e| format!("\"{name}\": {e}")),
            _ => Ok(None),
        }
    }
}

/// `--flag=<val>` inline values parse as booleans.
fn ctx_bool(s: &str) -> Result<bool, String> {
    match s {
        "true" | "1" | "yes" => Ok(true),
        "false" | "0" | "no" => Ok(false),
        _ => Err(format!("expected a boolean, got {s:?}")),
    }
}

/// `-5`-looking tokens are values, not flags.
fn looks_negative(t: &str) -> bool {
    t[1..].chars().next().is_some_and(|c| c.is_ascii_digit())
}

/// `--opt value` / `--opt=value` — the value half.
fn take_value(
    key: &str,
    inline: Option<String>,
    tokens: &[String],
    i: &mut usize,
) -> Result<String, String> {
    if let Some(v) = inline {
        return Ok(v);
    }
    let v = tokens
        .get(*i)
        .ok_or_else(|| format!("\"--{key}\" needs a value"))?;
    *i += 1;
    Ok(v.clone())
}

/// A wire scalar → its string form (numbers stringify so
/// `"limit": 5` and `"5"` decode alike).
fn json_scalar(v: &Value) -> Result<String, String> {
    match v {
        Value::String(s) => Ok(s.clone()),
        Value::Number(n) => Ok(n.to_string()),
        Value::Bool(b) => Ok(b.to_string()),
        _ => Err(format!("expected a string or number, got {v}")),
    }
}

// ---------------------------------------------------------------- verbs

/// The CLI-facing context — the global flags every handler sees.
#[derive(Debug, Default, Clone)]
pub struct Ctx {
    /// `--json` — machine output.
    pub json: bool,
    /// `--no-commit` — skip the per-verb commit.
    pub no_commit: bool,
    /// `--dir <path>` — the project worktree override.
    pub dir: Option<PathBuf>,
    /// `--root <path>` — the repo root override.
    pub root: Option<PathBuf>,
}

/// The CLI handler — decoded args in, exit code out.
pub type CliHandler = fn(&Ctx, &Args) -> Result<i32, String>;
/// The wire handler — the serve/batch payload out.
pub type WireHandler = fn(&serve::Server, &Args) -> Result<Value, serve::Fail>;

/// One verb — name, help, param schema, kind-lock, mutability,
/// and its front-end handlers.
pub struct Verb {
    /// The canonical name — the serve method name; `.` splits the
    /// CLI path (`todo.add` → `myco todo add`).
    pub name: &'static str,
    /// Help text (clap `about` + generated docs).
    pub about: &'static str,
    /// The param schema — shared by the clap tree, the wire
    /// decoder, and (for aliases) argv baking. Slices-of-slices so
    /// shared bundles (`META`, `LIST`, `FILTER`) declare once.
    pub params: &'static [&'static [Param]],
    /// The item kind the verb family locks to, when one exists —
    /// part of the declared schema (P-007); the kind-aware bits
    /// that read it (alias lint, completions) land with them.
    #[allow(dead_code)]
    pub kind_lock: Option<myco_core::ItemKind>,
    /// Can write the ledger/index — alias targets must be
    /// read-only, so `mutates` disqualifies a builtin.
    pub mutates: bool,
    /// Hidden from help (docs pipeline verbs still work).
    pub hidden: bool,
    /// The CLI handler — `None` for wire-only verbs.
    pub cli: Option<CliHandler>,
    /// The serve/batch handler — `None` for unserved verbs.
    pub wire: Option<WireHandler>,
}

impl Verb {
    /// Every declared param, flattened.
    pub fn params(&self) -> impl Iterator<Item = &Param> {
        self.params.iter().flat_map(|s| s.iter())
    }
}

/// The verb groups — `myco <group> <leaf>`; leaf names in the
/// table are `<group>.<leaf>`.
struct Group {
    name: &'static str,
    about: &'static str,
}

const GROUPS: &[Group] = &[
    Group {
        name: "todo",
        about: "TODO.md tasks — add / done / cancel / defer / list.",
    },
    Group {
        name: "backlog",
        about: "BACKLOG.md parked items — add / promote / list.",
    },
    Group {
        name: "done",
        about: "DONE.md — the archived-task archive (`todo done`/`todo cancel` land here; `item set --status open|deferred` reopens, `done remove` deletes).",
    },
    Group {
        name: "issue",
        about: "Tracked issues, one file each under `issues/`.",
    },
    Group {
        name: "proposal",
        about: "Design proposals under `proposals/` (`P-NNN`).",
    },
    Group {
        name: "plan",
        about: "Execution plans under `plans/` (`PL-NNN`).",
    },
    Group {
        name: "adr",
        about: "Architecture decision records under `adrs/` (`ADR-NNNN`).",
    },
    Group {
        name: "item",
        about: "Kind-agnostic item verbs — show / set / move.",
    },
    Group {
        name: "absorb",
        about: "Import work into the ledger — TODO/FIXME comments, Jira CSV exports, GitHub issue dumps.",
    },
    Group {
        name: "index",
        about: "The derived index: register repos, query/rebuild snapshots.",
    },
    Group {
        name: "org",
        about: "Cross-repo scaffolding — re-run `init` on every registered repo. Cross-repo *views* are `myco view org-board|org-status` (index-sourced).",
    },
];

// ---------------------------------------------------------------- param bundles

/// `--pri/--owner/--label/--blocks/--links/--epic/--milestone/
/// --due/--est/--cycle` — every `add`/`new` verb's metadata flags.
const META: &[Param] = &[
    opt("pri", Val::Pri, "Priority: low|med|high|crit.").alias("priority"),
    opt(
        "owner",
        Val::Actor,
        "Owner actor id (`devin`, `human:luis`, …).",
    ),
    opt("label", Val::Str, "`#label` — repeatable.").multi(),
    opt(
        "blocks",
        Val::Id,
        "Ids this item blocks — comma-separated or repeatable.",
    )
    .multi()
    .delim(),
    opt(
        "links",
        Val::Id,
        "Ids this item is associated with (its plan, proposal, …) — comma-separated or repeatable.",
    )
    .multi()
    .delim(),
    opt(
        "epic",
        Val::Str,
        "Epic name — the delivery grouping this item belongs to (`[A-Za-z0-9_.-]+`).",
    ),
    opt(
        "milestone",
        Val::Str,
        "Milestone name — the target this item ships under (`[A-Za-z0-9_.-]+`).",
    ),
    opt("due", Val::Str, "Due date — `YYYY-MM-DD`."),
    opt(
        "est",
        Val::Count,
        "Estimate in points (0–1000) — relative size, not hours.",
    )
    .alias("estimate"),
    opt(
        "cycle",
        Val::Str,
        "Cycle name — the iteration this item is scheduled in (`[A-Za-z0-9_.-]+`).",
    ),
];

/// `--status/--owner/--label/--pri/--epic/--milestone/--due/
/// --est/--cycle/--query/--limit` — every `list` verb's filters.
const LIST: &[Param] = &[
    opt("status", Val::Status, "Keep only items with this status."),
    opt("owner", Val::Str, "Keep only items owned by this actor."),
    opt(
        "label",
        Val::Str,
        "Keep only items carrying all of these labels (repeatable).",
    )
    .multi(),
    opt("pri", Val::Pri, "Keep only items at this priority."),
    opt("epic", Val::Str, "Keep only items in this epic."),
    opt("milestone", Val::Str, "Keep only items in this milestone."),
    opt(
        "due",
        Val::Str,
        "Keep only items due on this date (`YYYY-MM-DD`).",
    ),
    opt(
        "est",
        Val::Count,
        "Keep only items with this point estimate.",
    )
    .alias("estimate"),
    opt("cycle", Val::Str, "Keep only items in this cycle."),
    opt(
        "query",
        Val::Str,
        "Keep only items whose id, title, or a label contains this text (case-insensitive).",
    ),
    opt(
        "limit",
        Val::Count,
        "Keep only the first N items after filtering.",
    )
    .vname("N"),
];

/// `--archived` on top of [`LIST`] — the file-kind lists
/// (backlog items never leave `BACKLOG.md`, so only file kinds
/// carry it).
const FILE_LIST: &[Param] = &[
    flag("archived", "Include archived items."),
    opt("status", Val::Status, "Keep only items with this status."),
    opt("owner", Val::Str, "Keep only items owned by this actor."),
    opt(
        "label",
        Val::Str,
        "Keep only items carrying all of these labels (repeatable).",
    )
    .multi(),
    opt("pri", Val::Pri, "Keep only items at this priority."),
    opt("epic", Val::Str, "Keep only items in this epic."),
    opt("milestone", Val::Str, "Keep only items in this milestone."),
    opt(
        "due",
        Val::Str,
        "Keep only items due on this date (`YYYY-MM-DD`).",
    ),
    opt(
        "est",
        Val::Count,
        "Keep only items with this point estimate.",
    )
    .alias("estimate"),
    opt("cycle", Val::Str, "Keep only items in this cycle."),
    opt(
        "query",
        Val::Str,
        "Keep only items whose id, title, or a label contains this text (case-insensitive).",
    ),
    opt(
        "limit",
        Val::Count,
        "Keep only the first N items after filtering.",
    )
    .vname("N"),
];

/// `--body/--text` — the `new` verbs' body source.
const BODY: &[Param] = &[
    opt(
        "body",
        Val::Str,
        "Body markdown from a file, or `-` for stdin.",
    )
    .conflicts("text")
    .vname("FILE|-"),
    opt("text", Val::Str, "Body markdown inline."),
];

/// `[id]` + `--note/--actor/--session` — `interrupt` (the
/// handoff verb requires the note). Omitting `id` makes
/// `--session` the bulk teardown: every item claimed under the
/// session reverts.
const INTERRUPT_PARAMS: &[Param] = &[
    pos(
        "id",
        Val::Id,
        "Any item id (`T-NNN`, `I-NNN`, `P-NNN`, `ADR-NNNN`, `PL-NNN`) — omit for the `--session` bulk teardown.",
    )
    .optional(),
    opt(
        "note",
        Val::Str,
        "Extra context appended to the `interrupted` note.",
    )
    .vname("TEXT"),
    opt("actor", Val::Actor, "Who interrupted it (default: the resolved actor)."),
    opt(
        "session",
        Val::Str,
        "Session scope — a sepia session id, `cli:<pid>`, any id string. With `id`: interrupt only when the item's claim tag matches. Without `id`: teardown — interrupt every item claimed under it.",
    ),
];

/// The `view`/`next` filter flags — CLI-only (the wire merges a
/// `spec` patch instead). Each maps onto the `ViewFilter` field
/// `filter = { … }` uses.
const FILTER: &[Param] = &[
    opt(
        "kind",
        Val::Kind,
        "Item kinds — task|issue|proposal|adr|plan (repeatable/comma-separated).",
    )
    .multi()
    .delim()
    .cli(),
    opt(
        "status",
        Val::Status,
        "Statuses — open|claimed|in-progress|blocked|done|deferred|cancelled (repeatable/comma-separated).",
    )
    .multi()
    .delim()
    .cli(),
    opt(
        "pri_min",
        Val::Pri,
        "Inclusive lower severity bound — low|med|high|crit.",
    )
    .cli(),
    opt("pri_max", Val::Pri, "Inclusive upper severity bound.").cli(),
    opt(
        "label",
        Val::Str,
        "`#`-less label — item carries any of these (repeatable/comma-separated).",
    )
    .multi()
    .delim()
    .cli(),
    opt(
        "owner",
        Val::Actor,
        "Owner actor — item's owner is any of these (repeatable/comma-separated).",
    )
    .multi()
    .delim()
    .cli(),
    flag("claimed", "Items with an owner.")
        .conflicts("unclaimed")
        .cli(),
    flag("unclaimed", "Items with no owner.").cli(),
    opt("epic", Val::Str, "Items in this epic.").cli(),
    opt("milestone", Val::Str, "Items in this milestone.").cli(),
    opt("due", Val::Str, "Items due on this date (`YYYY-MM-DD`).").cli(),
    opt("est", Val::Count, "Items with this point estimate.")
        .alias("estimate")
        .cli(),
    opt("cycle", Val::Str, "Items in this cycle.").cli(),
    flag("live_only", "Items not superseded by another (ADRs).")
        .conflicts("superseded")
        .cli(),
    flag("superseded", "Items superseded by another (ADRs).").cli(),
];

/// The ad-hoc spec flags — [`FILTER`] plus the
/// group/sort/limit/format `ViewSpec` fields.
const SPEC_FIELDS: &[Param] = &[
    opt(
        "source",
        Val::Source,
        "Item universe — repo (default) reads this worktree; index unions every registered repo's snapshot (org views).",
    )
    .cli(),
    opt(
        "group_by",
        Val::Axis,
        "Group axis — status|priority|kind|owner|epic|milestone|repo|cycle.",
    )
    .cli(),
    opt("sort", Val::Sort, "Sort keys — `pri,id,-created`; `-` flips a key's default direction.")
        .multi()
        .delim()
        .cli(),
    opt("limit", Val::Count, "Keep the first N items after sorting.").vname("N").cli(),
    opt(
        "format",
        Val::Format,
        "Renderer — list|board|repos|timeline|stats|burnup.",
    )
    .cli(),
];

/// `spec` — the wire-only `ViewSpec` patch (`view`/`next`).
const SPEC_PATCH: &[Param] = &[opt(
    "spec",
    Val::Spec,
    "A partial ViewSpec — each set field replaces the base's value.",
)
.wire()];

/// `id` — required positional, task kind.
const TASK_ID: &[Param] = &[pos("id", Val::Id, "Task id (`T-NNN`).")];

/// `id` — 1..n positional ids.
const IDS: &[Param] = &[pos(
    "id",
    Val::Id,
    "Any item id (`T-NNN`, `I-NNN`, `P-NNN`, `ADR-NNNN`, `PL-NNN`) — live or archived.",
)
.multi()];

const NO_PARAMS: &[&[Param]] = &[];

// ---------------------------------------------------------------- the table

/// The builtin verb surface — declaration order is help/manifest
/// order. `name` is the serve method name; `.` splits the CLI
/// path. `mutates` keeps config verbs read-only (alias targets
/// can't write).
pub static VERBS: &[Verb] = &[
    Verb {
        name: "manifest",
        about: "Capabilities + protocol version — agent self-discovery.",
        params: NO_PARAMS,
        kind_lock: None,
        mutates: false,
        hidden: false,
        cli: Some(crate::h_manifest),
        wire: Some(serve::w_manifest),
    },
    Verb {
        name: "gen-cli-docs",
        about: "Print the CLI reference as markdown (docs pipeline).",
        params: NO_PARAMS,
        kind_lock: None,
        mutates: false,
        hidden: true,
        cli: Some(crate::h_gen_docs),
        wire: None,
    },
    Verb {
        name: "schema",
        about: "The file/state spec, machine-readable JSON Schema.",
        params: NO_PARAMS,
        kind_lock: None,
        mutates: false,
        hidden: false,
        cli: Some(crate::schema::h),
        wire: Some(serve::w_schema),
    },
    Verb {
        name: "init",
        about: "Scaffold `project/`: orphan branch + worktree + .gitignore + seed files + the managed AGENTS.md agent-awareness block. Idempotent — existing worktree and AGENTS.md prose are preserved, only the marked block is (re)written.",
        params: NO_PARAMS,
        kind_lock: None,
        mutates: true,
        hidden: false,
        cli: Some(crate::init::h),
        wire: None,
    },
    Verb {
        name: "check",
        about: "Validate the ledger (schemas, state machine, ids, refs) and the worktree it lives in (detached HEAD, wrong branch, unpushed commits); also validates the managed AGENTS.md block on the repo root. Findings carry a severity: `error` exits 1, `warning` is reported and costs nothing (ADR-0002). `--fix` applies safe repairs + commits, including reattaching a detached worktree.",
        params: &[&[flag(
            "fix",
            "Repair drift in place instead of only reporting.",
        )
        .cli()]],
        kind_lock: None,
        mutates: true,
        hidden: false,
        cli: Some(crate::check::h),
        wire: Some(serve::w_check),
    },
    Verb {
        name: "migrate",
        about: "Convert legacy ledgers to v2: `pm/` → `project/`, or normalize files already on the `project` branch.",
        params: NO_PARAMS,
        kind_lock: None,
        mutates: true,
        hidden: false,
        cli: Some(crate::migrate::h),
        wire: None,
    },
    Verb {
        name: "commit",
        about: "Stage + commit worktree changes (conventional message).",
        params: &[&[opt(
            "message",
            Val::Str,
            "Commit message (default: `chore: update project ledger`).",
        )
        .short('m')]],
        kind_lock: None,
        mutates: true,
        hidden: false,
        cli: Some(crate::commit::h),
        wire: None,
    },
    Verb {
        name: "undo",
        about: "Undo the last ledger commit — resets the `project` branch tip one back (`HEAD~1`). One verb = one commit, so the reset is the inverse mutation, file deletions included; the undone commit is journaled for `redo`. Refuses on a dirty worktree (uncommitted `--no-commit` writes), at the root commit, and when the tip is already published on `origin` (unless `--force`). `--list` shows the redoable stack without mutating.",
        params: &[
            &[flag(
                "force",
                "Undo even when the tip commit is already published on the remote.",
            )],
            &[flag(
                "list",
                "List the redoable commits (newest first) instead of undoing.",
            )],
        ],
        kind_lock: None,
        mutates: true,
        hidden: false,
        cli: Some(crate::undo::h),
        wire: Some(serve::w_undo),
    },
    Verb {
        name: "redo",
        about: "Redo the last undo — pops the undo journal and restores the commit `myco undo` moved away from (`git reset --hard <target>`). Repeatable while entries remain; the stack self-invalidates when the ledger moves (any commit/pull/manual git op since the undo). Refuses on a dirty worktree.",
        params: &[],
        kind_lock: None,
        mutates: true,
        hidden: false,
        cli: Some(crate::redo::h),
        wire: Some(serve::w_redo),
    },
    Verb {
        name: "sync",
        about: "Reconcile the ledger branch with `origin/<branch>` — fetch, fast-forward or rebase local commits onto the remote tip, then heal merge damage: `meta.toml` counters reconcile, duplicate ids renumber remote-wins (the pushed item keeps its id; the local one moves to a fresh id, inbound `blocks:`/`links:`/supersede refs rebind to it, and a `renumbered` handoff entry records the audit). Mutating verbs refuse while behind (`ensure_synced`; `sync.auto_fetch` config fetches first), so this is the gate. A rebase conflict stops mid-flight — resolve + `git rebase --continue` + `myco sync` again, or `--abort` to bail. With `sync.auto_push` enabled, a successful sync also pushes the reconciled tip (best-effort — a push failure warns, never fails).",
        params: &[&[flag(
            "abort",
            "Abort a stopped rebase/merge (`git rebase --abort`) and return to the pre-sync tip.",
        )
        .cli()]],
        kind_lock: None,
        mutates: true,
        hidden: false,
        cli: Some(crate::sync::h),
        wire: None,
    },
    Verb {
        name: "absorb.comments",
        about: "Sweep code comments into the ledger via ripgrep (`rg` must be installed — respects `.gitignore`; `project/`, `docs/`, `target/` are excluded).",
        params: &[&[
            opt("path", Val::Path, "Directory to scan (default: the repo root)."),
            opt(
                "kinds",
                Val::Str,
                "Comment kinds to match — comma-separated or repeatable (default TODO,FIXME,HACK).",
            )
            .multi()
            .delim(),
            opt("to", Val::Str, "Where findings land: `backlog` items or `issues` files (default `backlog`).")
                .default("backlog"),
            flag("dry", "Report findings without creating items or committing."),
        ]],
        kind_lock: None,
        mutates: true,
        hidden: false,
        cli: Some(crate::absorb::h_comments),
        wire: None,
    },
    Verb {
        name: "absorb.jira",
        about: "Import a Jira CSV export. `Done`/`Resolved` rows are skipped unless `--include-done` (created done + archived).",
        params: &[&[
            pos("file", Val::Path, "The export file (`.csv`)."),
            flag("include_done", "Import done/resolved issues too."),
        ]],
        kind_lock: Some(myco_core::ItemKind::Issue),
        mutates: true,
        hidden: false,
        cli: Some(crate::absorb::h_jira),
        wire: None,
    },
    Verb {
        name: "absorb.github",
        about: "Import a GitHub issues dump — a JSON array (`number`, `title`, `state`, `labels`, `assignee`, `body`) or the CSV equivalent. `closed` rows are skipped unless `--include-done` (created done + archived).",
        params: &[&[
            pos("file", Val::Path, "The export file (`.json` or `.csv`)."),
            flag("include_done", "Import closed issues too."),
        ]],
        kind_lock: Some(myco_core::ItemKind::Issue),
        mutates: true,
        hidden: false,
        cli: Some(crate::absorb::h_github),
        wire: None,
    },
    Verb {
        name: "todo.add",
        about: "Add a task — allocs a T-id, appends to a section of TODO.md.",
        params: &[&[
            pos("title", Val::Str, "One-line title."),
            opt("section", Val::Str, "Section to add under (created when absent; default: the first section, or an unsectioned run).")
                .conflicts("parent"),
            opt("parent", Val::Id, "Nest under this task instead of a section — the new item becomes a child (`  - [ ]` under the parent)."),
        ], META],
        kind_lock: Some(myco_core::ItemKind::Task),
        mutates: true,
        hidden: false,
        cli: Some(crate::todo::h_add),
        wire: Some(serve::w_todo_add),
    },
    Verb {
        name: "todo.done",
        about: "Mark done — walks the state machine to `done`, removes the line, and archives the subtree into DONE.md. Done on a parent archives the whole subtree — children die with it. Confirms on a tty (`--yes`/`$MYCO_YES` skips; scripts never prompt).",
        params: &[TASK_ID, &[flag(
            "yes",
            "Skip the confirmation prompt (also `$MYCO_YES`; non-interactive runs never prompt).",
        )
        .cli()]],
        kind_lock: Some(myco_core::ItemKind::Task),
        mutates: true,
        hidden: false,
        cli: Some(crate::todo::h_done),
        wire: Some(serve::w_todo_done),
    },
    Verb {
        name: "todo.cancel",
        about: "Mark cancelled — walks the state machine to `cancelled` (archived like done but the work never happened), removes the line, and archives the subtree into DONE.md with `status:cancelled` preserved. Works on BACKLOG.md items too — no promote needed. Confirms on a tty (`--yes`/`$MYCO_YES` skips; scripts never prompt).",
        params: &[TASK_ID, &[flag(
            "yes",
            "Skip the confirmation prompt (also `$MYCO_YES`; non-interactive runs never prompt).",
        )
        .cli()]],
        kind_lock: Some(myco_core::ItemKind::Task),
        mutates: true,
        hidden: false,
        cli: Some(crate::todo::h_cancel),
        wire: Some(serve::w_todo_cancel),
    },
    Verb {
        name: "todo.defer",
        about: "Park a task — transition to `deferred`, move the line to BACKLOG.md. Defer on a parent moves the whole subtree (children park with it); on a child it detaches alone.",
        params: &[TASK_ID],
        kind_lock: Some(myco_core::ItemKind::Task),
        mutates: true,
        hidden: false,
        cli: Some(crate::todo::h_defer),
        wire: Some(serve::w_todo_defer),
    },
    Verb {
        name: "todo.list",
        about: "List tasks.",
        params: &[LIST, &[flag(
            "archived",
            "(wire compat — ignored; tasks don't archive.)",
        )
        .wire()]],
        kind_lock: Some(myco_core::ItemKind::Task),
        mutates: false,
        hidden: false,
        cli: Some(crate::todo::h_list),
        wire: Some(serve::w_todo_list),
    },
    Verb {
        name: "backlog.add",
        about: "Add a deferred item — allocs a T-id straight into BACKLOG.md.",
        params: &[&[
            pos("title", Val::Str, "One-line title."),
            opt("section", Val::Str, "Section to add under (created when absent; default: the first section, or an unsectioned run)."),
        ], META],
        kind_lock: Some(myco_core::ItemKind::Task),
        mutates: true,
        hidden: false,
        cli: Some(crate::backlog::h_add),
        wire: Some(serve::w_backlog_add),
    },
    Verb {
        name: "backlog.promote",
        about: "Re-activate a parked item — `deferred → open`, move the line to TODO.md.",
        params: &[TASK_ID],
        kind_lock: Some(myco_core::ItemKind::Task),
        mutates: true,
        hidden: false,
        cli: Some(crate::backlog::h_promote),
        wire: Some(serve::w_backlog_promote),
    },
    Verb {
        name: "backlog.list",
        about: "List backlog items.",
        params: &[LIST, &[flag(
            "archived",
            "(wire compat — ignored; tasks don't archive.)",
        )
        .wire()]],
        kind_lock: Some(myco_core::ItemKind::Task),
        mutates: false,
        hidden: false,
        cli: Some(crate::backlog::h_list),
        wire: Some(serve::w_backlog_list),
    },
    Verb {
        name: "done.list",
        about: "List archived tasks — the DONE.md archive `todo done`/`todo cancel`/`item set --status done|cancelled` land in. Read-only; `item set --status open|deferred` reopens one back into the live docs.",
        params: &[LIST],
        kind_lock: Some(myco_core::ItemKind::Task),
        mutates: false,
        hidden: false,
        cli: Some(crate::done::h_list),
        wire: Some(serve::w_done_list),
    },
    Verb {
        name: "done.remove",
        about: "Delete an archived task subtree from DONE.md outright — the ledger forgets it (scaffold junk, mistakes). Reopen unfinished work with `item set --status open` instead. Confirms on a tty (`--yes`/`$MYCO_YES` skips; scripts never prompt).",
        params: &[TASK_ID, &[flag(
            "yes",
            "Skip the confirmation prompt (also `$MYCO_YES`; non-interactive runs never prompt).",
        )
        .cli()]],
        kind_lock: Some(myco_core::ItemKind::Task),
        mutates: true,
        hidden: false,
        cli: Some(crate::done::h_remove),
        wire: Some(serve::w_done_remove),
    },
    Verb {
        name: "issue.new",
        about: "File an issue — allocs an I-id, writes `issues/issue-NNNN-<slug>.md` with frontmatter + template.",
        params: &[&[pos(
            "title",
            Val::Str,
            "One-line title (drives the filename slug).",
        )], BODY, META],
        kind_lock: Some(myco_core::ItemKind::Issue),
        mutates: true,
        hidden: false,
        cli: Some(crate::issue::h_new),
        wire: None,
    },
    Verb {
        name: "issue.list",
        about: "List issues (live files; `--archived` includes `issues/archive/`).",
        params: &[FILE_LIST],
        kind_lock: Some(myco_core::ItemKind::Issue),
        mutates: false,
        hidden: false,
        cli: Some(crate::issue::h_list),
        wire: Some(serve::w_issue_list),
    },
    Verb {
        name: "issue.resolve",
        about: "Close an issue — walks the state machine to `done`; `--resolution` appends text under `## Resolution`.",
        params: &[&[
            pos("id", Val::Id, "Issue id (`I-NNN`)."),
            opt("resolution", Val::Str, "Resolution text appended to the body."),
        ]],
        kind_lock: Some(myco_core::ItemKind::Issue),
        mutates: true,
        hidden: false,
        cli: Some(crate::issue::h_resolve),
        wire: None,
    },
    Verb {
        name: "issue.archive",
        about: "Shelve a done issue — moves the file to `issues/archive/`.",
        params: &[&[pos("id", Val::Id, "Issue id (`I-NNN`).")]],
        kind_lock: Some(myco_core::ItemKind::Issue),
        mutates: true,
        hidden: false,
        cli: Some(crate::issue::h_archive),
        wire: None,
    },
    Verb {
        name: "issue.comment",
        about: "Append a dated note under `## Comments` (created when missing).",
        params: &[&[
            pos("id", Val::Id, "Issue id (`I-NNN`)."),
            pos("text", Val::Str, "Comment text."),
        ]],
        kind_lock: Some(myco_core::ItemKind::Issue),
        mutates: true,
        hidden: false,
        cli: Some(crate::issue::h_comment),
        wire: None,
    },
    Verb {
        name: "proposal.new",
        about: "File a proposal — allocs a P-id, writes the proposal file with frontmatter + template.",
        params: &[&[pos(
            "title",
            Val::Str,
            "One-line title (drives the filename slug).",
        )], BODY, META],
        kind_lock: Some(myco_core::ItemKind::Proposal),
        mutates: true,
        hidden: false,
        cli: Some(crate::proposal::h_new),
        wire: None,
    },
    Verb {
        name: "proposal.list",
        about: "List proposals.",
        params: &[LIST],
        kind_lock: Some(myco_core::ItemKind::Proposal),
        mutates: false,
        hidden: false,
        cli: Some(crate::proposal::h_list),
        wire: Some(serve::w_proposal_list),
    },
    Verb {
        name: "proposal.status",
        about: "Set a proposal's status. Proposal words alias the state machine: `draft` = open, `accepted` = done, `rejected` = deferred — the transition is walked through the machine, so `accepted` passes `open → in-progress → done`.",
        params: &[&[
            pos("id", Val::Id, "Proposal id (`P-NNN`)."),
            pos("status", Val::Str, "`draft` | `accepted` | `rejected` (raw machine statuses like `in-progress` also accepted)."),
        ]],
        kind_lock: Some(myco_core::ItemKind::Proposal),
        mutates: true,
        hidden: false,
        cli: Some(crate::proposal::h_status),
        wire: None,
    },
    Verb {
        name: "plan.new",
        about: "File a plan — allocs a PL-id, writes the plan file with frontmatter + a phase-checklist template.",
        params: &[&[pos(
            "title",
            Val::Str,
            "One-line title (drives the filename slug).",
        )], BODY, META],
        kind_lock: Some(myco_core::ItemKind::Plan),
        mutates: true,
        hidden: false,
        cli: Some(crate::plan::h_new),
        wire: None,
    },
    Verb {
        name: "plan.list",
        about: "List plans.",
        params: &[LIST],
        kind_lock: Some(myco_core::ItemKind::Plan),
        mutates: false,
        hidden: false,
        cli: Some(crate::plan::h_list),
        wire: Some(serve::w_plan_list),
    },
    Verb {
        name: "adr.new",
        about: "File an ADR — allocs an ADR-id, writes the file with frontmatter (status `open` = \"proposed\") + template.",
        params: &[&[pos(
            "title",
            Val::Str,
            "One-line title (drives the filename slug).",
        )], BODY, META],
        kind_lock: Some(myco_core::ItemKind::Adr),
        mutates: true,
        hidden: false,
        cli: Some(crate::adr::h_new),
        wire: None,
    },
    Verb {
        name: "adr.list",
        about: "List ADRs.",
        params: &[LIST],
        kind_lock: Some(myco_core::ItemKind::Adr),
        mutates: false,
        hidden: false,
        cli: Some(crate::adr::h_list),
        wire: Some(serve::w_adr_list),
    },
    Verb {
        name: "adr.supersede",
        about: "Mark an ADR superseded by another — the old file gets `superseded_by:` + status `done`, the replacement gets a reciprocal `supersedes:` key. Both must already exist.",
        params: &[&[
            pos("id", Val::Id, "The ADR being replaced (`ADR-NNNN`)."),
            opt("by", Val::Id, "The ADR replacing it (`ADR-NNNN`).").req(),
        ]],
        kind_lock: Some(myco_core::ItemKind::Adr),
        mutates: true,
        hidden: false,
        cli: Some(crate::adr::h_supersede),
        wire: None,
    },
    Verb {
        name: "item.show",
        about: "Dump items — frontmatter fields, kind-grouped related edges (blocks/blocked_by/supersedes/associated/references/handoff, live-derived), plus the body for file-backed kinds (`--json` = the full Item + location + related + body). 1..n ids: a batch repeats the block in argument order; `--json` emits the per-id `[{id, ok, …}]` array.",
        params: &[IDS],
        kind_lock: None,
        mutates: false,
        hidden: false,
        cli: Some(crate::itemcmd::h_show),
        wire: Some(serve::w_item_show),
    },
    Verb {
        name: "item.set",
        about: "Mutate an item in place. `--status` is the only field routed through the state machine — single steps only, invalid ones error with the valid targets. Status changes on tasks move the line between TODO.md and BACKLOG.md (`done`/`cancelled` archive the subtree into DONE.md — both confirm on a tty, and both reopen via `--status open|deferred`, also confirmed: `--yes`/`$MYCO_YES` skips).",
        params: &[&[
            pos("id", Val::Id, "Any item id."),
            opt("status", Val::Status, "New status — one legal step from the current one."),
            opt("pri", Val::Pri, "Priority: low|med|high|crit.").alias("priority"),
            opt("owner", Val::Actor, "Owner actor id."),
            opt("label", Val::Str, "Add (`+x` or `x`) or remove (`-x`) a label — repeatable.")
                .multi()
                .hyphen(),
            opt("unlabel", Val::Str, "Remove a label — repeatable.").multi(),
            opt("blocks", Val::Str, "Add (`+T-1` or `T-1`) or remove (`-T-1`) a blocks dep — repeatable or comma-separated.")
                .multi()
                .hyphen()
                .delim(),
            opt("unblock", Val::Id, "Remove a blocks dep — repeatable or comma-separated.")
                .multi()
                .delim(),
            opt("links", Val::Str, "Add (`+PL-1` or `PL-1`) or remove (`-PL-1`) a link — repeatable or comma-separated.")
                .multi()
                .hyphen()
                .delim(),
            opt("unlink", Val::Id, "Remove a link — repeatable or comma-separated.")
                .multi()
                .delim(),
            opt("epic", Val::Str, "Epic name — `\"\"` clears the epic."),
            opt("milestone", Val::Str, "Milestone name — `\"\"` clears the milestone."),
            opt("due", Val::Str, "Due date (`YYYY-MM-DD`) — `\"\"` clears the due date."),
            opt("est", Val::Str, "Estimate in points (0–1000) — `\"\"` clears the estimate.").alias("estimate"),
            opt("cycle", Val::Str, "Cycle name (`[A-Za-z0-9_.-]+`) — `\"\"` clears the cycle."),
            opt("session", Val::Str, "Tag the item's claim with a session id — appends a `claimed` handoff entry carrying it (`interrupt --session` scopes teardown to it). The item must land in a work state."),
            flag(
                "steal",
                "Take the item even when another actor owns the claim — records a `taken from` handoff entry.",
            ),
            flag(
                "yes",
                "Skip the archive/reopen confirmation prompts (also `$MYCO_YES`; non-interactive runs never prompt).",
            )
            .cli(),
        ]],
        kind_lock: None,
        mutates: true,
        hidden: false,
        cli: Some(crate::itemcmd::h_set),
        wire: Some(serve::w_item_set),
    },
    Verb {
        name: "item.move",
        about: "Move an item to another repo's ledger — re-creates it in the destination's own id sequence (kind, title, body, pri/owner/labels/epic/milestone carried; `blocks:`/`links:` dropped — they name source-ledger ids — and the drop is recorded as plain text in both provenance handoffs), writes a `moved from`/`moved to` provenance handoff on both sides, then walks the source to `done` (a task's line leaves the ledger; its handoff drops to `handoffs/archive/`). `--to` takes the destination repo root, its `project/` worktree, or an index-universe repo name (`myco index list`). Two commits — one per worktree. `--dry-run` previews the plan (prospective dest id, dropped refs) with no writes; `--finish` completes a move that died after the destination copy + `moved to` pointer landed but before the source close committed — the pointer is the resume handle.",
        params: &[&[
            pos("id", Val::Id, "Any item id."),
            opt(
                "to",
                Val::Path,
                "Destination — a repo root, its `project/` worktree, or a registered repo name. Required unless `--finish`.",
            ),
            opt(
                "note",
                Val::Str,
                "Extra context appended to the source's `moved to` handoff note.",
            ),
            opt("actor", Val::Actor, "Who moved it (default: resolved actor)."),
            flag(
                "dry-run",
                "Validate + report the plan (prospective destination id, dropped refs) — no writes, no commits.",
            ),
            flag(
                "finish",
                "Complete a partial move — finds the `moved to` handoff pointer, verifies the destination copy exists, closes the source, commits both worktrees. No `--to`.",
            ),
            flag(
                "yes",
                "Skip the confirmation prompt (also `$MYCO_YES`; non-interactive runs never prompt).",
            )
            .cli(),
        ]],
        kind_lock: None,
        mutates: true,
        hidden: false,
        cli: Some(crate::movecmd::h_move),
        wire: Some(serve::w_item_move),
    },
    Verb {
        name: "item.reorder",
        about: "Sibling reorder inside TODO.md/BACKLOG.md — the TUI's `Alt+↑/↓` as a verb: `--delta -1` swaps with the previous sibling, `+1` with the next (larger deltas walk that many steps, stopping at the list edge). `Ledger::move_within` semantics exactly — a move never crosses a `##` section boundary, and a nested child swaps among its own siblings only. One commit per call (`chore: reorder <id>`); tasks only — file-backed items have no siblings.",
        params: &[&[
            pos("id", Val::Id, "Task id (`T-NNN`) — a live TODO.md/BACKLOG.md line, children included."),
            opt(
                "delta",
                Val::Str,
                "Sibling steps — `-1` up, `+1` down; larger values walk that far, stopping at the edge.",
            )
            .req()
            .hyphen()
            .vname("N"),
        ]],
        kind_lock: Some(myco_core::ItemKind::Task),
        mutates: true,
        hidden: false,
        cli: Some(crate::itemcmd::h_reorder),
        wire: Some(serve::w_item_reorder),
    },
    Verb {
        name: "view",
        about: "Declarative views — named presets (`[views.<name>]` in config + built-ins `board|open|epic|milestone|next|roadmap|stats|burnup`, plus index-sourced `org-board|org-status` for cross-repo) or an ad-hoc spec from flags (`--source index` unions the index universe — lasius clones ∪ registered repos). `myco view board` is the kanban; `format = \"repos\"` emits one row per repo; `format = \"timeline\"|\"stats\"|\"burnup\"` are the analytics emits (roadmap lanes, count tallies, done-over-time — `done:` archive stamps drive burnup); bare `myco view` lists views (or renders `default_view` when config names one).",
        params: &[&[
            pos("name", Val::Str, "View name — builtin preset or config view.").optional(),
            flag("list", "List defined views and exit.").cli(),
            flag("md", "Emit a markdown table (board/repos formats).").cli(),
            flag("all", "Show every row — no cap.").cli(),
        ], FILTER, SPEC_FIELDS, SPEC_PATCH],
        kind_lock: None,
        mutates: false,
        hidden: false,
        cli: Some(crate::view::h),
        wire: Some(serve::w_view),
    },
    Verb {
        name: "next",
        about: "The single highest-priority open item — \"what should I work on\" (a `view` eval with limit 1: open tasks+issues, `pri,id`; the filter flags merge over that default). `--claim` mutates it to `claimed` + an owner in one commit.",
        params: &[FILTER, &[
            opt("sort", Val::Sort, "Sort keys — `pri,id,-created`; `-` flips a key's default direction. Default `pri,id`.")
                .multi()
                .delim()
                .cli(),
            flag("claim", "Claim the emitted item — status `claimed`, owner set to the `--owner` actor (else the resolved actor); `--owner` names the claimer here and doesn't filter. Mutating: one commit.").cli(),
            flag("steal", "With `--claim`: take the item even when another actor owns the claim — records a `taken from` handoff entry.").cli(),
            opt("session", Val::Str, "With `--claim`: tag the claim with a session id — appends a `claimed` handoff entry carrying it (`interrupt --session` scopes teardown to it).").cli(),
        ], SPEC_PATCH],
        kind_lock: None,
        mutates: true,
        hidden: false,
        cli: Some(crate::next::h),
        wire: Some(serve::w_next),
    },
    Verb {
        name: "status",
        about: "Cheap digest of the work ledger. `--prompt` = token-sized.",
        params: &[&[flag(
            "prompt",
            "Render for an agent prompt (minimal tokens).",
        )]],
        kind_lock: None,
        mutates: false,
        hidden: false,
        cli: Some(crate::status::h),
        wire: Some(serve::w_status),
    },
    Verb {
        name: "report",
        about: "Aging + velocity + blocked — how the ledger is trending.",
        params: NO_PARAMS,
        kind_lock: None,
        mutates: false,
        hidden: false,
        cli: Some(crate::report::h),
        wire: Some(serve::w_report),
    },
    Verb {
        name: "index.add",
        about: "Register a repo — `<path>` is a repo root (or its `project/` worktree). Needed outside a lasius workspace (clones under `lasius.toml`'s `repos_dir` join the index universe automatically); inside one it still records an explicit entry. Ledger-less repos register as `ledger = \"none\"` with no snapshot; `--org` marks the org/global ledger (equivalent to `[index] ledger = \"org\"]` in the repo's `myco.toml`).",
        params: &[
            &[
                pos(
                    "path",
                    Val::Path,
                    "Repo root (or project worktree) to register.",
                ),
                flag(
                    "org",
                    "Designate this repo as the org/global ledger — the fallback target verbs in ledger-less universe repos resolve to (at most one; clears the mark elsewhere). `[index] ledger = \"org\"]` in the repo's myco.toml is the durable equivalent.",
                ),
            ],
        ],
        kind_lock: None,
        mutates: true,
        hidden: false,
        cli: Some(crate::indexcmd::h_add),
        wire: None,
    },
    Verb {
        name: "index.remove",
        about: "Unregister a repo and drop its snapshot.",
        params: &[&[pos(
            "path",
            Val::Path,
            "Registered repo root (or project worktree).",
        )]],
        kind_lock: None,
        mutates: true,
        hidden: false,
        cli: Some(crate::indexcmd::h_remove),
        wire: None,
    },
    Verb {
        name: "index.list",
        about: "List registered repos with snapshot freshness.",
        params: NO_PARAMS,
        kind_lock: None,
        mutates: false,
        hidden: false,
        cli: Some(crate::indexcmd::h_list),
        wire: None,
    },
    Verb {
        name: "index.rebuild",
        about: "Re-scan a registered repo (all of them without `<path>`).",
        params: &[&[pos(
            "path",
            Val::Path,
            "Registered repo root (or project worktree).",
        )
        .optional()]],
        kind_lock: None,
        mutates: true,
        hidden: false,
        cli: Some(crate::indexcmd::h_rebuild),
        wire: None,
    },
    Verb {
        name: "org.init",
        about: "Re-run `init` on every registered repo — refreshes the worktree scaffold and root files (.gitignore, the AGENTS.md agent-awareness block). The rollout path for new scaffolding.",
        params: NO_PARAMS,
        kind_lock: None,
        mutates: true,
        hidden: false,
        cli: Some(crate::org::h_init),
        wire: None,
    },
    Verb {
        name: "handoff",
        about: "Attach resumable state at session end — appends a dated entry to `handoffs/<ID>.md` (any item kind, live or archived; append-only).",
        params: &[&[
            pos("id", Val::Id, "Any item id (`T-NNN`, `I-NNN`, `P-NNN`, `ADR-NNNN`, `PL-NNN`)."),
            opt("note", Val::Str, "Handoff note — done/next/open. `-` reads stdin.")
                .req()
                .vname("TEXT|-"),
            opt("actor", Val::Actor, "Who wrote it (`devin`, `human:luis`, …; default: the resolved actor)."),
            opt("session", Val::Str, "Session provenance — a sepia session id, `cli:<pid>`, any id string."),
        ]],
        kind_lock: None,
        mutates: true,
        hidden: false,
        cli: Some(crate::continuity::h_handoff),
        wire: Some(serve::w_handoff),
    },
    Verb {
        name: "interrupt",
        about: "A session ended with work still claimed/in-progress/blocked — revert the item to `open` + `interrupted` handoff, one commit. Without an id, `--session <id>` is the teardown: every item claimed under that session reverts in one commit. Session-lifecycle hooks (sepia, CLI session-end) call this; myco never detects sessions itself.",
        params: &[INTERRUPT_PARAMS],
        kind_lock: None,
        mutates: true,
        hidden: false,
        cli: Some(crate::continuity::h_interrupt),
        wire: Some(serve::w_interrupt),
    },
    Verb {
        name: "reap",
        about: "Bulk stale-claim reaper — every live `claimed`/`in-progress` item whose last activity (latest handoff entry, else `created`) is older than `--idle-days` reverts to `open` + an `interrupted` handoff, ONE commit for the whole sweep. `--session <id>` scopes to items whose claim tag names that session; `--dry-run` lists the sweep with zero writes. Scheduler-facing — peregrin campaigns call it periodically; the no-hook safety net behind `report`'s stale-claims finding.",
        params: &[&[
            opt(
                "idle-days",
                Val::Count,
                "Idle threshold in days — last activity older than this reaps. Default: the report's stale-claim threshold (3).",
            )
            .vname("N"),
            opt(
                "session",
                Val::Str,
                "Scope to items whose claim tag names this session (`interrupt --session`'s rule).",
            ),
            opt("actor", Val::Actor, "Who reaped it (default: the resolved actor)."),
            flag(
                "dry-run",
                "List what would be reaped — no writes, no commit.",
            ),
        ]],
        kind_lock: None,
        mutates: true,
        hidden: false,
        cli: Some(crate::reap::h_reap),
        wire: Some(serve::w_reap),
    },
    Verb {
        name: "resume",
        about: "Minimal context block to restart work on an item — the item, `blocks:` dep titles, latest handoffs, open checklist. The single-item `status --prompt`. 1..n ids batch in one call.",
        params: &[IDS, &[opt(
            "last",
            Val::Count,
            "Only the latest N handoff entries (default 10).",
        )]],
        kind_lock: None,
        mutates: false,
        hidden: false,
        cli: Some(crate::continuity::h_resume),
        wire: Some(serve::w_resume),
    },
    #[cfg(feature = "tui")]
    Verb {
        name: "ui",
        about: "Interactive board TUI (ratatui) — TODO/BACKLOG tabs, item preview, writes through the same ops the verbs use; auto-reloads when the ledger changes on disk. `--view` with a `source = \"index\"` spec (org-*) opens the cross-repo index board: `repo:ID` rows, verbs write the owning repo.",
        params: &[&[opt(
            "view",
            Val::Str,
            "Open with a named view's filter narrowing every board; index-sourced views load the cross-repo universe.",
        )
        .vname("NAME")]],
        kind_lock: None,
        mutates: true,
        hidden: false,
        cli: Some(crate::h_ui),
        wire: None,
    },
    Verb {
        name: "serve",
        about: "Sequential request/response verb surface on stdio — JSON-RPC 2.0, one line in / one line out. For embedding (peregrin spawns one per repo); stdin IS the wire, so no served method reads content from it.",
        params: NO_PARAMS,
        kind_lock: None,
        mutates: true,
        hidden: false,
        cli: Some(crate::serve::h_serve),
        wire: None,
    },
    Verb {
        name: "batch",
        about: "One-shot `serve` — NDJSON verb requests from stdin or `--file`, one JSON-RPC response line per request, dispatched through the same served surface. Per-line errors never fail the run; mutations keep per-request commits (`--no-commit`/`$MYCO_NO_COMMIT` apply). stdin IS the wire here too — requests that would read it are rejected.",
        params: &[&[opt(
            "file",
            Val::Path,
            "Read NDJSON requests from this file instead of stdin.",
        )
        .vname("PATH")]],
        kind_lock: None,
        mutates: true,
        hidden: false,
        cli: Some(crate::batch::h),
        wire: None,
    },
    Verb {
        name: "initialize",
        about: "The serve handshake — protocol caps + the method table (registry ∪ this repo's config verbs).",
        params: NO_PARAMS,
        kind_lock: None,
        mutates: false,
        hidden: true,
        cli: None,
        wire: Some(serve::w_initialize),
    },
];

// ---------------------------------------------------------------- lookups

/// One verb by canonical name (`"todo.add"`, `"status"`).
pub fn find(name: &str) -> Option<&'static Verb> {
    VERBS.iter().find(|v| v.name == name)
}

/// Serve/batch method names — every verb with a wire handler.
pub fn served_names() -> Vec<&'static str> {
    VERBS
        .iter()
        .filter(|v| v.wire.is_some())
        .map(|v| v.name)
        .collect()
}

/// `manifest().verbs` — the CLI surface as space-joined names
/// (`todo add`); hidden verbs stay out.
pub fn manifest_verbs() -> Vec<String> {
    VERBS
        .iter()
        .filter(|v| v.cli.is_some() && !v.hidden)
        .map(|v| v.name.replace('.', " "))
        .collect()
}

/// `manifest().implemented` — the verbs plus the hidden pipeline
/// verbs that exist (currently `gen-cli-docs`).
pub fn manifest_implemented() -> Vec<String> {
    let mut out = manifest_verbs();
    out.push("gen-cli-docs".into());
    out
}

/// The builtin table as `myco_ops::verb` needs it — name +
/// mutability + CLI presence for collision/alias checks.
pub fn builtin_refs() -> &'static [BuiltinVerb] {
    static REFS: OnceLock<Vec<BuiltinVerb>> = OnceLock::new();
    REFS.get_or_init(|| {
        VERBS
            .iter()
            .map(|v| BuiltinVerb {
                name: v.name,
                mutates: v.mutates,
                cli: v.cli.is_some(),
            })
            .collect()
    })
}

// ---------------------------------------------------------------- clap

/// `Param` → a clap `Arg` — validator per `Val`, multi/delimited/
/// hyphen/conflicts honored, `_` → `-` on the long name.
fn param_arg(p: &Param) -> Arg {
    // `--pri-min` for `pri_min` — the set is tiny and built once,
    // so leaking the few `String`s beats a `Str`-typed field
    let long: &'static str = if p.name.contains('_') {
        Box::leak(p.name.replace('_', "-").into_boxed_str())
    } else {
        p.name
    };
    let val = p.val;
    let mut a = Arg::new(p.name).help(p.help);
    if p.kind != PKind::Flag {
        a = a.value_parser(move |s: &str| validate(val, s).map(|()| s.to_string()));
    }
    match p.kind {
        PKind::Pos => {
            a = a.required(p.required);
            if p.multi {
                a = a.num_args(1..);
            }
        }
        PKind::Flag => {
            a = a.long(long).action(ArgAction::SetTrue);
        }
        PKind::Opt => {
            a = a.long(long).action(if p.multi {
                ArgAction::Append
            } else {
                ArgAction::Set
            });
        }
    }
    if p.delimited {
        a = a.value_delimiter(',');
    }
    if p.hyphen {
        a = a.allow_hyphen_values(true);
    }
    if p.required && p.kind == PKind::Opt {
        a = a.required(true);
    }
    if let Some(d) = p.default {
        a = a.default_value(d);
    }
    if let Some(c) = p.short {
        a = a.short(c);
    }
    if let Some(al) = p.alias {
        a = a.visible_alias(al);
    }
    if let Some(c) = p.conflicts {
        a = a.conflicts_with(c);
    }
    if let Some(v) = p.vname {
        a = a.value_name(v);
    }
    a
}

/// One verb → its clap `Command` (leaf name after `.`).
fn verb_command(v: &Verb) -> Command {
    let leaf = v.name.rsplit('.').next().unwrap_or(v.name);
    let mut c = Command::new(leaf).about(v.about).hide(v.hidden);
    for p in v.params() {
        if p.on == On::Wire {
            continue;
        }
        c = c.arg(param_arg(p));
    }
    c
}

/// The four global flags — derive-era behavior kept exactly:
/// `--json`/`--no-commit`/`--dir`/`--root` work on every verb.
fn global_args() -> Vec<Arg> {
    vec![
        Arg::new("json")
            .long("json")
            .help("Emit JSON (accepted everywhere; default output for machines).")
            .global(true)
            .action(ArgAction::SetTrue),
        Arg::new("no_commit")
            .long("no-commit")
            .help("Skip the per-verb commit (also suppressed by $MYCO_NO_COMMIT).")
            .global(true)
            .action(ArgAction::SetTrue),
        Arg::new("dir")
            .long("dir")
            .help("Project worktree to act on, overriding discovery.")
            .value_name("PATH")
            .global(true),
        Arg::new("root")
            .long("root")
            .help("Repo root to act on, overriding discovery (the worktree is `<root>/project`).")
            .value_name("PATH")
            .global(true),
    ]
}

/// The full `myco` command — registry verbs under their groups,
/// global flags on top, and `allow_external_subcommands` so a
/// `[verbs.<name>]` config verb parses as `myco <name> [args…]`.
pub fn command() -> Command {
    let mut cmd = Command::new("myco")
        .version(env!("CARGO_PKG_VERSION"))
        .about("myco — agent-first project management for a repo's `project/` worktree.")
        .allow_external_subcommands(true)
        .subcommand_required(true)
        .args(global_args());
    for g in GROUPS {
        let mut gc = Command::new(g.name)
            .about(g.about)
            .subcommand_required(true);
        for v in VERBS.iter().filter(|v| {
            v.cli.is_some()
                && v.name.starts_with(g.name)
                && v.name.as_bytes().get(g.name.len()) == Some(&b'.')
        }) {
            gc = gc.subcommand(verb_command(v));
        }
        cmd = cmd.subcommand(gc);
    }
    for v in VERBS
        .iter()
        .filter(|v| v.cli.is_some() && !v.name.contains('.'))
    {
        cmd = cmd.subcommand(verb_command(v));
    }
    cmd
}

// ---------------------------------------------------------------- dispatch

/// The top-level dispatch — builtin verb (leaf or group),
/// else a `[verbs.<name>]` config-declared verb.
pub fn dispatch(ctx: &Ctx, m: &ArgMatches) -> Result<i32, String> {
    let Some((name, sub)) = m.subcommand() else {
        return Err("a subcommand is required — `myco help` lists verbs".into());
    };
    if let Some(v) = find(name) {
        return run_builtin(ctx, v, sub);
    }
    if GROUPS.iter().any(|g| g.name == name) {
        let Some((leaf, lm)) = sub.subcommand() else {
            return Err(format!("myco {name}: a subcommand is required"));
        };
        let dotted = format!("{name}.{leaf}");
        let v =
            find(&dotted).ok_or_else(|| format!("myco {name}: unknown subcommand \"{leaf}\""))?;
        return run_builtin(ctx, v, lm);
    }
    // external subcommand — a config-declared verb (or a typo)
    let tokens: Vec<String> = sub
        .get_many::<OsString>("")
        .map(|vs| vs.map(|s| s.to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    crate::cfgverb::run(ctx, name, &tokens)
}

/// Builtin leaf — warn when a config verb shadowed it (the
/// builtin wins; the warning mirrors view-shadowing).
fn run_builtin(ctx: &Ctx, v: &Verb, sub: &ArgMatches) -> Result<i32, String> {
    if !v.name.contains('.') {
        crate::cfgverb::warn_if_shadowed(ctx, v.name);
    }
    let cli = v
        .cli
        .ok_or_else(|| format!("verb \"{}\" has no CLI handler", v.name))?;
    cli(ctx, &Args::from_matches(v, sub))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn every_verb_is_reachable_and_consistent() {
        for v in VERBS {
            assert!(v.cli.is_some() || v.wire.is_some(), "{}: dead verb", v.name);
            assert!(!v.name.is_empty());
            // a dotted name is a group leaf — the group exists
            if let Some((g, _)) = v.name.split_once('.') {
                assert!(
                    GROUPS.iter().any(|x| x.name == g),
                    "{}: group {g} missing",
                    v.name
                );
            }
            // param names are unique per verb
            let names: Vec<&str> = v.params().map(|p| p.name).collect();
            let mut seen = std::collections::BTreeSet::new();
            for n in &names {
                assert!(seen.insert(n), "{}: param {n} twice", v.name);
            }
        }
        // every group has leaves
        for g in GROUPS {
            assert!(
                VERBS.iter().any(|v| v.name.starts_with(g.name)),
                "empty group {}",
                g.name
            );
        }
    }

    #[test]
    fn command_tree_builds_and_parses() {
        let cmd = command();
        let m = cmd
            .try_get_matches_from(["myco", "todo", "add", "fix it", "--pri", "high", "--json"])
            .unwrap();
        assert!(m.get_flag("json"));
        let (name, sub) = m.subcommand().unwrap();
        assert_eq!(name, "todo");
        let (leaf, lm) = sub.subcommand().unwrap();
        assert_eq!(leaf, "add");
        let v = find("todo.add").unwrap();
        let a = Args::from_matches(v, lm);
        assert_eq!(a.text("title").unwrap(), "fix it");
        assert_eq!(
            a.parsed::<myco_core::Priority>("pri").unwrap(),
            Some(myco_core::Priority::High)
        );
    }

    #[test]
    fn from_json_decodes_wire_params() {
        let v = find("todo.add").unwrap();
        let a = Args::from_json(
            v,
            &serde_json::json!({"title": "x", "pri": "crit", "label": ["a", "b"]}),
        )
        .unwrap();
        assert_eq!(a.text("title").unwrap(), "x");
        assert_eq!(a.many("label"), ["a", "b"]);
        // unknown key → error
        assert!(Args::from_json(v, &serde_json::json!({"title":"x","nope":1})).is_err());
        // missing required → error
        assert!(Args::from_json(v, &serde_json::json!({})).is_err());
        // bad value → error
        assert!(Args::from_json(v, &serde_json::json!({"title":"x","pri":"zzz"})).is_err());
    }

    #[test]
    fn from_argv_bakes_alias_tokens() {
        let v = find("view").unwrap();
        let mut ctx = Ctx::default();
        let a = Args::from_argv(
            v,
            &[
                "board".into(),
                "--status".into(),
                "open".into(),
                "--json".into(),
            ],
            &mut ctx,
            false,
        )
        .unwrap();
        assert_eq!(a.opt("name"), Some("board"));
        assert_eq!(a.many("status"), ["open"]);
        assert!(ctx.json);
    }

    #[test]
    fn manifest_and_serve_cover_the_table() {
        let verbs = manifest_verbs();
        assert!(verbs.contains(&"item show".to_string()));
        assert!(verbs.contains(&"todo add".to_string()));
        let served = served_names();
        for m in ["initialize", "status", "todo.add", "handoff"] {
            assert!(served.contains(&m), "{m}");
        }
        assert!(!served.contains(&"init"));
    }
}
