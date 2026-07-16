# Architecture

See the spec for full detail. Summary of what exists after Phase 1:

- **Rust core** (`crates/*`): all domain and persistence logic. The UI never
  computes stock or touches SQLite directly.
- **Desktop** (`apps/desktop`): React UI over typed Tauri commands. Startup:
  resolve data dir (`ELECTRONICS_INVENTORY_DATA_DIR` override, else
  `%APPDATA%\ElectronicsInventory`) → ensure layout → init redacting logging →
  open + migrate SQLite → serve `app_status`. Only failures before logging
  init (data dir resolution, layout creation) reach stderr alone; database
  open/migration failures are logged to the file before the process exits.
  Recovery-mode surfacing arrives in Phase 7.
- **Web** (`apps/web`): static SPA that loads `/inventory.snapshot.json` and
  renders read-only state. No write paths exist.
- **Tokens** (`packages/shared`): primitive palette + semantic tokens emitted
  as CSS custom properties; stylelint forbids raw colors anywhere else.
- **Migrations**: numbered SQL embedded in `inventory-db`, applied in one
  transaction each, `PRAGMA user_version` tracks state, pre-migration safety
  backup via SQLite online backup API, newer-schema refusal.
- **Quantities**: exact fixed-point milli-units (`Quantity`, x1000).
- **Ledger** (`inventory-core::ledger` + `inventory-db::ledger`): every stock
  change is a transaction row plus an aggregate update in one SQL transaction.
  Pure state-transition logic (deltas, validation) lives in core; SQL
  application, groups, and reversals in db. See `docs/schema.md`.
- **Attributes & units** (`inventory-core::units`, `inventory-db::attributes`):
  typed category attributes with exact-decimal normalization; built-in category
  taxonomy seeds idempotently. Dimensions normalize to mm/g with provenance.
- **Search & matching** (`inventory-core::search`, `inventory-db::search`/
  `identity`/`matching`): every part-content mutation funnels through
  `refresh_search_text`'s `search_text` choke-point, kept in sync with the
  `parts_fts` FTS5 external-content index by migration 0004's triggers; the
  query grammar is a syntactic, DB-blind parser reusable verbatim by a future
  web TS twin (see `docs/search.md`). Duplicate detection walks a 7-level
  verdict hierarchy (`ExactSku` → `ExactMpn` → `KnownAlias` → `ExactIdentity`
  → `ProbableEquivalent` → `Similar` → `None`) built on exact-form identity
  comparison (`ParsedValue`, never `f64`) shared with the units engine;
  passive categories (resistors, capacitors, etc.) auto-combine on exact
  identity, actives cap at `ProbableEquivalent` so ICs are never silently
  merged. `part_aliases` and `equivalence_decisions` give matching memory
  across repeat imports and past user judgments.
- **Commands** (`apps/desktop/src-tauri/src/commands.rs`): the full domain
  surface (37 commands) is exposed as thin typed wrappers over `Database`,
  generating `apps/desktop/src/bindings.gen.ts` via `tauri-specta`;
  `CommandError { code, message }` (variant name, `Display` text) is the
  only shape that crosses the IPC boundary, and a drift-detecting test keeps
  the generated bindings and the committed file in sync.
