//! `myco workspace list|use|show|clear` — the multi-repo
//! selector (P-016): `[workspace] repos` glob patterns in user
//! `myco.toml`, matched against universe repo names. Logic lives
//! in `myco_ops::workspace`; these handlers own argv →
//! JSON/human output and the index handle resolution needs.

use std::path::PathBuf;

use serde_json::{json, Value};

use crate::ops::CONFIG;
use crate::registry::{Args, Ctx};
use myco_ops::workspace;

fn cwd() -> Result<PathBuf, String> {
    std::env::current_dir().map_err(|e| format!("cwd: {e}"))
}

fn open_index() -> Result<myco_index_redb::RedbIndex, String> {
    crate::indexcmd::open_index()
}

fn as_index(i: &myco_index_redb::RedbIndex) -> &dyn myco_index::Index {
    i
}

/// The shared payload builders — cli prints them, serve returns
/// them verbatim.
pub fn list_json() -> Result<Value, String> {
    let cwd = cwd()?;
    let index = open_index()?;
    let repos = myco_ops::universe::repos(&cwd, as_index(&index), CONFIG)?;
    let targets: Vec<Value> = repos
        .iter()
        .map(|u| {
            json!({
                "name": u.entry.name,
                "source": u.source.as_str(),
                "ledger": match u.entry.ledger {
                    myco_index::LedgerMode::Org => "org",
                    myco_index::LedgerMode::None => "none",
                    myco_index::LedgerMode::Local => "local",
                },
                "worktree": u.entry.worktree_path().map(|w| w.as_std_path().to_path_buf()),
            })
        })
        .collect();
    Ok(json!({"repos": targets}))
}

pub fn show_json() -> Result<Value, String> {
    let cwd = cwd()?;
    let globs = workspace::configured(CONFIG);
    let index = open_index()?;
    let resolved = workspace::resolve(&cwd, as_index(&index), CONFIG);
    Ok(json!({
        "globs": globs,
        "resolved": match &resolved {
            Ok((repos, warnings)) => json!({
                "repos": repos.iter().map(|u| json!({
                    "name": u.entry.name,
                    "source": u.source.as_str(),
                    "worktree": u.entry.worktree_path().map(|w| w.as_std_path().to_path_buf()),
                })).collect::<Vec<_>>(),
                "warnings": warnings,
            }),
            Err(e) => json!({"error": e}),
        },
    }))
}

pub fn use_json(globs: &[String]) -> Result<Value, String> {
    if globs.is_empty() {
        return Err(
            "workspace use needs at least one glob — `myco workspace clear` empties".into(),
        );
    }
    let cwd = cwd()?;
    let index = open_index()?;
    // validate now — a typo'd glob fails loudly at `use` time
    let (_repos, warnings) = workspace::resolve_globs(&cwd, as_index(&index), CONFIG, globs)?;
    let path = workspace::write_config(globs)?;
    Ok(json!({
        "config": path,
        "repos": globs,
        "warnings": warnings,
    }))
}

pub fn clear_json() -> Result<Value, String> {
    let path = workspace::write_config(&[])?;
    Ok(json!({"config": path, "repos": []}))
}

fn print_value(ctx: &Ctx, v: &Value) {
    if ctx.json {
        println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
        return;
    }
    match v {
        Value::Object(map) if map.contains_key("repos") && map.contains_key("globs") => {
            // show
            if let Some(g) = v["globs"].as_array() {
                println!(
                    "globs: {}",
                    g.iter()
                        .filter_map(|x| x.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            } else {
                println!("globs: (none — `myco workspace use <glob>…`)");
            }
            if let Some(r) = v["resolved"]["repos"].as_array() {
                for u in r {
                    println!(
                        "{}\t{}",
                        u["name"].as_str().unwrap_or("?"),
                        u["source"].as_str().unwrap_or("")
                    );
                }
            }
            for w in v["resolved"]["warnings"].as_array().into_iter().flatten() {
                eprintln!("warning: {}", w.as_str().unwrap_or(""));
            }
            if let Some(e) = v["resolved"]["error"].as_str() {
                eprintln!("error: {e}");
            }
        }
        Value::Object(map) if map.contains_key("repos") => {
            // list / use / clear — one name per line for list;
            // use/clear print the written file
            if v.get("config").is_some() {
                println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
            } else {
                for u in v["repos"].as_array().into_iter().flatten() {
                    let name = u["name"].as_str().unwrap_or("?");
                    let src = u["source"].as_str().unwrap_or("");
                    let wt = u["worktree"]
                        .as_str()
                        .map(|w| format!("  {w}"))
                        .unwrap_or_default();
                    println!("{name}\t{src}{wt}");
                }
            }
        }
        _ => println!("{}", serde_json::to_string_pretty(v).unwrap_or_default()),
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
    let globs = a.many("repos");
    let v = use_json(&globs)?;
    print_value(ctx, &v);
    Ok(0)
}

pub fn h_clear(ctx: &Ctx, _a: &Args) -> Result<i32, String> {
    let v = clear_json()?;
    print_value(ctx, &v);
    Ok(0)
}
