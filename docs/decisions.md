# Decision log

| Date | Decision | Why |
|---|---|---|
| 2026-07-14 | Rust core owns all domain/DB logic; React is thin | Integrity at the DB; one implementation; cargo-testable (spec ADR 1) |
| 2026-07-14 | Quantities: fixed-point integer x1000 | Exact; continuous units supported (spec ADR 4) |
| 2026-07-14 | ULIDs for IDs | Stable, sortable, deterministic exports (spec ADR 5) |
| 2026-07-14 | `PRAGMA user_version` + embedded numbered SQL migrations | Minimal, transactional, testable |
| 2026-07-14 | Redaction at the log-writer layer | Secrets cannot reach disk regardless of call site |
| 2026-07-14 | Hand-written TS bindings for the single Phase 1 command | tauri-specta adopted in Phase 2 when the command surface grows (spec ADR 11) |
| 2026-07-14 | TanStack Router deferred to Phase 3 | Phase 1 has one screen; YAGNI |
| 2026-07-14 | rusqlite backup feature + validated serde deserialization for Quantity | Review findings during Tasks 4-5 (see .superpowers/sdd reports) |
| 2026-07-14 | Web app uses Vite appType 'mpa' | Preview SPA fallback masked missing-snapshot 404; forward risk documented in vite.config.ts |
| 2026-07-14 | Phase 2 split into 2a (schema+ledger), 2b (categories/attributes/units/dimensions), 2c (search+matching+commands) | Keeps each plan reviewable; each ships working software |
| 2026-07-14 | Adjustments never touch lifetime counters and require a note | They are corrections, not history |
| 2026-07-14 | Archived parts allow only release/return/reversal | Stock must drain home without reactivating the part |
| 2026-07-14 | Reversal deltas recomputed from stored rows (`delta_from_stored`) | One source of truth shared by reversals and the validator |
| 2026-07-14 | Group members ordered by rowid | created_at is second-granular; ULIDs don't sort by creation time |
| 2026-07-14 | Group members cannot be reversed individually | Preserves atomic group reversibility; compensating ops cover line-level corrections |
| 2026-07-14 | Layer-1 SQL defense is CHECK constraints only (no verification triggers, deviating from spec §4.5) | Domain layer is the only writer; triggers add complexity without a second writer. Revisit if another writer appears |
| 2026-07-14 | Attribute normalization stores f64 for filtering; identity compares exact (mantissa, exp10) re-parsed from original text | No float-equality traps; original text is never lost |
| 2026-07-14 | Built-in seeds are insert-only with deterministic ids, run at every open | User customizations survive; new built-ins arrive in upgrades |
| 2026-07-14 | Bare chip package codes read as imperial (0603 = imperial unless 'metric' suffix) | Matches supplier convention |
| 2026-07-14 | Curated attribute sets for 17 key categories; others get shared basics | Full 70-category curation is data work that can grow incrementally |
| 2026-07-14 | quantity_unit changes blocked once a part has transactions | Stored milli values would silently change meaning |
| 2026-07-16 | Duplicate-matching identity comparison is exact-form (`ParsedValue`/package-canonical/trimmed text), never `f64` | Lossy float equality is unsound for "10k == 10000 ohm"; reuses the units engine's own exact comparable form |
| 2026-07-16 | Only a fixed list of seeded PASSIVE categories (Resistor, Capacitor, Inductor, Resistor network, Ferrite bead, Crystal) auto-combine on an exact identity match; every other category, including all custom ones, caps at `ProbableEquivalent` | Spec: never silently merge actives/ICs; passives are safe to fungibly combine, actives are not |
| 2026-07-16 | `search_text` is a single denormalized per-part choke-point rebuilt by `refresh_search_text`, kept in sync with an FTS5 external-content `parts_fts` index by triggers | One place knows how to build searchable content; FTS index stays consistent without duplicating part data |
| 2026-07-16 | `has:footprint` returns a typed `UnsupportedSearchKey`, not `UnknownSearchKey` or an empty result | Footprint/CAD-link data is a real, recognized concept just not modeled until Phase 3 — distinguishes "not yet" from "not a thing" |
| 2026-07-16 | Pinned specta 2.0.0-rc.25 / tauri-specta 2.0.0-rc.25 / specta-typescript 0.0.12 (the upstream-recommended matched set); i64 fields export as JS `number` via `dangerously_cast_bigints_to_number` | Reproducible generated bindings; all i64 values (milli-quantities, micro-prices) stay far under 2^53 in any realistic inventory |
| 2026-07-16 | `CommandError { code, message }`: `code` is the snake_case `DbError` variant name, `message` is its `Display` text (never `Debug`) | Stable, matchable-by-frontend error codes; `Debug` text is for logs, not UI |
| 2026-07-16 | Search attribute filters require the exact attribute key (e.g. `voltage_rating:`, not `voltage:`); no label/alias resolution yet | Safer to require exactness than guess a match; friendlier key resolution arrives Phase 3 |
