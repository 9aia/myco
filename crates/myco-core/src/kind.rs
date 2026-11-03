//! The kind table — `[[kinds]]` in `myco.toml` layered over the
//! compiled-in prelude (P-008).
//!
//! [`ItemKind`] (in `id.rs`) is the *id identity* — the prefix an
//! `ItemId` carries. [`KindSpec`] is the *declaration* — name,
//! storage shape, state list, zero-pad — and [`KindTable`] is the
//! resolved set: the five prelude kinds plus whatever `[[kinds]]`
//! declares. Missing config = the prelude alone, so zero-config
//! behavior is today's.
//!
//! Merge semantics follow the config-verbs rule: a `[[kinds]]`
//! entry named like a prelude kind *shadows* it (restating the
//! prelude verbatim is a no-op; `init` seeds it commented) — but
//! a builtin's `prefix` is identity-locked, since changing it
//! would reinterpret every existing id. New names must not
//! collide on name or prefix.

use serde::Deserialize;

use crate::{is_valid_label, ItemKind, Status};

/// How a kind's items persist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum Storage {
    /// Line items in the ledger docs (`TODO.md`/`BACKLOG.md`).
    Doc,
    /// One md file each under `<dir>/` (+ `<dir>/archive/`).
    Dir,
}

fn default_pad() -> usize {
    3
}

fn default_done() -> Status {
    Status::Done
}

/// One `[[kinds]]` entry — or one prelude row.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct KindSpec {
    /// Config/view vocabulary — `task`, `spike`.
    pub name: String,
    /// Id prefix — `T`, `S`. `[A-Z][A-Z0-9]*`, ≤ 15 bytes.
    pub prefix: String,
    /// `doc` (ledger lines) or `dir` (item files).
    pub storage: Storage,
    /// `dir`-storage subdirectory — defaults to `<name>s`
    /// (`spike` → `spikes/`). Unused for `doc` kinds.
    #[serde(default)]
    pub dir: Option<String>,
    /// Filename prefix before the number — defaults to `name`.
    /// Empty string is meaningful: ADR files are `adrs/0004-*.md`.
    #[serde(default)]
    pub file_prefix: Option<String>,
    /// The statuses this kind may carry — defaults to the full
    /// machine (today's behavior for every kind).
    #[serde(default = "all_states")]
    pub states: Vec<Status>,
    /// The status that counts as resolved — `done` removes doc
    /// lines and marks dir items resolvable. Defaults to `done`.
    #[serde(default = "default_done")]
    pub done_state: Status,
    /// `superseded_by:`/`supersedes:` frontmatter is meaningful
    /// for this kind (ADRs).
    #[serde(default)]
    pub supersede: bool,
    /// The kind has a resolve-style verb (`issue resolve`) —
    /// declaration only in v1; no verb reads it yet.
    #[serde(default)]
    pub resolve_verb: bool,
    /// Canonical zero-pad in emitted ids — `T-007` is 3,
    /// `ADR-0004` is 4. Defaults to 3.
    #[serde(default = "default_pad")]
    pub zero_pad: usize,
}

fn all_states() -> Vec<Status> {
    Status::ALL.to_vec()
}

impl KindSpec {
    /// Field defaults resolved — the spec's [`ItemKind`] identity
    /// (prefix + pad), its `dir` name, filename prefix, and the
    /// `meta.toml` `[next]` key.
    pub fn id_kind(&self) -> ItemKind {
        // validation guarantees the prefix parses
        ItemKind::for_spec(&self.prefix, self.zero_pad)
            .unwrap_or_else(|| ItemKind::from_prefix(&self.prefix).unwrap_or(ItemKind::Task))
    }

    /// The `dir` subdir — `<name>s` unless declared.
    pub fn dir_name(&self) -> String {
        self.dir
            .clone()
            .unwrap_or_else(|| format!("{}s", self.name))
    }

    /// Filename prefix — `name` unless declared (ADR declares "").
    pub fn file_prefix(&self) -> String {
        self.file_prefix
            .clone()
            .unwrap_or_else(|| self.name.clone())
    }

    /// The `meta.toml` `[next]` key — lowercase prefix (`t`,
    /// `adr`, `s`).
    pub fn counter_key(&self) -> String {
        self.prefix.to_lowercase()
    }

    /// Is `status` this kind's terminal state?
    pub fn is_done(&self, status: Status) -> bool {
        status == self.done_state
    }

    /// Is `from → to` legal *for this kind*? The base machine's
    /// graph, intersected with the declared `states` list — a
    /// kind may narrow the vocabulary but not invent edges.
    pub fn can_transition(&self, from: Status, to: Status) -> bool {
        self.states.contains(&from) && self.states.contains(&to) && from.can_transition_to(to)
    }

    /// The declared states as a token list — for error text.
    pub fn state_tokens(&self) -> String {
        self.states
            .iter()
            .map(|s| s.token())
            .collect::<Vec<_>>()
            .join("|")
    }

    /// Structural validation — a spec that fails here is rejected
    /// at config load, never at item time.
    fn validate(&self) -> Result<(), String> {
        if !is_valid_label(&self.name) {
            return Err(format!("kind {:?}: name must be [A-Za-z0-9_-]+", self.name));
        }
        if ItemKind::from_prefix(&self.prefix).is_none() {
            return Err(format!(
                "kind {:?}: prefix {:?} must be [A-Z][A-Z0-9]*, ≤ 15 bytes",
                self.name, self.prefix
            ));
        }
        if self.zero_pad == 0 || self.zero_pad > 9 {
            return Err(format!("kind {:?}: zero_pad must be 1..=9", self.name));
        }
        if self.states.is_empty() {
            return Err(format!("kind {:?}: states must be non-empty", self.name));
        }
        if !self.states.contains(&self.done_state) {
            return Err(format!(
                "kind {:?}: done_state {:?} not in states",
                self.name,
                self.done_state.token()
            ));
        }
        if self.storage == Storage::Dir && self.dir_name().is_empty() {
            return Err(format!("kind {:?}: dir must be non-empty", self.name));
        }
        if let Some(d) = &self.dir {
            if !is_valid_label(d) {
                return Err(format!("kind {:?}: invalid dir {d:?}", self.name));
            }
        }
        if let Some(fp) = &self.file_prefix {
            if !fp.is_empty() && !is_valid_label(fp) {
                return Err(format!("kind {:?}: invalid file_prefix {fp:?}", self.name));
            }
        }
        Ok(())
    }
}

/// The resolved kind set — prelude specs in declaration order,
/// then user kinds. Immutable after construction.
#[derive(Debug, Clone)]
pub struct KindTable {
    specs: Vec<KindSpec>,
}

impl KindTable {
    /// Today's five — task/issue/proposal/adr/plan — as data.
    /// `Default` = this, so a missing `[[kinds]]` is unchanged.
    pub fn prelude() -> KindTable {
        KindTable {
            specs: vec![
                KindSpec {
                    name: "task".into(),
                    prefix: "T".into(),
                    storage: Storage::Doc,
                    dir: None,
                    file_prefix: None,
                    states: all_states(),
                    done_state: Status::Done,
                    supersede: false,
                    resolve_verb: false,
                    zero_pad: 3,
                },
                KindSpec {
                    name: "issue".into(),
                    prefix: "I".into(),
                    storage: Storage::Dir,
                    dir: Some("issues".into()),
                    file_prefix: Some("issue".into()),
                    states: all_states(),
                    done_state: Status::Done,
                    supersede: false,
                    resolve_verb: true,
                    zero_pad: 3,
                },
                KindSpec {
                    name: "proposal".into(),
                    prefix: "P".into(),
                    storage: Storage::Dir,
                    dir: Some("proposals".into()),
                    file_prefix: Some("proposal".into()),
                    states: all_states(),
                    done_state: Status::Done,
                    supersede: false,
                    resolve_verb: false,
                    zero_pad: 3,
                },
                KindSpec {
                    name: "adr".into(),
                    prefix: "ADR".into(),
                    storage: Storage::Dir,
                    dir: Some("adrs".into()),
                    file_prefix: Some(String::new()),
                    states: all_states(),
                    done_state: Status::Done,
                    supersede: true,
                    resolve_verb: false,
                    zero_pad: 4,
                },
                KindSpec {
                    name: "plan".into(),
                    prefix: "PL".into(),
                    storage: Storage::Dir,
                    dir: Some("plans".into()),
                    file_prefix: Some("pl".into()),
                    states: all_states(),
                    done_state: Status::Done,
                    supersede: false,
                    resolve_verb: false,
                    zero_pad: 3,
                },
            ],
        }
    }

    /// Prelude + `user` specs — the [`crate::Config::kinds`]
    /// merge. Later entries shadow earlier ones *by name* (repo
    /// layer last wins, like verbs); a prelude name may be
    /// restated but its prefix is locked. Errors on bad specs and
    /// name/prefix collisions.
    pub fn new(user: Vec<KindSpec>) -> Result<KindTable, String> {
        let mut table = KindTable::prelude();
        for spec in user {
            spec.validate()?;
            match table.specs.iter().position(|s| s.name == spec.name) {
                Some(i) => {
                    let existing = &table.specs[i];
                    if i < PRELUDE_LEN && existing.prefix != spec.prefix {
                        return Err(format!(
                            "kind {:?}: prefix is locked to {:?} — builtin ids can't be reinterpreted",
                            spec.name, existing.prefix
                        ));
                    }
                    table.specs[i] = spec;
                }
                None => {
                    if let Some(other) = table
                        .specs
                        .iter()
                        .find(|s| s.prefix == spec.prefix || s.counter_key() == spec.counter_key())
                    {
                        return Err(format!(
                            "kind {:?}: prefix {:?} already used by kind {:?}",
                            spec.name, spec.prefix, other.name
                        ));
                    }
                    table.specs.push(spec);
                }
            }
        }
        Ok(table)
    }

    /// Specs in declaration order — prelude first.
    pub fn iter(&self) -> impl Iterator<Item = &KindSpec> {
        self.specs.iter()
    }

    /// The number of declared kinds.
    pub fn len(&self) -> usize {
        self.specs.len()
    }

    /// `len() == 0` — never true (the prelude is always present).
    pub fn is_empty(&self) -> bool {
        self.specs.is_empty()
    }

    /// Spec by `name` — `task`, `spike`.
    pub fn by_name(&self, name: &str) -> Option<&KindSpec> {
        self.specs.iter().find(|s| s.name == name)
    }

    /// Spec by id prefix — `T`, `S`.
    pub fn by_prefix(&self, prefix: &str) -> Option<&KindSpec> {
        self.specs.iter().find(|s| s.prefix == prefix)
    }

    /// The spec for an id-carried kind (`id.kind()`).
    pub fn spec_of(&self, kind: ItemKind) -> Option<&KindSpec> {
        self.by_prefix(kind.prefix())
    }

    /// `name`-or-`prefix` resolution — view filters accept both
    /// vocabularies (`kind = ["issue"]` and `kind = ["I"]`).
    pub fn resolve(&self, token: &str) -> Option<&KindSpec> {
        self.by_name(token).or_else(|| self.by_prefix(token))
    }

    /// Position in declaration order — the grouping/sort axis for
    /// `kind` buckets. `None` for undeclared prefixes.
    pub fn position(&self, kind: ItemKind) -> Option<usize> {
        self.specs.iter().position(|s| s.id_kind() == kind)
    }

    /// The kind's display name for a group label — the declared
    /// name, or the bare prefix for undeclared kinds.
    pub fn name_of(&self, kind: ItemKind) -> String {
        self.spec_of(kind)
            .map(|s| s.name.clone())
            .unwrap_or_else(|| kind.prefix().to_string())
    }

    /// `dir`-storage specs — the kinds `list_items`/`all_ids`
    /// scan files for.
    pub fn file_backed(&self) -> impl Iterator<Item = &KindSpec> {
        self.specs.iter().filter(|s| s.storage == Storage::Dir)
    }
}

impl Default for KindTable {
    /// The prelude — zero-config is today's five kinds.
    fn default() -> Self {
        KindTable::prelude()
    }
}

/// The shared prelude — cheap `&'static` access for trait
/// defaults (`Store::kinds`) and table-free call sites.
static PRELUDE: std::sync::LazyLock<KindTable> = std::sync::LazyLock::new(KindTable::prelude);

/// The prelude as a static — zero-config callers needn't
/// construct a [`KindTable`].
pub fn prelude_table() -> &'static KindTable {
    &PRELUDE
}

/// Prelude length — entries below this index are builtin names.
const PRELUDE_LEN: usize = 5;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn spike() -> KindSpec {
        toml::from_str::<KindSpec>(
            "name = \"spike\"\nprefix = \"S\"\nstorage = \"dir\"\n\
             states = [\"open\",\"in-progress\",\"done\"]\n",
        )
        .unwrap()
    }

    #[test]
    fn prelude_is_todays_five() {
        let t = KindTable::prelude();
        assert_eq!(t.len(), 5);
        let spec = t.by_name("adr").unwrap();
        assert_eq!(spec.prefix, "ADR");
        assert_eq!(spec.zero_pad, 4);
        assert_eq!(spec.dir_name(), "adrs");
        assert_eq!(spec.file_prefix(), "");
        assert!(spec.supersede);
        let issue = t.by_name("issue").unwrap();
        assert_eq!(issue.dir_name(), "issues");
        assert_eq!(issue.file_prefix(), "issue");
        assert_eq!(issue.counter_key(), "i");
        // doc-backed task has no dir
        assert_eq!(t.file_backed().count(), 4);
        assert_eq!(t.by_name("task").unwrap().storage, Storage::Doc);
    }

    #[test]
    fn id_kind_and_resolution() {
        let t = KindTable::new(vec![spike()]).unwrap();
        let s = t.by_name("spike").unwrap();
        assert_eq!(s.dir_name(), "spikes");
        assert_eq!(s.file_prefix(), "spike");
        assert_eq!(s.counter_key(), "s");
        assert_eq!(s.id_kind().prefix(), "S");
        assert_eq!(s.id_kind().width(), 3);
        assert_eq!(t.resolve("S").unwrap().name, "spike");
        assert_eq!(t.resolve("issue").unwrap().prefix, "I");
        assert!(t.resolve("nope").is_none());
        assert_eq!(t.position(s.id_kind()), Some(5));
        assert_eq!(t.position(ItemKind::Task), Some(0));
    }

    #[test]
    fn custom_states_narrow_the_machine() {
        let t = KindTable::new(vec![spike()]).unwrap();
        let s = t.by_name("spike").unwrap();
        // base-graph edge inside the declared states — legal
        assert!(s.can_transition(Status::Open, Status::InProgress));
        assert!(s.can_transition(Status::InProgress, Status::Done));
        // claimed isn't declared — the kind narrowed it away
        assert!(!s.can_transition(Status::Open, Status::Claimed));
        // still not in the base graph either way
        assert!(!s.can_transition(Status::Open, Status::Done));
        assert!(s.is_done(Status::Done));
        assert!(!s.is_done(Status::Open));
    }

    #[test]
    fn shadow_prelude_by_name() {
        // restating a builtin keeps its prefix; other fields may
        // change (states narrowed here)
        let narrowed = toml::from_str::<KindSpec>(
            "name = \"issue\"\nprefix = \"I\"\nstorage = \"dir\"\n\
             states = [\"open\",\"in-progress\",\"done\"]\n",
        )
        .unwrap();
        let t = KindTable::new(vec![narrowed]).unwrap();
        let i = t.by_name("issue").unwrap();
        assert_eq!(i.states.len(), 3);
        assert_eq!(t.len(), 5); // shadow, not append
                                // prefix hijack is refused
        let hijack =
            toml::from_str::<KindSpec>("name = \"issue\"\nprefix = \"X\"\nstorage = \"dir\"\n")
                .unwrap();
        assert!(KindTable::new(vec![hijack]).is_err());
    }

    #[test]
    fn collisions_and_bad_specs_rejected() {
        // prefix collision with a builtin
        let bad =
            toml::from_str::<KindSpec>("name = \"tracker\"\nprefix = \"T\"\nstorage = \"dir\"\n")
                .unwrap();
        assert!(KindTable::new(vec![bad]).is_err());
        // lowercase prefix — not a valid id prefix
        let bad = toml::from_str::<KindSpec>("name = \"x\"\nprefix = \"t\"\nstorage = \"dir\"\n")
            .unwrap();
        assert!(KindTable::new(vec![bad]).is_err());
        // done_state outside states
        let bad = toml::from_str::<KindSpec>(
            "name = \"x\"\nprefix = \"X\"\nstorage = \"doc\"\n\
             states = [\"open\"]\ndone_state = \"done\"\n",
        )
        .unwrap();
        assert!(KindTable::new(vec![bad]).is_err());
        // duplicate names in one layer — second shadows first
        let t = KindTable::new(vec![spike(), spike()]).unwrap();
        assert_eq!(t.len(), 6);
    }
}
