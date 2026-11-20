//! The workspace selector (P-016) — `[workspace] repos` glob
//! patterns in user-layer `myco.toml`, matched against universe
//! repo names (lasius clones ∪ registry). The workspace is a
//! *scope*, not a ledger: index-sourced views (`org-board`,
//! `org-status`) filter to the resolved set; single-ledger verbs
//! keep using `current`/discovery.
//!
//! Read/write policy mirrors [`crate::current`]: `MYCO_CONFIG`
//! may supply the table on reads (it wins), writes always land
//! in the canonical user file via `toml_edit`, and repo
//! `./myco.toml` is never a source.

use std::collections::BTreeSet;
use std::path::PathBuf;

use myco_index::Index;

use crate::config::ConfigLoader;
use crate::universe::{self, UniverseRepo};

/// The persisted `[workspace] repos` globs — `None` when
/// unconfigured or empty.
pub fn configured(cfg: ConfigLoader) -> Option<Vec<String>> {
    let path = crate::config::user_state_path()?;
    let c = cfg.load_file(&path).ok()?;
    c.workspace.map(|w| w.repos).filter(|r| !r.is_empty())
}

/// Resolve the configured globs to universe repos. Per-glob
/// misses warn; a set matching *nothing* is a hard error naming
/// the globs and the universe names tried (P-016).
pub fn resolve(
    cwd: &std::path::Path,
    index: &dyn Index,
    cfg: ConfigLoader,
) -> Result<(Vec<UniverseRepo>, Vec<String>), String> {
    let globs =
        configured(cfg).ok_or("no [workspace] configured — `myco workspace use <glob>…`")?;
    resolve_globs(cwd, index, cfg, &globs)
}

/// [`resolve`] with an explicit glob list — `workspace use`
/// validates the patterns it's about to persist.
pub fn resolve_globs(
    cwd: &std::path::Path,
    index: &dyn Index,
    cfg: ConfigLoader,
    globs: &[String],
) -> Result<(Vec<UniverseRepo>, Vec<String>), String> {
    let repos = universe::repos(cwd, index, cfg)?;
    let mut matched: Vec<UniverseRepo> = Vec::new();
    let mut seen = BTreeSet::new();
    let mut warnings = Vec::new();
    for g in globs {
        let pat =
            glob::Pattern::new(g).map_err(|e| format!("invalid workspace glob `{g}`: {e}"))?;
        let mut hits = 0;
        for u in &repos {
            if pat.matches(&u.entry.name) && seen.insert(u.entry.name.clone()) {
                matched.push(u.clone());
                hits += 1;
            }
        }
        if hits == 0 {
            warnings.push(format!("glob `{g}` matched no universe repo"));
        }
    }
    if matched.is_empty() {
        let names = repos
            .iter()
            .map(|u| u.entry.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!(
            "workspace globs matched nothing — globs: {}; universe: {}",
            globs.join(", "),
            names
        ));
    }
    matched.sort_by(|a, b| a.entry.name.cmp(&b.entry.name));
    Ok((matched, warnings))
}

/// `workspace use` — write `[workspace] repos` to the canonical
/// user-layer `myco.toml` via `toml_edit`.
pub fn write_config(globs: &[String]) -> Result<PathBuf, String> {
    let path = crate::config::user_config_write_path()
        .ok_or("no user config dir ($XDG_CONFIG_HOME or $HOME)".to_string())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let mut doc = text
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if doc.get("workspace").is_none() {
        doc["workspace"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    let ws = doc["workspace"]
        .as_table_mut()
        .ok_or("`workspace` in myco.toml is not a table")?;
    let mut arr = toml_edit::Array::new();
    for g in globs {
        arr.push(g.as_str());
    }
    ws["repos"] = toml_edit::Item::Value(toml_edit::Value::Array(arr));
    std::fs::write(&path, doc.to_string()).map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::config::test_loader;

    #[test]
    fn configured_reads_user_state() {
        let xdg = tempfile::tempdir().unwrap();
        temp_env::with_vars(
            [
                ("XDG_CONFIG_HOME", Some(xdg.path().to_str().unwrap())),
                ("HOME", None::<&str>),
                ("MYCO_CONFIG", None::<&str>),
            ],
            || {
                assert!(configured(test_loader()).is_none());
                let dir = xdg.path().join("myco");
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(
                    dir.join("myco.toml"),
                    "[workspace]\nrepos = [\"my*\", \"org\"]\n",
                )
                .unwrap();
                assert_eq!(
                    configured(test_loader()).unwrap(),
                    vec!["my*".to_string(), "org".to_string()]
                );
            },
        );
    }

    #[test]
    fn write_config_roundtrip() {
        let xdg = tempfile::tempdir().unwrap();
        temp_env::with_vars(
            [
                ("XDG_CONFIG_HOME", Some(xdg.path().to_str().unwrap())),
                ("HOME", None::<&str>),
                ("MYCO_CONFIG", None::<&str>),
            ],
            || {
                write_config(&["my*".into(), "org".into()]).unwrap();
                let p = xdg.path().join("myco/myco.toml");
                let text = std::fs::read_to_string(&p).unwrap();
                assert!(text.contains("repos = [\"my*\", \"org\"]"), "{text}");
                assert_eq!(
                    configured(test_loader()).unwrap(),
                    vec!["my*".to_string(), "org".to_string()]
                );
                // clear → empty repos reads as unconfigured
                write_config(&[]).unwrap();
                assert!(configured(test_loader()).is_none());
            },
        );
    }

    /// Glob resolution over a registry universe — matches by
    /// name, per-glob misses warn, an empty set hard-errors.
    #[test]
    fn resolve_globs_match_and_warn() {
        use myco_index::{Index, RepoEntry};
        use myco_index_redb::RedbIndex;
        use std::process::Command;

        let git = |d: &std::path::Path| {
            for a in [
                vec!["init", "-q"],
                vec!["config", "user.email", "t@t"],
                vec!["config", "user.name", "t"],
            ] {
                let st = Command::new("git")
                    .arg("-C")
                    .arg(d)
                    .args(&a)
                    .status()
                    .unwrap();
                assert!(st.success());
            }
        };
        let base = tempfile::tempdir().unwrap();
        let myrepo = base.path().join("myrepo");
        let orgrepo = base.path().join("orgrepo");
        for r in [&myrepo, &orgrepo] {
            std::fs::create_dir_all(r).unwrap();
            git(r);
        }
        let idx_dir = tempfile::tempdir().unwrap();
        let index = RedbIndex::open_at(
            camino::Utf8PathBuf::from_path_buf(idx_dir.path().join("idx")).unwrap(),
        )
        .unwrap();
        for (p, n) in [(&myrepo, "myrepo"), (&orgrepo, "orgrepo")] {
            index
                .register(&RepoEntry::new(
                    camino::Utf8PathBuf::from_path_buf(p.canonicalize().unwrap()).unwrap(),
                    n,
                ))
                .unwrap();
        }
        let outside = tempfile::tempdir().unwrap();

        temp_env::with_var("MYCO_CONFIG", None::<&str>, || {
            let globs = vec!["my*".to_string(), "zzz*".to_string()];
            let (repos, warnings) =
                resolve_globs(outside.path(), &index, test_loader(), &globs).unwrap();
            assert_eq!(repos.len(), 1);
            assert_eq!(repos[0].entry.name, "myrepo");
            assert_eq!(warnings.len(), 1);
            assert!(warnings[0].contains("zzz*"), "{warnings:?}");

            let empty = vec!["zzz*".to_string()];
            let e = resolve_globs(outside.path(), &index, test_loader(), &empty).unwrap_err();
            assert!(e.contains("matched nothing"), "{e}");
            assert!(e.contains("myrepo"), "{e}");
        });
    }
}
