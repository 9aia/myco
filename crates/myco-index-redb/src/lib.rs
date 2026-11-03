//! myco-index-redb — the redb impl of [`myco_index::Index`].
//!
//! Layout under the index dir (`$MYCO_HOME`, else
//! `$XDG_DATA_HOME/myco`, else `~/.local/share/myco`):
//!
//! - `index.redb` — the `snapshots` table (canonical repo path →
//!   JSON [`RepoSnapshot`]) plus the link-index tables (P-005):
//!   `files` (`repo\0rel` → JSON [`FileStamp`]), `file_edges`
//!   (`repo\0rel` → JSON edge set — the provenance that makes a
//!   file's outgoing edges swappable), `edges` (multimap
//!   `repo\0src\0kind` → dst), and `rev_edges` (multimap
//!   `repo\0dst` → `kind\0src`) for O(1) `related`.
//! - `repos.toml` — the registry (`[[repo]] path name`), kept as a
//!   plain file so it stays human-inspectable.
//!
//! Everything here is a derived cache — `rm -rf` the dir and
//! `myco index rebuild` derives it back from the worktrees.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use camino::{Utf8Path, Utf8PathBuf};
use myco_core::{Edge, EdgeKind, ItemId};
use myco_index::{Error, FileStamp, Index, LinkIndex, Related, RepoEntry, RepoSnapshot};
use redb::{Database, MultimapTableDefinition, ReadableDatabase, ReadableTable, TableDefinition};

/// `snapshots` table — repo path → JSON [`RepoSnapshot`].
const SNAPSHOTS: TableDefinition<&str, &str> = TableDefinition::new("snapshots");

/// `files` table — `repo\0rel` → JSON [`FileStamp`]. Presence is
/// also the sweep manifest: a row outlives its file → swept.
const FILES: TableDefinition<&str, &str> = TableDefinition::new("files");

/// `file_edges` table — `repo\0rel` → JSON `Vec<Edge>`. The
/// provenance table: `record` must delete exactly the edges the
/// file last produced, so it keeps them verbatim.
const FILE_EDGES: TableDefinition<&str, &str> = TableDefinition::new("file_edges");

/// `edges` multimap — `repo\0src\0kind` → `dst` (canonical id
/// string). One value per edge; the multimap keeps per-file swaps
/// to add/remove-delta.
const EDGES: MultimapTableDefinition<&str, &str> = MultimapTableDefinition::new("edges");

/// `rev_edges` multimap — `repo\0dst` → `kind\0src`. The incoming
/// half of `related`: value carries the kind so one lookup gets
/// every direction at once, mirrored at query time.
const REV_EDGES: MultimapTableDefinition<&str, &str> = MultimapTableDefinition::new("rev_edges");

/// The registry file name inside the index dir.
const REGISTRY: &str = "repos.toml";

/// The registry lock file — serializes the `repos.toml`
/// read-modify-write across concurrent `myco` processes.
const REGISTRY_LOCK: &str = "repos.toml.lock";

/// The redb file name inside the index dir.
const DB: &str = "index.redb";

/// Total time a contended index lock is retried before giving up —
/// lets concurrent `myco` processes queue instead of crash.
const LOCK_RETRY_BUDGET: Duration = Duration::from_secs(2);

/// First backoff after a busy lock; doubles each retry.
const LOCK_RETRY_MIN: Duration = Duration::from_millis(10);

/// Longest backoff between retries.
const LOCK_RETRY_MAX: Duration = Duration::from_millis(320);

/// The index dir: `$MYCO_HOME` → `$XDG_DATA_HOME/myco` →
/// `~/.local/share/myco`. The `$MYCO_HOME` override is how tests
/// (and unusual setups) relocate the whole cache. `Err` when no
/// base resolves (HOME unset) — better than writing the cache
/// into whatever directory `myco` happened to run from.
pub fn data_dir() -> Result<PathBuf, Error> {
    if let Some(d) = std::env::var_os("MYCO_HOME") {
        return Ok(PathBuf::from(d));
    }
    if let Some(d) = std::env::var_os("XDG_DATA_HOME") {
        return Ok(PathBuf::from(d).join("myco"));
    }
    std::env::var("HOME")
        .map(|h| PathBuf::from(h).join(".local/share/myco"))
        .map_err(|_| {
            Error::Backend("no index dir — HOME unset (set MYCO_HOME or XDG_DATA_HOME)".into())
        })
}

/// `repos.toml` on disk.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct Registry {
    #[serde(default)]
    repo: Vec<RepoEntry>,
}

/// Open `index.redb` at `path`, retrying on lock contention
/// ([`redb::DatabaseError::DatabaseAlreadyOpen`]) with exponential
/// backoff until `budget` is spent. Anything else — corrupt file,
/// IO, upgrade required — fails immediately.
fn open_db(path: &Utf8Path, budget: Duration) -> Result<Database, Error> {
    let start = Instant::now();
    let mut backoff = LOCK_RETRY_MIN;
    let mut attempt = 0u64;
    loop {
        match Database::create(path.as_std_path()) {
            Ok(db) => return Ok(db),
            Err(redb::DatabaseError::DatabaseAlreadyOpen) => {
                let left = budget.saturating_sub(start.elapsed());
                if left.is_zero() {
                    return Err(Error::Backend(format!(
                        "index busy — another myco process holds {DB} (waited ~{:.1}s)",
                        budget.as_secs_f64()
                    )));
                }
                // counter-based stagger — spreads contenders, no rand dep
                let jitter = Duration::from_millis((attempt % 4) * 5);
                std::thread::sleep((backoff + jitter).min(left));
                backoff = (backoff * 2).min(LOCK_RETRY_MAX);
                attempt += 1;
            }
            Err(e) => return Err(Error::Backend(format!("open {path}: {e}"))),
        }
    }
}

/// redb-backed index rooted at an index dir.
pub struct RedbIndex {
    dir: Utf8PathBuf,
}

impl RedbIndex {
    /// Open the default index ([`data_dir`]), creating it.
    pub fn open() -> Result<Self, Error> {
        let dir = Utf8PathBuf::from_path_buf(data_dir()?)
            .map_err(|p| Error::Backend(format!("{}: non-utf8 index dir", p.display())))?;
        Self::open_at(dir)
    }

    /// Open an index rooted at `dir`, creating it when missing.
    pub fn open_at(dir: impl Into<Utf8PathBuf>) -> Result<Self, Error> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;
        Ok(RedbIndex { dir })
    }

    /// The index dir.
    pub fn dir(&self) -> &Utf8Path {
        &self.dir
    }

    fn registry_path(&self) -> Utf8PathBuf {
        self.dir.join(REGISTRY)
    }

    fn db_path(&self) -> Utf8PathBuf {
        self.dir.join(DB)
    }

    fn load_registry(&self) -> Result<Registry, Error> {
        match std::fs::read_to_string(self.registry_path()) {
            Ok(src) => toml::from_str(&src).map_err(|e| Error::Codec(format!("{REGISTRY}: {e}"))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Registry::default()),
            Err(e) => Err(Error::Io(e)),
        }
    }

    /// Take the registry lock — `try_lock` with the same retry
    /// budget/backoff as [`open_db`]. Held across the
    /// load→mutate→save of `register`/`remove`; a contended
    /// registry queues instead of losing updates.
    fn lock_registry(&self) -> Result<std::fs::File, Error> {
        let path = self.dir.join(REGISTRY_LOCK);
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)?;
        let start = Instant::now();
        let mut backoff = LOCK_RETRY_MIN;
        let mut attempt = 0u64;
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(file),
                Err(std::fs::TryLockError::WouldBlock) => {
                    let left = LOCK_RETRY_BUDGET.saturating_sub(start.elapsed());
                    if left.is_zero() {
                        return Err(Error::Backend(format!(
                            "index busy — another myco process holds {REGISTRY_LOCK} \
                             (waited ~{:.1}s)",
                            LOCK_RETRY_BUDGET.as_secs_f64()
                        )));
                    }
                    // same stagger as open_db — spreads contenders
                    let jitter = Duration::from_millis((attempt % 4) * 5);
                    std::thread::sleep((backoff + jitter).min(left));
                    backoff = (backoff * 2).min(LOCK_RETRY_MAX);
                    attempt += 1;
                }
                Err(std::fs::TryLockError::Error(e)) => return Err(Error::Io(e)),
            }
        }
    }

    fn save_registry(&self, reg: &Registry) -> Result<(), Error> {
        let src = toml::to_string(reg).map_err(|e| Error::Codec(format!("{REGISTRY}: {e}")))?;
        let path = self.registry_path();
        // pid-tagged tmp — writers are serialized by the lock, but
        // a stale tmp from a killed process must not be clobbered
        // by a live one mid-rename. No fsync: the registry is a
        // disposable cache — the rename is the atomicity, not
        // power-loss durability — and the sync dominates the
        // critical section contenders queue behind.
        let tmp = path.with_extension(format!("toml.{}.tmp", std::process::id()));
        let write = (|| -> std::io::Result<()> {
            std::fs::write(&tmp, src)?;
            std::fs::rename(&tmp, &path)
        })();
        if write.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        write.map_err(Error::Io)
    }

    fn db(&self) -> Result<Database, Error> {
        open_db(&self.db_path(), LOCK_RETRY_BUDGET)
    }

    fn write_snapshot(&self, path: &Utf8Path, value: Option<&str>) -> Result<(), Error> {
        let db = self.db()?;
        let tx = db
            .begin_write()
            .map_err(|e| Error::Backend(format!("begin_write: {e}")))?;
        {
            let mut t = tx
                .open_table(SNAPSHOTS)
                .map_err(|e| Error::Backend(format!("open snapshots: {e}")))?;
            match value {
                Some(v) => {
                    t.insert(path.as_str(), v)
                        .map_err(|e| Error::Backend(format!("insert: {e}")))?;
                }
                None => {
                    t.remove(path.as_str())
                        .map_err(|e| Error::Backend(format!("remove: {e}")))?;
                }
            }
        }
        tx.commit()
            .map_err(|e| Error::Backend(format!("commit: {e}")))
    }
}

impl Index for RedbIndex {
    fn register(&self, repo: &RepoEntry) -> Result<(), Error> {
        let _lock = self.lock_registry()?;
        let mut reg = self.load_registry()?;
        match reg.repo.iter_mut().find(|r| r.path == repo.path) {
            Some(r) => {
                r.name = repo.name.clone();
                r.worktree = repo.worktree.clone();
            }
            None => reg.repo.push(repo.clone()),
        }
        self.save_registry(&reg)
    }

    fn remove(&self, path: &Utf8Path) -> Result<bool, Error> {
        let _lock = self.lock_registry()?;
        let mut reg = self.load_registry()?;
        let before = reg.repo.len();
        reg.repo.retain(|r| r.path != path);
        if reg.repo.len() == before {
            return Ok(false);
        }
        // drop the snapshot before the registry write — a failure
        // here leaves a registered repo with no snapshot (stale →
        // rebuild), not an orphan snapshot keyed to nothing. The
        // link records go with it — `purge` is just a sweep with
        // an empty live set.
        self.write_snapshot(path, None)?;
        LinkIndex::purge(self, path.as_str())?;
        self.save_registry(&reg)?;
        Ok(true)
    }

    fn repos(&self) -> Result<Vec<RepoEntry>, Error> {
        let mut repos = self.load_registry()?.repo;
        repos.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(repos)
    }

    fn snapshot(&self, path: &Utf8Path) -> Result<Option<RepoSnapshot>, Error> {
        let db = self.db()?;
        let tx = db
            .begin_read()
            .map_err(|e| Error::Backend(format!("begin_read: {e}")))?;
        let t = match tx.open_table(SNAPSHOTS) {
            Ok(t) => t,
            Err(_) => return Ok(None), // no table → no snapshots yet
        };
        let Some(v) = t
            .get(path.as_str())
            .map_err(|e| Error::Backend(format!("get: {e}")))?
        else {
            return Ok(None);
        };
        // a corrupt snapshot reads as a missing one — the index
        // is a disposable cache, so callers see "never scanned",
        // mark it stale, and rebuild instead of erroring every
        // read (one bad row used to brick `org`/`index list`)
        Ok(serde_json::from_str(v.value()).ok())
    }

    fn save_snapshot(&self, snapshot: &RepoSnapshot) -> Result<(), Error> {
        let json = serde_json::to_string(snapshot)
            .map_err(|e| Error::Codec(format!("snapshot {}: {e}", snapshot.path)))?;
        self.write_snapshot(&snapshot.path, Some(&json))
    }
}

// --- link index (P-005) ---------------------------------------------------

/// `files`/`file_edges` row key — `repo\0rel`.
fn file_key(repo: &str, rel: &Utf8Path) -> String {
    format!("{repo}\0{rel}")
}

/// `edges` multimap key — `repo\0src\0kind`.
fn edge_key(repo: &str, src: ItemId, kind: EdgeKind) -> String {
    format!("{repo}\0{src}\0{}", kind.name())
}

/// `rev_edges` multimap key — `repo\0dst`.
fn rev_key(repo: &str, dst: ItemId) -> String {
    format!("{repo}\0{dst}")
}

/// `rev_edges` multimap value — `kind\0src`.
fn rev_val(kind: EdgeKind, src: ItemId) -> String {
    format!("{}\0{src}", kind.name())
}

/// Remove every `edges`/`rev_edges` pair an edge set owns —
/// the delete half of `record`'s swap and `sweep`'s cleanup.
fn drop_edges(
    repo: &str,
    edges: &[Edge],
    ed: &mut redb::MultimapTable<&str, &str>,
    rv: &mut redb::MultimapTable<&str, &str>,
) -> Result<(), Error> {
    for e in edges {
        ed.remove(
            edge_key(repo, e.src, e.kind).as_str(),
            e.dst.to_string().as_str(),
        )
        .map_err(|e| Error::Backend(format!("edges remove: {e}")))?;
        rv.remove(
            rev_key(repo, e.dst).as_str(),
            rev_val(e.kind, e.src).as_str(),
        )
        .map_err(|e| Error::Backend(format!("rev_edges remove: {e}")))?;
    }
    Ok(())
}

/// The stored edge set one file produced — corrupt JSON reads as
/// empty, same "disposable cache" stance as `snapshot`.
fn read_file_edges(
    t: &impl redb::ReadableTable<&'static str, &'static str>,
    key: &str,
) -> Result<Vec<Edge>, Error> {
    let Some(v) = t
        .get(key)
        .map_err(|e| Error::Backend(format!("file_edges get: {e}")))?
    else {
        return Ok(Vec::new());
    };
    Ok(serde_json::from_str(v.value()).unwrap_or_default())
}

impl LinkIndex for RedbIndex {
    fn stamps(&self, repo: &str) -> Result<BTreeMap<Utf8PathBuf, FileStamp>, Error> {
        let db = self.db()?;
        let tx = db
            .begin_read()
            .map_err(|e| Error::Backend(format!("begin_read: {e}")))?;
        let t = match tx.open_table(FILES) {
            Ok(t) => t,
            Err(_) => return Ok(BTreeMap::new()),
        };
        let prefix = format!("{repo}\0");
        let mut out = BTreeMap::new();
        let iter = t
            .iter()
            .map_err(|e| Error::Backend(format!("files iter: {e}")))?;
        for row in iter {
            let (k, v) = row.map_err(|e| Error::Backend(format!("files iter: {e}")))?;
            let Some(rel) = k.value().strip_prefix(&prefix) else {
                continue;
            };
            if let Ok(stamp) = serde_json::from_str::<FileStamp>(v.value()) {
                out.insert(Utf8PathBuf::from(rel), stamp);
            }
        }
        Ok(out)
    }

    fn record(
        &self,
        repo: &str,
        rel: &Utf8Path,
        stamp: &FileStamp,
        edges: &[Edge],
    ) -> Result<(), Error> {
        let json =
            serde_json::to_string(edges).map_err(|e| Error::Codec(format!("edges {rel}: {e}")))?;
        let stamp_json =
            serde_json::to_string(stamp).map_err(|e| Error::Codec(format!("stamp {rel}: {e}")))?;
        let db = self.db()?;
        let tx = db
            .begin_write()
            .map_err(|e| Error::Backend(format!("begin_write: {e}")))?;
        {
            let key = file_key(repo, rel);
            let mut fe = tx
                .open_table(FILE_EDGES)
                .map_err(|e| Error::Backend(format!("open file_edges: {e}")))?;
            let mut ed = tx
                .open_multimap_table(EDGES)
                .map_err(|e| Error::Backend(format!("open edges: {e}")))?;
            let mut rv = tx
                .open_multimap_table(REV_EDGES)
                .map_err(|e| Error::Backend(format!("open rev_edges: {e}")))?;
            // swap: out with the file's old set, in with the new —
            // one commit, so a crash can't leave either half
            let old = read_file_edges(&fe, &key)?;
            drop_edges(repo, &old, &mut ed, &mut rv)?;
            fe.insert(key.as_str(), json.as_str())
                .map_err(|e| Error::Backend(format!("file_edges insert: {e}")))?;
            for e in edges {
                ed.insert(
                    edge_key(repo, e.src, e.kind).as_str(),
                    e.dst.to_string().as_str(),
                )
                .map_err(|e| Error::Backend(format!("edges insert: {e}")))?;
                rv.insert(
                    rev_key(repo, e.dst).as_str(),
                    rev_val(e.kind, e.src).as_str(),
                )
                .map_err(|e| Error::Backend(format!("rev_edges insert: {e}")))?;
            }
            {
                let mut fs = tx
                    .open_table(FILES)
                    .map_err(|e| Error::Backend(format!("open files: {e}")))?;
                fs.insert(key.as_str(), stamp_json.as_str())
                    .map_err(|e| Error::Backend(format!("files insert: {e}")))?;
            }
        }
        tx.commit()
            .map_err(|e| Error::Backend(format!("commit: {e}")))
    }

    fn restamp(&self, repo: &str, rel: &Utf8Path, stamp: &FileStamp) -> Result<(), Error> {
        let json =
            serde_json::to_string(stamp).map_err(|e| Error::Codec(format!("stamp {rel}: {e}")))?;
        let db = self.db()?;
        let tx = db
            .begin_write()
            .map_err(|e| Error::Backend(format!("begin_write: {e}")))?;
        {
            let mut fs = tx
                .open_table(FILES)
                .map_err(|e| Error::Backend(format!("open files: {e}")))?;
            fs.insert(file_key(repo, rel).as_str(), json.as_str())
                .map_err(|e| Error::Backend(format!("files insert: {e}")))?;
        }
        tx.commit()
            .map_err(|e| Error::Backend(format!("commit: {e}")))
    }

    fn related(
        &self,
        repo: &str,
        id: ItemId,
        alive: &BTreeSet<ItemId>,
    ) -> Result<Vec<Related>, Error> {
        let db = self.db()?;
        let tx = db
            .begin_read()
            .map_err(|e| Error::Backend(format!("begin_read: {e}")))?;
        let mut out = BTreeSet::new();
        // outgoing — only extractable kinds can be stored forward
        if let Ok(ed) = tx.open_multimap_table(EDGES) {
            for kind in EdgeKind::ALL.into_iter().filter(|k| k.extracted()) {
                let vals = match ed.get(edge_key(repo, id, kind).as_str()) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                for v in vals {
                    let v = v.map_err(|e| Error::Backend(format!("edges get: {e}")))?;
                    if let Ok(dst) = v.value().parse::<ItemId>() {
                        if alive.contains(&dst) {
                            out.insert(Related { other: dst, kind });
                        }
                    }
                }
            }
        }
        // incoming — the reverse table mirrors the kind for us
        if let Ok(rv) = tx.open_multimap_table(REV_EDGES) {
            if let Ok(vals) = rv.get(rev_key(repo, id).as_str()) {
                for v in vals {
                    let v = v.map_err(|e| Error::Backend(format!("rev_edges get: {e}")))?;
                    let Some((kind, src)) = v.value().split_once('\0') else {
                        continue;
                    };
                    let (Some(kind), Ok(src)) = (EdgeKind::from_name(kind), src.parse::<ItemId>())
                    else {
                        continue;
                    };
                    // a self-edge (handoff) is already reported by
                    // the outgoing pass — the set dedups it anyway
                    if alive.contains(&src) {
                        out.insert(Related {
                            other: src,
                            kind: kind.mirror(),
                        });
                    }
                }
            }
        }
        Ok(out.into_iter().collect())
    }

    fn reach(
        &self,
        repo: &str,
        id: ItemId,
        kind: EdgeKind,
        max_depth: usize,
    ) -> Result<Vec<ItemId>, Error> {
        let db = self.db()?;
        let tx = db
            .begin_read()
            .map_err(|e| Error::Backend(format!("begin_read: {e}")))?;
        let ed = match tx.open_multimap_table(EDGES) {
            Ok(t) => t,
            Err(_) => return Ok(Vec::new()),
        };
        // BFS with a visited set — cyclic input terminates at the
        // depth cap, never on a loop
        let mut seen = BTreeSet::from([id]);
        let mut frontier = vec![id];
        for _ in 0..max_depth {
            let mut next = Vec::new();
            for cur in &frontier {
                let vals = match ed.get(edge_key(repo, *cur, kind).as_str()) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                for v in vals {
                    let v = v.map_err(|e| Error::Backend(format!("edges get: {e}")))?;
                    if let Ok(dst) = v.value().parse::<ItemId>() {
                        if seen.insert(dst) {
                            next.push(dst);
                        }
                    }
                }
            }
            if next.is_empty() {
                break;
            }
            frontier = next;
        }
        seen.remove(&id);
        Ok(seen.into_iter().collect())
    }

    fn sweep(&self, repo: &str, live: &BTreeSet<Utf8PathBuf>) -> Result<usize, Error> {
        let db = self.db()?;
        let tx = db
            .begin_write()
            .map_err(|e| Error::Backend(format!("begin_write: {e}")))?;
        let mut removed = 0;
        {
            let mut fs = tx
                .open_table(FILES)
                .map_err(|e| Error::Backend(format!("open files: {e}")))?;
            let mut fe = tx
                .open_table(FILE_EDGES)
                .map_err(|e| Error::Backend(format!("open file_edges: {e}")))?;
            let mut ed = tx
                .open_multimap_table(EDGES)
                .map_err(|e| Error::Backend(format!("open edges: {e}")))?;
            let mut rv = tx
                .open_multimap_table(REV_EDGES)
                .map_err(|e| Error::Backend(format!("open rev_edges: {e}")))?;
            // collect first — can't mutate a table while an
            // iterator borrows it
            let prefix = format!("{repo}\0");
            let dead: Vec<String> = {
                let iter = fs
                    .iter()
                    .map_err(|e| Error::Backend(format!("files iter: {e}")))?;
                let mut dead = Vec::new();
                for row in iter {
                    let (k, _) = row.map_err(|e| Error::Backend(format!("files iter: {e}")))?;
                    let Some(rel) = k.value().strip_prefix(&prefix) else {
                        continue;
                    };
                    if !live.contains(Utf8Path::new(rel)) {
                        dead.push(k.value().to_string());
                    }
                }
                dead
            };
            for key in dead {
                let old = read_file_edges(&fe, &key)?;
                drop_edges(repo, &old, &mut ed, &mut rv)?;
                fe.remove(key.as_str())
                    .map_err(|e| Error::Backend(format!("file_edges remove: {e}")))?;
                fs.remove(key.as_str())
                    .map_err(|e| Error::Backend(format!("files remove: {e}")))?;
                removed += 1;
            }
        }
        tx.commit()
            .map_err(|e| Error::Backend(format!("commit: {e}")))?;
        Ok(removed)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use myco_core::{Item, ItemId, ItemKind, Status};
    use std::collections::BTreeSet;

    fn idx() -> (tempfile::TempDir, RedbIndex) {
        let d = tempfile::tempdir().unwrap();
        let dir = Utf8PathBuf::from_path_buf(d.path().join("myco")).unwrap();
        (d, RedbIndex::open_at(dir).unwrap())
    }

    fn entry(n: &str) -> RepoEntry {
        RepoEntry::new(Utf8PathBuf::from(format!("/repos/{n}")), n)
    }

    fn snap(e: &RepoEntry, head: &str) -> RepoSnapshot {
        RepoSnapshot {
            path: e.path.clone(),
            name: e.name.clone(),
            head_commit: head.into(),
            scanned_at: "2026-02-09T00:00:00Z".into(),
            items: vec![Item::new(ItemId::new(ItemKind::Task, 1), "x").status(Status::InProgress)],
            archived: BTreeSet::new(),
        }
    }

    #[test]
    fn register_list_remove_cycle() {
        let (_d, i) = idx();
        assert!(i.repos().unwrap().is_empty());
        i.register(&entry("b")).unwrap();
        i.register(&entry("a")).unwrap();
        // re-register updates the name, doesn't duplicate
        i.register(&RepoEntry::new(Utf8PathBuf::from("/repos/b"), "b-renamed"))
            .unwrap();
        let repos = i.repos().unwrap();
        assert_eq!(repos.len(), 2);
        // sorted by path
        assert_eq!(repos[0].path.as_str(), "/repos/a");
        assert_eq!(repos[1].name, "b-renamed");
        // registry persists on disk as plain toml
        let src = std::fs::read_to_string(i.registry_path()).unwrap();
        assert!(src.contains("/repos/a"));
        // remove drops the entry + its snapshot
        i.save_snapshot(&snap(&repos[1], "h1")).unwrap();
        assert!(i.remove(&repos[1].path).unwrap());
        assert!(!i.remove(&repos[1].path).unwrap());
        assert_eq!(i.repos().unwrap().len(), 1);
        assert!(i.snapshot(&repos[1].path).unwrap().is_none());
    }

    #[test]
    fn snapshot_roundtrip_and_overwrite() {
        let (_d, i) = idx();
        let e = entry("x");
        assert!(i.snapshot(&e.path).unwrap().is_none());
        i.save_snapshot(&snap(&e, "h1")).unwrap();
        let got = i.snapshot(&e.path).unwrap().unwrap();
        assert_eq!(got.head_commit, "h1");
        assert_eq!(got.items.len(), 1);
        // overwrite — freshness check sees the new head
        i.save_snapshot(&snap(&e, "h2")).unwrap();
        assert_eq!(i.snapshot(&e.path).unwrap().unwrap().head_commit, "h2");
        // survives a reopen
        let i2 = RedbIndex::open_at(i.dir().to_path_buf()).unwrap();
        assert_eq!(i2.snapshot(&e.path).unwrap().unwrap().head_commit, "h2");
    }

    #[test]
    fn data_dir_env_override() {
        // MYCO_HOME wins over everything
        let d = tempfile::tempdir().unwrap();
        let ds = d.path().to_str().unwrap();
        temp_env::with_var("MYCO_HOME", Some(ds), || {
            assert_eq!(data_dir().unwrap(), d.path());
        });
        temp_env::with_vars(
            [("MYCO_HOME", None::<&str>), ("XDG_DATA_HOME", Some(ds))],
            || {
                assert_eq!(data_dir().unwrap(), d.path().join("myco"));
            },
        );
    }

    #[test]
    fn malformed_registry_is_codec_error() {
        let (_d, i) = idx();
        std::fs::write(i.registry_path(), "nope = [").unwrap();
        assert!(matches!(i.repos(), Err(Error::Codec(_))));
        assert!(matches!(i.register(&entry("x")), Err(Error::Codec(_))));
    }

    #[test]
    fn data_dir_falls_back_to_home_then_errors() {
        let d = tempfile::tempdir().unwrap();
        let home = d.path().to_str().unwrap();
        // no MYCO_HOME/XDG → ~/.local/share/myco
        temp_env::with_vars(
            [
                ("MYCO_HOME", None::<&str>),
                ("XDG_DATA_HOME", None::<&str>),
                ("HOME", Some(home)),
            ],
            || {
                assert_eq!(data_dir().unwrap(), d.path().join(".local/share/myco"));
            },
        );
        // HOME unset too → error, never a cwd-relative cache
        temp_env::with_vars(
            [
                ("MYCO_HOME", None::<&str>),
                ("XDG_DATA_HOME", None::<&str>),
                ("HOME", None::<&str>),
            ],
            || {
                let e = data_dir().unwrap_err();
                assert!(e.to_string().contains("HOME unset"), "{e}");
            },
        );
    }

    #[test]
    fn contended_open_retries_until_free() {
        let (_d, i) = idx();
        // a second "myco" holds the lock for a beat, then drops
        let holder = Database::create(i.db_path().as_std_path()).unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(80));
            drop(holder);
        });
        // write queues behind the holder instead of dying
        let e = entry("busy");
        i.save_snapshot(&snap(&e, "h1")).unwrap();
        release.join().unwrap();
        assert_eq!(i.snapshot(&e.path).unwrap().unwrap().head_commit, "h1");
    }

    #[test]
    fn contended_open_reports_index_busy() {
        let (_d, i) = idx();
        let _holder = Database::create(i.db_path().as_std_path()).unwrap();
        // test-scale budget — same path, no 2s wait
        let err = match open_db(&i.db_path(), Duration::from_millis(50)) {
            Ok(_) => panic!("expected busy error"),
            Err(e) => e,
        };
        match err {
            Error::Backend(msg) => assert!(msg.contains("index busy"), "got: {msg}"),
            e => panic!("expected backend error, got {e:?}"),
        }
    }

    #[test]
    fn concurrent_registers_dont_lose_entries() {
        let (_d, i) = idx();
        // N writers RMW the registry at once — the lockfile
        // serializes them, so every entry lands. Unlocked, the
        // load→mutate→save + shared tmp name lost most updates.
        let n = 8;
        std::thread::scope(|s| {
            for k in 0..n {
                let i = &i;
                s.spawn(move || i.register(&entry(&format!("r{k}"))).unwrap());
            }
        });
        assert_eq!(i.repos().unwrap().len(), n);
    }

    #[test]
    fn corrupt_snapshot_reads_as_missing() {
        let (_d, i) = idx();
        let e = entry("x");
        // poison the row directly — truncated/old-schema JSON
        {
            let db = Database::create(i.db_path().as_std_path()).unwrap();
            let tx = db.begin_write().unwrap();
            {
                let mut t = tx.open_table(SNAPSHOTS).unwrap();
                t.insert(e.path.as_str(), "{not json").unwrap();
            }
            tx.commit().unwrap();
        }
        // used to be Error::Codec bricking org/index list — now
        // "never scanned", so callers mark it stale and rebuild
        assert!(i.snapshot(&e.path).unwrap().is_none());
    }

    #[test]
    fn register_updates_name_and_worktree() {
        let (_d, i) = idx();
        i.register(&entry("x")).unwrap();
        // re-register refreshes the recorded worktree too — a repo
        // whose project dir moved must not keep the old path
        i.register(
            &RepoEntry::new(Utf8PathBuf::from("/repos/x"), "x2")
                .with_worktree(Utf8PathBuf::from("/elsewhere/pm")),
        )
        .unwrap();
        let repos = i.repos().unwrap();
        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0].name, "x2");
        assert_eq!(repos[0].worktree_path(), Utf8PathBuf::from("/elsewhere/pm"));
    }

    #[test]
    fn unreadable_registry_is_io_error() {
        let (_d, i) = idx();
        // repos.toml as a directory — exists but can't be read
        std::fs::create_dir(i.registry_path()).unwrap();
        assert!(matches!(i.repos(), Err(Error::Io(_))));
    }

    // --- link index (P-005) ----------------------------------------------

    fn rel(p: &str) -> Utf8PathBuf {
        Utf8PathBuf::from(p)
    }

    fn fstamp(n: u64) -> FileStamp {
        FileStamp {
            mtime_ms: n,
            size: n,
            fingerprint: n,
        }
    }

    fn tid(n: u32) -> ItemId {
        ItemId::new(ItemKind::Task, n)
    }

    fn iid(n: u32) -> ItemId {
        ItemId::new(ItemKind::Issue, n)
    }

    fn alive(ids: &[ItemId]) -> BTreeSet<ItemId> {
        ids.iter().copied().collect()
    }

    /// One record call with edges — the test-facing shorthand.
    fn rec(i: &RedbIndex, repo: &str, p: &str, edges: &[Edge]) {
        i.record(repo, &rel(p), &fstamp(1), edges).unwrap();
    }

    #[test]
    fn record_related_mirrors_and_filters() {
        let (_d, i) = idx();
        let repo = "/r";
        // T-1 blocks T-2; T-2 mentions I-3; T-1 has a handoff file
        rec(
            &i,
            repo,
            "TODO.md",
            &[
                Edge::new(tid(1), tid(2), EdgeKind::Blocks),
                Edge::new(tid(2), iid(3), EdgeKind::References),
                Edge::handoff(tid(1)),
            ],
        );
        let all = alive(&[tid(1), tid(2), iid(3)]);
        // outgoing verbatim, incoming mirrored
        assert_eq!(
            i.related(repo, tid(1), &all).unwrap(),
            [
                Related {
                    other: tid(1),
                    kind: EdgeKind::Handoff
                },
                Related {
                    other: tid(2),
                    kind: EdgeKind::Blocks
                },
            ]
        );
        assert_eq!(
            i.related(repo, tid(2), &all).unwrap(),
            [
                Related {
                    other: tid(1),
                    kind: EdgeKind::BlockedBy
                },
                Related {
                    other: iid(3),
                    kind: EdgeKind::References
                },
            ]
        );
        assert_eq!(
            i.related(repo, iid(3), &all).unwrap(),
            [Related {
                other: tid(2),
                kind: EdgeKind::ReferencedBy
            }]
        );
        // dangling ends are filtered at query time
        assert_eq!(i.related(repo, iid(3), &alive(&[])).unwrap(), []);
    }

    #[test]
    fn record_swap_is_atomic_and_exact() {
        let (_d, i) = idx();
        let repo = "/r";
        rec(
            &i,
            repo,
            "issues/issue-0001-a.md",
            &[Edge::new(iid(1), tid(1), EdgeKind::References)],
        );
        let all = alive(&[iid(1), tid(1), tid(2)]);
        assert_eq!(i.related(repo, iid(1), &all).unwrap().len(), 1);
        // re-record with a different set — the old edge must be
        // gone, not unioned (the swap is what file_edges proves)
        rec(
            &i,
            repo,
            "issues/issue-0001-a.md",
            &[Edge::new(iid(1), tid(2), EdgeKind::Blocks)],
        );
        assert_eq!(
            i.related(repo, iid(1), &all).unwrap(),
            [Related {
                other: tid(2),
                kind: EdgeKind::Blocks
            }]
        );
        // the reverse table moved too — no stale incoming edge
        assert_eq!(
            i.related(repo, tid(1), &all).unwrap(),
            [],
            "old edge must not survive the swap"
        );
        assert_eq!(
            i.related(repo, tid(2), &all).unwrap(),
            [Related {
                other: iid(1),
                kind: EdgeKind::BlockedBy
            }]
        );
    }

    #[test]
    fn stamps_dirty_restamp() {
        let (_d, i) = idx();
        let repo = "/r";
        rec(&i, repo, "a.md", &[]);
        rec(&i, repo, "b.md", &[]);
        let stored = i.stamps(repo).unwrap();
        assert_eq!(stored.len(), 2);
        // dirty diff: same stamp → clean; changed/new → dirty
        let mut live = BTreeMap::new();
        live.insert(rel("a.md"), fstamp(1));
        live.insert(rel("b.md"), fstamp(2));
        live.insert(rel("c.md"), fstamp(9));
        let mut dirty = i.dirty(repo, &live).unwrap();
        dirty.sort();
        assert_eq!(dirty, [rel("b.md"), rel("c.md")]);
        // restamp moves the stamp without touching edges
        i.restamp(repo, &rel("b.md"), &fstamp(7)).unwrap();
        assert_eq!(i.stamps(repo).unwrap()[&rel("b.md")], fstamp(7));
    }

    #[test]
    fn reach_terminates_on_cycles_and_caps_depth() {
        let (_d, i) = idx();
        let repo = "/r";
        // a cycle: 1→2→3→1, plus a tail 3→4
        rec(
            &i,
            repo,
            "TODO.md",
            &[
                Edge::new(tid(1), tid(2), EdgeKind::Blocks),
                Edge::new(tid(2), tid(3), EdgeKind::Blocks),
                Edge::new(tid(3), tid(1), EdgeKind::Blocks),
                Edge::new(tid(3), tid(4), EdgeKind::Blocks),
            ],
        );
        let got = i.reach(repo, tid(1), EdgeKind::Blocks, 32).unwrap();
        assert_eq!(got, [tid(2), tid(3), tid(4)]);
        // depth cap — 1 hop sees only the first neighbor
        assert_eq!(
            i.reach(repo, tid(1), EdgeKind::Blocks, 1).unwrap(),
            [tid(2)]
        );
        // starting inside the cycle still terminates; self excluded
        assert_eq!(
            i.reach(repo, tid(2), EdgeKind::Blocks, 32).unwrap(),
            [tid(1), tid(3), tid(4)]
        );
        // unknown kind / empty graph → empty
        assert_eq!(i.reach(repo, tid(1), EdgeKind::References, 32).unwrap(), []);
    }

    #[test]
    fn sweep_drops_vanished_files_and_their_edges() {
        let (_d, i) = idx();
        let repo = "/r";
        rec(
            &i,
            repo,
            "TODO.md",
            &[Edge::new(tid(1), tid(2), EdgeKind::Blocks)],
        );
        rec(
            &i,
            repo,
            "gone.md",
            &[Edge::new(iid(1), tid(1), EdgeKind::References)],
        );
        // keep only TODO.md live — gone.md sweeps out with its edges
        let n = i.sweep(repo, &BTreeSet::from([rel("TODO.md")])).unwrap();
        assert_eq!(n, 1);
        assert_eq!(i.stamps(repo).unwrap().len(), 1);
        let all = alive(&[tid(1), tid(2), iid(1)]);
        assert_eq!(
            i.related(repo, iid(1), &all).unwrap(),
            [],
            "swept file's edges are gone"
        );
        // …and the incoming end too
        assert_eq!(
            i.related(repo, tid(1), &all).unwrap(),
            [Related {
                other: tid(2),
                kind: EdgeKind::Blocks
            }]
        );
        // sweeping everything is a purge; remove() uses it
        assert_eq!(i.sweep(repo, &BTreeSet::new()).unwrap(), 1);
        assert!(i.stamps(repo).unwrap().is_empty());
    }

    #[test]
    fn edges_are_repo_scoped_and_remove_purges() {
        let (_d, i) = idx();
        let all = alive(&[tid(1), tid(2)]);
        let e = [Edge::new(tid(1), tid(2), EdgeKind::Blocks)];
        rec(&i, "/a", "TODO.md", &e);
        rec(&i, "/b", "TODO.md", &e);
        assert_eq!(i.related("/a", tid(1), &all).unwrap().len(), 1);
        assert_eq!(i.related("/b", tid(1), &all).unwrap().len(), 1);
        // index remove drops the repo's link records too
        i.register(&RepoEntry::new(Utf8PathBuf::from("/a"), "a"))
            .unwrap();
        assert!(i.remove(&Utf8PathBuf::from("/a")).unwrap());
        assert_eq!(i.related("/a", tid(1), &all).unwrap(), []);
        assert!(i.stamps("/a").unwrap().is_empty());
        // the other repo is untouched
        assert_eq!(i.related("/b", tid(1), &all).unwrap().len(), 1);
    }
}
