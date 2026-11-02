//! Re-export — lives in `myco_ops::ops` so `myco-tui` shares it.
//! `store_at` is cli-local: the concrete `MdStore` factory —
//! `myco-ops` is generic over [`myco_store::Store`], so the
//! composition root (this crate) names the impl.
pub use myco_ops::ops::*;

/// `MdStore` rooted at a worktree path.
pub fn store_at(worktree: &std::path::Path) -> Result<myco_store_md::MdStore, String> {
    let root = camino::Utf8PathBuf::from_path_buf(worktree.to_path_buf())
        .map_err(|p| format!("{}: non-utf8 path", p.display()))?;
    Ok(myco_store_md::MdStore::new(root))
}
