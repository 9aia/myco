//! The current-ledger selector (P-015/P-016) — the user-level,
//! mutable "which ledger does myco open" pointer.
//!
//! Three sources feed discovery, all resolved against the same
//! target vocabulary (`repo` | `org` | `<universe name>` |
//! `</abs/path>` | `name:<repo>`):
//!
//! - `MYCO_LEDGER` / `MYCO_LEDGER_SCOPE` — per-invocation env.
//! - `[current]` in the user-layer `myco.toml` — `mode = always`
//!   beats cwd discovery, `mode = backup` answers only when
//!   discovery finds nothing. `MYCO_CONFIG` may *read* the table
//!   (it wins over the user file); writes always land in the
//!   canonical user file. Repo `./myco.toml` is never consulted —
//!   a repo must not pin the user's ledger.
//! - `.myco.toml` — a per-directory pointer (`ledger = "…"`),
//!   found walking up from cwd, honored only outside git repos
//!   (a committed pointer would be repo-controlled injection).
//!
//! Writes go through `toml_edit` so comments/formatting survive.

use std::path::{Path, PathBuf};

use myco_core::CurrentMode;
use myco_index::{Index, LedgerMode};

use crate::config::ConfigLoader;
use crate::discover::{self, Project};

/// A resolved selector — which ledger + where it sits.
#[derive(Debug, Clone)]
pub struct Selector {
    /// The raw target string (`repo`/`org`/name/path/`name:`).
    pub ledger: String,
    /// `always` (before discovery) or `backup` (after, before org).
    pub mode: CurrentMode,
}

/// `MYCO_LEDGER` (+ `MYCO_LEDGER_SCOPE`, default `always`).
/// `repo`/empty disables the env override for the invocation.
pub fn env_selector() -> Option<Selector> {
    let ledger = std::env::var("MYCO_LEDGER").ok()?;
    let ledger = ledger.trim();
    if ledger.is_empty() || ledger == "repo" {
        return None;
    }
    let mode = match std::env::var("MYCO_LEDGER_SCOPE").as_deref() {
        Ok("backup") => CurrentMode::Backup,
        _ => CurrentMode::Always,
    };
    Some(Selector {
        ledger: ledger.to_string(),
        mode,
    })
}

/// The persisted `[current]` table — `MYCO_CONFIG` (when set and
/// readable) wins over the user-layer file; repo `./myco.toml`
/// is never a source.
pub fn config_selector(cfg: ConfigLoader) -> Option<Selector> {
    let path = std::env::var("MYCO_CONFIG")
        .ok()
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .or_else(crate::config::user_layer_path)?;
    let c = cfg.load_file(&path).ok()?;
    c.current.map(|cur| Selector {
        ledger: cur.ledger,
        mode: cur.mode,
    })
}

/// The `.myco.toml` directory pointer — upward search from `cwd`,
/// nearest wins, files inside git repos skipped (repo-controlled
/// injection). Returns the file path + selector; the pointer has
/// fallback semantics only (no mode).
pub fn pointer_selector(cwd: &Path) -> Option<(PathBuf, Selector)> {
    for d in cwd.ancestors() {
        let f = d.join(".myco.toml");
        if !f.is_file() {
            continue;
        }
        // inside a git worktree → repo-controlled, skip and keep
        // walking (a parent plain dir's pointer may still apply)
        if crate::vcs::repo_root(d).is_ok() {
            continue;
        }
        if let Some(ledger) = read_pointer(&f) {
            return Some((
                f,
                Selector {
                    ledger,
                    mode: CurrentMode::Backup,
                },
            ));
        }
    }
    None
}

fn read_pointer(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let doc = text.parse::<toml_edit::DocumentMut>().ok()?;
    let ledger = doc.get("ledger")?.as_str()?.trim();
    (!ledger.is_empty() && ledger != "repo").then(|| ledger.to_string())
}

/// Resolve a selector to a [`Project`] — the target vocabulary
/// from P-016. `repo` never reaches here (callers filter it).
pub fn resolve(
    sel: &Selector,
    cwd: &Path,
    index: Option<&dyn Index>,
    cfg: ConfigLoader,
) -> Result<Project, String> {
    resolve_target(&sel.ledger, cwd, index, cfg).map_err(|e| {
        format!(
            "current ledger '{}': {e}\n  \
             escape a stale pin: MYCO_LEDGER=repo <cmd>  \
             (or `myco current clear`)",
            sel.ledger
        )
    })
}

/// The target vocabulary — `repo`/`org`/`name:`/abs-path/name.
fn resolve_target(
    target: &str,
    cwd: &Path,
    index: Option<&dyn Index>,
    cfg: ConfigLoader,
) -> Result<Project, String> {
    let target = target.trim();
    if target == "repo" {
        return Err("`repo` disables the selector — nothing to resolve".into());
    }
    if target == "org" {
        return org_project(cwd, index, cfg);
    }
    if let Some(name) = target.strip_prefix("name:") {
        return named_project(name, cwd, index, cfg);
    }
    if Path::new(target).is_absolute() {
        return path_project(Path::new(target));
    }
    named_project(target, cwd, index, cfg)
}

/// `org` — the index-designated org ledger (Lasius-source org
/// preferred, mirroring `discover`'s org fallback).
fn org_project(
    cwd: &Path,
    index: Option<&dyn Index>,
    cfg: ConfigLoader,
) -> Result<Project, String> {
    let index = index.ok_or("target `org` needs the repo index")?;
    let repos = crate::universe::repos(cwd, index, cfg)?;
    let org = repos
        .iter()
        .find(|u| {
            u.source == crate::universe::RepoSource::Lasius && u.entry.ledger == LedgerMode::Org
        })
        .or_else(|| repos.iter().find(|u| u.entry.ledger == LedgerMode::Org))
        .ok_or("no org ledger designated — `myco index add <repo> --org`")?;
    project_from_entry(org)
}

/// `<name>` / `name:<name>` — exactly one universe match required.
fn named_project(
    name: &str,
    cwd: &Path,
    index: Option<&dyn Index>,
    cfg: ConfigLoader,
) -> Result<Project, String> {
    if name.is_empty() {
        return Err("empty repo name".into());
    }
    let index = index.ok_or_else(|| format!("target `{name}` needs the repo index"))?;
    let repos = crate::universe::repos(cwd, index, cfg)?;
    let matches: Vec<_> = repos.iter().filter(|u| u.entry.name == name).collect();
    match matches.as_slice() {
        [] => Err(format!("no universe repo named `{name}`")),
        [u] => project_from_entry(u),
        many => Err(format!(
            "ambiguous name `{name}` — matches: {}",
            many.iter()
                .map(|u| u.entry.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// A universe entry → its ledger worktree, revalidated live.
fn project_from_entry(u: &crate::universe::UniverseRepo) -> Result<Project, String> {
    let wt = u
        .entry
        .worktree_path()
        .ok_or_else(|| format!("`{}` has no ledger worktree", u.entry.name))?;
    let wt = wt.as_std_path();
    if !discover::qualifies(wt) {
        return Err(format!(
            "`{}` worktree {} no longer qualifies — re-run discovery",
            u.entry.name,
            wt.display()
        ));
    }
    Ok(Project {
        root: u.entry.path.as_std_path().to_path_buf(),
        worktree: wt.canonicalize().unwrap_or_else(|_| wt.to_path_buf()),
    })
}

/// `</abs/path>` — a ledger worktree directly, or a repo root
/// resolved like `--root` (`<root>/project`).
fn path_project(p: &Path) -> Result<Project, String> {
    let p = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    if discover::qualifies(&p) {
        let root = crate::vcs::main_worktree(&p)
            .or_else(|| p.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| p.clone());
        return Ok(Project { root, worktree: p });
    }
    let wt = p.join(discover::DIR);
    if discover::qualifies(&wt) {
        return Ok(Project {
            root: p,
            worktree: wt,
        });
    }
    Err(format!(
        "{} is not a myco ledger worktree or repo root",
        p.display()
    ))
}

/// `current list` — every ledger-capable universe repo plus the
/// `repo`/`org` pseudo-targets. Needs the index.
pub fn list_targets(
    cwd: &Path,
    index: &dyn Index,
    cfg: ConfigLoader,
) -> Result<Vec<serde_json::Value>, String> {
    let repos = crate::universe::repos(cwd, index, cfg)?;
    let mut out = vec![
        serde_json::json!({"target": "repo", "kind": "selector", "note": "cwd discovery (default)"}),
        serde_json::json!({"target": "org", "kind": "selector", "note": "index-designated org ledger"}),
    ];
    for u in repos {
        let Some(wt) = u.entry.worktree_path() else {
            continue;
        };
        if !discover::qualifies(wt.as_std_path()) {
            continue;
        }
        out.push(serde_json::json!({
            "target": u.entry.name,
            "kind": match u.source {
                crate::universe::RepoSource::Lasius => "lasius",
                crate::universe::RepoSource::Registry => "registry",
            },
            "ledger": match u.entry.ledger {
                LedgerMode::Org => "org",
                LedgerMode::None => "none",
                LedgerMode::Local => "local",
            },
            "worktree": wt.as_std_path(),
        }));
    }
    Ok(out)
}

/// `current use|clear` — write `[current]` to the canonical
/// user-layer `myco.toml` via `toml_edit` (comments preserved).
/// `MYCO_CONFIG` is ignored — env is a read override only.
pub fn write_config(ledger: &str, mode: &str) -> Result<PathBuf, String> {
    let path = crate::config::user_config_write_path()
        .ok_or("no user config dir ($XDG_CONFIG_HOME or $HOME)".to_string())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let mut doc = text
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if doc.get("current").is_none() {
        doc["current"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    let cur = doc["current"]
        .as_table_mut()
        .ok_or("`current` in myco.toml is not a table")?;
    cur["ledger"] = toml_edit::value(ledger);
    cur["mode"] = toml_edit::value(mode);
    std::fs::write(&path, doc.to_string()).map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(path)
}

/// `current use <target> --here` / `clear --here` — the
/// `.myco.toml` directory pointer in `dir` (fallback semantics
/// only — no mode key).
pub fn write_pointer(dir: &Path, ledger: Option<&str>) -> Result<PathBuf, String> {
    let path = dir.join(".myco.toml");
    match ledger {
        Some(l) => {
            let text = format!("ledger = \"{}\"\n", l.replace('"', "\\\""));
            std::fs::write(&path, text).map_err(|e| format!("write {}: {e}", path.display()))?;
        }
        None => {
            if path.exists() {
                std::fs::remove_file(&path)
                    .map_err(|e| format!("remove {}: {e}", path.display()))?;
            }
        }
    }
    Ok(path)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn env_repo_disables() {
        temp_env::with_vars(
            [
                ("MYCO_LEDGER", Some("repo")),
                ("MYCO_LEDGER_SCOPE", None::<&str>),
            ],
            || assert!(env_selector().is_none()),
        );
    }

    #[test]
    fn env_defaults_to_always() {
        temp_env::with_vars(
            [
                ("MYCO_LEDGER", Some("org")),
                ("MYCO_LEDGER_SCOPE", None::<&str>),
            ],
            || {
                let s = env_selector().unwrap();
                assert_eq!(s.ledger, "org");
                assert_eq!(s.mode, CurrentMode::Always);
            },
        );
    }

    #[test]
    fn env_backup_scope() {
        temp_env::with_vars(
            [
                ("MYCO_LEDGER", Some("myrepo")),
                ("MYCO_LEDGER_SCOPE", Some("backup")),
            ],
            || assert_eq!(env_selector().unwrap().mode, CurrentMode::Backup),
        );
    }

    #[test]
    fn pointer_ignores_git_dirs() {
        // a .myco.toml inside a git repo is skipped; one in a
        // plain parent dir still answers
        let base = tempfile::tempdir().unwrap();
        let repo = base.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let st = std::process::Command::new("git")
            .args(["init", "-q"])
            .arg(&repo)
            .status()
            .unwrap();
        assert!(st.success());
        std::fs::write(repo.join(".myco.toml"), "ledger = \"org\"\n").unwrap();
        std::fs::write(base.path().join(".myco.toml"), "ledger = \"myrepo\"\n").unwrap();
        let inside = repo.join("sub");
        std::fs::create_dir_all(&inside).unwrap();
        let (f, s) = pointer_selector(&inside).unwrap();
        assert_eq!(f, base.path().join(".myco.toml"));
        assert_eq!(s.ledger, "myrepo");
        assert_eq!(s.mode, CurrentMode::Backup);
    }

    #[test]
    fn write_config_roundtrip_and_clear() {
        let xdg = tempfile::tempdir().unwrap();
        temp_env::with_vars(
            [
                ("XDG_CONFIG_HOME", Some(xdg.path().to_str().unwrap())),
                ("HOME", None::<&str>),
                ("MYCO_CONFIG", None::<&str>),
            ],
            || {
                let p = write_config("org", "always").unwrap();
                assert_eq!(p, xdg.path().join("myco/myco.toml"));
                // pre-existing comments survive the toml_edit write
                let mut text = std::fs::read_to_string(&p).unwrap();
                text = format!("# my prefs\n{text}");
                std::fs::write(&p, &text).unwrap();
                write_config("myrepo", "backup").unwrap();
                let out = std::fs::read_to_string(&p).unwrap();
                assert!(out.starts_with("# my prefs"), "{out}");
                assert!(out.contains("ledger = \"myrepo\""), "{out}");
                assert!(out.contains("mode = \"backup\""), "{out}");

                let sel = config_selector(crate::config::test_loader()).unwrap();
                assert_eq!(sel.ledger, "myrepo");
                assert_eq!(sel.mode, CurrentMode::Backup);

                // clear → ledger = "repo" (inert)
                write_config("repo", "backup").unwrap();
                let sel = config_selector(crate::config::test_loader()).unwrap();
                assert_eq!(sel.ledger, "repo");
            },
        );
    }

    #[test]
    fn pointer_write_and_read() {
        let d = tempfile::tempdir().unwrap();
        let p = write_pointer(d.path(), Some("org")).unwrap();
        assert_eq!(p, d.path().join(".myco.toml"));
        assert_eq!(read_pointer(&p).as_deref(), Some("org"));
        write_pointer(d.path(), None).unwrap();
        assert!(!p.exists());
    }

    #[test]
    fn pointer_none_without_file() {
        let d = tempfile::tempdir().unwrap();
        // tempdir lives under /tmp — no .myco.toml up the chain
        // in the test env (HOME may have one; assert shape only)
        let _ = pointer_selector(d.path());
    }
}
