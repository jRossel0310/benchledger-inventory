//! Search query grammar: syntactic, kind-blind tokenizer for the inventory
//! search bar. Splits raw input into free-text terms, `key:value` filters,
//! and boolean flags. Infallible — every input string produces some
//! `ParsedQuery`, however degenerate. No DB knowledge lives here: Task 4's
//! DB layer resolves attribute keys against the schema and executes the
//! parsed filters. A future web TS twin re-implements this exact grammar.

/// Comparison operator extracted from a filter's value prefix.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterOp {
    Eq,
    Lt,
    Le,
    Gt,
    Ge,
    /// An "a..b" range; `value` keeps the raw "a..b" string unsplit — the
    /// DB layer's range splitter handles it (see
    /// `inventory-db::attributes::split_range`).
    Range,
}

/// A single `key:value` search filter, before the DB layer resolves `key`
/// against the schema (an attribute key, `has`, or another reserved word).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RawFilter {
    pub key: String,
    pub op: FilterOp,
    pub value: String,
}

/// Boolean flags recognized by reserved query words (`is:archived`,
/// `is:active`, `is:low`, bare `low stock` / `low-stock`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct QueryFlags {
    pub low_stock: bool,
    /// `None` = don't filter on archived state; `Some(true)` = archived
    /// only; `Some(false)` = active only.
    pub archived: Option<bool>,
}

/// Result of parsing a raw search-bar string.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ParsedQuery {
    /// Space-joined free-text terms, order preserved. Empty when the
    /// query is only filters/flags.
    pub text: String,
    pub filters: Vec<RawFilter>,
    pub flags: QueryFlags,
}

/// Parse a raw search-bar string into free text, filters, and flags.
/// Infallible: every input, including the empty string, produces a
/// `ParsedQuery`.
pub fn parse_query(input: &str) -> ParsedQuery {
    let tokens = tokenize(input);
    let mut text_terms: Vec<String> = Vec::new();
    let mut filters: Vec<RawFilter> = Vec::new();
    let mut flags = QueryFlags {
        low_stock: false,
        archived: None,
    };

    let mut i = 0;
    while i < tokens.len() {
        let tok = tokens[i].as_str();

        // Bare "low stock" (two tokens) or "low-stock" (one token).
        if tok == "low" && tokens.get(i + 1).map(String::as_str) == Some("stock") {
            flags.low_stock = true;
            i += 2;
            continue;
        }
        if tok == "low-stock" {
            flags.low_stock = true;
            i += 1;
            continue;
        }

        if let Some(colon) = tok.find(':') {
            let key = tok[..colon].to_string();
            let raw_value = &tok[colon + 1..];
            let (op, value) = extract_op(raw_value);

            if key == "is" && op == FilterOp::Eq {
                match value.as_str() {
                    "archived" => {
                        flags.archived = Some(true);
                        i += 1;
                        continue;
                    }
                    "active" => {
                        flags.archived = Some(false);
                        i += 1;
                        continue;
                    }
                    "low" => {
                        flags.low_stock = true;
                        i += 1;
                        continue;
                    }
                    _ => {}
                }
            }

            filters.push(RawFilter { key, op, value });
            i += 1;
            continue;
        }

        text_terms.push(tok.to_string());
        i += 1;
    }

    ParsedQuery {
        text: text_terms.join(" "),
        filters,
        flags,
    }
}

/// Split raw input into tokens on whitespace, treating double-quoted spans
/// (which may contain whitespace) as atomic. Quote characters are kept in
/// the token here; `extract_op` strips them from filter values afterward.
fn tokenize(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for c in input.chars() {
        if c == '"' {
            in_quotes = !in_quotes;
            current.push(c);
        } else if c.is_whitespace() && !in_quotes {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
        } else {
            current.push(c);
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// Extract a comparison operator from a filter value's prefix, per the
/// grammar: `>=`/`<=`/`>`/`<`/`=` strip to their operator (value keeps the
/// remainder); a value containing ".." is a `Range` and keeps its full raw
/// string; a fully double-quoted value is a literal (`Eq`, quotes
/// stripped, no prefix scanning); anything else is `Eq` verbatim.
fn extract_op(value: &str) -> (FilterOp, String) {
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        return (FilterOp::Eq, value[1..value.len() - 1].to_string());
    }
    if let Some(rest) = value.strip_prefix(">=") {
        (FilterOp::Ge, rest.to_string())
    } else if let Some(rest) = value.strip_prefix("<=") {
        (FilterOp::Le, rest.to_string())
    } else if let Some(rest) = value.strip_prefix('>') {
        (FilterOp::Gt, rest.to_string())
    } else if let Some(rest) = value.strip_prefix('<') {
        (FilterOp::Lt, rest.to_string())
    } else if let Some(rest) = value.strip_prefix('=') {
        (FilterOp::Eq, rest.to_string())
    } else if value.contains("..") {
        (FilterOp::Range, value.to_string())
    } else {
        (FilterOp::Eq, value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pure_text_two_terms() {
        let q = parse_query("10k 0603");
        assert_eq!(q.text, "10k 0603");
        assert!(q.filters.is_empty());
        assert_eq!(
            q.flags,
            QueryFlags {
                low_stock: false,
                archived: None
            }
        );
    }

    #[test]
    fn pure_text_three_terms() {
        let q = parse_query("100n ceramic 50v");
        assert_eq!(q.text, "100n ceramic 50v");
        assert!(q.filters.is_empty());
        assert_eq!(
            q.flags,
            QueryFlags {
                low_stock: false,
                archived: None
            }
        );
    }

    #[test]
    fn filter_defaults_to_eq() {
        let q = parse_query("project:lightning");
        assert_eq!(q.text, "");
        assert_eq!(
            q.filters,
            vec![RawFilter {
                key: "project".to_string(),
                op: FilterOp::Eq,
                value: "lightning".to_string(),
            }]
        );
    }

    #[test]
    fn filter_bin_code() {
        let q = parse_query("bin:A12");
        assert_eq!(
            q.filters,
            vec![RawFilter {
                key: "bin".to_string(),
                op: FilterOp::Eq,
                value: "A12".to_string(),
            }]
        );
    }

    #[test]
    fn filter_has_stays_raw() {
        let q = parse_query("has:datasheet");
        assert_eq!(
            q.filters,
            vec![RawFilter {
                key: "has".to_string(),
                op: FilterOp::Eq,
                value: "datasheet".to_string(),
            }]
        );
    }

    #[test]
    fn filter_lt_operator() {
        let q = parse_query("available:<10");
        assert_eq!(
            q.filters,
            vec![RawFilter {
                key: "available".to_string(),
                op: FilterOp::Lt,
                value: "10".to_string(),
            }]
        );
    }

    #[test]
    fn filter_ge_operator() {
        let q = parse_query("voltage:>=25V");
        assert_eq!(
            q.filters,
            vec![RawFilter {
                key: "voltage".to_string(),
                op: FilterOp::Ge,
                value: "25V".to_string(),
            }]
        );
    }

    #[test]
    fn filter_range_operator_keeps_raw_string() {
        let q = parse_query("capacitance:10nF..1uF");
        assert_eq!(
            q.filters,
            vec![RawFilter {
                key: "capacitance".to_string(),
                op: FilterOp::Range,
                value: "10nF..1uF".to_string(),
            }]
        );
    }

    #[test]
    fn filter_lt_operator_with_unit_suffix() {
        let q = parse_query("height:<10mm");
        assert_eq!(
            q.filters,
            vec![RawFilter {
                key: "height".to_string(),
                op: FilterOp::Lt,
                value: "10mm".to_string(),
            }]
        );
    }

    #[test]
    fn bare_low_stock_sets_flag_and_consumes_both_tokens() {
        let q = parse_query("low stock");
        assert_eq!(q.text, "");
        assert!(q.filters.is_empty());
        assert!(q.flags.low_stock);
    }

    #[test]
    fn is_archived_sets_flag() {
        let q = parse_query("is:archived");
        assert_eq!(q.flags.archived, Some(true));
        assert!(q.filters.is_empty());
        assert_eq!(q.text, "");
    }

    #[test]
    fn quoted_value_preserves_internal_space() {
        let q = parse_query(r#"bin:"Drawer 3""#);
        assert_eq!(
            q.filters,
            vec![RawFilter {
                key: "bin".to_string(),
                op: FilterOp::Eq,
                value: "Drawer 3".to_string(),
            }]
        );
        assert_eq!(q.text, "");
    }

    #[test]
    fn mixed_text_and_filters() {
        let q = parse_query("dual op amp project:amp has:footprint");
        assert_eq!(q.text, "dual op amp");
        assert_eq!(
            q.filters,
            vec![
                RawFilter {
                    key: "project".to_string(),
                    op: FilterOp::Eq,
                    value: "amp".to_string(),
                },
                RawFilter {
                    key: "has".to_string(),
                    op: FilterOp::Eq,
                    value: "footprint".to_string(),
                },
            ]
        );
    }

    #[test]
    fn empty_input_yields_defaults() {
        let q = parse_query("");
        assert_eq!(
            q,
            ParsedQuery {
                text: String::new(),
                filters: Vec::new(),
                flags: QueryFlags {
                    low_stock: false,
                    archived: None
                },
            }
        );
    }

    // Additional coverage beyond the brief's enumerated list, exercising
    // other parts of the documented contract (is:active / is:low, and the
    // hyphenated low-stock token / non-matching bare "low").
    #[test]
    fn is_active_and_is_low_set_flags() {
        let q = parse_query("is:active");
        assert_eq!(q.flags.archived, Some(false));
        assert!(q.filters.is_empty());

        let q = parse_query("is:low");
        assert!(q.flags.low_stock);
        assert!(q.filters.is_empty());
    }

    #[test]
    fn hyphenated_low_stock_token_sets_flag() {
        let q = parse_query("low-stock");
        assert!(q.flags.low_stock);
        assert_eq!(q.text, "");
    }

    #[test]
    fn bare_low_without_stock_is_free_text() {
        let q = parse_query("low power");
        assert_eq!(q.text, "low power");
        assert!(!q.flags.low_stock);
    }

    /// Curated input list for the TS query-grammar twin
    /// (`packages/shared/src/query.ts`). Covers free text, every `FilterOp`,
    /// quoted phrases, `has:`/`is:` flags, and the grammar's documented
    /// quirks: an operator prefix wins over range detection even though the
    /// remaining value still contains ".." (`resistance:>1..5` -> `Gt` with
    /// value `"1..5"`); an unterminated quote never closes, so `in_quotes`
    /// stays true for the rest of the input and the whole remainder
    /// (including embedded spaces and the lone `"`) becomes one token/value;
    /// a bare quoted free-text term (no `key:` prefix) is never unwrapped —
    /// quote-stripping only happens inside `extract_op` for filter values.
    const QUERY_FIXTURE_INPUTS: &[&str] = &[
        "",
        "10k 0603",
        "100n ceramic 50v",
        "project:lightning",
        "bin:A12",
        "has:datasheet",
        "available:<10",
        "available:<=10",
        "voltage:>=25V",
        "resistance:>100",
        "capacitance:10nF..1uF",
        "height:<10mm",
        "low stock",
        "low-stock",
        "low power",
        "is:archived",
        "is:active",
        "is:low",
        r#"bin:"Drawer 3""#,
        "dual op amp project:amp has:footprint",
        "resistance:>1..5",
        r#"tag:"foo bar"#,
        r#""hello world""#,
        "10 kΩ project:µF",
        "is:archived is:low bin:A1 low stock 10k",
        r#"key:"=5""#,
        "bin:",
        "url:http://example.com",
        // U+FEFF (ZERO WIDTH NO-BREAK SPACE / BOM) is NOT whitespace to
        // `char::is_whitespace`, so it rides inside the token — pinned here
        // because JS `/\s/` DOES class it as whitespace and the TS twin must
        // exclude it explicitly to match (see query.ts's isWhitespace).
        "cap\u{FEFF}0603",
    ];

    #[derive(serde::Serialize)]
    struct QueryFixtureCase {
        input: String,
        parsed: ParsedQuery,
    }

    #[derive(serde::Serialize)]
    struct QueryFixture {
        comment: String,
        cases: Vec<QueryFixtureCase>,
    }

    /// Generates (or, without `EXPORT_QUERY_FIXTURE`, verifies)
    /// `packages/shared/fixtures/query-cases.json` — the drift-lock for the
    /// TS `parseQuery` twin, mirroring how `export_bindings`
    /// (`apps/desktop/src-tauri/src/commands.rs`) locks `bindings.gen.ts` to
    /// the Rust command surface. Regenerate after a legitimate grammar
    /// change with:
    ///
    /// `EXPORT_QUERY_FIXTURE=1 cargo test -p inventory-core query_cases_fixture_matches_generator`
    ///
    /// then commit the result. Without the env var, this test builds the
    /// fixture fresh from the current `parse_query` and asserts it is
    /// byte-identical to the committed file, printing the freshly generated
    /// JSON on mismatch so a stale fixture fails loudly instead of silently
    /// drifting from the parser it's meant to lock.
    #[test]
    fn query_cases_fixture_matches_generator() {
        const COMMITTED_PATH: &str = "../../packages/shared/fixtures/query-cases.json";

        let cases: Vec<QueryFixtureCase> = QUERY_FIXTURE_INPUTS
            .iter()
            .map(|&input| QueryFixtureCase {
                input: input.to_string(),
                parsed: parse_query(input),
            })
            .collect();
        let fixture = QueryFixture {
            comment:
                "Generated by inventory-core::search::tests::query_cases_fixture_matches_generator \
                      -- do not hand-edit. Each case: raw input string + the exact ParsedQuery \
                      parse_query(input) produces (field names snake_case, FilterOp/enum values \
                      snake_case serde output). Regenerate with EXPORT_QUERY_FIXTURE=1 cargo test \
                      -p inventory-core query_cases_fixture_matches_generator."
                    .to_string(),
            cases,
        };
        let mut fresh =
            serde_json::to_string_pretty(&fixture).expect("fixture is plain serde data");
        fresh.push('\n');

        if std::env::var_os("EXPORT_QUERY_FIXTURE").is_some() {
            std::fs::write(COMMITTED_PATH, &fresh).expect("failed to write query-cases.json");
            return;
        }

        let committed = std::fs::read_to_string(COMMITTED_PATH).unwrap_or_default();
        assert_eq!(
            committed.replace("\r\n", "\n"),
            fresh,
            "packages/shared/fixtures/query-cases.json is stale relative to parse_query. \
             Regenerate with `EXPORT_QUERY_FIXTURE=1 cargo test -p inventory-core \
             query_cases_fixture_matches_generator` and commit the result.\n\nFresh JSON:\n{fresh}"
        );
    }
}
