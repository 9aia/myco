# myco docs

VitePress + Diátaxis site — served from the `gh-pages` branch at
`9aia.com/myco`. Deploys are driven by the CI gate (currently the
peregrin `ci-gate` campaign — migrating back to bupha): a push to this
branch runs `scripts/deploy-docs.sh` in a detached worktree of
the pushed sha (gen → `cargo doc` → `vitepress build` →
`bunx gh-pages`). Pages is a legacy branch deploy — pushing
`gh-pages` is the whole deploy; there is no Actions workflow.

The detached worktree has no sibling `main` checkout, so cargo
work happens in `$MYCO_REPO` (default
`~/GitHub/9aia/workspace/repos/myco`): `cypera gen` reads
`$MYCO_REPO/cypera.toml` via `--repo`, and `gen-api.py` resolves
sources via `MYCO_ROOT`. Run `bash scripts/deploy-docs.sh` by hand
for the same pipeline locally.

From the repo root: `cargo xtask site` runs `docs:gen` +
`vitepress build`. Generated pages — `reference/cli.md` (cypera's
`--help` walk), `reference/config.md`, the crate map in
`explanation/architecture.md`, `reference/crate-api.md` — come
from `cypera gen`, never hand-edit. `bun run docs:api`
rebuilds the rustdoc under `public/api/` (gitignored — CI
regenerates it).
