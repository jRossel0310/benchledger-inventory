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

        let category = classify_category(&tokens);
        if let Some(cat) = category {
            candidates.push(candidate("category", cat));
        }
        match category {
            Some("Resistor") => parse_resistor_tokens(&tokens, &mut candidates),
            Some("Capacitor") => parse_capacitor_tokens(&tokens, &mut candidates),
            _ => {}
        }

        // Package tokens are worth looking for regardless of category — an
        // IC's `8DIP` or an unclassified passive's `SOT-23` are just as
        // useful as a resistor's `0603`.
        if let Some(pkg) = find_package_token(&tokens) {
            candidates.push(candidate("variant.package", pkg));
        }

        let mut notes = Vec::new();
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
/// category when a token maps unambiguously to a known catalog family
/// (matching the built-in seeded taxonomy in `inventory-db`'s `seed.rs`
/// where possible); anything else is left unclassified (`None`) rather than
/// guessed.
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
                // a coarse label beats a wrong guess.
                Some("Integrated Circuit")
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
fn parse_value(token: &str, kind: UnitKind) -> Option<(String, ParsedValue)> {
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

/// Parse resistor-specific tokens (everything after the leading `"RES"`)
/// into tolerance / power / resistance candidates, based on the token's
/// suffix — never by trying every kind in turn, since the unit engine's
/// bare SI-prefix notation (`"10K"`) is valid under any kind (a prefix
/// letter alone doesn't encode which physical quantity it scales), so a
/// blind kind-by-kind trial would non-deterministically misclassify it.
fn parse_resistor_tokens(tokens: &[&str], candidates: &mut Vec<FieldCandidate>) {
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
fn parse_capacitor_tokens(tokens: &[&str], candidates: &mut Vec<FieldCandidate>) {
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
    fn generic_ic_description_gets_a_coarse_category() {
        let (candidates, _) = candidates_for("IC MCU 32BIT ARM CORTEX M4 64QFN");
        assert_eq!(
            find(&candidates, "category").unwrap().value,
            "Integrated Circuit"
        );
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
