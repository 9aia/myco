//! Link index — the edge cache behind "what is attached to item
//! X?" (P-005). A second index face next to the repo snapshots:
//! `project/` files are the input signals, `(src, dst, kind)`
//! edges the memoized output.
//!
//! Invalidation is file-grained: each file's [`FileStamp`] pairs
//! the mtime+size precheck with a content [`fingerprint`] (xxh3).
//! On reload the caller diffs live files against [`LinkIndex::stamps`],
//! re-derives edges for the dirty set only (`myco_core::refs::extract`
//! is pure), and [`LinkIndex::record`] swaps that file's outgoing
//! edges + stamp in one txn — a crash mid-sync can never leave a
//! file half-updated. [`LinkIndex::sweep`] drops records whose
//! source file vanished; dangling edge ends are filtered per
//! query by the caller's `alive` set.

use std::collections::{BTreeMap, BTreeSet};

use camino::{Utf8Path, Utf8PathBuf};
use myco_core::{Edge, EdgeKind, ItemId};

use crate::Error;

/// Content fingerprint — xxh3 of the file's bytes. Port-owned so
/// a stored stamp means the same thing to every impl and every
/// caller; the hasher itself is a pure function of the bytes.
pub fn fingerprint(contents: &[u8]) -> u64 {
    xxhash_rust::xxh3::xxh3_64(contents)
}

/// The invalidation stamp kept per file: the cheap precheck
/// (`mtime_ms` + `size`) plus the content [`fingerprint`] that
/// settles "changed vs touched".
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FileStamp {
    /// `mtime` as millis since the epoch — the stat-level
    /// precheck; a match means the file wasn't even re-read.
    pub mtime_ms: u64,
    /// Byte length — same precheck role.
    pub size: u64,
    /// `fingerprint(contents)` — the real change detector.
    pub fingerprint: u64,
}

/// One neighbor of an item, as `related` reports it. `kind` is
/// already mirrored for incoming edges — a file that does
/// `blocks: [X]` surfaces on X's side as `blocked_by`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Related {
    /// The item at the other end of the edge.
    pub other: ItemId,
    /// The relationship as seen from the queried item —
    /// `blocked_by`/`referenced_by` for incoming edges.
    pub kind: EdgeKind,
}

/// The link-index face of the port — per-repo edge storage.
///
/// `repo` is the same key [`Index`](crate::Index) snapshots use
/// (the canonical repo-root path); edges from different repos
/// never mix. All methods are `&self`: impls open the backend
/// per call, like the snapshot face does.
pub trait LinkIndex {
    /// Every file stamp recorded for `repo` — the precheck input
    /// for the invalidation diff.
    fn stamps(&self, repo: &str) -> Result<BTreeMap<Utf8PathBuf, FileStamp>, Error>;

    /// Which `live` files need re-deriving — new paths plus
    /// stamps that differ. Default impl diffs against
    /// [`stamps`](LinkIndex::stamps); impls may override for a
    /// fused backend query.
    fn dirty(
        &self,
        repo: &str,
        live: &BTreeMap<Utf8PathBuf, FileStamp>,
    ) -> Result<Vec<Utf8PathBuf>, Error> {
        let stored = self.stamps(repo)?;
        Ok(live
            .iter()
            .filter(|(p, s)| stored.get(*p) != Some(*s))
            .map(|(p, _)| p.clone())
            .collect())
    }

    /// Replace `rel`'s outgoing edge set + stamp atomically —
    /// one txn, so a crash can never leave a file half-updated.
    /// `edges` is the whole set `extract` derived for this file
    /// (possibly empty — a stamp with no edges is how "file read,
    /// nothing declared" is recorded).
    fn record(
        &self,
        repo: &str,
        rel: &Utf8Path,
        stamp: &FileStamp,
        edges: &[Edge],
    ) -> Result<(), Error>;

    /// Update only the stamp — content matched (fingerprint
    /// equal) but mtime moved; keeps the next precheck cheap
    /// without touching edges.
    fn restamp(&self, repo: &str, rel: &Utf8Path, stamp: &FileStamp) -> Result<(), Error>;

    /// Everything attached to `id`, both directions — outgoing
    /// edges come back verbatim, incoming ones with `kind`
    /// mirrored (`blocks` → `blocked_by`). Edge ends not in
    /// `alive` are dropped here, at query time — dangling ids are
    /// cheap to filter and get compacted away on the next
    /// [`sweep`](LinkIndex::sweep). Sorted by `(kind, other)`.
    fn related(
        &self,
        repo: &str,
        id: ItemId,
        alive: &BTreeSet<ItemId>,
    ) -> Result<Vec<Related>, Error>;

    /// Forward reachability from `id` along `kind` edges, at most
    /// `max_depth` hops — the visited set makes cyclic input
    /// terminate (the graph tolerates cycles, per the proposal;
    /// nothing enforces acyclicity). Sorted, `id` itself excluded.
    fn reach(
        &self,
        repo: &str,
        id: ItemId,
        kind: EdgeKind,
        max_depth: usize,
    ) -> Result<Vec<ItemId>, Error>;

    /// Delete every record whose source file isn't in `live` —
    /// stamps, provenance, and the edges they carried. Returns
    /// the number of files removed.
    fn sweep(&self, repo: &str, live: &BTreeSet<Utf8PathBuf>) -> Result<usize, Error>;

    /// Drop all link records for `repo` — the `index remove`
    /// teardown. Default impl sweeps with an empty live set.
    fn purge(&self, repo: &str) -> Result<(), Error> {
        self.sweep(repo, &BTreeSet::new()).map(|_| ())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    struct Stub {
        stored: BTreeMap<Utf8PathBuf, FileStamp>,
    }

    impl LinkIndex for Stub {
        fn stamps(&self, _r: &str) -> Result<BTreeMap<Utf8PathBuf, FileStamp>, Error> {
            Ok(self.stored.clone())
        }
        fn record(
            &self,
            _r: &str,
            _p: &Utf8Path,
            _s: &FileStamp,
            _e: &[Edge],
        ) -> Result<(), Error> {
            unimplemented!()
        }
        fn restamp(&self, _r: &str, _p: &Utf8Path, _s: &FileStamp) -> Result<(), Error> {
            unimplemented!()
        }
        fn related(
            &self,
            _r: &str,
            _id: ItemId,
            _a: &BTreeSet<ItemId>,
        ) -> Result<Vec<Related>, Error> {
            unimplemented!()
        }
        fn reach(
            &self,
            _r: &str,
            _id: ItemId,
            _k: EdgeKind,
            _d: usize,
        ) -> Result<Vec<ItemId>, Error> {
            unimplemented!()
        }
        fn sweep(&self, _r: &str, _l: &BTreeSet<Utf8PathBuf>) -> Result<usize, Error> {
            unimplemented!()
        }
    }

    fn stamp(fp: u64) -> FileStamp {
        FileStamp {
            mtime_ms: 1,
            size: 1,
            fingerprint: fp,
        }
    }

    #[test]
    fn fingerprint_is_stable_and_content_sensitive() {
        assert_eq!(fingerprint(b"hello"), fingerprint(b"hello"));
        assert_ne!(fingerprint(b"hello"), fingerprint(b"hellp"));
        assert_ne!(fingerprint(b""), fingerprint(b"x"));
    }

    #[test]
    fn dirty_defaults_to_a_stamp_diff() {
        let mut stored = BTreeMap::new();
        stored.insert(Utf8PathBuf::from("a.md"), stamp(1));
        let idx = Stub { stored };
        let mut live = BTreeMap::new();
        live.insert(Utf8PathBuf::from("a.md"), stamp(1)); // same → clean
        live.insert(Utf8PathBuf::from("b.md"), stamp(2)); // new → dirty
        live.insert(Utf8PathBuf::from("c.md"), stamp(3)); // stored absent → dirty
        let mut got = idx.dirty("repo", &live).unwrap();
        got.sort();
        assert_eq!(got, [Utf8PathBuf::from("b.md"), Utf8PathBuf::from("c.md")]);
    }

    #[test]
    fn file_stamp_serde_roundtrip() {
        let s = FileStamp {
            mtime_ms: 42,
            size: 7,
            fingerprint: 0xdead_beef,
        };
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<FileStamp>(&json).unwrap(), s);
    }
}
