//! Electronics-aware unit parsing and normalization.
//!
//! Values are exact: `(mantissa, exp10)` with the mantissa stripped of
//! trailing zeros. `10k`, `10 kΩ`, and `10000 ohm` all canonicalize to
//! `(1, 4)` and compare equal. `to_f64()` exists ONLY for DB range
//! filtering; identity comparisons always use the exact form.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnitKind {
    Resistance,
    Capacitance,
    Inductance,
    Voltage,
    Current,
    Power,
    Frequency,
    Length,
    Mass,
    Time,
    Percent,
    Charge,
}

impl UnitKind {
    pub fn canonical_unit(&self) -> &'static str {
        match self {
            UnitKind::Resistance => "Ω",
            UnitKind::Capacitance => "F",
            UnitKind::Inductance => "H",
            UnitKind::Voltage => "V",
            UnitKind::Current => "A",
            UnitKind::Power => "W",
            UnitKind::Frequency => "Hz",
            UnitKind::Length => "m",
            UnitKind::Mass => "g",
            UnitKind::Time => "s",
            UnitKind::Percent => "%",
            UnitKind::Charge => "C",
        }
    }

    pub fn as_sql(&self) -> &'static str {
        match self {
            UnitKind::Resistance => "resistance",
            UnitKind::Capacitance => "capacitance",
            UnitKind::Inductance => "inductance",
            UnitKind::Voltage => "voltage",
            UnitKind::Current => "current",
            UnitKind::Power => "power",
            UnitKind::Frequency => "frequency",
            UnitKind::Length => "length",
            UnitKind::Mass => "mass",
            UnitKind::Time => "time",
            UnitKind::Percent => "percent",
            UnitKind::Charge => "charge",
        }
    }

    pub fn from_sql(s: &str) -> Option<Self> {
        Some(match s {
            "resistance" => UnitKind::Resistance,
            "capacitance" => UnitKind::Capacitance,
            "inductance" => UnitKind::Inductance,
            "voltage" => UnitKind::Voltage,
            "current" => UnitKind::Current,
            "power" => UnitKind::Power,
            "frequency" => UnitKind::Frequency,
            "length" => UnitKind::Length,
            "mass" => UnitKind::Mass,
            "time" => UnitKind::Time,
            "percent" => UnitKind::Percent,
            "charge" => UnitKind::Charge,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UnitParseError {
    #[error("empty value")]
    Empty,
    #[error("could not parse '{0}'")]
    Malformed(String),
    #[error("unit belongs to {found:?}, expected {expected:?}")]
    WrongKind { expected: UnitKind, found: UnitKind },
    #[error("value out of range")]
    Overflow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ParsedValue {
    pub kind: UnitKind,
    pub mantissa: i64,
    pub exp10: i16,
}

impl ParsedValue {
    fn canonical(
        kind: UnitKind,
        mut mantissa: i64,
        mut exp10: i32,
    ) -> Result<Self, UnitParseError> {
        if mantissa == 0 {
            return Ok(ParsedValue {
                kind,
                mantissa: 0,
                exp10: 0,
            });
        }
        while mantissa % 10 == 0 {
            mantissa /= 10;
            exp10 += 1;
        }
        let exp10 = i16::try_from(exp10).map_err(|_| UnitParseError::Overflow)?;
        Ok(ParsedValue {
            kind,
            mantissa,
            exp10,
        })
    }

    pub fn to_f64(&self) -> f64 {
        self.mantissa as f64 * 10f64.powi(self.exp10 as i32)
    }

    /// Engineering-notation display with SI prefix (exponent snapped to a
    /// multiple of 3 between -12 and 9), e.g. `10 kΩ`, `100 nF`, `3.3 V`.
    pub fn format(&self) -> String {
        let unit = self.kind.canonical_unit();
        if self.mantissa == 0 {
            return format!("0 {unit}");
        }
        // digits in mantissa
        let digits = (self.mantissa.unsigned_abs() as f64).log10().floor() as i32 + 1;
        let total_exp = self.exp10 as i32 + digits - 1; // exponent of leading digit
        let eng_exp = (total_exp).div_euclid(3) * 3;
        let eng_exp = eng_exp.clamp(-12, 9);
        let prefix = match eng_exp {
            -12 => "p",
            -9 => "n",
            -6 => "µ",
            -3 => "m",
            0 => "",
            3 => "k",
            6 => "M",
            9 => "G",
            _ => unreachable!(),
        };
        // value = mantissa * 10^(exp10 - eng_exp), rendered as decimal
        let shift = self.exp10 as i32 - eng_exp;
        let rendered = render_shifted(self.mantissa, shift);
        format!("{rendered} {prefix}{unit}")
    }
}

/// Render mantissa * 10^shift as a plain decimal string (no exponent).
fn render_shifted(mantissa: i64, shift: i32) -> String {
    let neg = mantissa < 0;
    let digits = mantissa.unsigned_abs().to_string();
    let mut s = if shift >= 0 {
        let mut s = digits;
        s.push_str(&"0".repeat(shift as usize));
        s
    } else {
        let frac_len = (-shift) as usize;
        if digits.len() > frac_len {
            let (int_part, frac_part) = digits.split_at(digits.len() - frac_len);
            let frac_part = frac_part.trim_end_matches('0');
            if frac_part.is_empty() {
                int_part.to_string()
            } else {
                format!("{int_part}.{frac_part}")
            }
        } else {
            let mut frac = "0".repeat(frac_len - digits.len());
            frac.push_str(&digits);
            let frac = frac.trim_end_matches('0');
            format!("0.{frac}")
        }
    };
    if neg {
        s.insert(0, '-');
    }
    s
}

/// SI prefix -> power of ten.
fn prefix_exp(c: char) -> Option<i32> {
    Some(match c {
        'p' => -12,
        'n' => -9,
        'u' | 'µ' | 'μ' => -6,
        'm' => -3,
        'k' | 'K' => 3,
        'M' => 6,
        'G' => 9,
        _ => return None,
    })
}

/// Unit symbol/word -> (kind, extra power of ten relative to the canonical
/// unit). `in` (inch) maps to Length with a fixed 25.4 mm conversion handled
/// separately because it is not a power of ten.
fn unit_symbol(s: &str) -> Option<UnitKind> {
    let lower = s.to_lowercase();
    Some(match lower.as_str() {
        "ω" | "ohm" | "ohms" | "r" => UnitKind::Resistance,
        "f" | "farad" | "farads" => UnitKind::Capacitance,
        "h" | "henry" | "henries" => UnitKind::Inductance,
        "v" | "volt" | "volts" => UnitKind::Voltage,
        "a" | "amp" | "amps" | "ampere" | "amperes" => UnitKind::Current,
        "w" | "watt" | "watts" => UnitKind::Power,
        "hz" | "hertz" => UnitKind::Frequency,
        "g" | "gram" | "grams" => UnitKind::Mass,
        "s" | "sec" | "second" | "seconds" => UnitKind::Time,
        "%" => UnitKind::Percent,
        "c" | "coulomb" | "coulombs" => UnitKind::Charge,
        // length: meter symbol is handled with care because "m" is also the
        // milli prefix; bare "m"/"mm"/"cm"/"in" are resolved in the tokenizer.
        "meter" | "meters" | "metre" | "metres" => UnitKind::Length,
        _ => return None,
    })
}

/// Parse `input` as a value of `kind`. Accepts:
///  - plain decimals ("4700", "3.3", ".5"), optional leading ±
///  - fractions ("1/4")
///  - SI prefix and/or unit ("10k", "10 kΩ", "100nF", "10000 ohm", "1%")
///  - embedded-prefix notation ("4k7", "3V3", "0R", "2n2")
///  - length specials: mm/cm/in ("5 mm", "0.1 in")
pub fn parse_with_kind(input: &str, kind: UnitKind) -> Result<ParsedValue, UnitParseError> {
    let s = input.trim().trim_start_matches('±').trim();
    if s.is_empty() {
        return Err(UnitParseError::Empty);
    }

    // 1) Embedded-prefix notation: <digits><letter><digits?> e.g. 4k7, 3V3, 0R, 2n2.
    if let Some(v) = try_embedded(s, kind)? {
        return Ok(v);
    }

    // 2) Fraction: <num>/<den> [unit]
    if let Some(v) = try_fraction(s, kind)? {
        return Ok(v);
    }

    // 3) General: <decimal> [prefix][unit]
    let (num_str, rest) = split_number(s).ok_or_else(|| UnitParseError::Malformed(s.into()))?;
    let (mut mantissa, frac_exp) = parse_decimal(num_str)?;
    let rest = rest.trim();

    let mut exp: i32 = frac_exp;
    if !rest.is_empty() {
        match resolve_suffix(rest, kind) {
            Ok(e) => exp += e,
            Err(UnitParseError::Malformed(m)) if m == "__inches__" => {
                // 1 in = 25.4 mm: multiply mantissa by 254, shift by -4.
                mantissa = mantissa.checked_mul(254).ok_or(UnitParseError::Overflow)?;
                exp += -4;
            }
            Err(e) => return Err(e),
        }
    }
    ParsedValue::canonical(kind, mantissa, exp)
}

/// Resolve a prefix+unit suffix like "kΩ", "nF", "mm", "in", "k", "ohm", "%".
/// Returns the power-of-ten adjustment. Errors if the unit contradicts `kind`.
fn resolve_suffix(rest: &str, kind: UnitKind) -> Result<i32, UnitParseError> {
    // Length specials first (mm/cm/in and bare m as METER, not milli).
    if kind == UnitKind::Length {
        match rest.to_lowercase().as_str() {
            "m" => return Ok(0),
            "mm" => return Ok(-3),
            "cm" => return Ok(-2),
            "um" | "µm" | "μm" => return Ok(-6),
            "in" | "\"" | "inch" | "inches" => {
                // 1 in = 25.4 mm = 0.0254 m: fold 254/10^4 into the mantissa
                // via a sentinel handled by the caller? Simpler: treat as a
                // scale of 254 * 10^-4 — see try_scale_inches below.
                return Err(UnitParseError::Malformed("__inches__".into()));
            }
            _ => {}
        }
    }
    let chars: Vec<char> = rest.chars().collect();
    // whole thing is a unit word/symbol?
    if let Some(found) = unit_symbol(rest) {
        return if found == kind {
            Ok(0)
        } else {
            Err(UnitParseError::WrongKind {
                expected: kind,
                found,
            })
        };
    }
    // first char prefix + remainder unit?
    if let Some(p) = prefix_exp(chars[0]) {
        let unit_part: String = chars[1..].iter().collect();
        if unit_part.is_empty() {
            return Ok(p); // bare prefix, kind supplied by caller
        }
        if let Some(found) = unit_symbol(&unit_part) {
            return if found == kind {
                Ok(p)
            } else {
                Err(UnitParseError::WrongKind {
                    expected: kind,
                    found,
                })
            };
        }
    }
    Err(UnitParseError::Malformed(rest.into()))
}

fn try_embedded(s: &str, kind: UnitKind) -> Result<Option<ParsedValue>, UnitParseError> {
    let chars: Vec<char> = s.chars().collect();
    let first_alpha = chars
        .iter()
        .position(|c| c.is_alphabetic() || *c == 'µ' || *c == 'μ' || *c == 'Ω');
    let Some(i) = first_alpha else {
        return Ok(None);
    };
    if i == 0 {
        return Ok(None);
    }
    let int_part: String = chars[..i].iter().collect();
    if !int_part.chars().all(|c| c.is_ascii_digit()) {
        return Ok(None);
    }
    let marker = chars[i];
    let frac_part: String = chars[i + 1..].iter().collect();
    if !frac_part.chars().all(|c| c.is_ascii_digit()) {
        return Ok(None); // not the embedded form (e.g. "10kΩ" has Ω after k)
    }
    // Marker is either an SI prefix (4k7, 2n2) or the kind's own unit letter (3V3, 0R5, 0R).
    let marker_exp = if let Some(p) = prefix_exp(marker) {
        Some(p)
    } else {
        let m = marker.to_string();
        match unit_symbol(&m) {
            Some(found) if found == kind => Some(0),
            Some(found) => {
                return Err(UnitParseError::WrongKind {
                    expected: kind,
                    found,
                })
            }
            None => None,
        }
    };
    let Some(marker_exp) = marker_exp else {
        return Ok(None);
    };
    if frac_part.is_empty() {
        let mantissa: i64 = int_part.parse().map_err(|_| UnitParseError::Overflow)?;
        return Ok(Some(ParsedValue::canonical(kind, mantissa, marker_exp)?));
    }
    let combined = format!("{int_part}{frac_part}");
    let mantissa: i64 = combined.parse().map_err(|_| UnitParseError::Overflow)?;
    let exp = marker_exp - frac_part.len() as i32;
    Ok(Some(ParsedValue::canonical(kind, mantissa, exp)?))
}

fn try_fraction(s: &str, kind: UnitKind) -> Result<Option<ParsedValue>, UnitParseError> {
    let Some(slash) = s.find('/') else {
        return Ok(None);
    };
    let (num, rest) = s.split_at(slash);
    let rest = &rest[1..];
    let mut num: i64 = num
        .trim()
        .parse()
        .map_err(|_| UnitParseError::Malformed(s.into()))?;
    let (den_str, unit_rest) =
        split_number(rest.trim()).ok_or_else(|| UnitParseError::Malformed(s.into()))?;
    let den: i64 = den_str
        .parse()
        .map_err(|_| UnitParseError::Malformed(s.into()))?;
    if den == 0 {
        return Err(UnitParseError::Malformed(s.into()));
    }
    let mut unit_exp = 0;
    let unit_rest = unit_rest.trim();
    if !unit_rest.is_empty() {
        match resolve_suffix(unit_rest, kind) {
            Ok(e) => unit_exp = e,
            Err(UnitParseError::Malformed(m)) if m == "__inches__" => {
                // 1 in = 25.4 mm: fold 254 * 10^-4 into the fraction's
                // numerator before reduction, so 1/2 in = 254/2 * 10^-4
                // = 127 * 10^-4 exactly (not an approximation).
                num = num.checked_mul(254).ok_or(UnitParseError::Overflow)?;
                unit_exp = -4;
            }
            Err(e) => return Err(e),
        }
    }
    // exact decimal expansion of num/den: scale numerator by 10^k until divisible
    let mut mantissa = num;
    let mut exp = unit_exp;
    let mut remainder_den = den;
    while remainder_den != 1 {
        if mantissa % remainder_den == 0 {
            mantissa /= remainder_den;
            remainder_den = 1;
        } else {
            mantissa = match mantissa.checked_mul(10) {
                Some(m) => m,
                // Non-terminating fraction (e.g. 1/3): growth without a
                // terminating decimal expansion overflows before the
                // exp < -30 guard below can fire. That's a malformed
                // input, not an out-of-range value.
                None => return Err(UnitParseError::Malformed(s.into())),
            };
            exp -= 1;
            // reduce common factors to keep the loop terminating for 2s and 5s
            let g = gcd(mantissa.unsigned_abs(), remainder_den.unsigned_abs());
            mantissa /= g as i64;
            remainder_den /= g as i64;
            if exp < -30 {
                return Err(UnitParseError::Malformed(s.into())); // non-terminating (e.g. 1/3)
            }
        }
    }
    Ok(Some(ParsedValue::canonical(kind, mantissa, exp)?))
}

fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a.max(1)
}

/// Split leading decimal number from the rest. Handles "3.3", ".5", "10".
fn split_number(s: &str) -> Option<(&str, &str)> {
    let mut end = 0;
    let bytes = s.as_bytes();
    let mut seen_digit = false;
    let mut seen_dot = false;
    for (i, b) in bytes.iter().enumerate() {
        match b {
            b'0'..=b'9' => {
                seen_digit = true;
                end = i + 1;
            }
            b'.' if !seen_dot => {
                seen_dot = true;
                end = i + 1;
            }
            b'-' | b'+' if i == 0 => {
                end = i + 1;
            }
            _ => break,
        }
    }
    if !seen_digit {
        return None;
    }
    Some(s.split_at(end))
}

/// Parse a plain decimal into (mantissa, exp10-adjustment). "3.3" -> (33, -1).
fn parse_decimal(s: &str) -> Result<(i64, i32), UnitParseError> {
    let neg = s.starts_with('-');
    let s2 = s.trim_start_matches(['-', '+']);
    let (int_part, frac_part) = match s2.find('.') {
        Some(i) => (&s2[..i], &s2[i + 1..]),
        None => (s2, ""),
    };
    let combined = format!("{int_part}{frac_part}");
    if combined.is_empty() {
        return Err(UnitParseError::Malformed(s.into()));
    }
    let mut mantissa: i64 = combined.parse().map_err(|_| UnitParseError::Overflow)?;
    if neg {
        mantissa = -mantissa;
    }
    Ok((mantissa, -(frac_part.len() as i32)))
}

/// Parse when the unit symbol itself identifies the kind ("10kΩ", "100nF",
/// "3V3", "0.25W"). Returns None when ambiguous (bare numbers/prefixes).
pub fn detect_and_parse(input: &str) -> Option<ParsedValue> {
    const KINDS: [UnitKind; 12] = [
        UnitKind::Resistance,
        UnitKind::Capacitance,
        UnitKind::Inductance,
        UnitKind::Voltage,
        UnitKind::Current,
        UnitKind::Power,
        UnitKind::Frequency,
        UnitKind::Length,
        UnitKind::Mass,
        UnitKind::Time,
        UnitKind::Percent,
        UnitKind::Charge,
    ];
    let s = input.trim();
    // must contain an alphabetic/symbol tail or embedded marker beyond digits
    let has_symbol = s
        .chars()
        .any(|c| c.is_alphabetic() || c == 'Ω' || c == '%' || c == 'µ' || c == 'μ');
    if !has_symbol {
        return None;
    }
    let mut hit: Option<ParsedValue> = None;
    for kind in KINDS {
        if let Ok(v) = parse_with_kind(s, kind) {
            match &hit {
                None => hit = Some(v),
                // Ambiguous across kinds (e.g. "100n" would match many if it
                // reached here; bare prefixes return Ok for every kind, so
                // require uniqueness).
                Some(prev) if prev.kind != v.kind => return None,
                Some(_) => {}
            }
        }
    }
    hit
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../../../packages/shared/fixtures/unit-cases.json");

    #[derive(serde::Deserialize)]
    struct Fixture {
        cases: Vec<Case>,
        #[serde(rename = "equalPairs")]
        equal_pairs: Vec<(String, String, String)>,
        rejects: Vec<Reject>,
    }
    #[derive(serde::Deserialize)]
    struct Case {
        input: String,
        kind: String,
        mantissa: i64,
        exp10: i16,
    }
    #[derive(serde::Deserialize)]
    struct Reject {
        input: String,
        kind: String,
    }

    fn kind(s: &str) -> UnitKind {
        UnitKind::from_sql(s).unwrap_or_else(|| panic!("unknown kind {s}"))
    }

    #[test]
    fn every_fixture_case_parses_to_canonical_form() {
        let fx: Fixture = serde_json::from_str(FIXTURE).unwrap();
        for c in &fx.cases {
            let parsed = parse_with_kind(&c.input, kind(&c.kind))
                .unwrap_or_else(|e| panic!("'{}' failed: {e}", c.input));
            assert_eq!(
                (parsed.mantissa, parsed.exp10),
                (c.mantissa, c.exp10),
                "'{}' parsed to {:?}",
                c.input,
                parsed
            );
        }
    }

    #[test]
    fn equal_pairs_compare_equal() {
        let fx: Fixture = serde_json::from_str(FIXTURE).unwrap();
        for (a, b, k) in &fx.equal_pairs {
            let pa = parse_with_kind(a, kind(k)).unwrap();
            let pb = parse_with_kind(b, kind(k)).unwrap();
            assert_eq!(pa, pb, "'{a}' != '{b}'");
        }
    }

    #[test]
    fn reject_cases_fail() {
        let fx: Fixture = serde_json::from_str(FIXTURE).unwrap();
        for r in &fx.rejects {
            assert!(
                parse_with_kind(&r.input, kind(&r.kind)).is_err(),
                "'{}' should have been rejected",
                r.input
            );
        }
    }

    #[test]
    fn wrong_kind_symbol_is_a_typed_error() {
        let err = parse_with_kind("10kV", UnitKind::Resistance).unwrap_err();
        assert!(matches!(err, UnitParseError::WrongKind { .. }));
    }

    #[test]
    fn detect_and_parse_needs_an_unambiguous_symbol() {
        assert_eq!(
            detect_and_parse("100nF").unwrap(),
            parse_with_kind("100n", UnitKind::Capacitance).unwrap()
        );
        assert_eq!(detect_and_parse("0.25W").unwrap().kind, UnitKind::Power);
        assert_eq!(detect_and_parse("3V3").unwrap().kind, UnitKind::Voltage);
        assert!(
            detect_and_parse("100n").is_none(),
            "bare prefix is ambiguous"
        );
        assert!(detect_and_parse("10").is_none());
    }

    #[test]
    fn formatting_uses_engineering_prefixes() {
        assert_eq!(
            parse_with_kind("10k", UnitKind::Resistance)
                .unwrap()
                .format(),
            "10 kΩ"
        );
        assert_eq!(
            parse_with_kind("100000 pF", UnitKind::Capacitance)
                .unwrap()
                .format(),
            "100 nF"
        );
        assert_eq!(
            parse_with_kind("0.25 W", UnitKind::Power).unwrap().format(),
            "250 mW"
        );
        assert_eq!(
            parse_with_kind("0R", UnitKind::Resistance)
                .unwrap()
                .format(),
            "0 Ω"
        );
        assert_eq!(
            parse_with_kind("3V3", UnitKind::Voltage).unwrap().format(),
            "3.3 V"
        );
        assert_eq!(
            parse_with_kind("1%", UnitKind::Percent).unwrap().format(),
            "1 %"
        );
    }

    #[test]
    fn to_f64_is_usable_for_range_filtering() {
        let v = parse_with_kind("100 nF", UnitKind::Capacitance)
            .unwrap()
            .to_f64();
        assert!((v - 1e-7).abs() < 1e-15);
    }

    #[test]
    fn canonicalization_strips_trailing_zeros() {
        let p = parse_with_kind("4700", UnitKind::Resistance).unwrap();
        assert_eq!((p.mantissa, p.exp10), (47, 2));
    }

    #[test]
    fn fractional_inches_convert_exactly() {
        let v = parse_with_kind("1/2 in", UnitKind::Length).unwrap();
        assert_eq!((v.mantissa, v.exp10), (127, -4)); // 12.7 mm
        assert!(
            !format!("{:?}", parse_with_kind("1/2 in", UnitKind::Length)).contains("__inches__")
        );
    }

    #[test]
    fn non_terminating_fractions_are_malformed() {
        assert!(matches!(
            parse_with_kind("1/3 W", UnitKind::Power).unwrap_err(),
            UnitParseError::Malformed(_)
        ));
    }
}
