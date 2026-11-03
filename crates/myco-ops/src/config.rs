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
