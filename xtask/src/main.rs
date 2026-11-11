//! xtask — repo chores. `cargo xtask <verb>` via the alias in
//! `.cargo/config.toml`.
//!
//! - `check`    — `cargo fmt --check` + `cargo clippy -D warnings`
//! - `test`     — `cargo nextest run` when installed, else `cargo test`
//! - `coverage` — `cargo llvm-cov` (HTML → target/llvm-cov/html);
//!   extra args pass through, e.g. `--fail-under-lines 100`
//! - `install`  — release-build `tamias-driver-myco` → ~/.cargo/bin
//!   (the `myco` command itself is the tamias pack's generated
//!   bins[] shim — `cargo xtask pack` + `tamias install
//!   ./libraries/myco`)
//! - `pack`     — stage libraries/myco driver payload
//!   (release tamias-driver-myco → drivers/bin/<triple>/)
//! - `site`     — build the docs site (docs/ worktree, VitePress)

use std::process::{Command, ExitStatus};

fn run(args: &[&str]) -> ExitStatus {
    let empty: &[&str] = &[];
    let (prog, rest) = args.split_first().unwrap_or((&"true", empty));
    Command::new(prog).args(rest).status().unwrap_or_else(|e| {
        eprintln!("xtask: failed to spawn {prog}: {e}");
        std::process::exit(1)
    })
}

/// `run` with a working directory — for commands that must execute
/// inside a child tree (the docs/ worktree).
fn run_in(dir: &str, args: &[&str]) -> ExitStatus {
    let empty: &[&str] = &[];
    let (prog, rest) = args.split_first().unwrap_or((&"true", empty));
    Command::new(prog)
        .args(rest)
        .current_dir(dir)
        .status()
        .unwrap_or_else(|e| {
            eprintln!("xtask: failed to spawn {prog}: {e}");
            std::process::exit(1)
        })
}

fn have(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn have_cargo_sub(sub: &str) -> bool {
    Command::new("cargo")
        .args([sub, "--version"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn check() -> i32 {
    let ok = run(&["cargo", "fmt", "--all", "--", "--check"]).success()
        && run(&[
            "cargo",
            "clippy",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ])
        .success();
    if ok {
        0
    } else {
        1
    }
}

fn test() -> i32 {
    if have_cargo_sub("nextest") {
        run(&["cargo", "nextest", "run", "--workspace"])
    } else {
        run(&["cargo", "test", "--workspace"])
    }
    .code()
    .unwrap_or(1)
}

fn coverage(extra: &[String]) -> i32 {
    if !have_cargo_sub("llvm-cov") {
        eprintln!("cargo-llvm-cov not installed — cargo install cargo-llvm-cov");
        return 1;
    }
    let mut args = vec!["cargo", "llvm-cov"];
    if have_cargo_sub("nextest") {
        args.push("nextest");
        args.extend(["--workspace", "--no-tests=pass"]);
    } else {
        args.push("--workspace");
    }
    if extra.is_empty() {
        args.push("--html");
    }
    let owned: Vec<&str> = extra.iter().map(String::as_str).collect();
    args.extend(owned);
    run(&args).code().unwrap_or(1)
}

fn install() -> i32 {
    // There is no `myco` cargo bin — the command ships as the
    // tamias pack (`cargo xtask pack` + `tamias install
    // ./libraries/myco`); the driver serves both the pavo view
    // surface and the `cli` bins[] shim passthrough. Installing the
    // driver binary alone keeps `tamias-driver-myco` resolvable on
    // PATH for dev; the pack install registers it under
    // <tamias-home>/drivers/ durably.
    if !run(&["cargo", "build", "--release", "-p", "myco-driver"]).success() {
        return 1;
    }
    let src = target_dir().join("release").join("tamias-driver-myco");
    let home = match std::env::var("HOME") {
        Ok(h) => h,
        Err(_) => {
            eprintln!(
                "xtask: $HOME unset — copy {} onto PATH yourself",
                src.display()
            );
            return 1;
        }
    };
    let dst = std::path::Path::new(&home)
        .join(".cargo/bin")
        .join("tamias-driver-myco");
    if let Err(e) = std::fs::copy(&src, &dst) {
        eprintln!("xtask: copy {} → {}: {e}", src.display(), dst.display());
        return 1;
    }
    println!("installed tamias-driver-myco → {}", dst.display());
    println!("install the myco command (generated bins[] shim):");
    println!("  cargo xtask pack && tamias install --allow-drivers ./libraries/myco");
    0
}

/// Effective target dir — `CARGO_TARGET_DIR` when set, else the
/// workspace `target/` (keeps `pack`/`install` correct under a
/// shared target dir without needing cargo metadata parsing).
fn target_dir() -> std::path::PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("target"))
}

/// Host target triple — the `drivers/bin/<triple>/` selector
/// tamias uses for compiled driver payloads.
fn host_triple() -> String {
    let out = Command::new("rustc")
        .args(["-vV"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|v| {
            v.lines()
                .find_map(|l| l.strip_prefix("host: ").map(str::to_string))
        })
        .unwrap_or_else(|| "x86_64-unknown-linux-gnu".to_string());
    out
}

/// `cargo xtask pack` — stage the tamias pack payload (T-006):
/// release-build `tamias-driver-myco` and drop it under
/// `libraries/myco/drivers/bin/<host-triple>/` — the
/// bundled-driver dir `tamias install` registers into
/// `<tamias-home>/drivers/`. Gitignored on purpose: the binary is a
/// build artifact; the committed pack is just `tamias.yaml` + this
/// staging verb.
fn pack() -> i32 {
    if !run(&["cargo", "build", "--release", "-p", "myco-driver"]).success() {
        return 1;
    }
    let triple = host_triple();
    let dest_dir = std::path::Path::new("libraries/myco/drivers/bin").join(&triple);
    if let Err(e) = std::fs::create_dir_all(&dest_dir) {
        eprintln!("xtask: create {}: {e}", dest_dir.display());
        return 1;
    }
    let src = target_dir().join("release").join("tamias-driver-myco");
    let dest = dest_dir.join("tamias-driver-myco");
    if let Err(e) = std::fs::copy(&src, &dest) {
        eprintln!("xtask: copy {} → {}: {e}", src.display(), dest.display());
        return 1;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755));
    }
    println!("pack staged: {}", dest.display());
    println!();
    println!("install the myco command (generated bins[] shim):");
    println!("  tamias install --allow-drivers ./libraries/myco");
    println!(
        "  # or a pinned ref: tamias install 'git:9aia/myco//libraries/myco#main' --allow-drivers"
    );
    0
}

fn site() -> i32 {
    if !std::path::Path::new("docs/package.json").exists() {
        eprintln!("no docs/ worktree — `git worktree add docs docs` first");
        return 1;
    }
    // run inside docs/ — works for bun and npm alike (`bun --cwd`
    // isn't a `bun run` flag, so flag plumbing gets fragile)
    let pkg = if have("bun") { "bun" } else { "npm" };
    if !run_in("docs", &[pkg, "install"]).success() {
        eprintln!("{pkg} install failed in docs/ — fix deps before building");
        return 1;
    }
    run_in("docs", &[pkg, "run", "docs:build"])
        .code()
        .unwrap_or(1)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let cmd = args.next().unwrap_or_default();
    let rest: Vec<String> = args.collect();
    let code = match cmd.as_str() {
        "check" => check(),
        "test" => test(),
        "coverage" => coverage(&rest),
        "install" => install(),
        "pack" => pack(),
        "site" => site(),
        other => {
            eprintln!(
                "usage: cargo xtask <check|test|coverage|install|pack|site>\n  unknown verb: {other:?}"
            );
            2
        }
    };
    std::process::exit(code);
}
