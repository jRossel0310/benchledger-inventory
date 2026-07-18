//! Match persisted import lines against existing parts (Phase 5b Task 2,
//! spec §10 "Match"). This is the read-only half of Match -> Review ->
//! Commit: it scores each part-kind line of a persisted import against parts
//! already on file and hands back ranked suggestions, but never writes to
//! `parts`/`manufacturer_variants`/`supplier_listings`/`transactions` —
//! commit (Task 4) is the only mutation.
//!
//! Matching itself is entirely delegated to the existing 7-level matcher in
//! `matching.rs` (`Database::find_matches`/`MatchCandidate`/`MatchResult`):
//! nothing here re-implements SKU/MPN/alias/identity comparison. This module
//! only adapts a persisted `ImportLineRecord` into a `MatchCandidate`
//! (`candidate_from_line`) and assembles the per-line/per-import result
//! shape the review layer (Task 3) will consume.

use inventory_core::ids::{ImportId, ImportLineId};

use crate::imports::ImportLineRecord;
use crate::matching::{MatchCandidate, MatchResult};
use crate::{Database, DbError};

/// Build the candidate `find_matches` scores against parts on file from a
/// persisted import line. `category_id`/`attributes`/`package` are
/// deliberately left empty/`None` here: description-based category/attribute
/// inference is enrichment (5c), and SKU/MPN/alias matching (levels 1-3,
/// which is all a bare import line can ever reach) works without them. A
/// blank (empty-after-trim) string on either the line or `supplier` is
/// treated as absent — the same "blank means None" convention
/// `find_matches`'s own `non_empty` helper applies internally — so a
/// line that merely has an empty-string field rather than a SQL NULL still
/// matches nothing spuriously.
pub fn candidate_from_line(line: &ImportLineRecord, supplier: &str) -> MatchCandidate {
    MatchCandidate {
        supplier: non_empty(supplier),
        supplier_sku: line.supplier_sku.as_deref().and_then(non_empty),
        manufacturer: line.manufacturer.as_deref().and_then(non_empty),
        mpn: line.mpn.as_deref().and_then(non_empty),
        category_id: None,
        attributes: vec![],
        package: None,
    }
}

fn non_empty(s: &str) -> Option<String> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// One import line's match results: `matches` ranked best-first exactly as
/// `find_matches` returns them, `top` the best (lowest-rank) entry if any —
/// `None` when the line matched nothing on file (a candidate for
/// `ProposedAction::CreateNew` in Task 3).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ImportLineMatch {
    pub line_id: ImportLineId,
    pub line_number: Option<i64>,
    pub matches: Vec<MatchResult>,
    pub top: Option<MatchResult>,
}

impl Database {
    /// Score one persisted import line against parts on file. Builds the
    /// `MatchCandidate` via `candidate_from_line` and delegates entirely to
    /// `find_matches` — no matching logic duplicated here. Read-only.
    pub fn match_import_line(
        &mut self,
        line: &ImportLineRecord,
        supplier: &str,
    ) -> Result<ImportLineMatch, DbError> {
        let candidate = candidate_from_line(line, supplier);
        let matches = self.find_matches(&candidate)?;
        let top = matches.first().cloned();
        Ok(ImportLineMatch {
            line_id: line.id.clone(),
            line_number: line.line_number,
            matches,
            top,
        })
    }

    /// Match every `line_kind = 'part'` line of an import against parts on
    /// file, in the import's own line order (`list_import_lines`'s order).
    /// Non-inventory lines (fee/tariff/no_charge/unknown) are excluded
    /// entirely — spec §10: only Part lines are ever eligible to become a
    /// part or a receive, so they are never matched in the first place.
    /// Read-only; no inventory mutation.
    pub fn match_import(&mut self, import_id: &ImportId) -> Result<Vec<ImportLineMatch>, DbError> {
        let import = self.get_import(import_id)?.ok_or(DbError::ImportNotFound)?;
        let lines = self.list_import_lines(import_id)?;
        let mut out = Vec::with_capacity(lines.len());
        for line in lines.iter().filter(|l| l.line_kind == "part") {
            out.push(self.match_import_line(line, &import.supplier)?);
        }
        Ok(out)
    }
}
