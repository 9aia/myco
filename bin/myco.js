#!/usr/bin/env node
// myco npm launcher — the real command is the tamias bins[] shim
// (a `# tamias-bin:` script → `tamias bin exec` →
// `tamias-driver-myco cli` → the myco engine lib), deployed by
// `tamias install git:9aia/myco//libraries/myco`. Node (always
// present — npm installed us) finds the shim on PATH and execs it;
// when it's missing, bootstrap via tamias on first use.
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

function haveOnPath(tool) {
    const r = spawnSync(tool, ["--version"], { stdio: "ignore" });
    return !r.error && r.status === 0;
}

function bootstrap() {
    // The `myco` command is a tamias bins[] shim — installing it
    // means installing tamias, then the myco pack (which registers
    // tamias-driver-myco and deploys the shim into the tamias bin
    // dir, ~/.local/bin by default).
    if (!haveOnPath("tamias")) {
        if (!haveOnPath("cargo")) {
            console.error("myco: shim not found and neither tamias nor cargo is installed.");
            console.error(
                "Install tamias (https://github.com/9aia/tamias), then run `myco` once to fetch the myco pack."
            );
            process.exit(1);
        }
        console.error("myco: installing tamias (first run, ~1-2 min)…");
        const r = spawnSync(
            "cargo",
            ["install", "--git", "https://github.com/9aia/tamias", "tamias"],
            { stdio: "inherit" }
        );
        if (r.status !== 0) process.exit(r.status ?? 1);
    }
    console.error("myco: installing the myco pack via tamias (first run)…");
    const r = spawnSync(
        "tamias",
        [
            "install",
            "git:9aia/myco//libraries/myco#main",
            "--allow-drivers",
        ],
        { stdio: "inherit" }
    );
    if (r.status !== 0) process.exit(r.status ?? 1);
}

let bin = findBin();
if (!bin) {
    bootstrap();
    bin = findBin();
    if (!bin) {
        console.error(
            "myco: installed but the shim isn't on PATH — check that the tamias bin dir (~/.local/bin) is on PATH."
        );
        process.exit(1);
    }
}

const res = spawnSync(bin, process.argv.slice(2), { stdio: "inherit" });
if (res.error) {
    console.error(`myco: failed to run ${bin}: ${res.error.message}`);
    process.exit(1);
}
process.exit(res.status ?? 1);
