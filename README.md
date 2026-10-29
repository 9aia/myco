# myco docs

VitePress + Diátaxis site — deployed to GitHub Pages at
`9aia.com/myco` on push to this branch (see
`.github/workflows/docs.yml`, which mirrors the local worktree
layout: `main` at the root, this branch at `docs/`).

From the repo root: `cargo xtask site` runs `docs:gen` +
`vitepress build`. Generated pages — `reference/cli.md`
(`myco gen-cli-docs`), `reference/config.md`, the crate map in
`explanation/architecture.md`, `reference/crate-api.md` — come
from `gen-*.py` / the CLI, never hand-edit. `bun run docs:api`
rebuilds the rustdoc under `public/api/` (gitignored — CI
regenerates it).
