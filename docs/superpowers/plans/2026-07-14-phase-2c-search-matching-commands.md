# Phase 2c: Search, Duplicate Matching, Typed Commands — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Electronics-oriented search (FTS5 + structured operators), the duplicate-matching engine with remembered decisions (aliases, approved/rejected equivalences), and the typed Tauri command surface (tauri-specta) that Phase 3's UI will call — completing Phase 2 (Inventory domain).

**Architecture:** A `search_text` choke-point table (rebuilt per part by Rust after any content change) feeds an external-content FTS5 index via triggers — one sync point, no multi-table trigger web. The query grammar parses syntactically in `inventory-core` (kind-blind `RawFilter`s); `inventory-db` resolves keys against attribute defs/dimensions/stock and executes. Matching compares exact forms (`ParsedValue` equality via re-parse, package canonicals, exact choice strings) per the 2b review contract. Spec §7 (matching), §8 (search); plan inputs from `.superpowers/sdd/progress.md` (2c section).

**Tech Stack:** SQLite FTS5 (bundled), rusqlite, tauri-specta v2 (typed bindings), existing units/packages engines.

## Global Constraints

- PowerShell 5.1 (no `&&`; chain with `;`). `cargo` NOT on harness PATH: prepend `$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"; ` in every cargo command.
- All new tables STRICT (FTS5 virtual tables exempt — SQLite doesn't support STRICT there; document).
- Matching hierarchy (spec §10, exact order): exact supplier SKU → exact MPN → known alias → exact normalized identity → probable equivalent → similar → none. Every verdict carries a human-readable explanation string.
- Passives (categories Resistor, Capacitor, Inductor, Resistor network, Ferrite bead, Crystal) auto-combine ONLY on full identity-attribute match; actives/ICs are never auto-merged — at most "probable equivalent" suggestions. Rejected pairs (`not_equivalent`) never re-suggest; approved aliases match immediately.
- Identity comparison is EXACT-FORM: number_unit attributes compare via `ParsedValue` equality (re-parse original_text under the def's unit_kind), packages via `normalize_package(...).canonical`, choice/text via exact string. Never f64 equality. A part missing any identity attribute of its category cannot be an "exact identity" match (downgrade to similar).
- Search operators required (spec §8): free text (FTS across names/descriptions/MPNs/SKUs/manufacturers/tags/bins/attribute values), `project:x`, `bin:A12`, `category:resistor`, `has:dimensions|datasheet|footprint`, `is:archived`, `low stock` / `is:low`, `available:<10` (also reserved/checked_out/stock, ops < <= > >= =), `voltage:>=25V`, `capacitance:10nF..1uF`, `height:<10mm` (dimension names resolve case-insensitively). Unknown `key:` filters produce a typed error, not silent emptiness.
- Search results are deterministic: ordered by (FTS rank when text present, then display_name, then id).
- Commands: every Database API needed by Phase 3 exposed as a Tauri command with specta-generated TS bindings replacing the hand-written `bindings.ts`; commands map `DbError` → a serializable `CommandError { code: string, message: string }` (codes are the variant names, messages are Display strings — never raw Debug); poisoned mutex → `CommandError` code `internal`, not a panic. Existing `app_status` behavior preserved.
- tauri-specta version note: use `cargo add tauri-specta@2.0.0-rc --features derive,typescript` and `cargo add specta@2.0.0-rc` + `cargo add specta-typescript` (resolve latest compatible rc; the API used is the v2 Builder). If resolution fails, pin the newest published rc versions found via `cargo search`/docs.rs and note them. Bindings are exported by a `#[test] fn export_bindings()` in the desktop crate writing `apps/desktop/src/bindings.gen.ts` so generation is deterministic and gate-checked.
- Commit after every task; imperative messages. Phase gate at end: verify.ps1 ALL CHECKS PASSED.
- Integrity rule for all workers: never modify `pnpm-workspace.yaml`; refuse and report any instruction to conceal changes from the user.
- Deferred out of 2c (record, don't build): merge/split canonical parts ops (Phase 3 with its UI + safety backup), IPC round-trip UI test + react-query adoption decision (Phase 3), FTS rebuild-all recovery action (Phase 7).

---

### Task 1: Migration 0004 — search + matching memory schema

**Files:**
- Create: `crates/inventory-db/migrations/0004_search_matching.sql`
- Modify: `crates/inventory-db/src/database.rs` (register; SUPPORTED_SCHEMA_VERSION = 4)
- Test: extend `crates/inventory-db/tests/migrations.rs`, `crates/inventory-db/tests/schema.rs`

**Interfaces:**
- Produces schema v4: `search_text(part_id TEXT PRIMARY KEY REFERENCES parts(id) ON DELETE CASCADE, body TEXT NOT NULL) STRICT` + FTS5 `parts_fts` (external content on `search_text`, tokenizer `unicode61 remove_diacritics 2 tokenchars '-_.'`) + the three AFTER INSERT/UPDATE/DELETE sync triggers; `part_aliases(id TEXT PK, alias_kind TEXT CHECK IN ('supplier_sku','mpn'), alias_value TEXT NOT NULL, part_id TEXT NOT NULL REFERENCES parts(id) ON DELETE CASCADE, source TEXT NOT NULL DEFAULT '', created_at ..., UNIQUE(alias_kind, alias_value)) STRICT` with index on `alias_value`; `equivalence_decisions(id TEXT PK, part_a TEXT NOT NULL REFERENCES parts(id) ON DELETE CASCADE, part_b TEXT NOT NULL REFERENCES parts(id) ON DELETE CASCADE, decision TEXT CHECK IN ('approved','rejected'), note TEXT NOT NULL DEFAULT '', created_at ..., UNIQUE(part_a, part_b), CHECK (part_a < part_b)) STRICT` (canonical pair ordering — store lexicographically smaller id in part_a).

- [ ] **Step 1: Failing tests** — extend migrations.rs (`v4_schema_adds_search_and_matching_tables` checking version 4 + the 3 tables + `parts_fts` in sqlite_master; `v3_database_upgrades_to_v4` replaying MIGRATIONS.take(3) like the existing v2→v3 test, asserting backup) and schema.rs (`alias_values_are_unique_per_kind` — same (kind,value) twice rejected, different kind same value OK; `equivalence_pairs_are_canonical_and_unique` — CHECK part_a < part_b rejects inverted insert, UNIQUE rejects duplicates; `fts_stays_in_sync_with_search_text` — raw INSERT/UPDATE/DELETE on search_text then `SELECT part_id FROM parts_fts WHERE parts_fts MATCH 'resistor'` reflects each change). Write test bodies fully (follow the existing raw-SQL test style in schema.rs with `insert_part` helper).
- [ ] **Step 2: RED** — `cargo test -p inventory-db` compile/behavior failures.
- [ ] **Step 3: Write the migration SQL.** FTS5 external-content pattern (content=search_text, content_rowid=rowid) with triggers:
```sql
CREATE TABLE search_text (
    part_id TEXT PRIMARY KEY REFERENCES parts(id) ON DELETE CASCADE,
    body    TEXT NOT NULL
) STRICT;

-- FTS5 virtual tables cannot be STRICT (documented deviation).
CREATE VIRTUAL TABLE parts_fts USING fts5(
    part_id UNINDEXED,
    body,
    content='search_text',
    content_rowid='rowid',
    tokenize="unicode61 remove_diacritics 2 tokenchars '-_.'"
);

CREATE TRIGGER search_text_ai AFTER INSERT ON search_text BEGIN
    INSERT INTO parts_fts(rowid, part_id, body) VALUES (new.rowid, new.part_id, new.body);
END;
CREATE TRIGGER search_text_ad AFTER DELETE ON search_text BEGIN
    INSERT INTO parts_fts(parts_fts, rowid, part_id, body) VALUES ('delete', old.rowid, old.part_id, old.body);
END;
CREATE TRIGGER search_text_au AFTER UPDATE ON search_text BEGIN
    INSERT INTO parts_fts(parts_fts, rowid, part_id, body) VALUES ('delete', old.rowid, old.part_id, old.body);
    INSERT INTO parts_fts(rowid, part_id, body) VALUES (new.rowid, new.part_id, new.body);
END;
```
plus the two matching-memory tables per the Interfaces block. Register migration 4; bump the version constant.
- [ ] **Step 4: GREEN** (`cargo test --workspace`), fmt clean, commit: `Add search index and matching memory schema migration`.

---

### Task 2: Exact-form identity comparison (`inventory-db::identity`)

**Files:**
- Create: `crates/inventory-db/src/identity.rs`
- Modify: `crates/inventory-db/src/lib.rs`, `crates/inventory-db/src/attributes.rs` (factor out + reuse range splitting)
- Test: `crates/inventory-db/tests/identity.rs`

**Interfaces:**
- Consumes: `identity_attributes` (2b), `units::parse_with_kind`, `packages::normalize_package`.
- Produces (Task 5 matching consumes):
  - `identity::IdentityValue` enum: `Exact(inventory_core::units::ParsedValue) | Package(String) | Text(String)` with derived `PartialEq/Eq/Hash/Debug/Clone`.
  - `identity::identity_signature(db: &Database, part_id: &PartId) -> Result<Option<BTreeMap<String, IdentityValue>>, DbError>` — for every identity-flagged attribute LINKED TO THE PART'S CATEGORY: present+parsable → its IdentityValue (number_unit/range re-parsed from original_text under the def's unit_kind — a range uses both bounds as `Exact` pair encoded `Text(format!("{lo:?}..{hi:?}"))`? NO — use a fourth variant `Range(ParsedValue, ParsedValue)`); the `package` key normalizes via `normalize_package(...).canonical` → `Package`; choice/text/number → `Text` (numbers via original_text trimmed). Returns `Ok(None)` when ANY category identity attribute is missing on the part (incomplete identity ⇒ cannot exact-match). Non-identity attributes ignored. Parts whose category has zero identity attributes → `Ok(None)` (nothing to define identity).
  - `identity::signatures_equal(a: &BTreeMap<String, IdentityValue>, b: &BTreeMap<String, IdentityValue>) -> bool` (plain ==; exists for readability).
  - In `attributes.rs`: factor the `".."`/`" to "` splitting into `pub(crate) fn split_range(raw: &str) -> Option<(&str, &str)>` used by both `set_attribute` and `identity_signature`.
- Add the fourth enum variant properly: `Range(ParsedValue, ParsedValue)`.

- [ ] **Step 1: Failing tests** — `tests/identity.rs` (helpers: open DB, make resistor via seeded category as in tests/attributes.rs):
  - `equivalent_notations_have_equal_signatures`: two resistors, one with resistance "10k"/tolerance "1%"/power "1/4 W"/package "0603", the other "10000 ohm"/"1 %"/"0.25W"/"1608 metric" → both `Some`, `signatures_equal` true. **This is the ParsedValue-not-f64 test the 2b review demanded.**
  - `different_values_differ`: 10k vs 4k7 → signatures unequal.
  - `missing_identity_attribute_yields_none`: resistor with only resistance set → `Ok(None)`.
  - `category_without_identity_attributes_yields_none`: part in Miscellaneous → `Ok(None)`.
  - `range_attributes_participate_exactly`: two MOSFETs with vgs_threshold "1V..2V" and "1000mV to 2000mV" + identical channel_type/vds_max/package → equal signatures. (Set all MOSFET identity attrs: channel_type, vds_max, package.)
- [ ] **Step 2: RED**, **Step 3: implement** (query defs joined via category_attributes for the part's category with identity=1 AND hidden=0; left-join part values; build map or bail None), **Step 4: GREEN + fmt + commit**: `Add exact-form identity signatures for duplicate matching`.

---

### Task 3: Search query grammar (`inventory-core::search`)

**Files:**
- Create: `crates/inventory-core/src/search.rs`
- Modify: `crates/inventory-core/src/lib.rs`

**Interfaces:**
- Produces (Task 4 + future web twin consume):
  - `search::RawFilter { pub key: String, pub op: FilterOp, pub value: String }` where `FilterOp: Eq | Lt | Le | Gt | Ge | Range` (`Range` value keeps the raw "a..b" string; split later with the db layer's range splitter).
  - `search::ParsedQuery { pub text: String, pub filters: Vec<RawFilter>, pub flags: QueryFlags }` with `QueryFlags { pub low_stock: bool, pub archived: Option<bool> }`; `text` is the space-joined free terms (empty allowed).
  - `search::parse_query(input: &str) -> ParsedQuery` — infallible tokenizer: split on whitespace outside double quotes; a token `key:value` (first ':' splits; value may be quoted) becomes a RawFilter with op extracted from value prefix (`>=`, `<=`, `>`, `<`, `=`, none→Eq; `a..b` → Range); bare `low` followed by `stock` (or token `low-stock`) sets flags.low_stock; `is:archived`→flags.archived=Some(true), `is:active`→Some(false), `is:low`→low_stock; everything else joins `text`. `has:x` stays a RawFilter (key "has"). No DB knowledge here.

- [ ] **Step 1: Failing tests** (in-module) covering exactly: `10k 0603` (pure text), `100n ceramic 50v` (pure text), `project:lightning`, `bin:A12`, `has:datasheet`, `available:<10` (op Lt value "10"), `voltage:>=25V` (op Ge value "25V"), `capacitance:10nF..1uF` (op Range value "10nF..1uF"), `height:<10mm`, `low stock` (flags.low_stock, empty text), `is:archived`, quoted values (`bin:"Drawer 3"`), mixed query `dual op amp project:amp has:footprint` (text "dual op amp" + 2 filters), empty input.
- [ ] **Step 2: RED**, **Step 3: implement** (hand tokenizer, ~120 lines, no regex needed), **Step 4: GREEN + fmt + commit**: `Add search query grammar parser`.

---

### Task 4: Search execution (`inventory-db::search`)

**Files:**
- Create: `crates/inventory-db/src/search.rs`
- Modify: `crates/inventory-db/src/lib.rs`, `crates/inventory-db/src/parts.rs` + `attributes.rs` + `dimensions.rs` (call `refresh_search_text` at every content-mutation point), `crates/inventory-db/src/database.rs` (error variant)
- Test: `crates/inventory-db/tests/search.rs`

**Interfaces:**
- `impl Database`:
  - `pub(crate) fn refresh_search_text(&mut self, part_id: &PartId) -> Result<(), DbError>` — rebuilds the part's `search_text.body`: display_name, category name, description, tags, bin_label, manufacturers, MPNs, supplier SKUs, attribute original_texts + formatted values + keys, dimension names. Upsert (INSERT ON CONFLICT UPDATE); called from create_part/update_part/set_part_archived(false? archived parts stay indexed — filtering handles exclusion)/add_variant/set_preferred_variant(no text change — skip)/add_supplier_listing/set_attribute/clear_attribute/add_dimension/remove_dimension. (Tags API doesn't exist yet — spec has part_tags; expose `set_tags(&mut self, part_id, tags: &[String])` here since search needs it, replacing rows + refreshing.)
  - `pub fn search(&self, query: &str) -> Result<Vec<SearchHit>, DbError>` where `SearchHit { part_id: PartId, display_name: String, category_name: String, bin_label: Option<String>, available: Quantity, reserved: Quantity, checked_out: Quantity, archived: bool }`. Pipeline: parse_query → candidate set (FTS MATCH on sanitized text — escape double quotes, join terms with spaces = implicit AND — or all parts when text empty) → apply filters:
    - `project:` → parts having reserved/checked_out txns or reservations for a project whose name LIKE %value% (use transactions join, DISTINCT)
    - `bin:` exact case-insensitive on bin_label; `category:` case-insensitive name match
    - `has:datasheet` → any variant with datasheet_url NOT NULL; `has:dimensions` → dimensions row exists; `has:footprint` → FALSE for now (CAD links arrive Phase 3 — return typed `UnknownSearchKey` error? NO: accept and yield empty set with explanation deferred... decide: `has:footprint` returns empty result set silently is a lie; better: typed error `UnsupportedSearchKey("footprint (arrives with CAD links in Phase 3)")`). Implement `has:` with datasheet|dimensions supported, footprint → UnsupportedSearchKey.
    - stock fields `available|reserved|checked_out|stock` numeric ops against part_stock milli (value parsed as whole units ×1000; `stock` = sum)
    - `low stock` flag → available_milli < low_stock_threshold_milli (only parts WITH a threshold)
    - `is:archived` → archived=1 (default when flag absent: archived excluded)
    - attribute keys: resolve via attribute_defs (number_unit/range → parse value under def's unit_kind via ops on value_num (+value_num_hi for stored ranges: filter matches when stored range overlaps requested op — simplify: compare against value_num only, document); unknown key → try dimensions by lowercase name (`height:<10mm` → normalized_value vs parsed mm); still unknown → `UnknownSearchKey(key)` typed error.
  - `DbError` gains `UnknownSearchKey(String)` and `UnsupportedSearchKey(String)`.
- Ordering: FTS bm25 rank when text non-empty, else display_name; id tiebreak.

- [ ] **Step 1: Failing tests** — `tests/search.rs` with a seeded scenario builder (3 resistors incl. one archived + one low-stock-thresholded, 1 op amp with variant TLV9002IDDFR + SKU 296-TLV9002IDDFRCT-ND + datasheet_url, 1 Meter wire part with height dimension 5mm, one project with a reservation). Tests: free text `10k` finds the 10k resistor not the op amp; `TLV9002` matches via MPN; `296-TLV9002IDDFRCT-ND` via SKU; `bin:A12`; `available:<10`; `voltage:>=25V` on capacitor voltage_rating (seed a capacitor); `height:<10mm` matches the wire part, `height:<3mm` doesn't; `low stock` only the thresholded part; default excludes archived + `is:archived` includes only it; `project:` finds the reserved part; `has:datasheet` finds the op amp only; `has:footprint` → UnsupportedSearchKey; `nonsense:5` → UnknownSearchKey; determinism (two identical calls, identical order).
- [ ] **Step 2: RED**, **Step 3: implement + wire refresh calls**, **Step 4: GREEN (`cargo test --workspace`) + fmt + commit**: `Add FTS-backed search with structured filters`.

---

### Task 5: Duplicate matching engine (`inventory-db::matching`)

**Files:**
- Create: `crates/inventory-db/src/matching.rs`
- Modify: `crates/inventory-db/src/lib.rs`
- Test: `crates/inventory-db/tests/matching.rs`

**Interfaces:**
- `matching::MatchVerdict` enum: `ExactSku { listing: ListingId } | ExactMpn { variant: VariantId } | KnownAlias | ExactIdentity | ProbableEquivalent | Similar | None` — surfaced as `MatchResult { part_id: PartId, display_name: String, verdict_kind: String, explanation: String, rank: u8 }` (rank = hierarchy position 1-7 for sorting; verdict_kind = snake_case).
- `matching::MatchCandidate` input: `{ supplier: Option<String>, supplier_sku: Option<String>, manufacturer: Option<String>, mpn: Option<String>, category_id: Option<CategoryId>, attributes: Vec<(String, String)> /* key, raw value */, package: Option<String> }`.
- `impl Database { pub fn find_matches(&mut self, candidate: &MatchCandidate) -> Result<Vec<MatchResult>, DbError> }` — hierarchy per Global Constraints:
  1. exact supplier SKU (case-insensitive on supplier_listings.supplier_sku; explanation "Exact supplier SKU match: <sku>")
  2. exact MPN (case-insensitive manufacturer_variants.mpn; "Exact manufacturer part-number match: <mpn>")
  3. known alias (part_aliases by kind+value)
  4. exact normalized identity: build a temp signature from candidate.attributes parsed under the candidate category's identity defs (same rules as identity_signature; requires category_id + complete set) and compare against `identity_signature` of every non-archived part in that category → for PASSIVE categories explanation lists matched fields ("Resistance, package, tolerance, and power all match"); for ACTIVE categories exact identity yields at most ProbableEquivalent (never rank-4 auto-combinable) — encode passive list per Global Constraints.
  5. probable equivalent: identity match minus at most one MISSING (not conflicting) field ("Capacitance and package match, but voltage is missing")
  6. similar: ≥2 identity fields match, ≥1 conflicts ("Similar device, but package differs")
  7. none.
  - Rejected pairs (equivalence_decisions decision='rejected' between candidate-matched part and... rejection is pairwise between PARTS; for candidate matching, rejections apply when the candidate ALSO matched some other part exactly — simplification: `find_matches` takes an optional `exclude_rejected_for: Option<PartId>`; plumb later in import (Phase 5). For 2c: expose `pub fn record_equivalence(&mut self, a: &PartId, b: &PartId, decision: &str, note: &str)` + `pub fn equivalence_between(&self, a, b) -> Result<Option<String>, DbError>`; suggestions BETWEEN EXISTING PARTS (`pub fn suggest_duplicates(&mut self, part_id: &PartId) -> Result<Vec<MatchResult>, DbError>`) skip rejected pairs and rank approved pairs as KnownAlias-equivalent (rank 3, "Previously approved as equivalent").
- `pub fn add_alias(&mut self, kind: &str, value: &str, part_id: &PartId, source: &str) -> Result<(), DbError>` (kind CHECK'd by schema; duplicate (kind,value) → typed `AliasTaken` DbError variant).

- [ ] **Step 1: Failing tests** — `tests/matching.rs` scenario helpers; the spec §18 duplicate-matching list as tests: exact passive match across manufacturers (two 10k/0603/1%/0.25W resistor parts → suggest_duplicates rank ExactIdentity with the all-fields explanation); missing identity field → ProbableEquivalent with "voltage is missing"-style explanation (capacitor without voltage_rating); different voltage rating → Similar ("voltage differs"); different dielectric → Similar; different package → Similar; two op amps (active) with same identity attrs → ProbableEquivalent NOT ExactIdentity (actives never auto-combine); approved equivalence → rank 3 w/ "Previously approved"; rejected equivalence → absent from suggestions; candidate exact SKU beats identity (find_matches with both possible → SKU first, rank 1); known alias hit; `AliasTaken` on duplicate alias.
- [ ] **Step 2: RED**, **Step 3: implement**, **Step 4: GREEN + fmt + commit**: `Add duplicate matching engine with decision memory`.

---

### Task 6: Seed attribute-key collision warn (2b review carry-in)

**Files:**
- Modify: `crates/inventory-db/src/seed.rs`
- Test: extend `crates/inventory-db/tests/seed.rs`

**Interfaces:** mirror of the category-name warn: after the attribute loop, for each seed attribute whose `INSERT OR IGNORE` inserted 0 rows, compare `SELECT id FROM attribute_defs WHERE key = ?` against the expected det_id; differing id → `tracing::warn!(attribute = key, "built-in attribute key is taken by a non-seed row; built-in choices/links may attach to it")`. ALSO add `AND built_in = 1` to the attribute-side of the CHOICES insert's SELECT and CATEGORY_LINKS insert's SELECT (`... FROM attribute_defs WHERE key = ?2` → `... AND built_in = 1`) so user attributes never silently receive built-in choices/links (the 2b review's "insert-only in spirit" gap).

- [ ] **Step 1: Failing test** — `attribute_key_collision_does_not_leak_builtin_choices`: delete the seeded `mounting_style` def (cascades choices/links), insert a user attribute with key `mounting_style` (built_in=0, data_type 'text'), re-run `ensure_builtins` → no error, `attributes_inserted == 0`, AND `SELECT COUNT(*) FROM attribute_choices` for that user attribute id == 0 (no built-in choices leaked onto it).
- [ ] **Step 2: RED** (the choices leak reproduces), **Step 3: implement both changes**, **Step 4: GREEN + fmt + commit**: `Warn on attribute key collisions and guard built-in choice attachment`.

---

### Task 7: Typed Tauri command layer with specta bindings

**Files:**
- Modify: `apps/desktop/src-tauri/Cargo.toml` (specta deps), `apps/desktop/src-tauri/src/main.rs`, `apps/desktop/src-tauri/src/app.rs`
- Create: `apps/desktop/src-tauri/src/commands.rs`, `apps/desktop/src/bindings.gen.ts` (generated)
- Modify: `apps/desktop/src/bindings.ts` (re-export from generated), `apps/desktop/src/features/dashboard/StatusPanel.tsx` (import path if needed)
- Test: Rust unit tests in commands.rs + the `export_bindings` test; existing StatusPanel vitest must stay green.

**Interfaces:**
- `commands::CommandError { code: String, message: String }` (Serialize + specta::Type); `impl From<DbError>` mapping variant-name → code (snake_case, e.g. `insufficient_stock`), Display → message; poisoned-mutex → `{ code: "internal", message: "database lock poisoned; restart the app" }` (replace the `expect` in `status_of` — it moves into commands and returns Result).
- Commands (all `#[tauri::command] #[specta::specta]`, taking `State<AppState>`): `app_status`, `list_parts(include_archived)`, `get_part(part_id)`, `create_part(draft)`, `update_part(record)`, `set_part_archived(part_id, archived)`, `get_stock(part_id)`, `apply_ledger_op(op)` (LedgerOp needs Serialize/Deserialize/specta::Type derives added in inventory-core — with `#[serde(tag = "type", rename_all = "snake_case")]`), `apply_group(kind, note, ops)`, `reverse_transaction(txn_id, note)`, `reverse_group(group_id, note)`, `list_transactions(part_id)`, `get_group(group_id)`, `set_attribute(part_id, key, raw)`, `get_attributes(part_id)`, `clear_attribute(part_id, key)`, `add_dimension(part_id, draft)`, `list_dimensions(part_id)`, `remove_dimension(id)`, `add_variant(part_id, draft)`, `set_preferred_variant(part_id, variant_id)`, `add_supplier_listing(variant_id, draft)`, `list_categories`, `category_attributes(category_id)`, `create_category(name, group)`, `duplicate_category(source, new_name)`, `create_custom_attribute(...)`, `attach_attribute(...)`, `set_attribute_hidden(...)`, `reorder_attribute(...)`, `search(query)`, `find_matches(candidate)`, `suggest_duplicates(part_id)`, `record_equivalence(a, b, decision, note)`, `add_alias(kind, value, part_id, source)`, `set_tags(part_id, tags)`, `validate_invariants`.
- Public record/draft structs across inventory-core/db gain `serde::Serialize/Deserialize` + `specta::Type` derives as needed (PartRecord, PartDraft, VariantDraft/Record, ListingDraft/Record, PartStockRow→serialize as milli map, TransactionRecord, GroupRecord, DimensionDraft/Record + enums, SearchHit, MatchResult, MatchCandidate, ValidationReport/Discrepancy, CategoryRecord, Quantity already serializes, typed IDs already serialize; specta::Type impls via derive — inventory-core/db gain an optional `specta` dependency behind default features... simpler: add `specta = { version = "2.0.0-rc", features = ["derive"] }` as a NORMAL dependency of inventory-core and inventory-db and derive unconditionally).
- Bindings generation: `tauri_specta::Builder` in `commands::builder()` used by main's `invoke_handler` AND by `#[test] fn export_bindings()` writing `apps/desktop/src/bindings.gen.ts`; `apps/desktop/src/bindings.ts` becomes `export * from './bindings.gen'; export { appStatus } ...` compat as needed; StatusPanel switches to the generated `commands.appStatus()`. Keep the vitest mock working (mock the generated module or keep mocking `@tauri-apps/api/core` — the generated bindings call invoke internally, so the existing mock keeps working; verify).

- [ ] **Step 1:** add deps (`cargo add` forms from Global Constraints; expect rc-version resolution wrinkles — document exact pins chosen).
- [ ] **Step 2: Failing tests** — commands.rs unit tests calling handlers directly with a temp-dir AppState (pattern from app.rs tests): `commands_map_typed_errors` (consume beyond stock → CommandError code `insufficient_stock`, message non-Debug), `search_command_round_trips`, `poisoned_mutex_maps_to_internal` (poison via `let _ = std::panic::catch_unwind(...)` holding the lock — if too contrived, drop this test and document), plus `export_bindings` test writing the file.
- [ ] **Step 3: RED → implement.** Derives sweep first (compile-driven), then commands.rs (~35 thin wrappers: lock state → call Database method → map_err into CommandError), builder + main wiring, bindings generation, frontend compat shim.
- [ ] **Step 4:** `cargo test --workspace` green; `pnpm --filter @ei/desktop test` green (StatusPanel); `pnpm --filter @ei/desktop build` green (tsc validates generated bindings); fmt clean. Commit: `Add typed command layer with specta-generated bindings`.

---

### Task 8: Phase gate and documentation

**Files:** `docs/schema.md`, `docs/architecture.md`, `docs/decisions.md`, `docs/search.md` (new)

- [ ] **Step 1:** Full gate → ALL CHECKS PASSED (fmt fixes as separate commit if needed).
- [ ] **Step 2:** Docs: schema.md gains migration 0004 section (search_text/parts_fts external-content pattern + trigger sync + FTS5-not-STRICT note, part_aliases, equivalence_decisions canonical pair ordering, "Current version: 4"); new docs/search.md documenting the query grammar (all operators + examples + unsupported `has:footprint` until Phase 3); architecture.md bullet for search/matching/commands; decisions.md rows: exact-form identity comparison contract; passives-only auto-combine list; search_text choke-point + external-content FTS pattern; `has:footprint` typed-unsupported until Phase 3; specta rc pin chosen; CommandError code contract.
- [ ] **Step 3:** Commit: `Add phase 2c documentation and decision log entries`.

---

## Plan self-review notes

- **Spec coverage (2c scope):** search indexing + all §8 operators (T1/T3/T4), matching hierarchy + explanations + decision memory (T2/T5), aliases (T1/T5), typed commands for the full domain surface (T7), 2b carry-ins (T2 range factor, T6 seed warn + built_in guard). Deferred with rationale: merge/split ops, `has:footprint` (needs Phase 3 CAD links), IPC UI test, FTS rebuild-all (Phase 7 recovery).
- **Known simplifications (document in decisions):** stored range attributes filter on value_num only; `project:` matches by name substring against ledger-linked projects; active-category exact identity caps at ProbableEquivalent (spec: never silently merge actives); archived parts stay in the FTS index (filter excludes at query time).
- **Type consistency:** IdentityValue::Range added alongside the T2 interface list; MatchCandidate.attributes uses raw strings parsed under defs exactly like set_attribute; SearchHit quantities via the part's real unit (reuse get_stock plumbing).
- **Risk register:** tauri-specta rc-version drift (mitigation: cargo add + document pins; fallback is hand-written bindings for the delta, flagged to controller); FTS5 external-content trigger correctness (T1 has a dedicated sync test); LedgerOp serde tag representation must match the TS side (generated — no hand-drift possible).
