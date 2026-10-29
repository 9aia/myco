//! myco-core — pure domain for the PM ledger: item models, ID
//! registry, state machine, deps. No I/O, no ports.
//!
//! Phase-1 bootstrap keeps only the generic primitives carried over
//! from the bupha anatomy. The real PM domain (numbered slugs,
//! `open → claimed → in-progress → blocked → done` transitions,
//! actor IDs) lands in phase 2 — see `project/proposals/v2.md`.

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

/// The `myco.toml`-shaped config value every `myco-config` source
/// loads. Fields grow per phase; unknown keys are rejected so a typo
/// fails loud instead of silently doing nothing.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Path to the project worktree, relative to the repo root.
    pub project_dir: Option<String>,
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
