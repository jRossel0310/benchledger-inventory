//! Package/footprint-code normalization. Chip packages have dual
//! imperial/metric codes (0603 == 1608 metric); bare codes are read as
//! imperial (documented decision). Named packages fold common aliases.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedPackage {
    pub canonical: String,
    pub imperial: Option<String>,
    pub metric: Option<String>,
}

/// (imperial, metric) chip package code pairs.
const CHIP_CODES: &[(&str, &str)] = &[
    ("01005", "0402"),
    ("0201", "0603"),
    ("0402", "1005"),
    ("0603", "1608"),
    ("0805", "2012"),
    ("1206", "3216"),
    ("1210", "3225"),
    ("1812", "4532"),
    ("2010", "5025"),
    ("2512", "6332"),
];

pub fn normalize_package(input: &str) -> Option<NormalizedPackage> {
    let cleaned = input.trim();
    if cleaned.is_empty() {
        return None;
    }
    let upper = cleaned.to_uppercase();
    let collapsed: String = upper.split_whitespace().collect::<Vec<_>>().join(" ");

    // metric-suffixed chip code: "1608 METRIC" or "1608M"
    let metric_code = collapsed
        .strip_suffix(" METRIC")
        .map(str::to_string)
        .or_else(|| {
            let s = collapsed.strip_suffix('M')?;
            if s.chars().all(|c| c.is_ascii_digit()) && s.len() >= 4 {
                Some(s.to_string())
            } else {
                None
            }
        });
    if let Some(code) = metric_code {
        if let Some((imp, met)) = CHIP_CODES.iter().find(|(_, m)| *m == code) {
            return Some(NormalizedPackage {
                canonical: (*imp).to_string(),
                imperial: Some((*imp).to_string()),
                metric: Some((*met).to_string()),
            });
        }
    }

    // bare chip code: read as imperial
    if collapsed.chars().all(|c| c.is_ascii_digit()) {
        if let Some((imp, met)) = CHIP_CODES.iter().find(|(i, _)| *i == collapsed) {
            return Some(NormalizedPackage {
                canonical: (*imp).to_string(),
                imperial: Some((*imp).to_string()),
                metric: Some((*met).to_string()),
            });
        }
    }

    // named packages: fold separators, then alias-normalize
    let squished: String = collapsed
        .chars()
        .filter(|c| !matches!(c, ' ' | '-' | '_'))
        .collect();
    for family in [
        "PDIP", "DIP", "SOIC", "SOT", "TO", "QFN", "TQFP", "LQFP", "SSOP", "TSSOP", "MSOP", "BGA",
    ] {
        // FAMILY<digits> or <digits>FAMILY -> FAMILY-<digits> (preserving
        // multi-segment tails like SOT235 -> SOT-23-5)
        if let Some(rest) = squished.strip_prefix(family) {
            if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
                return Some(named(family, rest));
            }
            if rest.is_empty() {
                return Some(passthrough(family));
            }
        }
        if let Some(front) = squished.strip_suffix(family) {
            if !front.is_empty() && front.chars().all(|c| c.is_ascii_digit()) {
                return Some(named(family, front));
            }
        }
    }
    Some(passthrough(&collapsed))
}

fn named(family: &str, digits: &str) -> NormalizedPackage {
    // Only known 2-digit SOT bodies take a pin-count suffix (SOT235 -> SOT-23-5).
    // Standalone 3-digit SOT numbers (SOT-223, SOT-323, ...) must not split.
    const SOT_BODIES_WITH_PINCOUNT: [&str; 2] = ["23", "89"];
    let canonical = if family == "SOT"
        && digits.len() > 2
        && SOT_BODIES_WITH_PINCOUNT.contains(&&digits[..2])
    {
        format!("SOT-{}-{}", &digits[..2], &digits[2..])
    } else {
        format!("{family}-{digits}")
    };
    passthrough(&canonical)
}

fn passthrough(name: &str) -> NormalizedPackage {
    NormalizedPackage {
        canonical: name.to_string(),
        imperial: None,
        metric: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imperial_and_metric_chip_codes_normalize_to_same_canonical() {
        for (imp, met) in [
            ("0201", "0603"),
            ("0402", "1005"),
            ("0603", "1608"),
            ("0805", "2012"),
            ("1206", "3216"),
            ("1210", "3225"),
            ("2512", "6332"),
        ] {
            let a = normalize_package(imp).unwrap();
            let b = normalize_package(&format!("{met} metric")).unwrap();
            assert_eq!(a.canonical, b.canonical, "{imp} vs {met}M");
            assert_eq!(a.canonical, imp);
            assert_eq!(b.imperial.as_deref(), Some(imp));
        }
        // suffix form
        assert_eq!(normalize_package("1608M").unwrap().canonical, "0603");
    }

    #[test]
    fn ambiguous_bare_code_is_treated_as_imperial() {
        // 0603 exists in both systems; bare codes read as imperial (documented).
        assert_eq!(normalize_package("0603").unwrap().canonical, "0603");
        assert_eq!(normalize_package("0603 metric").unwrap().canonical, "0201");
    }

    #[test]
    fn named_packages_fold_aliases() {
        assert_eq!(normalize_package("SOT23").unwrap().canonical, "SOT-23");
        assert_eq!(normalize_package("sot-23").unwrap().canonical, "SOT-23");
        assert_eq!(normalize_package("SOT-23-5").unwrap().canonical, "SOT-23-5");
        assert_eq!(normalize_package("DIP8").unwrap().canonical, "DIP-8");
        assert_eq!(normalize_package("8-DIP").unwrap().canonical, "DIP-8");
        assert_eq!(normalize_package("PDIP-8").unwrap().canonical, "PDIP-8");
        assert_eq!(normalize_package("8-PDIP").unwrap().canonical, "PDIP-8");
        assert_eq!(normalize_package("TO-92").unwrap().canonical, "TO-92");
        assert_eq!(normalize_package("to92").unwrap().canonical, "TO-92");
        assert_eq!(normalize_package("SOIC-8").unwrap().canonical, "SOIC-8");
        assert_eq!(normalize_package(" soic 8 ").unwrap().canonical, "SOIC-8");
    }

    #[test]
    fn three_digit_sot_packages_do_not_split() {
        assert_eq!(normalize_package("SOT223").unwrap().canonical, "SOT-223");
        assert_eq!(normalize_package("SOT-223").unwrap().canonical, "SOT-223");
        assert_eq!(normalize_package("sot 323").unwrap().canonical, "SOT-323");
        assert_eq!(normalize_package("SOT523").unwrap().canonical, "SOT-523");
        // pin-count split still works for known bodies
        assert_eq!(normalize_package("SOT235").unwrap().canonical, "SOT-23-5");
        assert_eq!(normalize_package("SOT893").unwrap().canonical, "SOT-89-3");
    }

    #[test]
    fn unknown_input_returns_none_or_uppercase_passthrough() {
        // Free-form unknown names still normalize casing/whitespace so equal
        // inputs compare equal, but codes stay None.
        let n = normalize_package("Weird-Package 3000").unwrap();
        assert_eq!(n.canonical, "WEIRD-PACKAGE 3000");
        assert!(n.imperial.is_none());
        assert!(normalize_package("").is_none());
        assert!(normalize_package("   ").is_none());
    }
}
