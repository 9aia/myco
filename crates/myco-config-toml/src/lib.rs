//! TOML impl of `ConfigSource` — explicit path → $MYCO_CONFIG →
//! ./myco.toml → ~/.config/myco/myco.toml.
//!
//! Scalar fields keep first-match semantics (the repo layer wins
//! over the user layer). `[views]` and `[verbs]` are the *merged*
//! sections (P-006, P-007): both layers contribute entries. Views
//! let the user layer win name collisions (personal presets reach
//! through); verbs let the repo layer win — repo verbs are shared
//! surface, personal ones can't override them. Builtins win over
//! both at resolution.

use myco_config::ConfigSource;
use std::path::{Path, PathBuf};

/// File-backed config source.
pub struct TomlSource {
    /// Explicit `--config` path, or None for the default search.
    pub path: Option<String>,
}

/// Parse exactly one config file — no layer search, no merge.
/// For callers that need a specific layer (e.g. `myco check`
/// linting only the repo's own myco.toml).
pub fn load_file(path: &Path) -> Result<myco_core::Config, String> {
    let text =
        std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    toml::from_str(&text).map_err(|e| format!("parse {}: {e}", path.display()))
}

fn parse(path: &Path) -> Result<myco_core::Config, String> {
    load_file(path)
}

/// First existing path wins within the user layer — XDG dir
/// preferred over ~/.config.
fn user_layer() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        candidates.push(PathBuf::from(xdg).join("myco/myco.toml"));
    }
    if let Ok(home) = std::env::var("HOME") {
        candidates.push(PathBuf::from(home).join(".config/myco/myco.toml"));
    }
    candidates.into_iter().find(|p| p.exists())
}

impl ConfigSource for TomlSource {
    fn load(&self) -> Result<myco_core::Config, String> {
        // an explicit path (--config or $MYCO_CONFIG) that doesn't
        // exist is an error naming it — not a silent skip
        let explicit = self
            .path
            .clone()
            .or_else(|| std::env::var("MYCO_CONFIG").ok());
        let repo_layer: Option<PathBuf> = match explicit {
            Some(p) => {
                let p = PathBuf::from(p);
                if !p.exists() {
                    return Err(format!("config {} not found", p.display()));
                }
                Some(p)
            }
            None => {
                let p = PathBuf::from("myco.toml");
                p.exists().then_some(p)
            }
        };
        let user_path = user_layer();
        if repo_layer.is_none() && user_path.is_none() {
            return Err(
                "no myco.toml found (search: $MYCO_CONFIG, ., $XDG_CONFIG_HOME/myco/, ~/.config/myco/)"
                    .into(),
            );
        }

        let repo_cfg = repo_layer.as_deref().map(parse).transpose()?;
        let user_cfg = user_path.as_deref().map(parse).transpose()?;

        // scalars: repo layer wins (today's first-match behavior);
        // views: merged, user layer wins name collisions;
        // verbs: merged, *repo* layer wins — repo verbs are shared
        // surface, personal verbs don't override them
        let mut merged = myco_core::Config {
            project_dir: repo_cfg
                .as_ref()
                .and_then(|c| c.project_dir.clone())
                .or_else(|| user_cfg.as_ref().and_then(|c| c.project_dir.clone())),
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
        Ok(merged)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn explicit_missing_file_errors() {
        let src = TomlSource {
            path: Some("/nonexistent/myco.toml".into()),
        };
        let err = src.load().err().unwrap_or_default();
        assert!(err.contains("myco.toml"));
    }

    #[test]
    fn parses_project_dir() {
        let dir = std::env::temp_dir().join(format!("myco-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap_or_default();
        let f = dir.join("myco.toml");
        std::fs::write(&f, "project_dir = \"project\"\n").unwrap_or_default();
        let src = TomlSource {
            path: Some(f.to_string_lossy().into_owned()),
        };
        let cfg = src.load().unwrap_or_default();
        assert_eq!(cfg.project_dir.as_deref(), Some("project"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn default_search_builds_candidates() {
        // no explicit path, no MYCO_CONFIG → ./myco.toml +
        // ~/.config/myco/myco.toml are probed (and miss in a
        // bare cwd)
        let src = TomlSource { path: None };
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
                let first = src.load();
                assert!(first.is_err(), "expected search error: {first:?}");
                // ./myco.toml is found on the default search
                std::fs::write(dir.join("myco.toml"), "project_dir = \"proj\"\n").unwrap();
                let out = src.load().unwrap();
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
                let cfg = TomlSource { path: None }.load().unwrap();
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
            let src = TomlSource {
                path: Some(f.to_string_lossy().into_owned()),
            };
            assert!(src.load().is_err(), "{bad}");
            let _ = std::fs::remove_dir_all(&dir);
        }
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
                let cfg = TomlSource { path: None }.load().unwrap();
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
}
