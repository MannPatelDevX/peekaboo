//! Parsers for `claudex` CLI output. Ported from claudex-dash `src/parse.ts`
//! (MIT, © 2026 Aditya Thummar; see THIRD_PARTY_NOTICES.md).
//!
//! claudex has no `--json` and self-updates, so its printed text is the only contract. Every parser
//! returns None on a shape it does not recognise rather than inventing values; the UI then shows the
//! raw text.
use serde::Serialize;

const BORDER: &[char] = &['┌', '┬', '┐', '├', '┼', '┤', '└', '┴', '┘'];
const BARS: &[char] = &['█', '░'];
/// claudex prints an em dash when it has no reading at all, which must not become 0.
const NO_DATA: char = '—';
const CLI_ERRORS: &[&str] = &["isn't a thing", "does not exist", "unrecognized arguments"];

fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn is_cli_error(text: &str) -> bool {
    CLI_ERRORS.iter().any(|e| text.contains(e))
}

/// Splits a leading status glyph (● ○ ✓ ✗ …) from the rest of a trimmed line.
fn glyph_line(line: &str) -> Option<(char, &str)> {
    let t = line.trim();
    let g = t.chars().next()?;
    let rest = &t[g.len_utf8()..];
    (rest.starts_with(char::is_whitespace)).then(|| (g, rest.trim_start()))
}

/// Splits "label   detail" on the first run of two or more spaces.
fn label_detail(s: &str) -> Option<(&str, &str)> {
    let i = s.find("  ")?;
    Some((s[..i].trim(), s[i..].trim()))
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Gauge {
    pub pct: Option<u32>,
    pub at: Option<String>,
}

/// `"███░░░░░░░  30%  11:39pm"` → 30% at 11:39pm; `"░░░  —"` → no reading.
pub fn gauge(cell: &str) -> Gauge {
    let none = Gauge { pct: None, at: None };
    let body: String = cell.chars().filter(|c| !BARS.contains(c)).collect();
    let body = body.trim();
    if body.is_empty() || body.starts_with(NO_DATA) {
        return none;
    }
    let digits: String = body.chars().take_while(char::is_ascii_digit).collect();
    let Some(rest) = body[digits.len()..].trim_start().strip_prefix('%') else { return none };
    let Ok(pct) = digits.parse() else { return none };
    let at = rest.trim();
    Gauge { pct: Some(pct), at: (!at.is_empty()).then(|| at.to_string()) }
}

/// `usage` and `list` print the same box-drawing table.
fn box_table(text: &str) -> Option<(Vec<String>, Vec<Vec<String>>)> {
    let mut headers = None;
    let mut rows = Vec::new();
    for line in strip_ansi(text).lines() {
        if !line.contains('│') || line.contains(BORDER) {
            continue;
        }
        let parts: Vec<&str> = line.split('│').collect();
        if parts.len() < 3 {
            continue;
        }
        let cells: Vec<String> = parts[1..parts.len() - 1].iter().map(|c| c.trim().to_string()).collect();
        if headers.is_none() {
            headers = Some(cells);
        } else {
            rows.push(cells);
        }
    }
    let headers = headers?;
    (!rows.is_empty()).then_some((headers, rows))
}

fn cell(r: &[String], i: usize) -> String {
    r.get(i).cloned().unwrap_or_default()
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UsageRow {
    pub account: String,
    pub active: bool,
    pub tier: String,
    pub session: Gauge,
    pub week: Gauge,
    pub updated: String,
}

pub fn parse_usage(text: &str) -> Option<Vec<UsageRow>> {
    if is_cli_error(text) {
        return None;
    }
    let (headers, rows) = box_table(text)?;
    if headers.get(3).map(String::as_str) != Some("SESSION") {
        return None;
    }
    Some(
        rows.iter()
            .map(|r| UsageRow {
                active: cell(r, 0) == "●",
                account: cell(r, 1),
                tier: cell(r, 2),
                session: gauge(&cell(r, 3)),
                week: gauge(&cell(r, 4)),
                updated: cell(r, 5),
            })
            .collect(),
    )
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AccountRow {
    pub account: String,
    pub active: bool,
    pub email: String,
    pub org: String,
    pub plan: String,
    pub saved: String,
}

pub fn parse_list(text: &str) -> Option<Vec<AccountRow>> {
    if is_cli_error(text) {
        return None;
    }
    let (headers, rows) = box_table(text)?;
    if headers.get(2).map(String::as_str) != Some("EMAIL") {
        return None;
    }
    Some(
        rows.iter()
            .map(|r| AccountRow {
                active: cell(r, 0) == "●",
                account: cell(r, 1),
                email: cell(r, 2),
                org: cell(r, 3),
                plan: cell(r, 4),
                saved: cell(r, 5),
            })
            .collect(),
    )
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Current {
    pub account: String,
    pub email: String,
    pub org: String,
    pub plan: String,
}

/// `● brian` / `brian@x.com` / `acme corp  ·  team`
pub fn parse_current(text: &str) -> Option<Current> {
    if is_cli_error(text) {
        return None;
    }
    let clean = strip_ansi(text);
    let ls: Vec<&str> = clean.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let (g, account) = glyph_line(ls.first()?)?;
    if g != '●' && g != '○' {
        return None;
    }
    let mut meta = ls.get(2).unwrap_or(&"").split('·').map(|s| s.trim().to_string());
    Some(Current {
        account: account.to_string(),
        email: ls.get(1).unwrap_or(&"").to_string(),
        org: meta.next().unwrap_or_default(),
        plan: meta.next().unwrap_or_default(),
    })
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PoolFlag {
    pub on: bool,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PoolStatus {
    /// This account borrows from the pool (token-swap) instead of its own token.
    pub consuming: PoolFlag,
    pub sharing: PoolFlag,
}

pub fn parse_pool_status(text: &str) -> Option<PoolStatus> {
    if is_cli_error(text) {
        return None;
    }
    let (mut consuming, mut sharing) = (None, None);
    for line in strip_ansi(text).lines() {
        let Some((g, rest)) = glyph_line(line) else { continue };
        let Some((label, detail)) = label_detail(rest) else { continue };
        let flag = Some(PoolFlag { on: g == '●', detail: detail.to_string() });
        match label {
            "consuming" if g == '●' || g == '○' => consuming = flag,
            "sharing" if g == '●' || g == '○' => sharing = flag,
            _ => {}
        }
    }
    Some(PoolStatus { consuming: consuming?, sharing: sharing? })
}

/// Reads the `autoswitch` row of `claudex doctor`: exactly "disabled" means off; anything else
/// (it describes hook install state when on) means on.
pub fn parse_autoswitch(doctor: &str) -> Option<bool> {
    strip_ansi(doctor).lines().find_map(|line| {
        let (_, rest) = glyph_line(line)?;
        let (label, detail) = label_detail(rest)?;
        (label == "autoswitch").then(|| detail != "disabled")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(format!("{}/tests/fixtures/claudex/{name}.txt", env!("CARGO_MANIFEST_DIR"))).unwrap()
    }

    #[test]
    fn gauge_cells() {
        assert_eq!(gauge("███░░░░░░░  32%  11:39pm"), Gauge { pct: Some(32), at: Some("11:39pm".into()) });
        assert_eq!(gauge("░░░░░░░░   0%"), Gauge { pct: Some(0), at: None });
        assert_eq!(gauge("░░░░░░░░   —"), Gauge { pct: None, at: None });
        assert_eq!(gauge(""), Gauge { pct: None, at: None });
    }

    #[test]
    fn usage_fixture() {
        let rows = parse_usage(&fixture("usage")).unwrap();
        assert_eq!(rows.len(), 4);
        let brian = &rows[1];
        assert_eq!((brian.account.as_str(), brian.active, brian.tier.as_str()), ("brian", true, "Max 5x"));
        assert_eq!(brian.session.pct, Some(32));
        assert_eq!(brian.week, Gauge { pct: Some(87), at: Some("Aug 20 4:29am".into()) });
        assert_eq!(rows[3].week.pct, Some(100));
        assert!(!rows[0].active);
    }

    #[test]
    fn list_fixture() {
        let rows = parse_list(&fixture("list")).unwrap();
        assert_eq!(rows.iter().map(|r| r.account.as_str()).collect::<Vec<_>>(), ["alice", "brian", "cheryl", "daniel"]);
        assert_eq!(rows[1].email, "brian.cooley@example.com");
        assert!(rows[1].active);
    }

    #[test]
    fn tables_are_not_confused() {
        assert!(parse_usage(&fixture("list")).is_none());
        assert!(parse_list(&fixture("usage")).is_none());
    }

    #[test]
    fn current_fixture() {
        let c = parse_current(&fixture("current")).unwrap();
        assert_eq!((c.account.as_str(), c.email.as_str(), c.org.as_str(), c.plan.as_str()),
            ("brian", "brian.cooley@example.com", "acme corp", "team"));
    }

    #[test]
    fn pool_status_fixture() {
        let p = parse_pool_status(&fixture("pool-status")).unwrap();
        assert!(!p.consuming.on);
        assert!(p.sharing.on);
        assert!(p.consuming.detail.starts_with("token-swap OFF"));
    }

    #[test]
    fn autoswitch_from_doctor() {
        assert_eq!(parse_autoswitch(&fixture("doctor")), Some(false));
        assert_eq!(parse_autoswitch("  ✓ autoswitch   Stop + SessionStart hooks present\n"), Some(true));
        assert_eq!(parse_autoswitch("  ✓ macOS\n"), None);
    }

    #[test]
    fn ansi_and_cli_errors() {
        let colored = fixture("current").replace("brian\n", "\x1b[1mbrian\x1b[0m\n");
        assert_eq!(parse_current(&colored).unwrap().account, "brian");
        assert!(parse_usage("error: unrecognized arguments: --json").is_none());
        assert!(parse_current("'frob' isn't a thing").is_none());
        assert!(parse_pool_status("something new").is_none());
    }
}
