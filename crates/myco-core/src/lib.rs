//! myco-core — pure domain for the PM ledger: item models, ID
//! registry, state machine, deps. No I/O, no ports.
//!
//! The numbered-slug domain (`T-012`/`ADR-0004` ids, the owned
//! `open → claimed → in-progress → blocked → done` state machine,
//! actor IDs, document models, `meta.toml` counters) is file-spec
//! agnostic — `myco-store-md` owns the markdown grammar.

pub mod actor;
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
pub use doc::{canonical_body, BacklogDoc, ItemFile, Section, TodoDoc};
pub use handoff::{is_valid_session, HandoffEntry};
pub use id::{require_kind, ItemId, ItemKind, ParseItemIdError, ParseItemKindError};
pub use item::{is_valid_group_name, is_valid_label, tree_view, Item, ItemMeta, NewItemMeta};
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

use serde::Deserialize;
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
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewEmitVerb {
    /// The view's spec — same fields as `[views.<name>]`.
    #[serde(flatten)]
    pub spec: ViewSpec,
    /// The output shape.
    pub emit: EmitMode,
}

/// How a `view-emit` verb renders its evaluated spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
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

/// The `myco.toml`-shaped config value every `myco-config` source
/// loads. Fields grow per phase; unknown keys are rejected so a typo
/// fails loud instead of silently doing nothing.
#[derive(Debug, Clone, Default, Deserialize)]
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
}

impl Config {
    /// The resolved kind table — prelude + this config's
    /// `[[kinds]]`. Missing `kinds` = the prelude alone, so
    /// zero-config is today's five kinds.
    pub fn kind_table(&self) -> Result<KindTable, String> {
        KindTable::new(self.kinds.clone())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

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
