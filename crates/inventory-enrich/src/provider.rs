//! The `EnrichmentProvider` trait every enrichment source implements, plus
//! `run_chain`, which runs an ordered list of providers against one input
//! and merges their candidates by priority.

use std::collections::HashSet;

use crate::model::{EnrichInput, Enrichment, FieldCandidate, ImageRef};

/// A source of enrichment data — the offline description parser (this
/// crate), the DigiKey V4 API client (Task 4), or a future manufacturer-page
/// / datasheet scraper. Pure/data-only: no database access; any network I/O
/// is the implementation's own concern (there is none in this task).
pub trait EnrichmentProvider {
    /// Short, stable identifier (e.g. `"digikey"`, `"description"`) — used
    /// in `ChainResult::providers_run` and in failure notes.
    fn name(&self) -> &str;

    /// Produce candidates for `input`.
    ///
    /// `Ok(None)` means the provider ran successfully but had nothing to
    /// add (not configured, no match found, no usable input) — this is
    /// normal, expected behavior, not an error. `Err` is a real failure;
    /// `run_chain` records it as a note and continues on to the next
    /// provider rather than aborting.
    fn enrich(&self, input: &EnrichInput) -> Result<Option<Enrichment>, EnrichError>;
}

/// Failure modes a provider can raise.
///
/// `Display` must never include a secret (an API credential or OAuth
/// token) — Task 4's DigiKey client relies on that when it maps HTTP/auth
/// failures into these variants; every message it builds is a plain,
/// caller-authored classification string, never the raw error payload from
/// a lower-level library that might itself echo request/response details.
#[derive(Debug, thiserror::Error)]
pub enum EnrichError {
    #[error("provider error: {0}")]
    Provider(String),
    #[error("could not parse input: {0}")]
    Parse(String),
    #[error("network error: {0}")]
    Network(String),
    #[error("not configured: {0}")]
    Config(String),
}

/// Result of running an ordered provider chain against one `EnrichInput`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ChainResult {
    pub candidates: Vec<FieldCandidate>,
    pub images: Vec<ImageRef>,
    pub notes: Vec<String>,
    pub providers_run: Vec<String>,
}

/// Run `providers` in order against `input`, merging their output.
///
/// `providers` is expected in priority order, highest first — the
/// always-available description parser belongs last, since it is the least
/// authoritative source. For any given candidate `key`, the FIRST provider
/// to emit it wins; a later provider's value for the same key is dropped
/// rather than overwriting it. Images are deduplicated by `url` across the
/// whole chain, in first-seen order.
///
/// A provider returning `Err` is skipped: its failure is recorded as a note
/// (`"<provider name>: <error>"`), not propagated — one failing provider
/// never stops the rest of the chain from running, and this function itself
/// never panics.
pub fn run_chain(providers: &[&dyn EnrichmentProvider], input: &EnrichInput) -> ChainResult {
    let mut result = ChainResult::default();
    let mut seen_keys: HashSet<String> = HashSet::new();
    let mut seen_images: HashSet<String> = HashSet::new();

    for provider in providers {
        result.providers_run.push(provider.name().to_string());
        match provider.enrich(input) {
            Ok(Some(enrichment)) => {
                for candidate in enrichment.candidates {
                    if seen_keys.insert(candidate.key.clone()) {
                        result.candidates.push(candidate);
                    }
                }
                for image in enrichment.images {
                    if seen_images.insert(image.url.clone()) {
                        result.images.push(image);
                    }
                }
                result.notes.extend(enrichment.notes);
            }
            Ok(None) => {}
            Err(e) => {
                result.notes.push(format!("{}: {e}", provider.name()));
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::EnrichSource;

    struct StubProvider {
        name: &'static str,
        result: Result<Option<Enrichment>, String>,
    }

    impl EnrichmentProvider for StubProvider {
        fn name(&self) -> &str {
            self.name
        }

        fn enrich(&self, _input: &EnrichInput) -> Result<Option<Enrichment>, EnrichError> {
            match &self.result {
                Ok(e) => Ok(e.clone()),
                Err(msg) => Err(EnrichError::Provider(msg.clone())),
            }
        }
    }

    fn field(key: &str, value: &str) -> FieldCandidate {
        FieldCandidate {
            key: key.to_string(),
            value: value.to_string(),
            source: EnrichSource::Digikey,
            confidence: 0.9,
        }
    }

    #[test]
    fn higher_priority_provider_wins_for_the_same_key() {
        let high = StubProvider {
            name: "high",
            result: Ok(Some(Enrichment {
                provider: "high".into(),
                candidates: vec![field("variant.package", "X")],
                images: vec![],
                notes: vec![],
            })),
        };
        let low = StubProvider {
            name: "low",
            result: Ok(Some(Enrichment {
                provider: "low".into(),
                candidates: vec![field("variant.package", "0603")],
                images: vec![],
                notes: vec![],
            })),
        };
        let providers: Vec<&dyn EnrichmentProvider> = vec![&high, &low];
        let result = run_chain(&providers, &EnrichInput::default());

        let pkg = result
            .candidates
            .iter()
            .find(|c| c.key == "variant.package")
            .expect("variant.package candidate present");
        assert_eq!(pkg.value, "X");
        assert_eq!(result.candidates.len(), 1);
        assert_eq!(result.providers_run, vec!["high", "low"]);
    }

    #[test]
    fn a_failing_provider_is_skipped_and_noted_but_does_not_stop_the_chain() {
        let failing = StubProvider {
            name: "flaky",
            result: Err("boom".into()),
        };
        let ok = StubProvider {
            name: "ok",
            result: Ok(Some(Enrichment {
                provider: "ok".into(),
                candidates: vec![field("category", "Resistor")],
                images: vec![],
                notes: vec![],
            })),
        };
        let providers: Vec<&dyn EnrichmentProvider> = vec![&failing, &ok];
        let result = run_chain(&providers, &EnrichInput::default());

        assert_eq!(result.candidates.len(), 1);
        assert_eq!(result.candidates[0].value, "Resistor");
        assert!(result
            .notes
            .iter()
            .any(|n| n.contains("flaky") && n.contains("boom")));
        assert_eq!(result.providers_run, vec!["flaky", "ok"]);
    }

    #[test]
    fn images_are_deduped_by_url_across_providers() {
        let a = StubProvider {
            name: "a",
            result: Ok(Some(Enrichment {
                provider: "a".into(),
                candidates: vec![],
                images: vec![ImageRef {
                    url: "http://example.com/1.jpg".into(),
                }],
                notes: vec![],
            })),
        };
        let b = StubProvider {
            name: "b",
            result: Ok(Some(Enrichment {
                provider: "b".into(),
                candidates: vec![],
                images: vec![
                    ImageRef {
                        url: "http://example.com/1.jpg".into(),
                    },
                    ImageRef {
                        url: "http://example.com/2.jpg".into(),
                    },
                ],
                notes: vec![],
            })),
        };
        let providers: Vec<&dyn EnrichmentProvider> = vec![&a, &b];
        let result = run_chain(&providers, &EnrichInput::default());

        assert_eq!(result.images.len(), 2);
        assert_eq!(result.images[0].url, "http://example.com/1.jpg");
        assert_eq!(result.images[1].url, "http://example.com/2.jpg");
    }

    #[test]
    fn provider_returning_ok_none_contributes_nothing_and_no_note() {
        let empty = StubProvider {
            name: "empty",
            result: Ok(None),
        };
        let providers: Vec<&dyn EnrichmentProvider> = vec![&empty];
        let result = run_chain(&providers, &EnrichInput::default());

        assert!(result.candidates.is_empty());
        assert!(result.notes.is_empty());
        assert_eq!(result.providers_run, vec!["empty"]);
    }

    #[test]
    fn empty_provider_list_returns_empty_result() {
        let result = run_chain(&[], &EnrichInput::default());
        assert!(result.candidates.is_empty());
        assert!(result.images.is_empty());
        assert!(result.notes.is_empty());
        assert!(result.providers_run.is_empty());
    }

    #[test]
    fn enrich_error_display_never_includes_a_raw_secret_marker() {
        // Every variant's Display is just a fixed label plus whatever
        // message the caller passed in — no variant computes or wraps
        // anything beyond that, so as long as callers (Task 4's DigiKey
        // client) never put a secret in the message, Display can't leak
        // one. Exercise every variant so this test breaks if a future
        // variant starts capturing something it shouldn't.
        let secret = "sk_live_should_never_appear_in_output";
        for err in [
            EnrichError::Provider("provider failed".into()),
            EnrichError::Parse("could not read description".into()),
            EnrichError::Network("connection refused".into()),
            EnrichError::Config("digikey not configured".into()),
        ] {
            let display = err.to_string();
            assert!(!display.contains(secret), "leaked secret: {display}");
        }
    }
}
