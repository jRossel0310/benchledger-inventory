//! Bin browser aggregates (Phase 3 Task 8): parts grouped by their physical
//! storage location (`parts.bin_label`), plus bulk bin rename. There is no
//! separate `bins` table — a bin is just the distinct set of `bin_label`
//! values currently on file, per the design direction's "a part's storage
//! location is its bin_label field, not a separate bins table."

use inventory_core::ids::PartId;

use crate::{Database, DbError};

/// One row of the bin browser's tile grid: a bin label (`None` for the
/// "Unassigned" bucket — every part with no `bin_label`) and how many
/// non-archived parts currently sit in it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct BinSummary {
    pub bin_label: Option<String>,
    pub part_count: i64,
}

impl Database {
    /// Every distinct bin label among non-archived parts (matching
    /// `list_parts(false)`/`search`'s default and `dashboard_summary`'s
    /// counts), each with its part count, plus one row for the "Unassigned"
    /// bucket (`bin_label IS NULL`) whenever at least one non-archived part
    /// has no bin. Named bins sort alphabetically (case-insensitive) first;
    /// the unassigned bucket, when present, always sorts last — it isn't a
    /// physical location like the others, so it reads as a distinct
    /// "needs a home" catch-all rather than just another alphabetical entry
    /// (an empty-string label would sort first under plain ASCII/NOCASE
    /// ordering, which would bury it above real bins instead).
    pub fn list_bins(&self) -> Result<Vec<BinSummary>, DbError> {
        // GROUP BY is case-insensitive (COLLATE NOCASE) to match the `bin:`
        // search filter, `rename_bin`'s WHERE, and the frontend, all of which
        // treat bin labels case-insensitively — a byte-exact GROUP BY would
        // otherwise split e.g. "A1" and "a1" into two tiles that each show
        // the merged set of parts once clicked (since search is
        // case-insensitive), which is confusing. SQLite doesn't guarantee
        // which case-variant's exact spelling is returned to represent the
        // group; that's fine here since same-bin case variants shouldn't
        // normally exist.
        let mut stmt = self.raw_conn().prepare(
            "SELECT bin_label, COUNT(*) FROM parts
             WHERE archived = 0
             GROUP BY bin_label COLLATE NOCASE
             ORDER BY (bin_label IS NULL) ASC, bin_label COLLATE NOCASE ASC",
        )?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(BinSummary {
                bin_label: row.get(0)?,
                part_count: row.get(1)?,
            });
        }
        Ok(out)
    }

    /// Move every non-archived part currently in `old_label` (exact,
    /// case-insensitive match — the same rule `search`'s `bin:` filter uses)
    /// to `new_label`, in one transaction — atomic, so a bin never ends up
    /// half-renamed if a later write in the batch failed. If `new_label`
    /// already has parts, they merge (the caller — the bin browser UI — is
    /// responsible for warning about that before calling this; this method
    /// itself never blocks a merge, matching the spec's "warn, don't forbid"
    /// rule for multiple parts sharing a bin). Returns how many parts moved
    /// (`0` if no non-archived part currently has `old_label` — a harmless
    /// no-op rather than an error, so a bin browser acting on a just-stale
    /// listing degrades safely).
    ///
    /// Archived parts keep their old `bin_label` untouched: `list_bins` (and
    /// so the bin browser's tile grid) never shows them, so a rename offered
    /// from that grid should only move what the grid actually displayed —
    /// not silently relabel history the user never saw as part of this bin.
    ///
    /// `new_label` is trimmed and rejected (`DbError::InvalidBinLabel`) if
    /// empty: renaming to "no bin" would silently bulk-unassign every part
    /// in `old_label`, a different and riskier action than a rename.
    /// Clearing a single part's bin is a deliberate per-part action via
    /// `update_part`, not something a rename should do to a whole bin.
    pub fn rename_bin(&mut self, old_label: &str, new_label: &str) -> Result<u32, DbError> {
        let new_trimmed = new_label.trim();
        if new_trimmed.is_empty() {
            return Err(DbError::InvalidBinLabel(
                "new bin label cannot be empty".to_string(),
            ));
        }

        let ids: Vec<String> = {
            let mut stmt = self.raw_conn().prepare(
                "SELECT id FROM parts WHERE bin_label = ?1 COLLATE NOCASE AND archived = 0",
            )?;
            let rows = stmt.query_map([old_label], |r| r.get(0))?;
            rows.collect::<Result<_, _>>()?
        };
        if ids.is_empty() {
            return Ok(0);
        }

        {
            let tx = self.conn_mut().transaction()?;
            for id in &ids {
                tx.execute(
                    "UPDATE parts SET bin_label = ?2, modified_at = datetime('now') WHERE id = ?1",
                    rusqlite::params![id, new_trimmed],
                )?;
            }
            tx.commit()?;
        }

        // search_text embeds the bin label (see `refresh_search_text`), so
        // every moved part's index must catch up outside the transaction
        // above — same two-phase pattern `set_tags` uses.
        for id in &ids {
            let part_id = PartId::from_string(id.clone())
                .map_err(|_| DbError::Corrupt("bad part id".into()))?;
            self.refresh_search_text(&part_id)?;
        }
        Ok(ids.len() as u32)
    }
}
