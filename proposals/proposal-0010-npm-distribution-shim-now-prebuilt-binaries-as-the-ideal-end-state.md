+++
id = "P-010"
title = "npm distribution: shim now, prebuilt binaries as the ideal end state"
status = "open"
created = "2026-11-08"
priority = "med"
+++

# npm distribution: shim now, prebuilt binaries as the ideal end state

Status: **draft** · Created: 2026-11-08

## Goal & Overview

Give the team a one-command install — `npm i -g 9aia/myco`, then
`cd <repo> && myco ui` — without asking everyone to learn cargo.
Three options were on the table; this proposal records the choice
and the intended end state.

## Options considered

### A — npm shim over `cargo install --git` (SHIPPED, e858b78)

`package.json` + `bin/myco.js` (node launcher — Windows-capable) at the repo root. The shim runs
`cargo install --git https://github.com/9aia/myco myco --locked`
on first use. No postinstall — npm deletes the package cwd before
lifecycle scripts on git installs, which crashed `node install.js`
at startup (found via container repro); the shim is the single
bootstrap path and covers every case.

- **Pros:** zero infra, works today, always builds the pushed
  HEAD, verified end-to-end in a clean container (npm install →
  shim → cargo build → `myco`).
- **Cons:** requires Rust + cargo; first `myco` run builds ~1–2
  min; npm ≥12 gates git fetches (`--allow-git=root` needed);
  node launcher — cross-platform; Rust side untested on Windows.

### B — publish `@9aia/myco` to a registry

Same shim, but published to npmjs (`@9aia` scope) or GitHub
Packages (`npm.pkg.github.com` + `.npmrc`). Cosmetic upgrade over
A — still needs Rust, still compiles on install. Deferred; not
required for the team flow.

### C — prebuilt binaries via GitHub Releases (THE IDEAL END STATE)

npm package whose `install.js` detects platform/arch and downloads
a release binary — fast installs, no toolchain, Windows support
possible. `dist-workspace.toml` already exists (cargo-dist, shell
installer, `x86_64-unknown-linux-gnu`) but `ci = "none"` — this
needs a release pipeline first (bupha or a workflow), then wider
targets.

- **Pros:** the real `npm install` experience; no Rust; fast.
- **Cons:** release infra to build/host/sign artifacts; version
  drift between npm package and binary until the pipeline exists.

## Requirements

- `npm i -g 9aia/myco` must work for a teammate with no myco
  checkout.
- Updates must reach users (`--force` rebuild today; release
  versions tomorrow).
- Rust requirement is acceptable **for now** — the org's team all
  has cargo; removing it is the phase-2 goal (tracked as T-041).

## Technical Design

Phase 1 (done): shim + postinstall → `cargo install --git`.
Phase 2 (this proposal's target): enable cargo-dist CI → GitHub
Releases artifacts → npm `install.js` downloads the matching
binary; keep the cargo path as fallback for unreleased commits.

## Open questions

- Registry for phase 2 packaging: npmjs `@9aia` scope vs GitHub
  Packages — decide when publishing (B) is wanted.
- Target list beyond `x86_64-unknown-linux-gnu` (macOS arm64 at
  minimum for the team?).
- Signing/checksums for downloaded binaries.
