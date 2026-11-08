#!/usr/bin/env node
// postinstall — pre-build the Rust binary so `myco` is ready when
// the npm install finishes. Non-fatal: the bin/myco shim retries
// the same cargo install on first use, so a missing toolchain
// only warns here.
const { spawnSync } = require("node:child_process");

const hasCargo =
    spawnSync("cargo", ["--version"], { stdio: "ignore" }).status === 0;
if (!hasCargo) {
    console.warn(
        "myco: cargo not found — skipping binary build. " +
            "Install Rust (https://rustup.rs), then run `myco` once " +
            "to build, or `npm rebuild -g @9aia/myco`."
    );
    process.exit(0);
}

console.log("myco: installing the Rust binary via cargo (~1-2 min)…");
const r = spawnSync(
    "cargo",
    // --force: cargo skips same-version reinstalls, but an npm
    // (re)install means "give me latest" — rebuild every time.
    ["install", "--git", "https://github.com/9aia/myco", "myco", "--locked", "--force"],
    { stdio: "inherit" }
);
if (r.status !== 0) {
    console.warn(
        "myco: cargo install failed — the `myco` shim will retry on first use."
    );
}
