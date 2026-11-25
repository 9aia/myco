//! Query-language filters (P-058) — a gh-projects-shaped string
//! compiled onto [`ViewFilter`]. `key:value` tokens set filter
//! fields; bare tokens are substring text terms matched by the
//! caller against title/id/labels/repo. Shared by `--q`/`--query`
//! on `view`/`next`, the TUI `/` filter, `myco ui --view`, and
//! the `[views.*] filter = "…"` config sugar.
//!
//! Grammar (phase 1 — no negation, no OR):
//!
//! ```text
//! owner:devin is:open pri:>=high label:bug epic:v2 due:2026-12-01
//! ```
//!
//! - `kind:`/`is:` — a status token (`open`, `in-progress`, …) or
//!   a kind name/prefix (`task`, `T`); status wins on overlap.
//! - `status:` — status tokens (repeatable, OR within the field).
//! - `pri:`/`priority:` — `>=p`, `<=p`, `>p`, `<p`, or bare `p`
//!   (exact — sets both bounds).
//! - `label:` — `#`-less label (a leading `#` is stripped).
//! - `owner:`/`assignee:` — an actor id; `@me`/`me` resolves to
//!   the [`Actor`] the caller passes in.
//! - `claimed:`/`unclaimed` — owner-presence (`claimed:true`,
//!   bare `claimed`, bare `unclaimed`).
//! - `epic:`/`milestone:`/`cycle:` — exact names.
//! - `due:` — `YYYY-MM-DD`; `est:`/`estimate:` — points;
//!   `superseded:` — `true`/`false`.
//! - Anything else is a bare text term.
//!
//! Repeated tokens merge per field (a set field replaces, like
//! [`ViewFilter::overlay`]); `ViewSpec::validate` invariants
//! (`claimed`+`owner`, `pri_min > pri_max`) surface as parse
//! errors. Negation (`-term`) and `OR` are phase 3 — a leading
//! `-` is a parse error, not silent text.

use std::str::FromStr;

use crate::date;
use crate::view::{ViewFilter, ViewSpec};
use crate::{Actor, Item, Priority, Status};

/// A compiled query — the structured filter plus the bare text
/// terms the eval surfaces apply as an extra predicate.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ViewQuery {
    /// `key:value` terms compiled onto the filter fields.
    pub filter: ViewFilter,
    /// Bare tokens — each must substring-match (case-insensitive)
    /// the title, id, a label, or the repo tag; AND across terms.
    pub text: Vec<String>,
}

impl ViewQuery {
    /// The query as a standalone spec — default group/sort/limit/
    /// format (the `myco ui --view "<query>"` shape).
    pub fn spec(&self) -> ViewSpec {
        ViewSpec {
            filter: self.filter.clone(),
            ..ViewSpec::default()
        }
    }

    /// Every text term substring-matches (case-insensitive) the
    /// title, the id, a label, or the repo tag — AND across
    /// terms. Empty `text` matches everything.
    pub fn text_matches(&self, item: &Item, repo: Option<&str>) -> bool {
        self.text.iter().all(|t| {
            let t = t.to_lowercase();
            item.title.to_lowercase().contains(&t)
                || item.id.to_string().to_lowercase().contains(&t)
                || repo.is_some_and(|r| r.to_lowercase().contains(&t))
                || item.labels.iter().any(|l| l.to_lowercase().contains(&t))
        })
    }
}

/// Failure to parse a query string — bad key, bad value, or a
/// spec-level invariant (`claimed`+`owner`, inverted pri bounds).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct ParseQueryError(pub String);

fn err(msg: impl Into<String>) -> ParseQueryError {
    ParseQueryError(msg.into())
}

/// The `key:` qualifiers — anything else before a `:` that looks
/// qualifier-shaped is an "unknown key" error, not text.
const KEYS: &[&str] = &[
    "kind",
    "is",
    "status",
    "pri",
    "priority",
    "label",
    "owner",
    "assignee",
    "claimed",
    "epic",
    "milestone",
    "due",
    "cycle",
    "est",
    "estimate",
    "superseded",
];

/// `key` is a known qualifier (case-insensitive).
fn is_key(key: &str) -> bool {
    KEYS.contains(&key.to_ascii_lowercase().as_str())
}

/// `key` looks like a qualifier attempt — alphabetic, so `10:30`
/// or `T-001:x` stay text while `fix:bug` errors as unknown.
fn qualifier_like(key: &str) -> bool {
    !key.is_empty()
        && key
            .bytes()
            .all(|b| b.is_ascii_alphabetic() || b == b'-' || b == b'_')
}

/// Parse `input` into a [`ViewQuery`]. `me` resolves `@me`/`me`
/// owner terms — `None` makes them a parse error (config has no
/// actor).
pub fn parse(input: &str, me: Option<&Actor>) -> Result<ViewQuery, ParseQueryError> {
    let mut filter = ViewFilter::default();
    let mut text = Vec::new();
    for tok in input.split_whitespace() {
        if tok.starts_with('-') {
            return Err(err(format!(
                "unexpected {tok:?} — negation isn't supported yet (P-058 phase 3)"
            )));
        }
        // bare claim flags — `claimed`/`unclaimed` need no value
        match tok {
            "claimed" => {
                filter.claimed = Some(true);
                continue;
            }
            "unclaimed" => {
                filter.claimed = Some(false);
                continue;
            }
            _ => {}
        }
        match tok.split_once(':') {
            Some((key, value)) if is_key(key) => apply(&mut filter, key, value, me)?,
            Some((key, _)) if qualifier_like(key) => {
                return Err(err(format!(
                    "unknown query key {key:?} — expected one of: {}",
                    KEYS.join("|")
                )));
            }
            _ => text.push(tok.to_string()),
        }
    }
    // spec-level invariants become parse errors
    ViewSpec {
        filter: filter.clone(),
        ..ViewSpec::default()
    }
    .validate()
    .map_err(err)?;
    Ok(ViewQuery { filter, text })
}

/// The config-sugar entry (`filter = "owner:devin is:open"`) —
/// the query must be all `key:value` terms; bare text has no
/// `ViewFilter` home. `@me` errors (no actor in config).
pub fn parse_filter(input: &str) -> Result<ViewFilter, ParseQueryError> {
    let q = parse(input, None)?;
    if !q.text.is_empty() {
        return Err(err(format!(
            "bare text terms ({}) aren't supported in a config filter — use key:value terms only",
            q.text.join(" ")
        )));
    }
    Ok(q.filter)
}

/// One `key:value` token onto `filter`.
fn apply(
    filter: &mut ViewFilter,
    key: &str,
    value: &str,
    me: Option<&Actor>,
) -> Result<(), ParseQueryError> {
    if value.is_empty() {
        return Err(err(format!("{key}: needs a value")));
    }
    match key.to_ascii_lowercase().as_str() {
        // kind names/prefixes + status shorthands — a status
        // token wins on overlap (`is:open` → status, `is:task`
        // → kind)
        "kind" | "is" => {
            if let Ok(s) = Status::from_str(value) {
                filter.status.push(s);
            } else {
                filter.kind.push(value.to_string());
            }
        }
        "status" => {
            let s = Status::from_str(value).map_err(|e| err(format!("status:{value}: {e}")))?;
            filter.status.push(s);
        }
        "pri" | "priority" => apply_pri(filter, value)?,
        "label" => filter.label.push(value.trim_start_matches('#').to_string()),
        "owner" | "assignee" => {
            let actor = match value {
                "me" | "@me" => me
                    .cloned()
                    .ok_or_else(|| err("owner:@me needs a resolved actor"))?,
                _ => value
                    .parse::<Actor>()
                    .map_err(|e| err(format!("owner:{value}: {e}")))?,
            };
            filter.owner.push(actor);
        }
        "claimed" => {
            filter.claimed = Some(parse_bool(value).map_err(|e| err(format!("claimed:{e}")))?);
        }
        "epic" => filter.epic = Some(value.to_string()),
        "milestone" => filter.milestone = Some(value.to_string()),
        "cycle" => filter.cycle = Some(value.to_string()),
        "due" => {
            let d = date::parse(value).map_err(|e| err(format!("due:{value}: {e}")))?;
            filter.due = Some(d);
        }
        "est" | "estimate" => {
            let n = value
                .parse::<u32>()
                .map_err(|_| err(format!("est:{value}: expected a point estimate")))?;
            filter.estimate = Some(n);
        }
        "superseded" => {
            filter.superseded =
                Some(parse_bool(value).map_err(|e| err(format!("superseded:{e}")))?);
        }
        _ => unreachable!("is_key gates apply"),
    }
    Ok(())
}

/// `pri:`/`priority:` value — `>=p`, `<=p`, `>p`, `<p`, or bare
/// `p` (exact: both bounds).
fn apply_pri(filter: &mut ViewFilter, value: &str) -> Result<(), ParseQueryError> {
    let (op, name) = [">=", "<=", ">", "<"]
        .iter()
        .find_map(|op| value.strip_prefix(op).map(|rest| (*op, rest)))
        .unwrap_or(("", value));
    let p = name
        .parse::<Priority>()
        .map_err(|e| err(format!("pri:{value}: {e}")))?;
    match op {
        "" => {
            filter.pri_min = Some(p);
            filter.pri_max = Some(p);
        }
        ">=" => filter.pri_min = Some(p),
        "<=" => filter.pri_max = Some(p),
        ">" => {
            filter.pri_min = Some(
                Priority::ALL
                    .iter()
                    .position(|x| *x == p)
                    .and_then(|i| Priority::ALL.get(i + 1).copied())
                    .ok_or_else(|| err(format!("pri:{value}: no priority above {}", p.token())))?,
            );
        }
        "<" => {
            filter.pri_max = Some(
                Priority::ALL
                    .iter()
                    .position(|x| *x == p)
                    .and_then(|i| i.checked_sub(1).map(|j| Priority::ALL[j]))
                    .ok_or_else(|| err(format!("pri:{value}: no priority below {}", p.token())))?,
            );
        }
        _ => unreachable!(),
    }
    Ok(())
}

/// `true`/`false` for the bool-valued keys.
fn parse_bool(value: &str) -> Result<bool, String> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(format!("expected true|false, got {value:?}")),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::ItemId;
    use crate::ItemKind;

    fn devin() -> Actor {
        "devin".parse().unwrap()
    }

    fn parse_ok(input: &str) -> ViewQuery {
        parse(input, Some(&devin())).unwrap()
    }

    #[test]
    fn key_value_terms_compile() {
        let q = parse_ok("owner:devin is:open pri:>=high label:bug epic:v2 milestone:m1 cycle:s1 due:2026-12-01 est:3");
        let f = &q.filter;
        assert_eq!(f.owner, vec![devin()]);
        assert_eq!(f.status, vec![Status::Open]);
        assert_eq!(f.pri_min, Some(Priority::High));
        assert_eq!(f.pri_max, None);
        assert_eq!(f.label, vec!["bug".to_string()]);
        assert_eq!(f.epic.as_deref(), Some("v2"));
        assert_eq!(f.milestone.as_deref(), Some("m1"));
        assert_eq!(f.cycle.as_deref(), Some("s1"));
        assert_eq!(f.due, Some(date::parse("2026-12-01").unwrap()));
        assert_eq!(f.estimate, Some(3));
        assert!(q.text.is_empty());
    }

    #[test]
    fn is_and_kind_take_statuses_and_kinds() {
        let q = parse_ok("is:in-progress kind:issue kind:T is:done");
        assert_eq!(q.filter.status, vec![Status::InProgress, Status::Done]);
        assert_eq!(q.filter.kind, vec!["issue".to_string(), "T".to_string()]);
        // status wins on overlap — a kind literally named "open"
        // is unreachable through is:/kind:
        let q = parse_ok("is:open");
        assert_eq!(q.filter.status, vec![Status::Open]);
        assert!(q.filter.kind.is_empty());
    }

    #[test]
    fn pri_operators() {
        let q = parse_ok("pri:crit");
        assert_eq!(q.filter.pri_min, Some(Priority::Critical));
        assert_eq!(q.filter.pri_max, Some(Priority::Critical));
        let q = parse_ok("priority:<=med pri:>low");
        assert_eq!(q.filter.pri_max, Some(Priority::Medium));
        assert_eq!(q.filter.pri_min, Some(Priority::Medium)); // >low → med
        assert!(parse("pri:>crit", None).is_err());
        assert!(parse("pri:<low", None).is_err());
        assert!(parse("pri:sideways", None).is_err());
    }

    #[test]
    fn owner_aliases_and_me() {
        let q = parse_ok("assignee:@me owner:human:luis");
        assert_eq!(q.filter.owner, vec![devin(), "human:luis".parse().unwrap()]);
        // me without a resolved actor errors
        assert!(parse("owner:me", None).is_err());
        assert!(parse("owner:@me", None).is_err());
        // invalid actor ids surface
        assert!(parse("owner:a::b", Some(&devin())).is_err());
    }

    #[test]
    fn claimed_forms() {
        let q = parse_ok("claimed");
        assert_eq!(q.filter.claimed, Some(true));
        let q = parse_ok("unclaimed");
        assert_eq!(q.filter.claimed, Some(false));
        let q = parse_ok("claimed:false");
        assert_eq!(q.filter.claimed, Some(false));
        assert!(parse("claimed:maybe", Some(&devin())).is_err());
        // claimed + owner is a spec error → parse error
        assert!(parse("unclaimed owner:devin", Some(&devin())).is_err());
    }

    #[test]
    fn pri_bounds_inversion_is_a_parse_error() {
        assert!(parse("pri:>=crit pri:<=low", Some(&devin())).is_err());
        assert!(parse("pri:>=med pri:<=crit", Some(&devin())).is_ok());
    }

    #[test]
    fn bare_tokens_are_text_and_match() {
        let q = parse_ok("owner:devin fix bug");
        assert_eq!(q.text, vec!["fix".to_string(), "bug".to_string()]);
        let item = Item::new(ItemId::new(ItemKind::Task, 1), "fix the bug")
            .label("bugfix")
            .owner(devin());
        assert!(q.text_matches(&item, None));
        assert!(!q.text_matches(&Item::new(ItemId::new(ItemKind::Task, 2), "other"), None));
        // id + repo tag match too
        let q = parse_ok("t-001 myco");
        assert!(q.text_matches(&item, Some("myco")));
        assert!(!q.text_matches(&item, Some("other-repo")));
        // empty text matches everything
        assert!(parse_ok("owner:devin").text_matches(&item, None));
    }

    #[test]
    fn unknown_keys_and_negation_error() {
        assert!(parse("wat:thing", Some(&devin())).is_err());
        assert!(parse("-owner:devin", Some(&devin())).is_err());
        assert!(parse("owner:", Some(&devin())).is_err());
        // qualifier-shaped but unknown → error; digit-led stays text
        let q = parse_ok("10:30");
        assert_eq!(q.text, vec!["10:30".to_string()]);
        // status/kind value errors name the token
        let e = parse("status:bogus", Some(&devin())).unwrap_err();
        assert!(e.0.contains("status:bogus"), "{e}");
    }

    #[test]
    fn parse_filter_rejects_text() {
        assert!(parse_filter("owner:devin is:open").is_ok());
        assert!(parse_filter("owner:devin bug").is_err());
        assert!(parse_filter("owner:@me").is_err());
    }

    #[test]
    fn spec_and_roundtrip_shape() {
        let q = parse_ok("is:open owner:devin");
        let spec = q.spec();
        spec.validate().unwrap();
        assert_eq!(spec.filter.status, vec![Status::Open]);
        assert_eq!(spec.format, crate::ViewFormat::List);
    }
}
