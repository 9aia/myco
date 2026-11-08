#!/usr/bin/env node
// myco npm launcher — the real binary is the Rust build under
// $CARGO_HOME/bin (or ~/.cargo/bin). Node (always present — npm
// installed us) finds it cross-platform and execs it; when it's
// missing, bootstrap via `cargo install --git` on first use.
//
// There is deliberately no postinstall: npm deletes the package
// cwd before lifecycle scripts on git installs, which crashed a
// node script at startup. This launcher is the single bootstrap
// path and covers every case.
"use strict";

const { spawnSync } = require("node:child_process");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const exe = process.platform === "win32" ? "myco.exe" : "myco";
const self = (() => {
    try {
        return fs.realpathSync(process.argv[1]);
    } catch {
        return process.argv[1];
    }
})();

function cargoHome() {
    return process.env.CARGO_HOME || path.join(os.homedir(), ".cargo");
}

function isFile(p) {
    try {
        return fs.statSync(p).isFile();
    } catch {
        return false;
    }
}

function sameFile(a, b) {
    try {
        return fs.realpathSync(a) === fs.realpathSync(b);
    } catch {
        return false;
    }
}

// CARGO_HOME first (the cargo install target), then every PATH
// dir — skipping this shim itself so we never recurse.
function findBin() {
    const dirs = [path.join(cargoHome(), "bin")];
    for (const d of (process.env.PATH || "").split(path.delimiter)) {
        if (d) dirs.push(d);
    }
    for (const d of dirs) {
        const cand = path.join(d, exe);
        if (isFile(cand) && !sameFile(cand, self)) return cand;
    }
    return null;
}

function haveCargo() {
    const r = spawnSync("cargo", ["--version"], { stdio: "ignore" });
    return !r.error && r.status === 0;
}

let bin = findBin();
if (!bin) {
    if (!haveCargo()) {
        console.error("myco: binary not found and cargo is not installed.");
        console.error(
            "Install Rust (https://rustup.rs), then run `myco` once to build."
        );
        process.exit(1);
    }
    console.error("myco: building the Rust binary (first run, ~1-2 min)…");
    const r = spawnSync(
        "cargo",
        [
            "install",
            "--git",
            "https://github.com/9aia/myco",
            "myco",
            "--locked",
        ],
        { stdio: "inherit" }
    );
    if (r.status !== 0) process.exit(r.status ?? 1);
    bin = path.join(cargoHome(), "bin", exe);
}

const res = spawnSync(bin, process.argv.slice(2), { stdio: "inherit" });
if (res.error) {
    console.error(`myco: failed to run ${bin}: ${res.error.message}`);
    process.exit(1);
}
process.exit(res.status ?? 1);
