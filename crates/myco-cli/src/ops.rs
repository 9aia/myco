//! Re-export — lives in `myco_ops::ops` so `myco-tui` shares it.
//! `store_at` is cli-local: the concrete `MdStore` factory —
//! `myco-ops` is generic over [`myco_store::Store`], so the
//! composition root (this crate) names the impl.
pub use myco_ops::ops::*;

/// `MdStore` rooted at a worktree path, resolving item layout
/// through the repo's kind table (`[[kinds]]`, P-008).
pub fn store_at(
    worktree: &std::path::Path,
    kinds: &myco_core::KindTable,
) -> Result<myco_store_md::MdStore, String> {
    let root = camino::Utf8PathBuf::from_path_buf(worktree.to_path_buf())
        .map_err(|p| format!("{}: non-utf8 path", p.display()))?;
    Ok(myco_store_md::MdStore::with_kinds(root, kinds.clone()))
}

/// Layered TOML — `Some(path)` pins the repo layer, `None` runs
/// the impl's default search. `myco-ops` keeps the *which path*
/// policy; this only names the format.
fn config_layered(path: Option<&std::path::Path>) -> Result<myco_core::Config, String> {
    use myco_config::ConfigSource;
    myco_config_toml::TomlSource {
        path: path.map(|p| p.to_string_lossy().into_owned()),
    }
    .load()
}

/// The config impl ops verbs share — cli-local like `store_at`:
/// the composition root names it, ops stays impl-free.
pub const CONFIG: myco_ops::config::ConfigLoader = myco_ops::config::ConfigLoader {
    layered: config_layered,
    file: myco_config_toml::load_file,
};
