//! Guard test: no committed fixture may contain a personal-information string.
//!
//! On the authoring machine, `samples/digikey/private/pii-denylist.txt`
//! (gitignored) lists the real personal details from the raw DigiKey samples.
//! This test reads every committed file under `tests/fixtures/` and fails if any
//! contains a denylisted string (case-insensitive), catching a sanitization slip
//! before it is committed. On a machine without the denylist (e.g. a fresh
//! clone/CI), the test passes with a printed note — the protection is at
//! authoring time, where the private data actually exists.

use std::fs;
use std::path::{Path, PathBuf};

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn denylist_entries() -> Option<Vec<String>> {
    // crates/inventory-import -> repo root is two levels up.
    let path = manifest_dir()
        .join("..")
        .join("..")
        .join("samples")
        .join("digikey")
        .join("private")
        .join("pii-denylist.txt");
    let raw = fs::read_to_string(path).ok()?;
    let entries: Vec<String> = raw
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.to_lowercase())
        .collect();
    Some(entries)
}

fn fixture_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                out.push(path);
            }
        }
    }
    out
}

#[test]
fn committed_fixtures_contain_no_personal_information() {
    let entries = match denylist_entries() {
        Some(e) if !e.is_empty() => e,
        _ => {
            println!(
                "pii-denylist.txt absent or empty — skipping PII scan (expected on a fresh clone)"
            );
            return;
        }
    };

    // Scan the committed fixtures AND the committed (non-private) sample docs —
    // any committed file near the raw data is a potential leak surface.
    let repo_root = manifest_dir().join("..").join("..");
    let mut files = fixture_files(&manifest_dir().join("tests").join("fixtures"));
    assert!(
        !files.is_empty(),
        "no fixtures found under tests/fixtures — expected at least the sanitized sample"
    );
    files.extend(fixture_files(&repo_root.join("samples").join("digikey")));

    for file in files {
        let contents = fs::read(&file).unwrap_or_default();
        let haystack = String::from_utf8_lossy(&contents).to_lowercase();
        for needle in &entries {
            assert!(
                !haystack.contains(needle.as_str()),
                "committed fixture {} contains a denylisted personal string — sanitize it before committing",
                file.display()
            );
        }
    }
}
