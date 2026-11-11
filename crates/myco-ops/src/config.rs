//! Config plumbing — ops owns the *policy* (which file is the
//! repo layer, merged vs repo-only reads); the file format is
//! injected by the composition root (`myco` wires TOML), the
//! same story as `items::open_with`'s `mk` naming the store
//! impl. Only `myco_core::Config` crosses the seam.

use std::path::Path;

/// The two config reads ops needs. `layered` runs the impl's
/// layered search — `Some(path)` pins the repo layer, `None`
/// its default probe (env → cwd → user layer); `file` reads
/// exactly one file with no search and no merge — `myco check`
/// lints the repo layer alone so findings are identical for
/// every user.
///
/// Plain fn pointers keep ops free of `myco-config` impls and
/// make the loader `Copy` — front-ends bind one `const`.
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
    use myco_config::ConfigSource;
    ConfigLoader {
        layered: |p| {
            myco_config_toml::TomlSource {
                path: p.map(|p| p.to_string_lossy().into_owned()),
            }
            .load()
        },
        file: myco_config_toml::load_file,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

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
