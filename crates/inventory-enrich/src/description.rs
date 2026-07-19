//! The always-available, offline `EnrichmentProvider`: parses a DigiKey-style
//! catalog description (`"RES 10K OHM 1% 1/4W 0603"`) into candidate fields
//! using `inventory_core`'s unit engine (`units::parse_with_kind`) and
//! package normalizer (`packages::normalize_package`).
//!
//! Never touches the network or the database, never fabricates a value it
//! can't confidently read off the text (an unmapped token is just left
//! alone, not guessed at), and never panics on garbage input. Everything it
//! emits is `EnrichSource::Inferred` with confidence < 1 — a catalog
//! description is shorthand, not a verified spec, so every candidate is
//! flagged for the user to confirm.

use std::collections::{HashMap, HashSet};

use inventory_core::packages::normalize_package;
use inventory_core::units::{parse_with_kind, ParsedValue, UnitKind};

use crate::model::{EnrichInput, EnrichSource, Enrichment, FieldCandidate};
use crate::provider::{EnrichError, EnrichmentProvider};

/// Confidence assigned to every candidate this parser emits. Never 1.0
/// (reserved for a trusted/manual/measured value) — a description is
/// catalog shorthand, not a verified spec.
const CONFIDENCE: f32 = 0.5;

pub struct DescriptionParser;

impl EnrichmentProvider for DescriptionParser {
    fn name(&self) -> &str {
        "description"
    }

    fn enrich(&self, input: &EnrichInput) -> Result<Option<Enrichment>, EnrichError> {
        let Some(description) = input.description.as_deref() else {
            return Ok(None);
        };
        let description = description.trim();
        if description.is_empty() {
            return Ok(None);
        }

        let tokens: Vec<&str> = description.split_whitespace().collect();
        let mut candidates = Vec::new();
        let mut notes = Vec::new();

        let category = classify_category(&tokens);
        if let Some(cat) = category {
            candidates.push(candidate("category", cat));
        }
        match category {
            Some("Resistor") => parse_resistor_tokens(&tokens, &mut candidates, &mut notes),
            Some("Capacitor") => parse_capacitor_tokens(&tokens, &mut candidates, &mut notes),
            _ => {}
        }

        // Package tokens are worth looking for regardless of category — an
        // IC's `8DIP` or an unclassified passive's `SOT-23` are just as
        // useful as a resistor's `0603`.
        if let Some(pkg) = find_package_token(&tokens) {
            candidates.push(candidate("variant.package", pkg));
        }

        // A provider must never hand the caller two different values for the
        // same key (see `dedupe_candidates`) — do this before the
        // empty-candidates check below, since an all-ambiguous description
        // should still get the "found nothing" note, not silently look like
        // a successful parse with zero candidates.
        let candidates = dedupe_candidates(candidates, &mut notes);
        if candidates.is_empty() {
            notes.push(format!(
                "description parser found no recognizable fields in '{description}'"
            ));
        }

        Ok(Some(Enrichment {
            provider: self.name().to_string(),
            candidates,
            images: Vec::new(),
            notes,
        }))
    }
}

fn candidate(key: &str, value: impl Into<String>) -> FieldCandidate {
    FieldCandidate {
        key: key.to_string(),
        value: value.into(),
        source: EnrichSource::Inferred,
        confidence: CONFIDENCE,
    }
}

/// Broad category from the description's leading token. Only returns a
/// category when a token maps unambiguously to a REAL row of the built-in
/// seeded taxonomy (`inventory-db`'s `seed.rs` `CATEGORIES` — the exact
/// string emitted here must match one of those names verbatim, since
/// nothing downstream can resolve a category name that was never seeded);
/// anything else is left unclassified (`None`) rather than guessed. There is
/// deliberately no generic "Integrated Circuit" catch-all: `seed.rs` has no
/// such row (only specific ones — "Logic IC", "Op amp", "Microcontroller",
/// etc.), so emitting it produced a candidate that could never be resolved.
fn classify_category(tokens: &[&str]) -> Option<&'static str> {
    let first = tokens.first()?.to_uppercase();
    match first.as_str() {
        "RES" => Some("Resistor"),
        "CAP" => Some("Capacitor"),
        "IC" => {
            let is_opamp = tokens
                .iter()
                .any(|t| t.eq_ignore_ascii_case("OPAMP") || t.eq_ignore_ascii_case("OP-AMP"));
            if is_opamp {
                // Matches the built-in "Op amp" category exactly.
                Some("Op amp")
            } else {
                // No confident mapping to a specific built-in IC category —
                // leave it unclassified. A coarse label that isn't in the
                // seeded taxonomy is strictly worse than no category at all.
                None
            }
        }
        _ => None,
    }
}

/// Whether `token` contains a character that could plausibly be part of a
/// unit or symbol (a letter, `%`, `Ω`, or a micro sign). The unit engine
/// itself accepts a bare number with no unit for ANY kind (a raw numeric
/// value is valid input from a form field), so without this guard a
/// package-like token such as `"0603"` would be misread as a bare
/// resistance/tolerance/power value. A catalog description needs this
/// extra check; the engine doesn't, since callers elsewhere always know
/// which field they're parsing.
fn has_unit_marker(token: &str) -> bool {
    token
        .chars()
        .any(|c| c.is_alphabetic() || c == '%' || c == 'Ω' || c == 'µ' || c == 'μ')
}

/// Try to parse `token` as a value of `kind`, first as written, then with a
/// lowercased copy. Returns the exact string that parsed, so a caller can
/// store a value that will round-trip through `Database::set_attribute`
/// (which calls `parse_with_kind` directly, with no case-folding of its
/// own) — that string is not necessarily the original token.
///
/// `parse_with_kind`'s SI-prefix matching is case-sensitive for the
/// lowercase-only prefixes `u`/`n`/`p`/`m` (uppercase `M` means mega — a
/// genuinely different value, not milli spelled loudly) but DigiKey
/// descriptions are conventionally all-caps (`"0.1UF"`, `"10PF"`), which the
/// as-written attempt then rejects outright.
///
/// Falling back to a fully lowercased retry on failure is safe precisely
/// because the ambiguous case never reaches it: `prefix_exp` already
/// recognizes uppercase `K`, `M`, and `G` with their correct (non-milli,
/// non-micro, non-nano) meaning, so any token using one of those correctly
/// succeeds on the first, as-written attempt and never falls through to the
/// lowercase retry. The retry only fires when the first attempt failed
/// outright — which for `U`/`N`/`P` means there was no valid uppercase
/// interpretation to lose by lowercasing.
///
/// Before either attempt, `token` is checked against
/// [`looks_like_part_number`] and rejected outright if it matches: a
/// transistor/diode MPN like `2N3904` or `2N2222` has the exact same
/// `<digits><letter><digits>` shape `parse_with_kind`'s embedded-prefix
/// grammar accepts as a value (`"2n3904"` lowercases and parses as
/// `2.3904` nΩ — a real bug this guard exists to close), so shape alone
/// isn't enough; this is the single choke point every kind's parsing goes
/// through, so the guard applies uniformly rather than needing a per-branch
/// check at each call site.
fn parse_value(token: &str, kind: UnitKind) -> Option<(String, ParsedValue)> {
    if looks_like_part_number(token) {
        return None;
    }
    if let Ok(v) = parse_with_kind(token, kind) {
        return Some((token.to_string(), v));
    }
    let lowered = token.to_lowercase();
    if lowered != token {
        if let Ok(v) = parse_with_kind(&lowered, kind) {
            return Some((lowered, v));
        }
    }
    None
}

/// Longest run of digits a genuine embedded-prefix value ever has after its
/// SI marker in a catalog description. Shorthand like `4k7` (4.7k), `10k2`
/// (10.2k), or a precision part's `4k75` (4.75k) expresses at most a couple
/// of extra significant digits; nothing in this parser's input needs three
/// or more.
const MAX_EMBEDDED_TAIL_DIGITS: usize = 2;

/// Whether `token` has the shape of a part number (`2N3904`, `2N2222`,
/// `1N4148`) rather than a genuine SI-embedded value (`4K7`, `10K2`,
/// `100N`): a numeric prefix, a single letter, then a run of
/// [`MAX_EMBEDDED_TAIL_DIGITS`]-or-fewer digits is a real value; a longer
/// all-digit run after the letter is instead the sequence number of a
/// JEDEC-style transistor/diode part (`1N`/`2N` + a 3-4 digit number) that
/// merely happens to start with `<digit><letter><digits>` — the same shape
/// the unit engine's embedded-prefix grammar accepts. The unit engine can't
/// tell these apart by shape alone (that's the bug this function exists to
/// close), so the description parser rejects the long-tail case itself
/// before ever handing the token to `parse_with_kind`.
fn looks_like_part_number(token: &str) -> bool {
    let chars: Vec<char> = token.chars().collect();
    let Some(marker_pos) = chars.iter().position(|c| c.is_ascii_alphabetic()) else {
        return false; // no letter at all — not this shape, let normal parsing decide
    };
    if marker_pos == 0 {
        return false; // no numeric prefix — not this shape either
    }
    let tail = &chars[marker_pos + 1..];
    tail.len() > MAX_EMBEDDED_TAIL_DIGITS && tail.iter().all(|c| c.is_ascii_digit())
}

/// Parse resistor-specific tokens (everything after the leading `"RES"`)
/// into tolerance / power / resistance candidates, based on the token's
/// suffix — never by trying every kind in turn, since the unit engine's
/// bare SI-prefix notation (`"10K"`) is valid under any kind (a prefix
/// letter alone doesn't encode which physical quantity it scales), so a
/// blind kind-by-kind trial would non-deterministically misclassify it.
fn parse_resistor_tokens(
    tokens: &[&str],
    candidates: &mut Vec<FieldCandidate>,
    notes: &mut Vec<String>,
) {
    for token in tokens.iter().skip(1) {
        let upper = token.to_uppercase();
        if upper == "OHM" || upper == "OHMS" {
            continue; // redundant with the resistance value token itself
        }
        if upper.ends_with('%') {
            if let Some((text, _)) = parse_value(token, UnitKind::Percent) {
                candidates.push(candidate("attr.tolerance", text));
            }
            continue;
        }
        if upper.ends_with('W') {
            if let Some((text, _)) = parse_value(token, UnitKind::Power) {
                candidates.push(candidate("attr.power_rating", text));
            }
            continue;
        }
        if has_unit_marker(token) {
            if looks_like_part_number(token) {
                // e.g. a "2N3904" sample/reference tucked into an assortment
                // kit's description — never a real resistance value, so
                // skip it rather than fabricate one (see `parse_value`).
                notes.push(format!(
                    "ignored '{token}' while looking for attr.resistance — looks like a part number, not a value"
                ));
                continue;
            }
            if let Some((text, _)) = parse_value(token, UnitKind::Resistance) {
                candidates.push(candidate("attr.resistance", text));
            }
        }
    }
}

/// Dielectric code/name -> the exact canonical string from `seed.rs`'s
/// `dielectric` choice list. Checked before the capacitance/voltage suffix
/// checks below, since `"Y5V"` ends in `V` and would otherwise be
/// misclassified as a voltage-rating token.
fn classify_dielectric(upper: &str) -> Option<&'static str> {
    Some(match upper {
        "C0G" | "NP0" | "C0G/NP0" => "C0G/NP0",
        "X7R" => "X7R",
        "X5R" => "X5R",
        "Y5V" => "Y5V",
        "TANTALUM" => "Tantalum",
        "FILM" => "Film",
        _ => return None,
    })
}

/// Parse capacitor-specific tokens (everything after the leading `"CAP"`)
/// into tolerance / dielectric / capacitance / voltage candidates.
fn parse_capacitor_tokens(
    tokens: &[&str],
    candidates: &mut Vec<FieldCandidate>,
    notes: &mut Vec<String>,
) {
    for token in tokens.iter().skip(1) {
        let upper = token.to_uppercase();
        if upper.ends_with('%') {
            if let Some((text, _)) = parse_value(token, UnitKind::Percent) {
                candidates.push(candidate("attr.tolerance", text));
            }
            continue;
        }
        if let Some(dielectric) = classify_dielectric(&upper) {
            candidates.push(candidate("attr.dielectric", dielectric));
            continue;
        }
        if upper.ends_with('F') {
            if looks_like_part_number(token) {
                notes.push(format!(
                    "ignored '{token}' while looking for attr.capacitance — looks like a part number, not a value"
                ));
                continue;
            }
            if let Some((text, _)) = parse_value(token, UnitKind::Capacitance) {
                candidates.push(candidate("attr.capacitance", text));
            }
            continue;
        }
        if upper.ends_with('V') {
            if let Some((text, _)) = parse_value(token, UnitKind::Voltage) {
                candidates.push(candidate("attr.voltage_rating", text));
            }
        }
        // Anything else (e.g. the "CER"/"ELEC" family hint) is left
        // unclassified rather than guessed at.
    }
}

/// Recognized package-family prefixes/suffixes, mirroring
/// `inventory_core::packages::normalize_package`'s family list. Kept
/// narrow (rather than accepting that function's permissive passthrough for
/// any unrecognized string) so a description word that merely happens to
/// survive `normalize_package` unmolested (e.g. `"CER"`) is never mistaken
/// for a real package code. Keep in sync with `packages.rs`.
const PACKAGE_FAMILIES: &[&str] = &[
    "PDIP", "DIP", "SOIC", "SOT", "TO", "QFN", "TQFP", "LQFP", "SSOP", "TSSOP", "MSOP", "BGA",
];

/// Bare imperial chip-package codes, mirroring `packages.rs`'s `CHIP_CODES`
/// imperial column.
const CHIP_CODES: &[&str] = &[
    "01005", "0201", "0402", "0603", "0805", "1206", "1210", "1812", "2010", "2512",
];

fn looks_like_package_token(token: &str) -> bool {
    let squished: String = token
        .to_uppercase()
        .chars()
        .filter(|c| !matches!(c, ' ' | '-' | '_'))
        .collect();
    if CHIP_CODES.contains(&squished.as_str()) {
        return true;
    }
    for family in PACKAGE_FAMILIES {
        if let Some(rest) = squished.strip_prefix(family) {
            if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
                return true;
            }
        }
        if let Some(front) = squished.strip_suffix(family) {
            if !front.is_empty() && front.chars().all(|c| c.is_ascii_digit()) {
                return true;
            }
        }
    }
    false
}

/// Find the first token that looks like a real package code and normalize
/// it via `inventory_core::packages::normalize_package`.
fn find_package_token(tokens: &[&str]) -> Option<String> {
    tokens
        .iter()
        .find(|t| looks_like_package_token(t))
        .and_then(|t| normalize_package(t))
        .map(|p| p.canonical)
}

/// Collapse same-key candidates to at most one before `enrich` returns them.
///
/// This provider can see the same identity key claimed by more than one
/// token in a single description (two tokens both look like a resistance
/// value, say). `run_chain`'s merge is a simple first-wins-per-key scan
/// over one flat candidate list, with no way to know two entries for the
/// same key came from this same provider and disagree — whichever one
/// happens to appear first silently wins, even if it's the wrong one. So
/// this provider must never hand back two candidates for the same key.
///
/// Two tokens that agree (identical value, e.g. the same token repeated)
/// collapse silently to one — that's not ambiguity, just redundancy. Two
/// tokens that disagree ARE genuinely ambiguous: rather than guess which of
/// two plausible values is correct (guessing is exactly the fabrication
/// this parser must never do — see the module doc), every candidate for
/// that key is dropped and a `"ambiguous <key> in description"` note is
/// added instead, so the caller knows the description had extractable but
/// conflicting values rather than none at all.
fn dedupe_candidates(
    candidates: Vec<FieldCandidate>,
    notes: &mut Vec<String>,
) -> Vec<FieldCandidate> {
    let mut first_key_order: Vec<String> = Vec::new();
    let mut values_by_key: HashMap<String, Vec<String>> = HashMap::new();
    for c in &candidates {
        let values = values_by_key.entry(c.key.clone()).or_insert_with(|| {
            first_key_order.push(c.key.clone());
            Vec::new()
        });
        if !values.contains(&c.value) {
            values.push(c.value.clone());
        }
    }

    let ambiguous_keys: HashSet<String> = first_key_order
        .iter()
        .filter(|key| values_by_key[*key].len() > 1)
        .cloned()
        .collect();
    for key in &first_key_order {
        if ambiguous_keys.contains(key) {
            notes.push(format!("ambiguous {key} in description"));
        }
    }

    let mut kept_keys: HashSet<String> = HashSet::new();
    candidates
        .into_iter()
        .filter(|c| !ambiguous_keys.contains(&c.key) && kept_keys.insert(c.key.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidates_for(description: &str) -> (Vec<FieldCandidate>, Vec<String>) {
        let parser = DescriptionParser;
        let input = EnrichInput {
            description: Some(description.to_string()),
            ..Default::default()
        };
        let result = parser
            .enrich(&input)
            .expect("description parser never errors")
            .expect("non-empty description always yields Some");
        (result.candidates, result.notes)
    }

    fn find<'a>(candidates: &'a [FieldCandidate], key: &str) -> Option<&'a FieldCandidate> {
        candidates.iter().find(|c| c.key == key)
    }

    #[test]
    fn provider_name_is_description() {
        assert_eq!(DescriptionParser.name(), "description");
    }

    #[test]
    fn resistor_description_parses_category_value_tolerance_power_and_package() {
        let (candidates, notes) = candidates_for("RES 10K OHM 1% 1/4W 0603");
        assert!(notes.is_empty(), "unexpected notes: {notes:?}");
        assert_eq!(find(&candidates, "category").unwrap().value, "Resistor");
        assert_eq!(find(&candidates, "attr.resistance").unwrap().value, "10K");
        assert_eq!(find(&candidates, "attr.tolerance").unwrap().value, "1%");
        assert_eq!(
            find(&candidates, "attr.power_rating").unwrap().value,
            "1/4W"
        );
        assert_eq!(find(&candidates, "variant.package").unwrap().value, "0603");
        for c in &candidates {
            assert_eq!(c.source, EnrichSource::Inferred);
            assert!(c.confidence < 1.0);
        }
    }

    #[test]
    fn capacitor_description_parses_category_capacitance_voltage_dielectric_and_package() {
        let (candidates, notes) = candidates_for("CAP CER 0.1UF 50V X7R 0603");
        assert!(notes.is_empty(), "unexpected notes: {notes:?}");
        assert_eq!(find(&candidates, "category").unwrap().value, "Capacitor");
        assert_eq!(
            find(&candidates, "attr.capacitance").unwrap().value,
            "0.1uf"
        );
        assert_eq!(
            find(&candidates, "attr.voltage_rating").unwrap().value,
            "50V"
        );
        assert_eq!(find(&candidates, "attr.dielectric").unwrap().value, "X7R");
        assert_eq!(find(&candidates, "variant.package").unwrap().value, "0603");
    }

    #[test]
    fn ic_description_maps_opamp_category_and_package() {
        let (candidates, notes) = candidates_for("IC OPAMP GP 2 CIRCUIT 8DIP");
        assert!(notes.is_empty(), "unexpected notes: {notes:?}");
        assert_eq!(find(&candidates, "category").unwrap().value, "Op amp");
        assert_eq!(find(&candidates, "variant.package").unwrap().value, "DIP-8");
    }

    #[test]
    fn generic_ic_description_yields_no_category_candidate() {
        // "Integrated Circuit" is not a row in inventory-db's seeded
        // `CATEGORIES` list (only specific ones — "Logic IC", "Op amp",
        // "Microcontroller", etc.) — a category name nothing downstream can
        // resolve is worse than no category candidate at all. The package
        // token is still real and still worth emitting.
        let (candidates, _) = candidates_for("IC MCU 32BIT ARM CORTEX M4 64QFN");
        assert!(find(&candidates, "category").is_none());
        assert!(find(&candidates, "variant.package").is_some());
    }

    #[test]
    fn package_only_description_yields_just_a_package_candidate() {
        let (candidates, notes) = candidates_for("SOT-23-5 TRANSISTOR NPN");
        assert!(notes.is_empty(), "unexpected notes: {notes:?}");
        assert!(find(&candidates, "category").is_none());
        assert_eq!(
            find(&candidates, "variant.package").unwrap().value,
            "SOT-23-5"
        );
        assert_eq!(candidates.len(), 1, "no fabricated extra candidates");
    }

    #[test]
    fn transistor_mpn_in_an_assortment_description_is_never_read_as_a_resistance_value() {
        // Regression for the fabrication bug: "2N3904" is a transistor part
        // number, not a resistance value, but its lowercased form
        // ("2n3904") happens to satisfy the unit engine's embedded-prefix
        // grammar ("2" + nano-prefix "n" + "3904") and used to be read as
        // 2.3904 nOhm with no note at all.
        let (candidates, notes) = candidates_for("RES ASSORTMENT KIT W/ 2N3904 SAMPLE");
        assert!(
            find(&candidates, "attr.resistance").is_none(),
            "must not fabricate a resistance from an MPN-shaped token: {candidates:?}"
        );
        assert!(
            notes.iter().any(|n| n.contains("2N3904")),
            "expected a note explaining the ignored token: {notes:?}"
        );
    }

    #[test]
    fn transistor_mpn_alongside_a_real_resistance_token_does_not_win_or_pollute_it() {
        // "2N2222" (a transistor MPN) sorts before the genuine "10K" token.
        // Before the fix this emitted attr.resistance TWICE — "2n2222"
        // first, "10K" second — and run_chain's first-wins merge kept the
        // fabricated one and silently dropped the real value.
        let (candidates, _notes) = candidates_for("RES 2N2222 10K OHM 1% 1/4W 0603");
        let resistance = find(&candidates, "attr.resistance");
        assert!(
            resistance.is_none_or(|c| c.value != "2n2222" && c.value != "2N2222"),
            "must never surface the fabricated MPN-derived value: {candidates:?}"
        );
        assert_eq!(
            resistance
                .expect("the genuine 10K token is still a real resistance value")
                .value,
            "10K"
        );
    }

    #[test]
    fn two_genuinely_conflicting_value_tokens_for_one_key_are_dropped_with_a_note() {
        // Both "10K" and "4K7" are real, well-formed resistance values — a
        // pathological description that contains two of them for the same
        // identity key is genuinely ambiguous, not a shape-guard case. The
        // parser must not guess between them; it should drop the key
        // entirely and say why, while leaving unrelated keys intact.
        let (candidates, notes) = candidates_for("RES 10K 4K7 OHM 1% 1/4W 0603");
        assert!(find(&candidates, "attr.resistance").is_none());
        assert!(notes
            .iter()
            .any(|n| n.contains("ambiguous") && n.contains("attr.resistance")));
        assert_eq!(find(&candidates, "attr.tolerance").unwrap().value, "1%");
        assert_eq!(
            find(&candidates, "attr.power_rating").unwrap().value,
            "1/4W"
        );
        assert_eq!(find(&candidates, "variant.package").unwrap().value, "0603");
    }

    #[test]
    fn unparseable_description_yields_no_candidates_and_a_note() {
        let (candidates, notes) = candidates_for("MISC HARDWARE ITEM");
        assert!(candidates.is_empty());
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("MISC HARDWARE ITEM"));
    }

    #[test]
    fn missing_description_yields_ok_none() {
        let parser = DescriptionParser;
        let input = EnrichInput::default();
        assert!(parser.enrich(&input).unwrap().is_none());
    }

    #[test]
    fn blank_description_yields_ok_none() {
        let parser = DescriptionParser;
        let input = EnrichInput {
            description: Some("   ".to_string()),
            ..Default::default()
        };
        assert!(parser.enrich(&input).unwrap().is_none());
    }

    #[test]
    fn never_panics_on_pathological_input() {
        let parser = DescriptionParser;
        for text in [
            "",
            " ",
            "%%%",
            "RES",
            "CAP",
            "IC",
            "////",
            "10K10K10K",
            "\u{0}\u{1}garbage",
            "RES %W/ / /",
        ] {
            let input = EnrichInput {
                description: Some(text.to_string()),
                ..Default::default()
            };
            let _ = parser.enrich(&input); // must not panic
        }
    }
}
