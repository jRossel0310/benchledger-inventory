//! Generates (or, without `EXPORT_SAMPLE_SNAPSHOT`, verifies)
//! `packages/shared/fixtures/sample-snapshot.json` -- a small, synthetic
//! published-snapshot fixture the TS `parseSnapshot` twin
//! (`packages/shared/src/snapshot.ts`, Phase 6 Task 2) is tested against.
//! Built from `inventory_db::dev_seed` (already-synthetic seed data used
//! for local dev/demo, never real inventory) through the exact
//! `build_snapshot` + `to_canonical_json` path Task 1's snapshot builder
//! uses -- so the fixture is sanitized by construction (the builder's
//! privacy exclusions, proven by `tests/snapshot.rs`'s denylist test,
//! apply here exactly as they do to a real publish) and can never drift
//! from the real `Snapshot` shape.
//!
//! Mirrors the regenerate-and-diff pattern used by
//! `search::tests::query_cases_fixture_matches_generator`
//! (`crates/inventory-core/src/search.rs`) and `export_bindings`
//! (`apps/desktop/src-tauri/src/commands.rs`): regenerate after a
//! legitimate `Snapshot` shape change with
//!
//! `EXPORT_SAMPLE_SNAPSHOT=1 cargo test -p inventory-sync sample_snapshot_fixture_matches_generator`
//!
//! then commit the result. Without the env var, this test rebuilds the
//! fixture fresh and asserts it is byte-identical to the committed file,
//! so a stale fixture fails loudly instead of silently drifting from
//! `build_snapshot`.

use std::collections::HashMap;

use inventory_db::{dev_seed, Database};
use inventory_sync::snapshot::{build_snapshot, to_canonical_json};

/// Fixed, synthetic timestamp -- NOT `SystemTime::now()` -- so the
/// committed fixture never churns from a rebuild alone. `to_canonical_json`
/// only guarantees byte-stability for a fixed `published_at`; using a live
/// clock here would make every regeneration produce a spurious diff
/// unrelated to any real `Snapshot` shape change.
const SAMPLE_PUBLISHED_AT: &str = "2026-01-01T00:00:00Z";

/// Crockford Base32 -- the exact alphabet `ulid::Ulid` encodes to (see
/// `crates/inventory-core/src/ids.rs`'s id-newtype macro, `Ulid::new()`).
const CROCKFORD_ALPHABET: &str = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";

fn is_ulid_char(c: char) -> bool {
    CROCKFORD_ALPHABET.contains(c)
}

/// Replaces every distinct 26-character ULID-shaped run with a stable
/// placeholder based on the order it first appears in `json`, left to
/// right.
///
/// `dev_seed::run` assigns every part/variant/project a fresh
/// `Ulid::new()` -- random by design, so two runs never produce the same
/// literal id strings, and a byte-for-byte fixture comparison against raw
/// ids would fail on every regeneration even with zero real drift.
/// Normalizing ids this way keeps the fixture exactly as drift-locked as
/// everything else in it -- field shapes, value formatting,
/// cross-reference consistency -- while not pretending the arbitrary
/// literal id text (which carries no meaning) is part of the contract.
///
/// This alone is NOT sufficient for a stable diff: `build_snapshot` sorts
/// every collection by id, and two entities `dev_seed` creates within the
/// same millisecond have no defined relative id order (confirmed the hard
/// way -- an earlier version of this test that skipped the re-sort below
/// was flaky, failing roughly half the time as `dev_seed`'s within-a-
/// millisecond entities shuffled). `stabilize_order` below re-sorts by a
/// human-meaningful key first so *entity order* is also deterministic
/// before normalization ever sees the text.
fn normalize_ulids(json: &str) -> String {
    let mut placeholders: HashMap<String, String> = HashMap::new();
    let mut out = String::with_capacity(json.len());
    let chars: Vec<char> = json.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if is_ulid_char(chars[i]) {
            let start = i;
            while i < chars.len() && is_ulid_char(chars[i]) {
                i += 1;
            }
            let run: String = chars[start..i].iter().collect();
            if run.chars().count() == 26 {
                let next_index = placeholders.len();
                let placeholder = placeholders
                    .entry(run)
                    .or_insert_with(|| format!("ID{next_index:024}"));
                out.push_str(placeholder);
            } else {
                out.push_str(&run);
            }
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Re-sorts every id-ordered collection in `snapshot` by a
/// human-meaningful key instead of `build_snapshot`'s id order, so entity
/// order is stable across regenerations regardless of `dev_seed`'s
/// same-millisecond id ties. Bins are untouched (`list_bins` already
/// orders by `bin_label`, never by id -- see `crates/inventory-db/src/
/// bins.rs`) as are each part's `attributes`/`dimensions` and each
/// variant's `listings` (`build_snapshot` already sorts those by key/
/// (supplier, supplier_sku), not id).
fn stabilize_order(snapshot: &mut inventory_sync::snapshot::Snapshot) {
    let id_to_name: HashMap<String, String> = snapshot
        .parts
        .iter()
        .map(|p| (p.id.clone(), p.display_name.clone()))
        .collect();

    snapshot.parts.sort_by(|a, b| {
        (a.display_name.as_str(), a.id.as_str()).cmp(&(b.display_name.as_str(), b.id.as_str()))
    });
    for part in &mut snapshot.parts {
        part.variants.sort_by(|a, b| {
            (a.manufacturer.as_str(), a.mpn.as_str(), a.id.as_str()).cmp(&(
                b.manufacturer.as_str(),
                b.mpn.as_str(),
                b.id.as_str(),
            ))
        });
    }

    snapshot
        .projects
        .sort_by(|a, b| (a.name.as_str(), a.id.as_str()).cmp(&(b.name.as_str(), b.id.as_str())));
    for project in &mut snapshot.projects {
        project.part_ids.sort_by(|a, b| {
            let na = id_to_name.get(a).map(String::as_str).unwrap_or(a.as_str());
            let nb = id_to_name.get(b).map(String::as_str).unwrap_or(b.as_str());
            (na, a.as_str()).cmp(&(nb, b.as_str()))
        });
    }
}

#[test]
fn sample_snapshot_fixture_matches_generator() {
    const COMMITTED_PATH: &str = "../../packages/shared/fixtures/sample-snapshot.json";

    let dir = tempfile::tempdir().unwrap();
    let mut db =
        Database::open_and_migrate(&dir.path().join("t.sqlite"), &dir.path().join("backups"))
            .unwrap();
    dev_seed::run(&mut db).expect("dev_seed must succeed against a freshly migrated DB");

    let mut snapshot =
        build_snapshot(&db).expect("build_snapshot must succeed against seeded data");
    stabilize_order(&mut snapshot);
    let fresh = normalize_ulids(&to_canonical_json(&snapshot, Some(SAMPLE_PUBLISHED_AT)));

    if std::env::var_os("EXPORT_SAMPLE_SNAPSHOT").is_some() {
        std::fs::write(COMMITTED_PATH, &fresh).expect("failed to write sample-snapshot.json");
        return;
    }

    let committed = std::fs::read_to_string(COMMITTED_PATH).unwrap_or_default();
    assert_eq!(
        committed.replace("\r\n", "\n"),
        fresh,
        "packages/shared/fixtures/sample-snapshot.json is stale relative to build_snapshot(dev_seed). \
         Regenerate with `EXPORT_SAMPLE_SNAPSHOT=1 cargo test -p inventory-sync \
         sample_snapshot_fixture_matches_generator` and commit the result."
    );
}
