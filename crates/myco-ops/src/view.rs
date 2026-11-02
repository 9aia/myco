//! Named views — built-in presets plus `[views]` from merged
//! config layers (P-006). Resolution is I/O (config files), so it
//! lives here; evaluation is pure and stays in `myco_core::view`.

use std::collections::BTreeMap;
use std::path::Path;

use myco_config::ConfigSource;
use myco_config_toml::TomlSource;
use myco_core::{GroupAxis, Status, ViewFilter, ViewFormat, ViewSpec};

/// Where a named view came from — config shadows builtin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// A preset compiled into the binary (`board`, `epic`, …).
    Builtin,
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
            spec: spec(live, Some(GroupAxis::Milestone), ViewFormat::Board),
            origin: Origin::Builtin,
        },
    ]
}

/// `[views]` for a repo — the repo layer is `<root>/myco.toml` when
/// it exists (else the default `./myco.toml` probe), merged with
/// the user layer by `TomlSource`. "No config anywhere" is empty,
/// not an error — builtins still resolve.
fn config_views(root: &Path) -> Result<BTreeMap<String, ViewSpec>, String> {
    let repo_cfg = root.join("myco.toml");
    let src = TomlSource {
        path: repo_cfg
            .exists()
            .then(|| repo_cfg.to_string_lossy().into_owned()),
    };
    match src.load() {
        Ok(c) => Ok(c.views),
        // the documented search-miss — absence isn't a failure
        Err(e) if e.contains("no myco.toml found") => Ok(BTreeMap::new()),
        Err(e) => Err(e),
    }
}

/// Builtin presets merged with the repo's config views — config
/// wins a name collision and the returned warnings name it.
/// Specs are *not* validated here — `myco check` lints them all.
pub fn named_views(root: &Path) -> Result<(Vec<NamedView>, Vec<String>), String> {
    let mut warnings = Vec::new();
    let mut out = builtins();
    for (name, spec) in config_views(root)? {
        if let Some(v) = out.iter_mut().find(|v| v.name == name) {
            warnings.push(format!("view \"{name}\" shadows the built-in preset"));
            v.spec = spec;
            v.origin = Origin::Config;
        } else {
            out.push(NamedView {
                name,
                spec,
                origin: Origin::Config,
            });
        }
    }
    Ok((out, warnings))
}

/// Lint the repo-layer config only — deterministic, so `myco
/// check` reports the same findings for every user and CI.
/// `Ok` = per-view validation errors (`(name, error)`); `Err` =
/// the file is unreadable/unparseable. Missing file is `Ok(vec![])`.
/// Personal (user-layer) views are intentionally not linted here.
pub fn validate_repo_views(root: &Path) -> Result<Vec<(String, String)>, String> {
    let path = root.join("myco.toml");
    if !path.exists() {
        return Ok(vec![]);
    }
    let cfg = myco_config_toml::load_file(&path)?;
    Ok(cfg
        .views
        .iter()
        .filter_map(|(name, spec)| spec.validate().err().map(|e| (name.clone(), e)))
        .collect())
}

/// Resolve one view by name — config shadows builtin (warns).
/// Unknown names error listing the defined set. The spec is
/// validated before return.
pub fn resolve(root: &Path, name: &str) -> Result<(NamedView, Vec<String>), String> {
    let (views, warnings) = named_views(root)?;
    let names: Vec<String> = views.iter().map(|v| v.name.clone()).collect();
    let view = views
        .into_iter()
        .find(|v| v.name == name)
        .ok_or_else(|| format!("unknown view \"{name}\" — defined: {}", names.join(", ")))?;
    view.spec.validate()?;
    Ok((view, warnings))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use myco_core::ItemKind;

    #[test]
    fn builtins_are_valid() {
        let names: Vec<_> = builtins().iter().map(|v| v.name.clone()).collect();
        assert_eq!(names, ["board", "open", "epic", "milestone"]);
        for v in builtins() {
            v.spec.validate().unwrap();
        }
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
            let (views, warnings) = named_views(dir.path()).unwrap();
            assert_eq!(warnings.len(), 1);
            assert!(warnings[0].contains("shadows"));
            // config's board replaces the builtin
            let board = views.iter().find(|v| v.name == "board").unwrap();
            assert_eq!(board.origin, Origin::Config);
            assert_eq!(board.spec.filter.kind, vec![ItemKind::Issue]);
            // repo + builtin names coexist
            assert!(views.iter().any(|v| v.name == "mine"));
            // unknown name errors listing defined views
            let err = resolve(dir.path(), "nope").unwrap_err();
            assert!(err.contains("mine") && err.contains("board"), "{err}");
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
                let (v, _) = resolve(dir.path(), "board").unwrap();
                assert_eq!(v.origin, Origin::Builtin);
            },
        );
    }
}
