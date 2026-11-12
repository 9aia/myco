//! Config plumbing — ops owns the *policy* (which file is the
//! repo layer, merged vs repo-only reads) **and** the TOML
//! wiring: parsing rides `scarab-config-toml` (P-017 — the
//! per-repo `myco-config`/`myco-config-toml` crates are gone),
//! while the two-layer merge stays here because its collision
//! rules are myco's, not scarab's generic fold. Only
//! `myco_core::Config` crosses the seam.

use std::path::{Path, PathBuf};

use scarab_config::{ConfigFormat, Discovery};
use scarab_config_file::FileSource;
use scarab_config_toml::TomlFormat;

/// The two config reads ops needs. `layered` runs the impl's
/// layered search — `Some(path)` pins the repo layer, `None`
/// its default probe (env → cwd → user layer); `file` reads
/// exactly one file with no search and no merge — `myco check`
/// lints the repo layer alone so findings are identical for
/// every user.
///
/// Plain fn pointers keep the loader `Copy` — front-ends bind
/// the [`TOML`] const, tests can inject fakes.
#[derive(Clone, Copy)]
pub struct ConfigLoader {
    /// Layered load — `Some(path)` pins the repo layer, `None`
    /// runs the impl's default search.
    pub layered: fn(Option<&Path>) -> Result<myco_core::Config, String>,
    /// Single-file load — no search, no merge.
    pub file: fn(&Path) -> Result<myco_core::Config, String>,
}

impl ConfigLoader {
    /// `(self.layered)(repo_path)` — reads better at call sites.
    pub fn load(&self, repo_path: Option<&Path>) -> Result<myco_core::Config, String> {
        (self.layered)(repo_path)
    }

    /// `(self.file)(path)` — reads better at call sites.
    pub fn load_file(&self, path: &Path) -> Result<myco_core::Config, String> {
        (self.file)(path)
    }
}

/// Parse exactly one config file — no layer search, no merge.
/// For callers that need a specific layer (e.g. `myco check`
/// linting only the repo's own myco.toml).
fn parse_file(path: &Path) -> Result<myco_core::Config, String> {
    let text =
        std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    ConfigFormat::<myco_core::Config>::parse(&scarab_config_toml::TomlFormat, &text)
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// The repo-layer probe — explicit arg → `$MYCO_CONFIG` →
/// `./myco.toml`, TOML only (myco's documented surface claims no
/// yaml). The *paths* are scarab's discovery (P-017); the
/// missing-layer-is-`None` rule stays here because it's merge
/// policy, not search policy.
fn repo_source() -> FileSource<myco_core::Config> {
    FileSource::with_formats(
        Discovery::bare("myco")
            .env_var("MYCO_CONFIG")
            .dirs([PathBuf::from(".")]),
        vec![Box::new(TomlFormat)],
    )
}

/// The pinned repo-layer path: an explicit `Some(path)` (the
/// caller already resolved `$MYCO_CONFIG` or `<root>/myco.toml`)
/// or the env var itself. An explicit path that doesn't exist
/// is an error naming it — not a silent skip. `None` explicit +
/// no env → `./myco.toml` when present.
fn repo_layer(explicit: Option<&Path>) -> Result<Option<PathBuf>, String> {
    let mut src = repo_source();
    src.explicit = explicit.map(|p| p.display().to_string());
    match src.locate_with(&|k| std::env::var(k).ok()) {
        Ok(p) => Ok(Some(p)),
        // discovery missed everywhere → no repo layer (the user
        // layer may still supply config); a *named* path that
        // misses stays an error.
        Err(e) if e.contains("no myco.") => Ok(None),
        Err(e) => Err(e),
    }
}

/// First existing path wins within the user layer — XDG dir
/// preferred over ~/.config. Same scarab probe, dirs swapped.
fn user_layer() -> Option<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        dirs.push(PathBuf::from(xdg).join("myco"));
    }
    if let Ok(home) = std::env::var("HOME") {
        dirs.push(PathBuf::from(home).join(".config/myco"));
    }
    FileSource::<myco_core::Config>::with_formats(
        Discovery::bare("myco").dirs(dirs),
        vec![Box::new(TomlFormat)],
    )
    .locate_with(&|_| None)
    .ok()
}

/// The two-layer merge — scalars keep first-match semantics (the
/// repo layer wins over the user layer). `[views]` and `[verbs]`
/// are the *merged* sections (P-006, P-007): both layers
/// contribute entries. Views let the user layer win name
/// collisions (personal presets reach through); verbs let the
/// repo layer win — repo verbs are shared surface, personal ones
/// can't override them. Builtins win over both at resolution.
fn merge(
    repo_cfg: Option<myco_core::Config>,
    user_cfg: Option<myco_core::Config>,
) -> myco_core::Config {
    let mut merged = myco_core::Config {
        project_dir: repo_cfg
            .as_ref()
            .and_then(|c| c.project_dir.clone())
            .or_else(|| user_cfg.as_ref().and_then(|c| c.project_dir.clone())),
        default_view: repo_cfg
            .as_ref()
            .and_then(|c| c.default_view.clone())
            .or_else(|| user_cfg.as_ref().and_then(|c| c.default_view.clone())),
        // `[sync]` merges like the scalars — repo layer wins
        // the whole table (today it holds one key)
        sync: repo_cfg
            .as_ref()
            .and_then(|c| c.sync.clone())
            .or_else(|| user_cfg.as_ref().and_then(|c| c.sync.clone())),
        // `[index]` merges like the scalars — repo layer wins the
        // whole table (the universe reads the repo file directly
        // anyway, so the user layer can't designate globally)
        index: repo_cfg
            .as_ref()
            .and_then(|c| c.index.clone())
            .or_else(|| user_cfg.as_ref().and_then(|c| c.index.clone())),
        ..Default::default()
    };
    for cfg in [&repo_cfg, &user_cfg].into_iter().flatten() {
        merged.views.extend(cfg.views.clone());
    }
    for cfg in [&user_cfg, &repo_cfg].into_iter().flatten() {
        merged.verbs.extend(cfg.verbs.clone());
    }
    // kinds: merged like verbs — user entries first, repo
    // shadows them by name at table construction (P-008)
    for cfg in [&user_cfg, &repo_cfg].into_iter().flatten() {
        merged.kinds.extend(cfg.kinds.clone());
    }
    merged
}

/// The layered load — `Some(path)` pins the repo layer (the
/// user layer still merges over it), `None` probes
/// `$MYCO_CONFIG` → `./myco.toml` → the user layer.
fn layered(repo_path: Option<&Path>) -> Result<myco_core::Config, String> {
    let repo_layer = repo_layer(repo_path)?;
    let user_path = user_layer();
    if repo_layer.is_none() && user_path.is_none() {
        return Err(
            "no myco.toml found (search: $MYCO_CONFIG, ., $XDG_CONFIG_HOME/myco/, ~/.config/myco/)"
                .into(),
        );
    }
    let repo_cfg = repo_layer.as_deref().map(parse_file).transpose()?;
    let user_cfg = user_path.as_deref().map(parse_file).transpose()?;
    Ok(merge(repo_cfg, user_cfg))
}

/// The single-file load — `myco check`'s repo-only lint read.
fn file(path: &Path) -> Result<myco_core::Config, String> {
    parse_file(path)
}

/// The TOML loader every front-end binds — the scarab-backed
/// replacement for the retired `myco-config-toml` crate.
pub const TOML: ConfigLoader = ConfigLoader { layered, file };

/// The layer's `default_view` — same path policy as
/// [`kinds_for`]: repo `myco.toml` (or `$MYCO_CONFIG`) if present,
/// the impl's default probe (user layer) otherwise. Missing
/// config, not error.
pub fn default_view_for(root: &Path, cfg: ConfigLoader) -> Option<String> {
    let path = std::env::var("MYCO_CONFIG")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| root.join("myco.toml"));
    cfg.load(path.exists().then_some(&path))
        .ok()
        .and_then(|c| c.default_view)
}

/// `sync.auto_fetch` for the repo rooted at `root` — same path
/// policy as [`kinds_for`]. Missing/broken config → `true` (the
/// documented default); the fetch itself is best-effort either
/// way, so a config read failure can't strand a mutation.
pub fn sync_auto_fetch_for(root: &Path, cfg: ConfigLoader) -> bool {
    let path = std::env::var("MYCO_CONFIG")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| root.join("myco.toml"));
    cfg.load(Some(&path))
        .ok()
        .and_then(|c| c.sync)
        .map(|s| s.auto_fetch)
        .unwrap_or(true)
}

/// The kind table for the repo rooted at `root` — the repo-layer
/// `myco.toml` (or `$MYCO_CONFIG`), same path policy as
/// `discover`'s `project_dir` read. The prelude on absence *or*
/// error — a broken config already fails the verbs that read it
/// strictly, and `check` lints `[[kinds]]` on its own.
pub fn kinds_for(root: &Path, cfg: ConfigLoader) -> myco_core::KindTable {
    let path = std::env::var("MYCO_CONFIG")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| root.join("myco.toml"));
    cfg.load(Some(&path))
        .ok()
        .and_then(|c| c.kind_table().ok())
        .unwrap_or_else(myco_core::KindTable::prelude)
}

/// The real TOML impl for tests — most ops tests exercise actual
/// `myco.toml` files (config-driven `project_dir`, `[views]`).
#[cfg(test)]
pub(crate) fn test_loader() -> ConfigLoader {
    TOML
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn explicit_missing_file_errors() {
        let err = layered(Some(Path::new("/nonexistent/myco.toml"))).unwrap_err();
        assert!(err.contains("myco.toml"));
    }

    #[test]
    fn parses_project_dir() {
        let dir = std::env::temp_dir().join(format!("myco-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap_or_default();
        let f = dir.join("myco.toml");
        std::fs::write(&f, "project_dir = \"project\"\n").unwrap_or_default();
        let cfg = file(&f).unwrap_or_default();
        assert_eq!(cfg.project_dir.as_deref(), Some("project"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn default_search_builds_candidates() {
        // no explicit path, no MYCO_CONFIG → ./myco.toml +
        // ~/.config/myco/myco.toml are probed (and miss in a
        // bare cwd)
        temp_env::with_vars(
            [
                ("MYCO_CONFIG", None::<&str>),
                ("XDG_CONFIG_HOME", None::<&str>),
                ("HOME", Some("/nonexistent-home")),
            ],
            || {
                let dir =
                    std::env::temp_dir().join(format!("myco-cfg-none-{}", std::process::id()));
                std::fs::create_dir_all(&dir).unwrap();
                let cwd = std::env::current_dir().unwrap();
                std::env::set_current_dir(&dir).unwrap();
                // no myco.toml anywhere reachable → search error
                let first = layered(None);
                assert!(first.is_err(), "expected search error: {first:?}");
                // ./myco.toml is found on the default search
                std::fs::write(dir.join("myco.toml"), "project_dir = \"proj\"\n").unwrap();
                let out = layered(None).unwrap();
                std::env::set_current_dir(cwd).unwrap();
                assert_eq!(out.project_dir.as_deref(), Some("proj"));
                let _ = std::fs::remove_dir_all(&dir);
            },
        );
    }

    #[test]
    fn verbs_parse_and_merge_with_repo_winning() {
        let base = std::env::temp_dir().join(format!("myco-cfg-verbs-{}", std::process::id()));
        let repo_dir = base.join("repo");
        let home = base.join("home");
        std::fs::create_dir_all(&repo_dir).unwrap();
        std::fs::create_dir_all(home.join(".config/myco")).unwrap();
        std::fs::write(
            repo_dir.join("myco.toml"),
            "[verbs.standup]\ntarget = \"view\"\nargs = [\"board\"]\n\n[verbs.hot]\nemit = \"ids\"\nfilter = { pri_min = \"high\" }\n\n[verbs.shared]\ntarget = \"status\"\n",
        )
        .unwrap();
        std::fs::write(
            home.join(".config/myco/myco.toml"),
            "[verbs.shared]\ntarget = \"report\"\n\n[verbs.user_only]\ntarget = \"view\"\nargs = [\"open\"]\n",
        )
        .unwrap();
        temp_env::with_vars(
            [
                ("MYCO_CONFIG", None::<&str>),
                ("XDG_CONFIG_HOME", None::<&str>),
                ("HOME", Some(home.to_string_lossy().as_ref())),
            ],
            || {
                let cwd = std::env::current_dir().unwrap();
                std::env::set_current_dir(&repo_dir).unwrap();
                let cfg = layered(None).unwrap();
                std::env::set_current_dir(cwd).unwrap();
                assert_eq!(cfg.verbs.len(), 4);
                // repo layer wins the `shared` collision
                match &cfg.verbs["shared"] {
                    myco_core::ConfigVerb::Alias(a) => assert_eq!(a.target, "status"),
                    _ => panic!("shared should be an alias"),
                }
                match &cfg.verbs["hot"] {
                    myco_core::ConfigVerb::ViewEmit(v) => {
                        assert_eq!(v.emit, myco_core::EmitMode::Ids);
                        assert_eq!(v.spec.filter.pri_min, Some(myco_core::Priority::High));
                    }
                    _ => panic!("hot should be view-emit"),
                }
                assert!(cfg.verbs.contains_key("user_only"));
            },
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn verbs_reject_mixed_kinds_and_unknown_fields() {
        // `target` + `emit` in one entry matches neither kind;
        // an unknown key on a view-emit fails loudly too
        for bad in [
            "[verbs.x]\ntarget = \"view\"\nemit = \"ids\"\n",
            "[verbs.x]\nemit = \"ids\"\nnonsense = 1\n",
        ] {
            let dir = std::env::temp_dir().join(format!(
                "myco-cfg-verbsbad-{}-{}",
                std::process::id(),
                bad.len()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            let f = dir.join("myco.toml");
            std::fs::write(&f, bad).unwrap();
            assert!(file(&f).is_err(), "{bad}");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn default_view_is_a_scalar_repo_wins() {
        let base = std::env::temp_dir().join(format!("myco-cfg-dv-{}", std::process::id()));
        let repo_dir = base.join("repo");
        let home = base.join("home");
        std::fs::create_dir_all(&repo_dir).unwrap();
        std::fs::create_dir_all(home.join(".config/myco")).unwrap();
        std::fs::write(
            home.join(".config/myco/myco.toml"),
            "default_view = \"org-status\"\n",
        )
        .unwrap();
        temp_env::with_vars(
            [
                ("MYCO_CONFIG", None::<&str>),
                ("XDG_CONFIG_HOME", None::<&str>),
                ("HOME", Some(home.to_string_lossy().as_ref())),
            ],
            || {
                let cwd = std::env::current_dir().unwrap();
                std::env::set_current_dir(&repo_dir).unwrap();
                // user layer alone supplies it
                let cfg = layered(None).unwrap();
                assert_eq!(cfg.default_view.as_deref(), Some("org-status"));
                // the repo layer wins once it names one
                std::fs::write(repo_dir.join("myco.toml"), "default_view = \"board\"\n").unwrap();
                let cfg = layered(None).unwrap();
                std::env::set_current_dir(cwd).unwrap();
                assert_eq!(cfg.default_view.as_deref(), Some("board"));
            },
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn views_merge_with_user_winning() {
        let base = std::env::temp_dir().join(format!("myco-cfg-merge-{}", std::process::id()));
        let repo_dir = base.join("repo");
        let home = base.join("home");
        std::fs::create_dir_all(&repo_dir).unwrap();
        std::fs::create_dir_all(home.join(".config/myco")).unwrap();
        std::fs::write(
            repo_dir.join("myco.toml"),
            "[views.shared]\nfilter = { kind = [\"issue\"] }\n[views.repo_only]\nfilter = { kind = [\"task\"] }\n",
        )
        .unwrap();
        std::fs::write(
            home.join(".config/myco/myco.toml"),
            "[views.shared]\nfilter = { kind = [\"task\"] }\n[views.user_only]\nfilter = { kind = [\"plan\"] }\n",
        )
        .unwrap();
        temp_env::with_vars(
            [
                ("MYCO_CONFIG", None::<&str>),
                ("XDG_CONFIG_HOME", None::<&str>),
                ("HOME", Some(home.to_string_lossy().as_ref())),
            ],
            || {
                let cwd = std::env::current_dir().unwrap();
                std::env::set_current_dir(&repo_dir).unwrap();
                let cfg = layered(None).unwrap();
                std::env::set_current_dir(cwd).unwrap();
                assert_eq!(cfg.views.len(), 3);
                // user layer wins the collision
                assert_eq!(cfg.views["shared"].filter.kind, vec!["task".to_string()]);
                assert!(cfg.views.contains_key("repo_only"));
                assert!(cfg.views.contains_key("user_only"));
            },
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn default_view_for_reads_the_repo_layer() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("myco.toml"), "default_view = \"board\"\n").unwrap();
        temp_env::with_var("MYCO_CONFIG", None::<&str>, || {
            assert_eq!(
                default_view_for(dir.path(), test_loader()).as_deref(),
                Some("board")
            );
        });
    }

    #[test]
    fn default_view_for_missing_config_is_none() {
        let dir = tempfile::tempdir().unwrap();
        // $MYCO_CONFIG naming a missing file → the impl's default
        // probe also misses → None, not an error
        temp_env::with_var("MYCO_CONFIG", Some("/nonexistent/myco.toml"), || {
            assert_eq!(default_view_for(dir.path(), test_loader()), None);
        });
    }
}
