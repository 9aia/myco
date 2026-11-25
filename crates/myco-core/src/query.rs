//! Query-language filters (P-058) — a gh-projects-shaped string
//! compiled onto [`ViewFilter`]. `key:value` tokens set filter
//! fields; bare tokens are substring text terms matched by the
//! caller against title/id/labels/repo. Shared by `--q`/`--query`
//! on `view`/`next`, the TUI `/` filter, `myco ui --view`, and
//! the `[views.*] filter = "…"` config sugar.
//!
//! Grammar (phase 3 — negation, `OR`, relative dates):
//!
//! ```text
//! owner:devin is:open pri:>=high label:bug epic:v2 due:>=today-7d
//! is:open OR label:bug -owner:bot fix
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
//! - `due:` — `YYYY-MM-DD`, `today`/`tomorrow`/`yesterday`,
//!   `Nd`/`Nw` offsets (`due:7d`, `due:-3d`), an optional
//!   `[+-]N[dw]` adjust on any date (`today-7d`), and ordering
//!   ops `>=`/`<=`/`>`/`<`/`=` (`due:>=today`, `due:<2026-12-01`)
//!   mapping onto `due`/`due_min`/`due_max`. `est:`/`estimate:` —
//!   points; `superseded:` — `true`/`false`.
//! - `-term` — negation on any token or bare word: `-owner:devin`
//!   compiles onto the filter's `not` list, `-bug` onto the
//!   branch's negated text terms.
//! - `OR` — an uppercase standalone token separates alternatives:
//!   each side is a full conjunctive clause (its own `key:value`
//!   terms, text, and `-` negations). No parenthesization —
//!   `a b OR c` reads `(a AND b) OR c`.
//!
//! Repeated tokens merge per field (a set field replaces, like
//! [`ViewFilter::overlay`]); `ViewSpec::validate` invariants
//! (`claimed`+`owner`, `pri_min > pri_max`) surface as parse
//! errors per alternative.

use std::str::FromStr;

use time::Duration;

use crate::date;
use crate::view::{ViewFilter, ViewSpec};
use crate::{kind::KindTable, Actor, Item, Priority, Status};

/// A compiled query — the OR alternatives plus, per alternative,
/// the structured filter and the bare text terms the eval
/// surfaces apply as extra predicates.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ViewQuery {
    /// OR alternatives — exactly one branch when the query has
    /// no `OR`.
    pub branches: Vec<QueryBranch>,
}

/// One conjunctive query branch — a [`ViewFilter`] plus its bare
/// text terms (positive and `-`-negated). The query matches an
/// item when *any* branch does.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueryBranch {
    /// `key:value`/`-key:value` terms compiled onto the filter —
    /// negated key terms land in `filter.not`.
    pub filter: ViewFilter,
    /// Bare tokens — each must substring-match (case-insensitive)
    /// the title, id, a label, or the repo tag; AND across terms.
    pub text: Vec<String>,
    /// `-word` bare-token exclusions — an item matching any of
    /// these substring terms is rejected (text has no
    /// `ViewFilter` home, so negated text rides the branch, not
    /// `filter.not`).
    pub not_text: Vec<String>,
}

impl QueryBranch {
    /// Every positive text term substring-matches — AND across
    /// terms. Empty `text` matches everything.
    fn text_matches(&self, item: &Item, repo: Option<&str>) -> bool {
        self.text.iter().all(|t| text_hit(t, item, repo))
    }

    /// No negated text term substring-matches.
    fn not_text_matches(&self, item: &Item, repo: Option<&str>) -> bool {
        self.not_text.iter().all(|t| !text_hit(t, item, repo))
    }
}

/// One term substring-matches (case-insensitive) the title, the
/// id, a label, or the repo tag.
fn text_hit(term: &str, item: &Item, repo: Option<&str>) -> bool {
    let t = term.to_lowercase();
    item.title.to_lowercase().contains(&t)
        || item.id.to_string().to_lowercase().contains(&t)
        || repo.is_some_and(|r| r.to_lowercase().contains(&t))
        || item.labels.iter().any(|l| l.to_lowercase().contains(&t))
}

impl ViewQuery {
    /// The query as one filter — a lone branch unwraps to its
    /// filter; `OR` alternatives land in `any_of` (each a full
    /// [`ViewFilter`], so nested `not`s ride along). Bare text
    /// has no `ViewFilter` home — [`ViewQuery::matches`] applies
    /// it, [`ViewQuery::spec`] drops it like before.
    pub fn filter(&self) -> ViewFilter {
        match self.branches.as_slice() {
            [b] => b.filter.clone(),
            _ => ViewFilter {
                any_of: self.branches.iter().map(|b| b.filter.clone()).collect(),
                ..ViewFilter::default()
            },
        }
    }

    /// The query as a standalone spec — default group/sort/limit/
    /// format (the `myco ui --view "<query>"` shape).
    pub fn spec(&self) -> ViewSpec {
        ViewSpec {
            filter: self.filter(),
            ..ViewSpec::default()
        }
    }

    /// The whole predicate — any branch whose filter AND text
    /// terms (positive and negated) match. `kinds` resolves
    /// `kind:` tokens inside branch filters.
    pub fn matches(&self, item: &Item, repo: Option<&str>, kinds: &KindTable) -> bool {
        self.branches.is_empty()
            || self.branches.iter().any(|b| {
                b.filter.matches(item, kinds)
                    && b.text_matches(item, repo)
                    && b.not_text_matches(item, repo)
            })
    }

    /// Any bare text term (positive or `-`-negated) — the gate
    /// post-filter surfaces use to skip the text pass.
    pub fn has_text(&self) -> bool {
        self.branches
            .iter()
            .any(|b| !b.text.is_empty() || !b.not_text.is_empty())
    }

    /// Every bare term across branches — `-x` negations keep
    /// their `-` prefix. For seeds and "mostly text" heuristics.
    pub fn text(&self) -> Vec<String> {
        self.branches
            .iter()
            .flat_map(|b| {
                b.text
                    .iter()
                    .cloned()
                    .chain(b.not_text.iter().map(|t| format!("-{t}")))
            })
            .collect()
    }
}

/// Failure to parse a query string — bad key, bad value, a bad
/// `OR`/`-` placement, or a spec-level invariant (`claimed`+`owner`,
/// inverted pri bounds).
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

/// Parse `input` into a [`ViewQuery`] — relative dates resolve
/// against today (UTC). `me` resolves `@me`/`me` owner terms —
/// `None` makes them a parse error (config has no actor).
pub fn parse(input: &str, me: Option<&Actor>) -> Result<ViewQuery, ParseQueryError> {
    parse_at(input, me, date::today())
}

/// [`parse`] with an explicit clock — tests pin `today`.
pub(crate) fn parse_at(
    input: &str,
    me: Option<&Actor>,
    today: time::Date,
) -> Result<ViewQuery, ParseQueryError> {
    if input.trim().is_empty() {
        return Ok(ViewQuery {
            branches: vec![QueryBranch::default()],
        });
    }
    // `OR` splits top-level alternatives — each side is a full
    // conjunctive clause; an empty side is a parse error.
    let mut raw: Vec<Vec<&str>> = vec![Vec::new()];
    for tok in input.split_whitespace() {
        if tok == "OR" {
            raw.push(Vec::new());
        } else {
            raw.last_mut().expect("raw starts non-empty").push(tok);
        }
    }
    if raw.iter().any(|a| a.is_empty()) {
        return Err(err("OR needs a clause on both sides"));
    }
    let mut branches = Vec::with_capacity(raw.len());
    for alt in raw {
        branches.push(parse_branch(&alt, me, today)?);
    }
    // spec-level invariants become parse errors, per alternative
    for b in &branches {
        ViewSpec {
            filter: b.filter.clone(),
            ..ViewSpec::default()
        }
        .validate()
        .map_err(err)?;
    }
    Ok(ViewQuery { branches })
}

/// One OR alternative — its tokens AND together into a branch.
fn parse_branch(
    tokens: &[&str],
    me: Option<&Actor>,
    today: time::Date,
) -> Result<QueryBranch, ParseQueryError> {
    let mut branch = QueryBranch::default();
    for tok in tokens {
        // `-term` negates the next clause — a `key:value` token
        // compiles onto a one-field `not` sub-filter, a bare word
        // onto `not_text`.
        let (neg, body) = match tok.strip_prefix('-') {
            Some(rest) if !rest.is_empty() => (true, rest),
            _ => (false, *tok),
        };
        // bare claim flags — `claimed`/`unclaimed` need no value
        match body {
            "claimed" | "unclaimed" => {
                let claimed = body == "claimed";
                if neg {
                    branch.filter.not.push(ViewFilter {
                        claimed: Some(claimed),
                        ..ViewFilter::default()
                    });
                } else {
                    branch.filter.claimed = Some(claimed);
                }
                continue;
            }
            _ => {}
        }
        match body.split_once(':') {
            Some((key, value)) if is_key(key) => {
                if neg {
                    let mut nf = ViewFilter::default();
                    apply(&mut nf, key, value, me, today)?;
                    branch.filter.not.push(nf);
                } else {
                    apply(&mut branch.filter, key, value, me, today)?;
                }
            }
            Some((key, _)) if qualifier_like(key) => {
                return Err(err(format!(
                    "unknown query key {key:?} — expected one of: {}",
                    KEYS.join("|")
                )));
            }
            _ => {
                if neg {
                    branch.not_text.push(body.to_string());
                } else {
                    branch.text.push(body.to_string());
                }
            }
        }
    }
    Ok(branch)
}

/// The config-sugar entry (`filter = "owner:devin is:open"`) —
/// the query must be all `key:value` terms *or* `OR`/`-` filter
/// terms; bare text has no `ViewFilter` home. `@me` errors (no
/// actor in config). Relative dates resolve at load time.
pub fn parse_filter(input: &str) -> Result<ViewFilter, ParseQueryError> {
    let q = parse(input, None)?;
    if q.has_text() {
        return Err(err(format!(
            "bare text terms ({}) aren't supported in a config filter — use key:value terms only",
            q.text().join(" ")
        )));
    }
    Ok(q.filter())
}

/// One `key:value` token onto `filter`.
fn apply(
    filter: &mut ViewFilter,
    key: &str,
    value: &str,
    me: Option<&Actor>,
    today: time::Date,
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
        "due" => apply_due(filter, value, today)?,
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

/// `due:` value — `[op] date-expr`. Bare/`=` is exact (`due`),
/// `>=`/`>` set `due_min`, `<=`/`<` set `due_max` (`>`/`<` shift
/// the bound a day). An exact write clears the bounds and vice
/// versa — last token wins per side, like the other fields.
fn apply_due(
    filter: &mut ViewFilter,
    value: &str,
    today: time::Date,
) -> Result<(), ParseQueryError> {
    let (op, rest) = [">=", "<=", ">", "<", "="]
        .iter()
        .find_map(|op| value.strip_prefix(op).map(|r| (*op, r)))
        .unwrap_or(("", value));
    let date = due_date(rest, today).map_err(|e| err(format!("due:{value}: {e}")))?;
    let day = Duration::days(1);
    match op {
        "" | "=" => {
            filter.due = Some(date);
            filter.due_min = None;
            filter.due_max = None;
        }
        ">=" => {
            filter.due = None;
            filter.due_min = Some(date);
        }
        ">" => {
            filter.due = None;
            filter.due_min = Some(
                date.checked_add(day)
                    .ok_or_else(|| err(format!("due:{value}: date overflow")))?,
            );
        }
        "<=" => {
            filter.due = None;
            filter.due_max = Some(date);
        }
        "<" => {
            filter.due = None;
            filter.due_max = Some(
                date.checked_sub(day)
                    .ok_or_else(|| err(format!("due:{value}: date overflow")))?,
            );
        }
        _ => unreachable!(),
    }
    Ok(())
}

/// A `due:` date expression → an absolute `Date`:
/// `today`/`tomorrow`/`yesterday`, `Nd`/`Nw` offsets from today
/// (`7d`, `-3d`, `2w`), `YYYY-MM-DD`, or any of those with a
/// trailing `[+-]N[dw]` adjust (`today-7d`, `2026-12-01+2w`).
fn due_date(expr: &str, today: time::Date) -> Result<time::Date, String> {
    let e = expr.to_ascii_lowercase();
    // trailing adjust — `<base><sign><N><unit>`; the sign is the
    // separator so `today-7d` is base `today` delta `-7d`.
    if let Some((base, delta)) = split_adjust(&e) {
        let d = due_date(base, today)?;
        return d
            .checked_add(delta)
            .ok_or_else(|| format!("{expr:?}: date overflow"));
    }
    match e.as_str() {
        "today" => Ok(today),
        "tomorrow" => today
            .checked_add(Duration::days(1))
            .ok_or_else(|| "tomorrow: date overflow".to_string()),
        "yesterday" => today
            .checked_sub(Duration::days(1))
            .ok_or_else(|| "yesterday: date overflow".to_string()),
        _ => {
            if let Ok(d) = date::parse(&e) {
                return Ok(d);
            }
            offset_date(&e, today).ok_or_else(|| {
                format!("expected YYYY-MM-DD|today|tomorrow|yesterday|Nd|Nw, got {expr:?}")
            })
        }
    }
}

/// Split a trailing `[+-]N[dw]` adjust off `e` — returns the base
/// expr and the signed delta. `-3d` alone is an offset, not an
/// adjust (no base), so the separator must sit past index 0.
fn split_adjust(e: &str) -> Option<(&str, Duration)> {
    let bytes = e.as_bytes();
    // the last `+`/`-` past index 0 — an ISO date's own `-`s fail
    // the offset parse and end the scan (no adjust).
    for i in (1..bytes.len()).rev() {
        if bytes[i] == b'+' || bytes[i] == b'-' {
            return offset_duration(&e[i + 1..])
                .map(|d| (&e[..i], if bytes[i] == b'-' { -d } else { d }));
        }
    }
    None
}

/// `N[dw]` (unsigned) → a `Duration` — `d` days, `w` weeks.
fn offset_duration(s: &str) -> Option<Duration> {
    let (num, unit) = s.split_at(s.len().checked_sub(1)?);
    let n: i64 = num.parse().ok()?;
    match unit {
        "d" => Some(Duration::days(n)),
        "w" => Some(Duration::weeks(n)),
        _ => None,
    }
}

/// `[+-]?N[dw]` → `today` shifted — the bare-offset form.
fn offset_date(e: &str, today: time::Date) -> Option<time::Date> {
    let (sign, rest) = match e.strip_prefix('-') {
        Some(r) => (-1i64, r),
        None => (1i64, e.strip_prefix('+').unwrap_or(e)),
    };
    let d = offset_duration(rest)?;
    let days = d.whole_days() * sign;
    today.checked_add(Duration::days(days))
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

    /// A pinned clock for relative-date assertions.
    fn at(input: &str) -> ViewQuery {
        let today = date::parse("2026-03-10").unwrap();
        parse_at(input, Some(&devin()), today).unwrap()
    }

    #[test]
    fn key_value_terms_compile() {
        let q = parse_ok("owner:devin is:open pri:>=high label:bug epic:v2 milestone:m1 cycle:s1 due:2026-12-01 est:3");
        assert_eq!(q.branches.len(), 1);
        let f = &q.branches[0].filter;
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
        assert!(q.branches[0].text.is_empty());
    }

    #[test]
    fn is_and_kind_take_statuses_and_kinds() {
        let q = parse_ok("is:in-progress kind:issue kind:T is:done");
        assert_eq!(
            q.branches[0].filter.status,
            vec![Status::InProgress, Status::Done]
        );
        assert_eq!(
            q.branches[0].filter.kind,
            vec!["issue".to_string(), "T".to_string()]
        );
        // status wins on overlap — a kind literally named "open"
        // is unreachable through is:/kind:
        let q = parse_ok("is:open");
        assert_eq!(q.branches[0].filter.status, vec![Status::Open]);
        assert!(q.branches[0].filter.kind.is_empty());
    }

    #[test]
    fn pri_operators() {
        let q = parse_ok("pri:crit");
        assert_eq!(q.branches[0].filter.pri_min, Some(Priority::Critical));
        assert_eq!(q.branches[0].filter.pri_max, Some(Priority::Critical));
        let q = parse_ok("priority:<=med pri:>low");
        assert_eq!(q.branches[0].filter.pri_max, Some(Priority::Medium));
        assert_eq!(q.branches[0].filter.pri_min, Some(Priority::Medium)); // >low → med
        assert!(parse("pri:>crit", None).is_err());
        assert!(parse("pri:<low", None).is_err());
        assert!(parse("pri:sideways", None).is_err());
    }

    #[test]
    fn owner_aliases_and_me() {
        let q = parse_ok("assignee:@me owner:human:luis");
        assert_eq!(
            q.branches[0].filter.owner,
            vec![devin(), "human:luis".parse().unwrap()]
        );
        // me without a resolved actor errors
        assert!(parse("owner:me", None).is_err());
        assert!(parse("owner:@me", None).is_err());
        // invalid actor ids surface
        assert!(parse("owner:a::b", Some(&devin())).is_err());
    }

    #[test]
    fn claimed_forms() {
        let q = parse_ok("claimed");
        assert_eq!(q.branches[0].filter.claimed, Some(true));
        let q = parse_ok("unclaimed");
        assert_eq!(q.branches[0].filter.claimed, Some(false));
        let q = parse_ok("claimed:false");
        assert_eq!(q.branches[0].filter.claimed, Some(false));
        assert!(parse("claimed:maybe", Some(&devin())).is_err());
        // claimed + owner is a spec error → parse error
        assert!(parse("unclaimed owner:devin", Some(&devin())).is_err());
    }

    #[test]
    fn pri_bounds_inversion_is_a_parse_error() {
        assert!(parse("pri:>=crit pri:<=low", Some(&devin())).is_err());
        assert!(parse("pri:>=med pri:<=crit", Some(&devin())).is_ok());
        // …including inside an OR alternative
        assert!(parse("is:open OR pri:>=crit pri:<=low", Some(&devin())).is_err());
    }

    #[test]
    fn bare_tokens_are_text_and_match() {
        let q = parse_ok("owner:devin fix bug");
        assert_eq!(
            q.branches[0].text,
            vec!["fix".to_string(), "bug".to_string()]
        );
        let item = Item::new(ItemId::new(ItemKind::Task, 1), "fix the bug")
            .label("bugfix")
            .owner(devin());
        assert!(q.matches(&item, None, crate::kind::prelude_table()));
        assert!(!q.matches(
            &Item::new(ItemId::new(ItemKind::Task, 2), "other"),
            None,
            crate::kind::prelude_table()
        ));
        // id + repo tag match too
        let q = parse_ok("t-001 myco");
        assert!(q.matches(&item, Some("myco"), crate::kind::prelude_table()));
        assert!(!q.matches(&item, Some("other-repo"), crate::kind::prelude_table()));
        // empty text matches everything
        assert!(parse_ok("owner:devin").matches(&item, None, crate::kind::prelude_table()));
    }

    #[test]
    fn or_splits_alternatives() {
        let q = parse_ok("owner:devin OR owner:human:luis");
        assert_eq!(q.branches.len(), 2);
        assert_eq!(q.branches[0].filter.owner, vec![devin()]);
        assert_eq!(
            q.branches[1].filter.owner,
            vec!["human:luis".parse().unwrap()]
        );
        // the collapsed filter carries the alternatives in any_of
        let f = q.filter();
        assert_eq!(f.any_of.len(), 2);
        assert!(f.owner.is_empty());
        // each side is a full clause — `a b OR c` = (a AND b) OR c
        let q = parse_ok("is:open label:bug OR is:done");
        assert_eq!(q.branches.len(), 2);
        assert_eq!(q.branches[0].filter.status, vec![Status::Open]);
        assert_eq!(q.branches[0].filter.label, vec!["bug".to_string()]);
        assert_eq!(q.branches[1].filter.status, vec![Status::Done]);
        // bare text distributes into its own branch
        let q = parse_ok("fix OR bug");
        assert_eq!(q.branches[0].text, vec!["fix".to_string()]);
        assert_eq!(q.branches[1].text, vec!["bug".to_string()]);
        // lowercase `or` is text, not an operator
        let q = parse_ok("a or b");
        assert_eq!(q.branches.len(), 1);
        assert_eq!(q.branches[0].text.len(), 3);
        // empty sides error
        assert!(parse("is:open OR", Some(&devin())).is_err());
        assert!(parse("OR is:open", Some(&devin())).is_err());
        assert!(parse("is:open OR OR is:done", Some(&devin())).is_err());
    }

    #[test]
    fn negation_compiles_onto_not() {
        let q = parse_ok("-owner:devin is:open");
        let f = &q.branches[0].filter;
        assert_eq!(f.status, vec![Status::Open]);
        assert_eq!(f.not.len(), 1);
        assert_eq!(f.not[0].owner, vec![devin()]);
        // several `-` terms stack
        let q = parse_ok("-is:done -label:bug");
        assert_eq!(q.branches[0].filter.not.len(), 2);
        // negated bare words land in not_text, not the filter
        let q = parse_ok("-bug fix");
        assert_eq!(q.branches[0].not_text, vec!["bug".to_string()]);
        assert_eq!(q.branches[0].text, vec!["fix".to_string()]);
        assert!(q.branches[0].filter.not.is_empty());
        // `-claimed`/`-unclaimed` negate the claim flags
        let q = parse_ok("-claimed");
        assert_eq!(q.branches[0].filter.not[0].claimed, Some(true));
        let q = parse_ok("-unclaimed");
        assert_eq!(q.branches[0].filter.not[0].claimed, Some(false));
        // a leading `-` parses — no more "negation isn't supported"
        assert!(parse("-owner:devin", Some(&devin())).is_ok());
        // `-` alone stays text
        let q = parse_ok("-");
        assert_eq!(q.branches[0].text, vec!["-".to_string()]);
    }

    #[test]
    fn negation_and_or_compose_in_eval() {
        let kinds = crate::kind::prelude_table();
        // `a OR b -c` reads `a OR (b AND NOT c)` — the `-` binds
        // to its own alternative
        let q = parse_ok("owner:devin OR owner:human:luis -is:done");
        let devin_item = Item::new(ItemId::new(ItemKind::Task, 1), "a").owner(devin());
        let luis_item =
            Item::new(ItemId::new(ItemKind::Task, 2), "b").owner("human:luis".parse().unwrap());
        let other = Item::new(ItemId::new(ItemKind::Task, 3), "c").owner("cli".parse().unwrap());
        let done_devin = Item::new(ItemId::new(ItemKind::Task, 4), "d")
            .owner(devin())
            .status(Status::Done);
        let done_luis = Item::new(ItemId::new(ItemKind::Task, 7), "e")
            .owner("human:luis".parse().unwrap())
            .status(Status::Done);
        assert!(q.matches(&devin_item, None, kinds));
        assert!(q.matches(&luis_item, None, kinds));
        // done+devin still matches — branch 1 carries no `-is:done`
        assert!(q.matches(&done_devin, None, kinds));
        // done+luis fails — branch 2's negation excludes it
        assert!(!q.matches(&done_luis, None, kinds));
        assert!(!q.matches(&other, None, kinds));
        // `OR` splits first, so a global exclusion repeats per
        // branch — there's no shared clause outside the
        // alternatives
        let q = parse_ok("owner:devin -is:done OR owner:human:luis -is:done");
        assert!(!q.matches(&done_devin, None, kinds));
        assert!(!q.matches(&done_luis, None, kinds));
        assert!(q.matches(&devin_item, None, kinds));
        // negated text excludes
        let q = parse_ok("-bug");
        let buggy = Item::new(ItemId::new(ItemKind::Task, 5), "has bug");
        let clean = Item::new(ItemId::new(ItemKind::Task, 6), "clean");
        assert!(!q.matches(&buggy, None, kinds));
        assert!(q.matches(&clean, None, kinds));
        // the collapsed filter evaluates the filter half — bare
        // text (`-bug`) has no `ViewFilter` home, so `matches`
        // is the predicate that sees it
        let f = q.filter();
        assert!(f.matches(&clean, kinds));
        assert!(f.matches(&buggy, kinds));
        assert!(!q.matches(&buggy, None, kinds));
        assert!(q.matches(&clean, None, kinds));
    }

    #[test]
    fn due_relative_dates() {
        // pinned clock: 2026-03-10
        let q = at("due:today");
        assert_eq!(
            q.branches[0].filter.due,
            Some(date::parse("2026-03-10").unwrap())
        );
        let q = at("due:tomorrow");
        assert_eq!(
            q.branches[0].filter.due,
            Some(date::parse("2026-03-11").unwrap())
        );
        let q = at("due:yesterday");
        assert_eq!(
            q.branches[0].filter.due,
            Some(date::parse("2026-03-09").unwrap())
        );
        let q = at("due:7d");
        assert_eq!(
            q.branches[0].filter.due,
            Some(date::parse("2026-03-17").unwrap())
        );
        let q = at("due:-3d");
        assert_eq!(
            q.branches[0].filter.due,
            Some(date::parse("2026-03-07").unwrap())
        );
        let q = at("due:2w");
        assert_eq!(
            q.branches[0].filter.due,
            Some(date::parse("2026-03-24").unwrap())
        );
        let q = at("due:today-7d");
        assert_eq!(
            q.branches[0].filter.due,
            Some(date::parse("2026-03-03").unwrap())
        );
        let q = at("due:2026-12-01+1w");
        assert_eq!(
            q.branches[0].filter.due,
            Some(date::parse("2026-12-08").unwrap())
        );
        // ordering ops → bounds
        let q = at("due:>=today");
        assert_eq!(
            q.branches[0].filter.due_min,
            Some(date::parse("2026-03-10").unwrap())
        );
        assert!(q.branches[0].filter.due.is_none());
        let q = at("due:>today");
        assert_eq!(
            q.branches[0].filter.due_min,
            Some(date::parse("2026-03-11").unwrap())
        );
        let q = at("due:<2026-12-01");
        assert_eq!(
            q.branches[0].filter.due_max,
            Some(date::parse("2026-11-30").unwrap())
        );
        let q = at("due:<=today");
        assert_eq!(
            q.branches[0].filter.due_max,
            Some(date::parse("2026-03-10").unwrap())
        );
        let q = at("due:>today-7d");
        assert_eq!(
            q.branches[0].filter.due_min,
            Some(date::parse("2026-03-04").unwrap())
        );
        // bounds on both sides merge; an exact write clears them
        let q = at("due:>=today due:<=2026-12-31");
        let f = &q.branches[0].filter;
        assert!(f.due.is_none());
        assert!(f.due_min.is_some() && f.due_max.is_some());
        let q = at("due:>=today due:2026-01-01");
        let f = &q.branches[0].filter;
        assert_eq!(f.due, Some(date::parse("2026-01-01").unwrap()));
        assert!(f.due_min.is_none() && f.due_max.is_none());
        // bad exprs error
        assert!(parse("due:soon", None).is_err());
        assert!(parse("due:3x", None).is_err());
        assert!(parse("due:", None).is_err());
    }

    #[test]
    fn unknown_keys_and_negation_parse() {
        assert!(parse("wat:thing", Some(&devin())).is_err());
        // qualifier-shaped but unknown → error; digit-led stays text
        let q = parse_ok("10:30");
        assert_eq!(q.branches[0].text, vec!["10:30".to_string()]);
        // status/kind value errors name the token
        let e = parse("status:bogus", Some(&devin())).unwrap_err();
        assert!(e.0.contains("status:bogus"), "{e}");
        // a negated unknown key still errors
        assert!(parse("-wat:thing", Some(&devin())).is_err());
    }

    #[test]
    fn parse_filter_rejects_text() {
        assert!(parse_filter("owner:devin is:open").is_ok());
        // OR and `-` filter terms are fine in config
        let f = parse_filter("owner:devin OR owner:human:luis -is:done").unwrap();
        assert_eq!(f.any_of.len(), 2);
        assert_eq!(f.any_of[1].not.len(), 1);
        assert!(parse_filter("owner:devin bug").is_err());
        assert!(parse_filter("-bug").is_err());
        assert!(parse_filter("owner:@me").is_err());
        // relative dates resolve at load time
        assert!(parse_filter("due:today").is_ok());
        assert!(parse_filter("due:>=today").is_ok());
    }

    #[test]
    fn spec_and_roundtrip_shape() {
        let q = parse_ok("is:open owner:devin");
        let spec = q.spec();
        spec.validate().unwrap();
        assert_eq!(spec.filter.status, vec![Status::Open]);
        assert_eq!(spec.format, crate::ViewFormat::List);
        // OR queries spec() into any_of
        let spec = parse_ok("is:open OR is:done").spec();
        spec.validate().unwrap();
        assert_eq!(spec.filter.any_of.len(), 2);
        assert!(spec.filter.status.is_empty());
        // text()/has_text() feed the seed + narrow checks
        let q = parse_ok("fix -bug owner:devin");
        assert!(q.has_text());
        assert_eq!(q.text(), vec!["fix".to_string(), "-bug".to_string()]);
        assert!(!parse_ok("owner:devin").has_text());
    }
}
