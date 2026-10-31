//! TOML impl of `ConfigSource` — explicit path → $MYCO_CONFIG →
//! ./myco.toml → ~/.config/myco/myco.toml.

use myco_config::ConfigSource;
use std::path::PathBuf;

/// File-backed config source.
pub struct TomlSource {
    /// Explicit `--config` path, or None for the default search.
    pub path: Option<String>,
}

impl ConfigSource for TomlSource {
    fn load(&self) -> Result<myco_core::Config, String> {
        // an explicit path (--config or $MYCO_CONFIG) that doesn't
        // exist is an error naming it — not a silent skip
        let explicit = self
            .path
            .clone()
            .or_else(|| std::env::var("MYCO_CONFIG").ok());
        let candidates: Vec<PathBuf> = match explicit {
            Some(p) => {
                let p = PathBuf::from(p);
                if !p.exists() {
                    return Err(format!("config {} not found", p.display()));
                }
                vec![p]
            }
            None => {
                let mut c = vec![PathBuf::from("myco.toml")];
                // XDG first, then ~/.config — a bare `./.config`
                // under an unrelated cwd must never load as config
                if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
                    c.push(PathBuf::from(xdg).join("myco/myco.toml"));
                }
                if let Ok(home) = std::env::var("HOME") {
                    c.push(PathBuf::from(home).join(".config/myco/myco.toml"));
                }
                c
            }
        };
        for p in candidates {
            if p.exists() {
                let text = std::fs::read_to_string(&p)
                    .map_err(|e| format!("read {}: {e}", p.display()))?;
                return toml::from_str(&text).map_err(|e| format!("parse {}: {e}", p.display()));
            }
        }
        Err(
            "no myco.toml found (search: $MYCO_CONFIG, ., $XDG_CONFIG_HOME/myco/, ~/.config/myco/)"
                .into(),
        )
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
}
