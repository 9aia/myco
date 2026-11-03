//! Config-declared verbs — `[verbs]` from merged config layers
//! (P-007). Resolution mirrors `view.rs`: config files are I/O so
//! the merge lives here; the verbs themselves are executed by the
//! front-end (cli). The builtin surface is injected — ops doesn't
//! know the registry, callers hand it in like `ConfigLoader`.
//!
//! Config verbs are read-only by construction: an alias may only
//! target a read-only builtin, and a view-emit can only evaluate a
//! spec. Collisions mirror view-shadowing except the *builtin*
//! wins — a config verb can never hide the stable surface.

use std::collections::BTreeMap;
use std::path::Path;

use myco_core::ConfigVerb;

use crate::config::ConfigLoader;

/// One builtin verb as the front-end's registry sees it — the
/// facts `[verbs]` resolution needs for collision and
/// alias-target checks.
#[derive(Debug, Clone, Copy)]
pub struct BuiltinVerb {
    /// The canonical name (`todo.list`, `status`).
    pub name: &'static str,
    /// Can write the ledger/index — alias targets must be
    /// read-only, so `mutates` disqualifies.
    pub mutates: bool,
    /// Has a CLI handler — wire-only builtins can't be aliased
    /// either.
    pub cli: bool,
}

/// A resolved config-declared verb — shadowed/invalid entries are
/// dropped from the effective set and reported as warnings.
#[derive(Debug, Clone)]
pub struct NamedVerb {
    /// The `[verbs.<name>]` name.
    pub name: String,
    /// The declared verb.
    pub verb: ConfigVerb,
}

/// Verb names are top-level CLI words — `[A-Za-z0-9_-]+`, never
/// flag-shaped.
pub fn is_valid_verb_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}

/// `[verbs]` for a repo — same layered read as [`crate::view`]:
/// `<root>/myco.toml` when it exists (else the impl's default
/// probe), merged by the injected `cfg`. "No config anywhere" is
/// empty, not an error.
fn config_verbs(
    root: &Path,
    strict: bool,
    cfg: ConfigLoader,
) -> Result<BTreeMap<String, ConfigVerb>, String> {
    let repo_cfg = root.join("myco.toml");
    match cfg.load(repo_cfg.exists().then_some(repo_cfg.as_path())) {
        Ok(c) => Ok(c.verbs),
        // the documented search-miss — absence isn't a failure
        Err(e) if e.contains("no myco.toml found") => Ok(BTreeMap::new()),
        Err(e) if !strict && e.contains("not found") => Ok(BTreeMap::new()),
        Err(e) => Err(e),
    }
}

/// The warning a config-vs-builtin name collision earns — the
/// builtin wins (the stable surface is part of the audit trail).
fn shadow_warning(name: &str) -> String {
    format!("verb \"{name}\" shadows a built-in — the built-in wins")
}

/// Why a config verb was dropped from the effective set.
fn skip_reason(name: &str, verb: &ConfigVerb, builtins: &[BuiltinVerb]) -> Option<String> {
    if !is_valid_verb_name(name) {
        return Some(format!(
            "verb \"{name}\": invalid name — verbs match `[A-Za-z0-9_-]+`"
        ));
    }
    if builtins.iter().any(|b| b.name == name) {
        return Some(shadow_warning(name));
    }
    if let ConfigVerb::Alias(a) = verb {
        match builtins.iter().find(|b| b.name == a.target) {
            None => {
                return Some(format!(
                    "verb \"{name}\": unknown alias target \"{}\" — skipped",
                    a.target
                ))
            }
            Some(b) if b.mutates || !b.cli => {
                return Some(format!(
                    "verb \"{name}\": alias target \"{}\" isn't a read-only verb — skipped",
                    a.target
                ))
            }
            _ => {}
        }
    }
    None
}

/// Config verbs effective for a repo — invalid names, builtin
/// collisions, and bad alias targets warn + skip.
fn merged_verbs(
    root: &Path,
    strict: bool,
    cfg: ConfigLoader,
    builtins: &[BuiltinVerb],
) -> Result<(Vec<NamedVerb>, Vec<String>), String> {
    let mut warnings = Vec::new();
    let mut out = Vec::new();
    for (name, verb) in config_verbs(root, strict, cfg)? {
        match skip_reason(&name, &verb, builtins) {
            Some(w) => warnings.push(w),
            None => out.push(NamedVerb { name, verb }),
        }
    }
    Ok((out, warnings))
}

/// [`merged_verbs`] with strict config handling — `myco serve`,
/// `myco check`.
pub fn named_verbs(
    root: &Path,
    cfg: ConfigLoader,
    builtins: &[BuiltinVerb],
) -> Result<(Vec<NamedVerb>, Vec<String>), String> {
    merged_verbs(root, true, cfg, builtins)
}

/// [`merged_verbs`] tolerant of a *missing* config — the verb
/// dispatch path: `$MYCO_CONFIG` naming a missing file degrades
/// to no config verbs; an unparseable config still errors.
pub fn named_verbs_lenient(
    root: &Path,
    cfg: ConfigLoader,
    builtins: &[BuiltinVerb],
) -> Result<(Vec<NamedVerb>, Vec<String>), String> {
    merged_verbs(root, false, cfg, builtins)
}

/// One *raw* `[verbs]` entry by name — lenient, no validation.
/// For the "is this builtin shadowed" check the effective set
/// can't answer (shadowed entries are dropped from it).
pub fn raw_verb(root: &Path, name: &str, cfg: ConfigLoader) -> Result<Option<ConfigVerb>, String> {
    Ok(config_verbs(root, false, cfg)?.remove(name))
}

/// Resolve one config verb by name — builtin names never resolve
/// (the builtin wins); unknown names error listing the defined
/// config verbs. A view-emit's spec is validated before return.
pub fn resolve(
    root: &Path,
    name: &str,
    cfg: ConfigLoader,
    builtins: &[BuiltinVerb],
) -> Result<NamedVerb, String> {
    resolve_in(root, name, true, cfg, builtins)
}

/// [`resolve`] tolerant of a *missing* config — the dispatch path.
pub fn resolve_lenient(
    root: &Path,
    name: &str,
    cfg: ConfigLoader,
    builtins: &[BuiltinVerb],
) -> Result<NamedVerb, String> {
    resolve_in(root, name, false, cfg, builtins)
}

fn resolve_in(
    root: &Path,
    name: &str,
    strict: bool,
    cfg: ConfigLoader,
    builtins: &[BuiltinVerb],
) -> Result<NamedVerb, String> {
    if let Some(reason) = config_verbs(root, strict, cfg)?
        .get(name)
        .and_then(|v| skip_reason(name, v, builtins))
    {
        return Err(reason);
    }
    let (verbs, _) = merged_verbs(root, strict, cfg, builtins)?;
    let verb = verbs.into_iter().find(|v| v.name == name).ok_or_else(|| {
        let names: Vec<String> = config_verbs(root, strict, cfg)
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default();
        if names.is_empty() {
            format!("unknown verb \"{name}\"")
        } else {
            format!(
                "unknown verb \"{name}\" — config verbs: {}",
                names.join(", ")
            )
        }
    })?;
    if let ConfigVerb::ViewEmit(v) = &verb.verb {
        v.spec.validate()?;
    }
    Ok(verb)
}

/// Lint the repo-layer config only — deterministic, so `myco
/// check` reports the same findings for every user and CI.
/// `Ok` = per-verb validation errors (`(name, error)`); `Err` =
/// the file is unreadable/unparseable. Missing file is
/// `Ok(vec![])`. Unlike views (where shadowing is legal), a
/// verb-name collision is dead config — it is a finding.
pub fn validate_repo_verbs(
    root: &Path,
    loader: ConfigLoader,
    builtins: &[BuiltinVerb],
) -> Result<Vec<(String, String)>, String> {
    let path = root.join("myco.toml");
    if !path.exists() {
        return Ok(vec![]);
    }
    let cfg = loader.load_file(&path)?;
    Ok(cfg
        .verbs
        .iter()
        .flat_map(|(name, verb)| {
            let mut errs: Vec<String> = skip_reason(name, verb, builtins).into_iter().collect();
            if let ConfigVerb::ViewEmit(v) = verb {
                if let Err(e) = v.spec.validate() {
                    errs.push(e);
                }
            }
            errs.into_iter().map(move |e| (name.clone(), e))
        })
        .collect())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::config::test_loader;

    const BUILTINS: &[BuiltinVerb] = &[
        BuiltinVerb {
            name: "view",
            mutates: false,
            cli: true,
        },
        BuiltinVerb {
            name: "status",
            mutates: false,
            cli: true,
        },
        BuiltinVerb {
            name: "todo.add",
            mutates: true,
            cli: true,
        },
        BuiltinVerb {
            name: "initialize",
            mutates: false,
            cli: false,
        },
    ];

    fn no_env<T>(f: impl FnOnce() -> T) -> T {
        temp_env::with_vars(
            [
                ("MYCO_CONFIG", None::<&str>),
                ("XDG_CONFIG_HOME", Some("/nonexistent-xdg")),
                ("HOME", Some("/nonexistent-home")),
            ],
            f,
        )
    }

    #[test]
    fn resolves_alias_and_view_emit() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("myco.toml"),
            "[verbs.standup]\ntarget = \"view\"\nargs = [\"board\"]\n\n\
             [verbs.hot]\nemit = \"first\"\nfilter = { status = [\"open\"] }\n",
        )
        .unwrap();
        no_env(|| {
            let (verbs, warnings) = named_verbs(dir.path(), test_loader(), BUILTINS).unwrap();
            assert!(warnings.is_empty());
            let names: Vec<&str> = verbs.iter().map(|v| v.name.as_str()).collect();
            assert_eq!(names, ["hot", "standup"]);
            let v = resolve(dir.path(), "hot", test_loader(), BUILTINS).unwrap();
            assert!(matches!(v.verb, ConfigVerb::ViewEmit(_)));
            let v = resolve(dir.path(), "standup", test_loader(), BUILTINS).unwrap();
            match &v.verb {
                ConfigVerb::Alias(a) => {
                    assert_eq!(a.target, "view");
                    assert_eq!(a.args, ["board"]);
                }
                _ => panic!("standup should be an alias"),
            }
        });
    }

    #[test]
    fn builtin_wins_and_warns() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("myco.toml"),
            "[verbs.status]\ntarget = \"view\"\n",
        )
        .unwrap();
        no_env(|| {
            let (verbs, warnings) = named_verbs(dir.path(), test_loader(), BUILTINS).unwrap();
            assert!(verbs.is_empty());
            assert_eq!(warnings.len(), 1);
            assert!(warnings[0].contains("built-in wins"));
            // resolving the shadowed name reports the collision
            let err = resolve(dir.path(), "status", test_loader(), BUILTINS).unwrap_err();
            assert!(err.contains("built-in wins"), "{err}");
        });
    }

    #[test]
    fn bad_entries_warn_and_skip() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("myco.toml"),
            "[verbs.\"bad name\"]\ntarget = \"view\"\n\n\
             [verbs.nope]\ntarget = \"not-a-verb\"\n\n\
             [verbs.mutant]\ntarget = \"todo.add\"\n\n\
             [verbs.wired]\ntarget = \"initialize\"\n\n\
             [verbs.ok]\ntarget = \"status\"\n",
        )
        .unwrap();
        no_env(|| {
            let (verbs, warnings) = named_verbs(dir.path(), test_loader(), BUILTINS).unwrap();
            assert_eq!(verbs.len(), 1);
            assert_eq!(verbs[0].name, "ok");
            assert_eq!(warnings.len(), 4, "{warnings:?}");
            assert!(warnings.iter().any(|w| w.contains("invalid name")));
            assert!(warnings.iter().any(|w| w.contains("unknown alias target")));
            assert!(warnings
                .iter()
                .any(|w| w.contains("isn't a read-only verb") && w.contains("todo.add")));
            assert!(warnings.iter().any(|w| w.contains("initialize")));
        });
    }

    #[test]
    fn validate_repo_verbs_reports_each_bad_entry() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("myco.toml"),
            "[verbs.nope]\ntarget = \"not-a-verb\"\n\n\
             [verbs.bad]\nemit = \"ids\"\nfilter = { claimed = true, owner = [\"devin\"] }\n\n\
             [verbs.status]\ntarget = \"view\"\n",
        )
        .unwrap();
        no_env(|| {
            let errs = validate_repo_verbs(dir.path(), test_loader(), BUILTINS).unwrap();
            assert_eq!(errs.len(), 3, "{errs:?}");
            assert!(errs.iter().all(|(n, _)| !n.is_empty()));
        });
    }

    #[test]
    fn no_config_resolves_nothing() {
        let dir = tempfile::tempdir().unwrap();
        no_env(|| {
            let (verbs, warnings) = named_verbs(dir.path(), test_loader(), BUILTINS).unwrap();
            assert!(verbs.is_empty() && warnings.is_empty());
            assert!(resolve(dir.path(), "x", test_loader(), BUILTINS).is_err());
        });
    }
}
