//! `myco current list|use|show|clear` — the current-ledger
//! selector verbs (P-015/P-016). Logic lives in
//! `myco_ops::current`; these handlers own argv → JSON/human
//! output and the index handle `list`/`use` validation need.

use std::path::PathBuf;

use serde_json::{json, Value};

use crate::ops::CONFIG;
use crate::registry::{Args, Ctx};
use myco_ops::current::{self, Selector};

fn cwd() -> Result<PathBuf, String> {
    std::env::current_dir().map_err(|e| format!("cwd: {e}"))
}

fn open_index() -> Result<myco_index_redb::RedbIndex, String> {
    crate::indexcmd::open_index()
}

fn as_index(i: &myco_index_redb::RedbIndex) -> &dyn myco_index::Index {
    i
}

fn mode_str(m: myco_core::CurrentMode) -> &'static str {
    match m {
        myco_core::CurrentMode::Always => "always",
        myco_core::CurrentMode::Backup => "backup",
    }
}

fn sel_json(source: &str, s: &Selector) -> Value {
    json!({"source": source, "ledger": s.ledger, "mode": mode_str(s.mode)})
}

/// The shared payload builders — cli prints them, serve returns
/// them verbatim.
pub fn list_json() -> Result<Value, String> {
    let cwd = cwd()?;
    let index = open_index()?;
    let targets = current::list_targets(&cwd, as_index(&index), CONFIG)?;
    Ok(json!({"targets": targets}))
}

pub fn show_json() -> Result<Value, String> {
    let cwd = cwd()?;
    let env = current::env_selector();
    let cfg_sel = current::config_selector(CONFIG);
    let pointer = current::pointer_selector(&cwd);
    let resolved = crate::discover::discover(None, None);
    Ok(json!({
        "env": env.as_ref().map(|s| sel_json("env", s)),
        "config": cfg_sel.as_ref().map(|s| sel_json("config", s)),
        "pointer": pointer.map(|(f, s)| json!({
            "file": f, "ledger": s.ledger,
        })),
        "resolved": match &resolved {
            Ok(p) => json!({"root": p.root, "worktree": p.worktree}),
            Err(e) => json!({"error": e}),
        },
    }))
}

pub fn use_json(target: &str, mode: &str, here: bool) -> Result<Value, String> {
    let cwd = cwd()?;
    let mode_enum = match mode {
        "always" => myco_core::CurrentMode::Always,
        "backup" => myco_core::CurrentMode::Backup,
        _ => return Err(format!("mode must be `always` or `backup`, got `{mode}`")),
    };
    // `repo` is inert — nothing to validate; every other target
    // resolves *now* so `use` fails loudly on a bad pin.
    if target != "repo" {
        let index = open_index().ok();
        let idx: Option<&dyn myco_index::Index> = index.as_ref().map(as_index);
        let sel = Selector {
            ledger: target.to_string(),
            mode: mode_enum,
        };
        current::resolve(&sel, &cwd, idx, CONFIG)?;
    }
    if here {
        // `repo` in a pointer file is inert — clear instead
        let path = current::write_pointer(&cwd, (target != "repo").then_some(target))?;
        return Ok(json!({"pointer": path, "ledger": target}));
    }
    let path = current::write_config(target, mode_str(mode_enum))?;
    Ok(json!({"config": path, "ledger": target, "mode": mode_str(mode_enum)}))
}

pub fn clear_json(here: bool) -> Result<Value, String> {
    let cwd = cwd()?;
    if here {
        let path = current::write_pointer(&cwd, None)?;
        return Ok(json!({"pointer": path, "cleared": true}));
    }
    let mode = current::config_selector(CONFIG)
        .map(|s| mode_str(s.mode))
        .unwrap_or("backup");
    let path = current::write_config("repo", mode)?;
    Ok(json!({"config": path, "ledger": "repo"}))
}

fn print_value(ctx: &Ctx, v: &Value) {
    if ctx.json {
        println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
    } else {
        match v {
            Value::Object(map) if map.contains_key("targets") => {
                for t in v["targets"].as_array().into_iter().flatten() {
                    let name = t["target"].as_str().unwrap_or("?");
                    let kind = t["kind"].as_str().unwrap_or("");
                    let extra = t["worktree"]
                        .as_str()
                        .map(|w| format!("  {w}"))
                        .or_else(|| t["note"].as_str().map(|n| format!("  {n}")))
                        .unwrap_or_default();
                    println!("{name}\t{kind}{extra}");
                }
            }
            _ => println!("{}", serde_json::to_string_pretty(v).unwrap_or_default()),
        }
    }
}

pub fn h_list(ctx: &Ctx, _a: &Args) -> Result<i32, String> {
    let v = list_json()?;
    print_value(ctx, &v);
    Ok(0)
}

pub fn h_show(ctx: &Ctx, _a: &Args) -> Result<i32, String> {
    let v = show_json()?;
    print_value(ctx, &v);
    Ok(0)
}

pub fn h_use(ctx: &Ctx, a: &Args) -> Result<i32, String> {
    let target = a.text("target")?;
    let mode = a.opt("mode").unwrap_or("backup");
    let v = use_json(target, mode, a.flag("here"))?;
    print_value(ctx, &v);
    Ok(0)
}

pub fn h_clear(ctx: &Ctx, a: &Args) -> Result<i32, String> {
    let v = clear_json(a.flag("here"))?;
    print_value(ctx, &v);
    Ok(0)
}
