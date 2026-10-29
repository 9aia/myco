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
        let candidates: Vec<PathBuf> = match self
            .path
            .clone()
            .or_else(|| std::env::var("MYCO_CONFIG").ok())
        {
            Some(p) => vec![PathBuf::from(p)],
            None => vec![
                PathBuf::from("myco.toml"),
                std::env::var("HOME")
                    .map(PathBuf::from)
                    .unwrap_or(".".into())
                    .join(".config/myco/myco.toml"),
            ],
        };
        for p in candidates {
            if p.exists() {
                let text = std::fs::read_to_string(&p)
                    .map_err(|e| format!("read {}: {e}", p.display()))?;
                return toml::from_str(&text).map_err(|e| format!("parse {}: {e}", p.display()));
            }
        }
        Err("no myco.toml found (search: $MYCO_CONFIG, ., ~/.config/myco/)".into())
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
}
