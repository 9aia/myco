//! Re-export — lives in `myco_ops::ops` so other front-ends
//! (`myco-driver`) share it.
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

/// The config impl ops verbs share — the scarab-backed TOML
/// loader `myco-ops` owns (the per-repo `myco-config*` crates are
/// gone); cli-local like `store_at`: the composition root names
/// it, ops stays format-free.
pub const CONFIG: myco_ops::config::ConfigLoader = myco_ops::config::TOML;
