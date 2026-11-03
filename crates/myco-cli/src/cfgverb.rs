//! Config-declared verbs — `[verbs.<name>]` in `myco.toml`
//! (P-007). `myco <name>` resolves a config verb (layered
//! repo<user, builtins win collisions); `alias` bakes argv for a
//! read-only builtin; `view-emit` evaluates a `ViewSpec` into one
//! of the emit shapes. Mutations stay builtin — both kinds are
//! read-only by construction.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use myco_core::{eval, eval_scoped, ConfigVerb, EmitMode, Scoped, ViewSource, ViewSpec};
use serde_json::{json, Value};

use crate::registry::{self, Args, Ctx};
use crate::serve::{Fail, Server};
use crate::{continuity, digest, items, ops, view};

/// The repo root for `[verbs]`/`[views]` resolution — mirrors
/// `view`'s: discovery's root, else `--root`/cwd (config verbs
/// that don't need a store still resolve outside a project).
fn cfg_root(ctx: &Ctx) -> PathBuf {
    crate::discover::repo_root_for(ctx.dir.as_deref(), ctx.root.as_deref())
        .ok()
        .or_else(|| ctx.root.clone())
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// A builtin shadowed by a `[verbs.<name>]` entry warns — the
/// builtin wins (the stable surface is part of the audit trail).
pub(crate) fn warn_if_shadowed(ctx: &Ctx, name: &str) {
    let root = cfg_root(ctx);
    if let Ok(Some(_)) = myco_ops::verb::raw_verb(&root, name, ops::CONFIG) {
        eprintln!("myco: warning: verb \"{name}\" shadows a built-in — the built-in wins");
    }
}

/// The CLI path — `myco <name> [args…]` where `<name>` isn't a
/// builtin. Load-time warnings (skipped entries) print first;
/// resolution errors are the verb error.
pub fn run(ctx: &Ctx, name: &str, tokens: &[String]) -> Result<i32, String> {
    let root = cfg_root(ctx);
    let (effective, warnings) =
        myco_ops::verb::named_verbs_lenient(&root, ops::CONFIG, registry::builtin_refs())?;
    view::warn(&warnings);
    let nv = match effective.into_iter().find(|v| v.name == name) {
        Some(v) => v,
        None => {
            // skipped or never declared — resolve gives the reason
            return myco_ops::verb::resolve_lenient(
                &root,
                name,
                ops::CONFIG,
                registry::builtin_refs(),
            )
            .map(|_| 0)
            .map_err(|e| e.to_string());
        }
    };
    match &nv.verb {
        ConfigVerb::Alias(al) => {
            let tv = registry::find(&al.target).ok_or_else(|| {
                format!("verb \"{name}\": unknown alias target \"{}\"", al.target)
            })?;
            let h = tv.cli.ok_or_else(|| {
                format!(
                    "verb \"{name}\": alias target \"{}\" has no CLI handler",
                    al.target
                )
            })?;
            let mut ctx2 = ctx.clone();
            let mut argv = al.args.clone();
            argv.extend(tokens.iter().cloned());
            let a = Args::from_argv(tv, &argv, &mut ctx2, false)?;
            h(&ctx2, &a)
        }
        ConfigVerb::ViewEmit(ve) => {
            let mut ctx2 = ctx.clone();
            let rest = split_globals(tokens, &mut ctx2)?;
            if let Some(extra) = rest.first() {
                return Err(format!(
                    "verb \"{name}\" takes no arguments — got {extra:?}"
                ));
            }
            let (payload, human) = emit(
                ctx2.dir.as_deref(),
                ctx2.root.as_deref(),
                &nv.name,
                &ve.spec,
                ve.emit,
            )?;
            if ctx2.json || ve.emit == EmitMode::Json {
                ops::print_json(&payload);
            } else {
                println!("{human}");
            }
            Ok(0)
        }
    }
}

/// The wire path — a method that isn't a builtin verb resolves
/// as a config verb for this repo.
pub fn wire(s: &Server, method: &str, params: &Value) -> Result<Value, Fail> {
    let root = s.cfg_root();
    let nv = myco_ops::verb::resolve_lenient(&root, method, ops::CONFIG, registry::builtin_refs())
        .map_err(|e| match e.as_str() {
            m if m.starts_with("unknown verb") => Fail::UnknownMethod,
            _ => Fail::Verb(e),
        })?;
    match &nv.verb {
        ConfigVerb::Alias(al) => {
            if !params.is_null() && params != &json!({}) {
                return Err(Fail::BadParams(
                    "config alias verbs take no params — args are baked".into(),
                ));
            }
            let tv = registry::find(&al.target)
                .ok_or_else(|| Fail::Verb(format!("unknown alias target \"{}\"", al.target)))?;
            let w = tv.wire.ok_or_else(|| {
                Fail::Verb(format!(
                    "verb \"{method}\": alias target \"{}\" isn't served",
                    al.target
                ))
            })?;
            let mut ctx = Ctx::default();
            let a = Args::from_argv(tv, &al.args, &mut ctx, true).map_err(Fail::BadParams)?;
            w(s, &a)
        }
        ConfigVerb::ViewEmit(ve) => {
            if !params.is_null() && params != &json!({}) {
                return Err(Fail::BadParams(
                    "view-emit verbs take no params — the spec is baked".into(),
                ));
            }
            let (payload, _) =
                emit(s.dir(), s.root(), &nv.name, &ve.spec, ve.emit).map_err(Fail::Verb)?;
            Ok(payload)
        }
    }
}

/// `--json/--no-commit/--dir/--root` out of a config verb's
/// trailing tokens; the rest comes back for rejection.
fn split_globals(tokens: &[String], ctx: &mut Ctx) -> Result<Vec<String>, String> {
    let mut rest = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let t = &tokens[i];
        i += 1;
        match t.as_str() {
            "--json" => ctx.json = true,
            "--no-commit" => ctx.no_commit = true,
            "--dir" | "--root" => {
                let v = tokens
                    .get(i)
                    .ok_or_else(|| format!("\"{t}\" needs a value"))?;
                i += 1;
                if t == "--dir" {
                    ctx.dir = Some(PathBuf::from(v));
                } else {
                    ctx.root = Some(PathBuf::from(v));
                }
            }
            _ => rest.push(t.clone()),
        }
    }
    Ok(rest)
}

/// Evaluate `spec` and shape it per `emit` — returns the wire
/// payload plus the human text (the CLI prints one or the other).
/// Mirrors `view`'s universe rule: `repo` reads this worktree,
/// `index` unions the registered repos' snapshots.
fn emit(
    dir: Option<&Path>,
    root: Option<&Path>,
    name: &str,
    spec: &ViewSpec,
    emit: EmitMode,
) -> Result<(Value, String), String> {
    spec.validate()?;
    let mut warnings: Vec<String> = Vec::new();
    // the universe owners — `groups` borrows item refs
    let (interrupted, scan, views) = match spec.source {
        ViewSource::Repo => {
            let (_proj, store) = items::open_read(dir, root)?;
            let s = digest::scan(&store)?;
            (
                continuity::interrupted_set(&store, s.live())?,
                Some(s),
                Vec::new(),
            )
        }
        ViewSource::Index => {
            let (v, repo_warnings) = view::index_universe()?;
            warnings.extend(repo_warnings);
            (BTreeSet::new(), None, v)
        }
    };
    let groups = match spec.source {
        ViewSource::Repo => eval(
            scan.iter()
                .flat_map(|s| s.items.iter().filter(|i| !s.archived.contains(&i.id))),
            spec,
        ),
        ViewSource::Index => eval_scoped(
            views.iter().flat_map(|v| {
                v.snap.iter().flat_map(|s| {
                    s.items
                        .iter()
                        .filter(|i| !s.archived.contains(&i.id))
                        .map(move |i| Scoped::tagged(s.name.as_str(), i))
                })
            }),
            spec,
        ),
    };
    match emit {
        EmitMode::Table => {
            let rendered = view::render_groups(spec, &groups, &interrupted, false, false);
            let human = if groups.iter().all(|g| g.items.is_empty()) {
                format!("{name}: empty view — no items match")
            } else {
                rendered.clone()
            };
            Ok((
                json!({"view": name, "emit": "table", "table": rendered, "warnings": warnings}),
                human,
            ))
        }
        EmitMode::Json => {
            let mut payload = view::view_json(Some(name), spec, &groups, &interrupted);
            payload["warnings"] = json!(warnings);
            let human = serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".into());
            Ok((payload, human))
        }
        EmitMode::Ids => {
            let ids: Vec<String> = groups
                .iter()
                .flat_map(|g| g.items.iter().map(|i| i.item.id.to_string()))
                .collect();
            let human = if ids.is_empty() {
                format!("{name}: no items match")
            } else {
                ids.join("\n")
            };
            Ok((
                json!({"view": name, "ids": ids, "warnings": warnings}),
                human,
            ))
        }
        EmitMode::First => {
            let item = groups.iter().find_map(|g| g.items.first().map(|i| i.item));
            let human = match item {
                Some(i) => {
                    let mut line = items::compact(i);
                    if interrupted.contains(&i.id) {
                        line.push_str(" ⚡");
                    }
                    line
                }
                None => format!("{name}: nothing matches"),
            };
            Ok((
                json!({"view": name, "item": item, "warnings": warnings}),
                human,
            ))
        }
    }
}
