//! Named views — built-in presets plus `[views]` from merged
//! config layers (P-006) and `views/*.toml` file-per-view
//! directories. Resolution is I/O (config files), so it lives
//! here; evaluation is pure and stays in `myco_core::view`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use myco_core::{GroupAxis, Status, ViewFilter, ViewFormat, ViewSource, ViewSpec};
use scarab_config::ConfigFormat;

use crate::config::ConfigLoader;

/// Where a named view came from — later origins shadow earlier
/// ones on a name collision (builtin < `views/` < `[views]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// A preset compiled into the binary (`board`, `epic`, …).
    Builtin,
    /// A `views/<name>.toml` file — repo root or the ledger
    /// worktree (the project branch carries its own views).
    Dir,
    /// A `[views.<name>]` entry from a config layer.
    Config,
}

/// A resolved named view.
#[derive(Debug, Clone)]
pub struct NamedView {
    /// The `[views.<name>]` / preset name.
    pub name: String,
    /// The spec (already merged — a config view fully replaces a
    /// same-named builtin).
    pub spec: ViewSpec,
    /// Builtin preset or config-defined.
    pub origin: Origin,
}

/// The built-in presets — the read verbs' shapes as data. Default
/// `sort` (empty) means the engine's `pri,id`.
pub fn builtins() -> Vec<NamedView> {
    let live = ViewFilter {
        status: vec![
            Status::Open,
            Status::Claimed,
            Status::InProgress,
            Status::Blocked,
            Status::Deferred,
        ],
        ..ViewFilter::default()
    };
    let spec = |filter: ViewFilter, group_by, format| ViewSpec {
        filter,
        group_by,
        sort: vec![],
        limit: None,
        format,
        ..ViewSpec::default()
    };
    let org = |mut s: ViewSpec| {
        s.source = ViewSource::Index;
        s
    };
    vec![
        NamedView {
            name: "board".into(),
            spec: spec(live.clone(), Some(GroupAxis::Status), ViewFormat::Board),
            origin: Origin::Builtin,
        },
        NamedView {
            name: "open".into(),
            spec: spec(
                ViewFilter {
                    status: vec![Status::Open],
                    ..ViewFilter::default()
                },
                None,
                ViewFormat::List,
            ),
            origin: Origin::Builtin,
        },
        NamedView {
            name: "epic".into(),
            spec: spec(live.clone(), Some(GroupAxis::Epic), ViewFormat::Board),
            origin: Origin::Builtin,
        },
        NamedView {
            name: "milestone".into(),
            spec: spec(live.clone(), Some(GroupAxis::Milestone), ViewFormat::Board),
            origin: Origin::Builtin,
        },
        NamedView {
            name: "next".into(),
            spec: ViewSpec {
                filter: ViewFilter {
                    kind: vec!["task".into(), "issue".into()],
                    status: vec![Status::Open],
                    ..ViewFilter::default()
                },
                group_by: None,
                sort: vec![],
                limit: Some(1),
                format: ViewFormat::List,
                ..ViewSpec::default()
            },
            origin: Origin::Builtin,
        },
        // the analytics emits as named presets (P-012) — same
        // shapes as `format = "timeline"|"stats"|"burnup"`; stats
        // and burnup stay permissive because the emits read the
        // archive on purpose
        NamedView {
            name: "roadmap".into(),
            spec: spec(live.clone(), None, ViewFormat::Timeline),
            origin: Origin::Builtin,
        },
        NamedView {
            name: "stats".into(),
            spec: spec(ViewFilter::default(), None, ViewFormat::Stats),
            origin: Origin::Builtin,
        },
        NamedView {
            name: "burnup".into(),
            spec: spec(ViewFilter::default(), None, ViewFormat::Burnup),
            origin: Origin::Builtin,
        },
        NamedView {
            name: "org-board".into(),
            // the index-sourced kanban — `myco org board`'s heir:
            // lane statuses only (done has no column), cards
            // prefixed `repo:ID` from each item's tag
            spec: org(spec(
                live.clone(),
                Some(GroupAxis::Status),
                ViewFormat::Board,
            )),
            origin: Origin::Builtin,
        },
        NamedView {
            name: "org-status".into(),
            // per-repo grouped list — the `org status` digest's
            // substitute: one bucket per registered repo
            spec: org(spec(
                ViewFilter::default(),
                Some(GroupAxis::Repo),
                ViewFormat::List,
            )),
            origin: Origin::Builtin,
        },
    ]
}

/// `[views]` for a repo — the repo layer is `<root>/myco.toml` when
/// it exists (else the impl's default probe), merged with the user
/// layer by the injected `cfg`. "No config anywhere" is empty, not
/// an error — builtins still resolve (the impl signals a
/// search-miss by error text). `strict`: a config path that names
/// a missing file errors (an explicit `$MYCO_CONFIG` is "the user
/// pointed at it" — `myco check` reports the miss); lenient
/// callers degrade to the presets — a read shouldn't brick on a
/// config-file absence.
fn config_views(
    root: &Path,
    strict: bool,
    cfg: ConfigLoader,
) -> Result<BTreeMap<String, ViewSpec>, String> {
    let repo_cfg = root.join("myco.toml");
    match cfg.load(repo_cfg.exists().then_some(repo_cfg.as_path())) {
        Ok(c) => Ok(c.views),
        // the documented search-miss — absence isn't a failure
        Err(e) if e.contains("no myco.toml found") => Ok(BTreeMap::new()),
        Err(e) if !strict && e.contains("not found") => Ok(BTreeMap::new()),
        Err(e) => Err(e),
    }
}

/// The warning a config-vs-builtin name collision earns.
fn shadow_warning(name: &str) -> String {
    format!("view \"{name}\" shadows the built-in preset")
}

/// Scan `<dir>/views/*.toml` — file-per-view: the filename stem
/// (minus `.toml`) names the view, the body is a bare
/// [`ViewSpec`] table (`filter` reads the inline table or a query
/// string, P-058). Sorted by path for a deterministic merge;
/// non-`.toml` entries are ignored. Every file yields a
/// `(name, result, path)` so callers pick warn-and-skip
/// (resolution) or lint (`myco check`).
fn scan_view_dir(dir: &Path) -> Vec<(String, Result<ViewSpec, String>, PathBuf)> {
    let mut files = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir.join("views")) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "toml") {
                files.push(p);
            }
        }
    }
    files.sort();
    files
        .into_iter()
        .filter_map(|p| {
            let name = p.file_stem()?.to_str()?.to_string();
            let spec = std::fs::read_to_string(&p)
                .map_err(|e| format!("{}: {e}", p.display()))
                .and_then(|text| {
                    ConfigFormat::<ViewSpec>::parse(&scarab_config_toml::TomlFormat, &text)
                        .map_err(|e| format!("{}: {e}", p.display()))
                })
                .and_then(|s| s.validate().map(|_| s));
            Some((name, spec, p))
        })
        .collect()
}

/// Name-scoped warnings — `(view name, message)`; `resolve`
/// surfaces just the ones matching the requested view.
type ViewWarnings = Vec<(String, String)>;

/// `(specs, warnings)` for one `views/` dir — unparseable or
/// invalid files warn on stderr's channel and drop out; a broken
/// view file must not brick every read.
fn dir_views(dir: &Path) -> (Vec<(String, ViewSpec)>, ViewWarnings) {
    let mut specs = Vec::new();
    let mut warnings = Vec::new();
    for (name, res, _path) in scan_view_dir(dir) {
        match res {
            Ok(spec) => specs.push((name, spec)),
            Err(e) => warnings.push((name, e)),
        }
    }
    (specs, warnings)
}

/// Insert-or-replace — a later source wins a name collision and
/// the warning names what got shadowed. Warnings are scoped to
/// the colliding name so `resolve` can surface just the relevant
/// ones.
fn upsert(
    out: &mut Vec<NamedView>,
    warnings: &mut ViewWarnings,
    name: String,
    spec: ViewSpec,
    origin: Origin,
) {
    if let Some(v) = out.iter_mut().find(|v| v.name == name) {
        match v.origin {
            Origin::Builtin => warnings.push((name.clone(), shadow_warning(&name))),
            Origin::Dir => warnings.push((
                name.clone(),
                format!("view \"{name}\" shadows a views/ file"),
            )),
            // config entries merge before they land here — a
            // config-over-config collision was settled inside
            // `config_views` (user layer wins, silently)
            Origin::Config => {}
        }
        v.spec = spec;
        v.origin = origin;
    } else {
        out.push(NamedView { name, spec, origin });
    }
}

/// Builtin presets merged with the repo's views — `views/*.toml`
/// files (repo root, then the ledger worktree so org-ledger
/// views travel with the `project` branch) under the merged
/// `[views]` config tables (repo layer then user layer — the
/// user still wins over everything). Name collisions resolve
/// later-wins and the returned warnings (name-scoped) name it.
/// Specs are *not* validated here — `myco check` lints them all.
fn merged_views(
    root: &Path,
    worktree: Option<&Path>,
    strict: bool,
    cfg: ConfigLoader,
) -> Result<(Vec<NamedView>, ViewWarnings), String> {
    let mut warnings = Vec::new();
    let mut out = builtins();
    // `views/` dirs — repo root first, then the ledger worktree
    // (skipped when it *is* the root); a later file wins a name
    // collision
    let dirs = std::iter::once(root).chain(worktree.filter(|w| *w != root));
    for dir in dirs {
        let (views, ws) = dir_views(dir);
        warnings.extend(ws);
        for (name, spec) in views {
            upsert(&mut out, &mut warnings, name, spec, Origin::Dir);
        }
    }
    for (name, spec) in config_views(root, strict, cfg)? {
        upsert(&mut out, &mut warnings, name, spec, Origin::Config);
    }
    Ok((out, warnings))
}

/// `merged_views` with strict config handling — `myco check`,
/// `myco serve`. Warnings flatten to plain strings.
pub fn named_views(
    root: &Path,
    worktree: Option<&Path>,
    cfg: ConfigLoader,
) -> Result<(Vec<NamedView>, Vec<String>), String> {
    let (views, warnings) = merged_views(root, worktree, true, cfg)?;
    Ok((views, warnings.into_iter().map(|(_, w)| w).collect()))
}

/// `merged_views` tolerant of a *missing* config — the `myco
/// view` read: `$MYCO_CONFIG` naming a missing file degrades to
/// the presets; an unparseable or invalid config still errors.
pub fn named_views_lenient(
    root: &Path,
    worktree: Option<&Path>,
    cfg: ConfigLoader,
) -> Result<(Vec<NamedView>, Vec<String>), String> {
    let (views, warnings) = merged_views(root, worktree, false, cfg)?;
    Ok((views, warnings.into_iter().map(|(_, w)| w).collect()))
}

/// Lint the repo-owned view sources — the repo-layer `myco.toml`
/// `[views]` plus both `views/` dirs — so `myco check` reports
/// the same findings for every user and CI. `Ok` = per-view
/// validation errors as `(file, name, error)` — `file` is the
/// report label (`myco.toml`, `views/hot.toml`, or
/// `<worktree>/views/hot.toml` for the ledger worktree); `Err` =
/// the myco.toml is unreadable/unparseable. Missing sources are
/// `Ok(vec![])`. Personal (user-layer) views are intentionally
/// not linted here.
pub fn validate_repo_views(
    root: &Path,
    worktree: Option<&Path>,
    loader: ConfigLoader,
) -> Result<Vec<(String, String, String)>, String> {
    let mut out = Vec::new();
    let path = root.join("myco.toml");
    if path.exists() {
        let cfg = loader.load_file(&path)?;
        out.extend(cfg.views.iter().filter_map(|(name, spec)| {
            spec.validate()
                .err()
                .map(|e| ("myco.toml".to_string(), name.clone(), e))
        }));
    }
    // both `views/` dirs — repo root labelled `views/…`, the
    // ledger worktree `<worktree-name>/views/…` so findings name
    // which side of the repo boundary the file lives on
    let mut dirs: Vec<(&Path, Option<String>)> = vec![(root, None)];
    if let Some(w) = worktree.filter(|w| *w != root) {
        let label = w
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "worktree".to_string());
        dirs.push((w, Some(label)));
    }
    for (dir, label) in dirs {
        for (name, res, path) in scan_view_dir(dir) {
            if let Err(e) = res {
                let rel = path
                    .strip_prefix(dir)
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|_| path.display().to_string());
                let file = match &label {
                    Some(wt) => format!("{wt}/{rel}"),
                    None => rel,
                };
                out.push((file, name, e));
            }
        }
    }
    Ok(out)
}

/// Resolve one view by name — later sources shadow earlier
/// (warns). Unknown names error listing the defined set. The
/// spec is validated before return. Warnings are scoped to the
/// resolved view — a `view board` lookup shouldn't hear about a
/// shadowed `views/epic.toml`.
pub fn resolve(
    root: &Path,
    worktree: Option<&Path>,
    name: &str,
    cfg: ConfigLoader,
) -> Result<(NamedView, Vec<String>), String> {
    resolve_in(root, worktree, name, true, cfg)
}

/// [`resolve`] tolerant of a *missing* config — the `myco view`
/// read.
pub fn resolve_lenient(
    root: &Path,
    worktree: Option<&Path>,
    name: &str,
    cfg: ConfigLoader,
) -> Result<(NamedView, Vec<String>), String> {
    resolve_in(root, worktree, name, false, cfg)
}

fn resolve_in(
    root: &Path,
    worktree: Option<&Path>,
    name: &str,
    strict: bool,
    cfg: ConfigLoader,
) -> Result<(NamedView, Vec<String>), String> {
    let (views, scoped_warnings) = merged_views(root, worktree, strict, cfg)?;
    let names: Vec<String> = views.iter().map(|v| v.name.clone()).collect();
    let view = views.into_iter().find(|v| v.name == name).ok_or_else(|| {
        // a dropped-out `views/<name>.toml` explains itself —
        // surface its warning inside the unknown-view error
        let mut e = format!("unknown view \"{name}\" — defined: {}", names.join(", "));
        for (n, w) in &scoped_warnings {
            if n == name {
                e.push_str(&format!("; warning: {w}"));
            }
        }
        e
    })?;
    view.spec.validate()?;
    // warnings scoped to this name — the shadow/collision
    // notices `upsert` recorded plus any `views/` file warning
    // for it (an invalid file that dropped out still explains
    // itself when the name is asked for directly)
    let warnings = scoped_warnings
        .into_iter()
        .filter(|(n, _)| n == name)
        .map(|(_, w)| w)
        .collect();
    Ok((view, warnings))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::config::test_loader;

    #[test]
    fn builtins_are_valid() {
        let names: Vec<_> = builtins().iter().map(|v| v.name.clone()).collect();
        assert_eq!(
            names,
            [
                "board",
                "open",
                "epic",
                "milestone",
                "next",
                "roadmap",
                "stats",
                "burnup",
                "org-board",
                "org-status"
            ]
        );
        for v in builtins() {
            v.spec.validate().unwrap();
        }
        let org_board = builtins()
            .into_iter()
            .find(|v| v.name == "org-board")
            .unwrap();
        assert_eq!(org_board.spec.source, ViewSource::Index);
        let org_status = builtins()
            .into_iter()
            .find(|v| v.name == "org-status")
            .unwrap();
        assert_eq!(org_status.spec.source, ViewSource::Index);
        assert_eq!(org_status.spec.group_by, Some(GroupAxis::Repo));
        let board = &builtins()[0].spec;
        assert_eq!(board.group_by, Some(GroupAxis::Status));
        assert_eq!(board.format, ViewFormat::Board);
        assert!(!board.filter.status.contains(&Status::Done));
    }

    #[test]
    fn resolve_merges_config_and_warns_on_shadow() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("myco.toml"),
            "[views.board]\nfilter = { kind = [\"issue\"] }\n[views.mine]\nfilter = { pri_min = \"high\" }\n",
        )
        .unwrap();
        temp_env::with_var("MYCO_CONFIG", None::<&str>, || {
            let (views, warnings) = named_views(dir.path(), None, test_loader()).unwrap();
            assert_eq!(warnings.len(), 1);
            assert!(warnings[0].contains("shadows"));
            // config's board replaces the builtin
            let board = views.iter().find(|v| v.name == "board").unwrap();
            assert_eq!(board.origin, Origin::Config);
            assert_eq!(board.spec.filter.kind, vec!["issue".to_string()]);
            // repo + builtin names coexist
            assert!(views.iter().any(|v| v.name == "mine"));
            // unknown name errors listing defined views
            let err = resolve(dir.path(), None, "nope", test_loader()).unwrap_err();
            assert!(err.contains("mine") && err.contains("board"), "{err}");
        });
    }

    #[test]
    fn views_dir_files_resolve_with_dir_origin() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("views")).unwrap();
        std::fs::write(
            dir.path().join("views/hot.toml"),
            "filter = \"pri:>=high is:open\"\ngroup_by = \"owner\"\nformat = \"list\"\n",
        )
        .unwrap();
        // non-.toml entries are ignored
        std::fs::write(dir.path().join("views/notes.md"), "hi").unwrap();
        temp_env::with_vars(
            [
                ("MYCO_CONFIG", None::<&str>),
                (
                    "HOME",
                    Some(dir.path().join("nohome").to_string_lossy().as_ref()),
                ),
                ("XDG_CONFIG_HOME", None::<&str>),
            ],
            || {
                let (v, w) = resolve_lenient(dir.path(), None, "hot", test_loader()).unwrap();
                assert!(w.is_empty(), "{w:?}");
                assert_eq!(v.origin, Origin::Dir);
                assert_eq!(v.spec.filter.pri_min, Some(myco_core::Priority::High));
                assert_eq!(v.spec.group_by, Some(GroupAxis::Owner));
                // --list sees it too
                let (views, _) = named_views_lenient(dir.path(), None, test_loader()).unwrap();
                assert!(views
                    .iter()
                    .any(|v| v.name == "hot" && v.origin == Origin::Dir));
            },
        );
    }

    #[test]
    fn worktree_views_dir_shadows_repo_dir() {
        let repo = tempfile::tempdir().unwrap();
        let wt = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(repo.path().join("views")).unwrap();
        std::fs::create_dir_all(wt.path().join("views")).unwrap();
        std::fs::write(
            repo.path().join("views/shared.toml"),
            "filter = { kind = [\"task\"] }\n",
        )
        .unwrap();
        std::fs::write(
            wt.path().join("views/shared.toml"),
            "filter = { kind = [\"issue\"] }\n",
        )
        .unwrap();
        std::fs::write(wt.path().join("views/wtonly.toml"), "limit = 3\n").unwrap();
        temp_env::with_var("MYCO_CONFIG", None::<&str>, || {
            let (v, w) =
                resolve_lenient(repo.path(), Some(wt.path()), "shared", test_loader()).unwrap();
            assert_eq!(v.origin, Origin::Dir);
            assert_eq!(v.spec.filter.kind, vec!["issue".to_string()]);
            assert_eq!(w.len(), 1);
            assert!(w[0].contains("shadows a views/ file"), "{w:?}");
            // the worktree-only file resolves through the same path
            let (v, _) =
                resolve_lenient(repo.path(), Some(wt.path()), "wtonly", test_loader()).unwrap();
            assert_eq!(v.spec.limit, Some(3));
            // without the worktree arg the repo file answers
            let (v, _) = resolve_lenient(repo.path(), None, "shared", test_loader()).unwrap();
            assert_eq!(v.spec.filter.kind, vec!["task".to_string()]);
        });
    }

    #[test]
    fn config_views_still_beat_dir_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("views")).unwrap();
        std::fs::write(
            dir.path().join("views/hot.toml"),
            "filter = { kind = [\"task\"] }\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("myco.toml"),
            "[views.hot]\nfilter = { kind = [\"issue\"] }\n",
        )
        .unwrap();
        temp_env::with_var("MYCO_CONFIG", None::<&str>, || {
            let (v, w) = resolve_lenient(dir.path(), None, "hot", test_loader()).unwrap();
            assert_eq!(v.origin, Origin::Config);
            assert_eq!(v.spec.filter.kind, vec!["issue".to_string()]);
            assert_eq!(w.len(), 1);
            assert!(w[0].contains("shadows a views/ file"), "{w:?}");
        });
    }

    #[test]
    fn dir_view_shadowing_a_builtin_warns() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("views")).unwrap();
        std::fs::write(
            dir.path().join("views/board.toml"),
            "filter = { kind = [\"issue\"] }\n",
        )
        .unwrap();
        temp_env::with_var("MYCO_CONFIG", None::<&str>, || {
            let (v, w) = resolve_lenient(dir.path(), None, "board", test_loader()).unwrap();
            assert_eq!(v.origin, Origin::Dir);
            assert_eq!(w.len(), 1);
            assert!(w[0].contains("shadows the built-in preset"), "{w:?}");
        });
    }

    #[test]
    fn broken_view_files_warn_and_drop_out() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("views")).unwrap();
        std::fs::write(dir.path().join("views/bad.toml"), "filter = [unclosed\n").unwrap();
        std::fs::write(
            dir.path().join("views/invalid.toml"),
            "filter = { claimed = true, owner = [\"devin\"] }\n",
        )
        .unwrap();
        temp_env::with_vars(
            [
                ("MYCO_CONFIG", None::<&str>),
                (
                    "HOME",
                    Some(dir.path().join("nohome").to_string_lossy().as_ref()),
                ),
                ("XDG_CONFIG_HOME", None::<&str>),
            ],
            || {
                let (views, warnings) =
                    named_views_lenient(dir.path(), None, test_loader()).unwrap();
                assert!(!views.iter().any(|v| v.name == "bad" || v.name == "invalid"));
                assert_eq!(warnings.len(), 2, "{warnings:?}");
                // asking for the broken name explains itself in the
                // unknown-view error
                let err = resolve_lenient(dir.path(), None, "bad", test_loader()).unwrap_err();
                assert!(err.contains("warning"), "{err}");
                assert!(err.contains("bad.toml"), "{err}");
            },
        );
    }

    #[test]
    fn validate_repo_views_lints_dir_files() {
        let repo = tempfile::tempdir().unwrap();
        let wt = tempfile::tempdir().unwrap();
        for (base, name) in [(repo.path(), "bad"), (wt.path(), "wtbad")] {
            std::fs::create_dir_all(base.join("views")).unwrap();
            std::fs::write(
                base.join(format!("views/{name}.toml")),
                "filter = { claimed = true, owner = [\"devin\"] }\n",
            )
            .unwrap();
        }
        temp_env::with_var("MYCO_CONFIG", None::<&str>, || {
            let errs = validate_repo_views(repo.path(), Some(wt.path()), test_loader()).unwrap();
            assert_eq!(errs.len(), 2, "{errs:?}");
            let repo_hit = errs.iter().find(|(_, n, _)| n == "bad").unwrap();
            assert_eq!(repo_hit.0, "views/bad.toml");
            let wt_hit = errs.iter().find(|(_, n, _)| n == "wtbad").unwrap();
            assert!(wt_hit.0.ends_with("views/wtbad.toml"), "{}", wt_hit.0);
            assert!(wt_hit
                .0
                .starts_with(&wt.path().file_name().unwrap().to_string_lossy().to_string()));
        });
    }

    #[test]
    fn no_config_still_resolves_builtins() {
        let dir = tempfile::tempdir().unwrap();
        temp_env::with_vars(
            [
                ("MYCO_CONFIG", None::<&str>),
                (
                    "XDG_CONFIG_HOME",
                    Some(dir.path().join("none").to_string_lossy().as_ref()),
                ),
                ("HOME", Some(dir.path().to_string_lossy().as_ref())),
            ],
            || {
                let (v, _) = resolve(dir.path(), None, "board", test_loader()).unwrap();
                assert_eq!(v.origin, Origin::Builtin);
            },
        );
    }
}
