//! myco-core — pure domain for the PM ledger: item models, ID
//! registry, state machine, deps. No I/O, no ports.
//!
//! The numbered-slug domain (`T-012`/`ADR-0004` ids, the owned
//! `open → claimed → in-progress → blocked → done` state machine,
//! actor IDs, document models, `meta.toml` counters) is file-spec
//! agnostic — `myco-store-md` owns the markdown grammar.

pub mod actor;
pub mod analytics;
pub mod date;
pub mod doc;
pub mod handoff;
pub mod id;
pub mod item;
pub mod kind;
pub mod ledger;
pub mod meta;
pub mod priority;
pub mod refs;
pub mod status;
pub mod view;
pub mod walk;

pub use actor::{is_valid_actor, Actor, ParseActorError};
pub use doc::{canonical_body, BacklogDoc, DoneDoc, ItemFile, Section, TodoDoc};
pub use handoff::{is_valid_session, HandoffEntry};
pub use id::{require_kind, ItemId, ItemKind, ParseItemIdError, ParseItemKindError};
pub use item::{
    is_valid_group_name, is_valid_label, tree_view, Item, ItemMeta, NewItemMeta, MAX_ESTIMATE,
};
pub use kind::{KindSpec, KindTable, Storage};
pub use ledger::{Ledger, TaskLoc};
pub use meta::{NextCounters, ProjectMeta, Registry};
pub use priority::{ParsePriorityError, Priority};
pub use refs::{Edge, EdgeKind};
pub use status::{InvalidTransition, ParseStatusError, Status};
pub use view::{
    eval, eval_scoped, GroupAxis, ParseGroupAxisError, ParseSortKeyError, ParseViewFormatError,
    ParseViewSourceError, Scoped, SortField, SortKey, ViewFilter, ViewFormat, ViewGroup,
    ViewSource, ViewSpec, UNGROUPED,
};
pub use walk::{status_path, targets, walk_descendants_to, walk_to};

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// A revision is path-safe — rejects path-injection payloads before
/// the value reaches worktree paths. Deliberately not hex-only: the
/// shape comes from the VCS, not the domain.
pub fn is_valid_rev(rev: &str) -> bool {
    (7..=64).contains(&rev.len())
        && !rev.contains("..")
        && !rev.contains('/')
        && rev
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// Bounded dedup set — `insert` returning false means "already
/// handled". Oldest ids are evicted past `cap`; memory-only.
pub struct Seen {
    ids: std::collections::HashSet<String>,
    order: VecDeque<String>,
    cap: usize,
}

impl Seen {
    pub fn new(cap: usize) -> Self {
        Seen {
            ids: Default::default(),
            order: VecDeque::new(),
            cap,
        }
    }

    /// true = fresh, false = replay/duplicate.
    pub fn insert(&mut self, id: String) -> bool {
        if !self.ids.insert(id.clone()) {
            return false;
        }
        self.order.push_back(id);
        while self.order.len() > self.cap {
            if let Some(old) = self.order.pop_front() {
                self.ids.remove(&old);
            }
        }
        true
    }
}

impl Default for Seen {
    fn default() -> Self {
        Self::new(10_000)
    }
}

/// A `[verbs.<name>]` config-declared verb (P-007). Both kinds are
/// read-only by construction: an alias bakes argv for a read-only
/// built-in; a view-emit binds a [`ViewSpec`] to an emit mode.
/// Mutations stay builtin — the state machine, id alloc, locking
/// and commit semantics aren't config data.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ConfigVerb {
    /// `target = "<builtin>"` + baked `args` — `myco <name>` runs
    /// the builtin with `args` then any extra tokens appended.
    Alias(AliasVerb),
    /// A flattened [`ViewSpec`] + `emit` — `myco <name>` evaluates
    /// the spec and emits in the declared shape.
    ViewEmit(ViewEmitVerb),
}

/// The `alias` config verb — name → builtin + baked args.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AliasVerb {
    /// The built-in verb it invokes (`view`, `status`, `todo.list`, …).
    pub target: String,
    /// Baked argv tokens — positionals and `--flag`/`--opt value`
    /// pairs for the target verb (global `--json`/`--dir`/`--root`/
    /// `--no-commit` also understood). Extra CLI tokens append.
    #[serde(default)]
    pub args: Vec<String>,
}

/// The `view-emit` config verb — a [`ViewSpec`] (every field
/// inline, like `[views.<name>]`) + an emit mode.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ViewEmitVerb {
    /// The view's spec — same fields as `[views.<name>]`.
    #[serde(flatten)]
    pub spec: ViewSpec,
    /// The output shape.
    pub emit: EmitMode,
}

/// How a `view-emit` verb renders its evaluated spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EmitMode {
    /// The rendered table/board text.
    Table,
    /// The full `{view, spec, groups, interrupted}` payload.
    Json,
    /// The matching item ids.
    Ids,
    /// The single top item — `{"item": …}` like `next`.
    First,
}

/// A repo's ledger relationship in the org index — the `ledger`
/// key in `repos.toml` (legacy registry) or `[index] ledger` in
/// `myco.toml` (P-026). Absent → [`LedgerMode::Local`], the
/// conventional reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LedgerMode {
    /// The repo's own ledger — `worktree`, else `<path>/project`.
    #[default]
    Local,
    /// The org/global ledger — the one fallback target
    /// ledger-less repos resolve to. At most one repo should
    /// carry it.
    Org,
    /// Explicitly ledger-less — an org member with no `project/`
    /// worktree; verbs run inside it fall back to the `org`
    /// entry's ledger, and index views contribute no items.
    None,
}

impl LedgerMode {
    /// `local` — the serde-skipped default.
    pub fn is_local(&self) -> bool {
        matches!(self, LedgerMode::Local)
    }
}

/// The `[index]` config table — the repo's designation in the org
/// index universe (P-026).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IndexConfig {
    /// `local` | `org` | `none` — the same vocabulary as the
    /// legacy `repos.toml` `ledger` key, which is still honored
    /// when `myco.toml` is silent.
    pub ledger: Option<LedgerMode>,
}

/// The `myco.toml`-shaped config value the config loader
/// (`myco_ops::config`) produces. Fields grow per phase; unknown keys are rejected so a typo
/// fails loud instead of silently doing nothing.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Path to the project worktree, relative to the repo root.
    pub project_dir: Option<String>,
    /// The named view bare `myco view` resolves when no name is
    /// given — a repo pins its landing view, a machine `~/myco.toml`
    /// pins a global one (scalar: repo layer wins).
    pub default_view: Option<String>,
    /// Named declarative views — merged across config layers
    /// (P-006): repo `myco.toml` presets are shared, user-config
    /// presets personal; the *user* layer wins a name collision.
    #[serde(default)]
    pub views: std::collections::BTreeMap<String, ViewSpec>,
    /// Config-declared verbs (P-007) — merged like `views`, but
    /// the *repo* layer wins collisions (repo verbs are shared
    /// surface, personal verbs don't override them). Builtins win
    /// over both at resolution.
    #[serde(default)]
    pub verbs: std::collections::BTreeMap<String, ConfigVerb>,
    /// `[[kinds]]` — user item kinds layered over the prelude
    /// (P-008). A prelude name *shadows* that spec (prefix stays
    /// locked); new names extend the table. Merged like verbs:
    /// the repo layer wins a name collision — a repo's kind
    /// vocabulary is shared surface.
    #[serde(default)]
    pub kinds: Vec<KindSpec>,
    /// `[sync]` — ledger-branch sync policy. `None` = defaults.
    /// Repo layer wins the table like the scalars.
    pub sync: Option<SyncConfig>,
    /// `[index]` — org-index designation (`ledger = "org"|"none"`).
    /// The index universe reads the repo-layer `myco.toml`
    /// directly, so a user-layer table can't leak a global
    /// designation into every repo.
    pub index: Option<IndexConfig>,
    /// `[[cycles]]` — named iteration windows (sprints, releases,
    /// quarters). Scheduling is config, not ledger memory: item
    /// lines just carry `cycle:<name>` and `check` warns when a
    /// name isn't declared here (P-012). Rollover is manual —
    /// nothing auto-advances; declare the next window when you
    /// start it. Merged like `kinds`: repo entries shadow user
    /// entries by name.
    #[serde(default)]
    pub cycles: Vec<CycleConfig>,
}

/// A `[[cycles]]` entry — one named window between two calendar
/// dates. Dates are `YYYY-MM-DD` — no times, no timezones; the
/// iteration boundary is a day. `end` is the last day of the
/// window (inclusive).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CycleConfig {
    /// The cycle's name — the value `cycle:<name>` tokens carry.
    /// Same charset as the other group names
    /// (`[A-Za-z0-9_.-]`, non-empty) so it round-trips through
    /// the line grammar.
    pub name: String,
    /// First day of the window.
    #[serde(with = "crate::date::req")]
    pub start: time::Date,
    /// Last day of the window (inclusive) — must be after
    /// `start`.
    #[serde(with = "crate::date::req")]
    pub end: time::Date,
}

/// The cycle-name charset — identical to the line grammar's
/// group-name rule so any declared name parses back cleanly.
fn valid_cycle_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

/// The `[sync]` config table — how `myco sync` and the mutating
/// verbs' divergence gate treat the ledger branch's remote.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SyncConfig {
    /// Fetch `origin/<branch>` before the behind-check every
    /// mutating verb runs — a stale remote-tracking ref can only
    /// *under*-report divergence. `false` opts out (offline-first
    /// repos); fetch failure never blocks a mutation either way.
    /// Default `true`.
    #[serde(default = "default_auto_fetch")]
    pub auto_fetch: bool,
    /// `git push origin <branch>` after every committed mutation —
    /// default `true`: the ledger branch publishes itself so clones
    /// stay convergent. `false` opts out (offline-first repos,
    /// deliberate-publish workflows). A failed push never fails
    /// the mutation — the commit already landed locally; the
    /// warning rides stderr and `myco check` keeps reporting
    /// unpushed commits.
    #[serde(default = "default_auto_push")]
    pub auto_push: bool,
}

fn default_auto_fetch() -> bool {
    true
}

fn default_auto_push() -> bool {
    true
}

impl Config {
    /// The resolved kind table — prelude + this config's
    /// `[[kinds]]`. Missing `kinds` = the prelude alone, so
    /// zero-config is today's five kinds.
    pub fn kind_table(&self) -> Result<KindTable, String> {
        KindTable::new(self.kinds.clone())
    }

    /// The validated `[[cycles]]` table — unique names, the
    /// group-name charset, `end > start`. Config errors are
    /// hard (config is code, not data), unlike the `check`
    /// warnings an undeclared `cycle:` token earns.
    pub fn validated_cycles(&self) -> Result<Vec<CycleConfig>, String> {
        let mut out = Vec::with_capacity(self.cycles.len());
        for c in &self.cycles {
            if !valid_cycle_name(&c.name) {
                return Err(format!(
                    "cycle {:?}: name must match [A-Za-z0-9_.-]+",
                    c.name
                ));
            }
            if c.end <= c.start {
                return Err(format!(
                    "cycle {:?}: end {} must be after start {}",
                    c.name,
                    crate::date::render(c.end),
                    crate::date::render(c.start)
                ));
            }
            if out.iter().any(|e: &CycleConfig| e.name == c.name) {
                return Err(format!("cycle {:?}: duplicate name", c.name));
            }
            out.push(c.clone());
        }
        Ok(out)
    }

    /// The cycle covering `d`, if any — the window
    /// `[start, end]` inclusive on both ends.
    pub fn cycle_on(&self, d: time::Date) -> Option<&CycleConfig> {
        self.cycles.iter().find(|c| c.start <= d && d <= c.end)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn cycles_validate_names_windows_and_duplicates() {
        let d = |s: &str| date::parse(s).unwrap();
        let cfg = |name: &str, s: &str, e: &str| CycleConfig {
            name: name.into(),
            start: d(s),
            end: d(e),
        };
        let mut c = Config::default();
        assert_eq!(c.validated_cycles().unwrap(), Vec::new());
        c.cycles = vec![cfg("sprint-12", "2026-01-05", "2026-01-18")];
        assert_eq!(c.validated_cycles().unwrap().len(), 1);
        // bad charset, inverted window, duplicate name — all hard errors
        for bad in [
            vec![cfg("bad name!", "2026-01-05", "2026-01-18")],
            vec![cfg("s", "2026-01-18", "2026-01-05")],
            vec![cfg("s", "2026-01-05", "2026-01-05")],
            vec![
                cfg("s", "2026-01-05", "2026-01-18"),
                cfg("s", "2026-02-01", "2026-02-14"),
            ],
        ] {
            c.cycles = bad;
            assert!(c.validated_cycles().is_err(), "{:?}", c.cycles);
        }
        // cycle_on finds the covering window
        c.cycles = vec![cfg("s", "2026-01-05", "2026-01-18")];
        assert_eq!(
            c.cycle_on(d("2026-01-10")).map(|x| x.name.as_str()),
            Some("s")
        );
        assert!(c.cycle_on(d("2026-01-19")).is_none());
    }

    #[test]
    fn rev_validation() {
        assert!(is_valid_rev("0cf91c6"));
        assert!(is_valid_rev("feature/x".replace('/', "-").as_str()));
        assert!(!is_valid_rev("../etc"));
        assert!(!is_valid_rev("a/b"));
        assert!(!is_valid_rev("short"));
    }

    #[test]
    fn seen_dedups_and_evicts() {
        let mut s = Seen::new(2);
        assert!(s.insert("a".into()));
        assert!(!s.insert("a".into()));
        assert!(s.insert("b".into()));
        assert!(s.insert("c".into()));
        // "a" was evicted once cap was exceeded
        assert!(s.insert("a".into()));
        assert!(Seen::default().insert("x".into()));
    }

    #[test]
    fn config_defaults() {
        let cfg = Config::default();
        assert!(cfg.project_dir.is_none());
    }
}
