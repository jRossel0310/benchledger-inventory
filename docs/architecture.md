# Architecture

See the spec for full detail. Summary of what exists after Phase 3:

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
  surface (58 commands, grown from Phase 2's 37 by Phase 3's dashboard
  aggregates, `list_bins`, `list_history`, attachment commands, and the
  debug-only `dev_seed`) is exposed as thin typed wrappers over `Database`,
  generating `apps/desktop/src/bindings.gen.ts` via `tauri-specta`;
  `CommandError { code, message }` (variant name, `Display` text) is the
  only shape that crosses the IPC boundary, and a drift-detecting test keeps
  the generated bindings and the committed file in sync.
- **Phase 3 desktop UI** (`apps/desktop/src`): the "bench instrument" workflow
  surface — React 18 over the generated typed `commands.*` **only** (never raw
  `invoke`), wrapped in **TanStack Query** hooks (`src/hooks/inventory.ts`);
  every mutation invalidates the affected query keys and toasts its effect.
  **TanStack Router** drives the screens (Dashboard, Inventory, Bins, History,
  Part detail, part form, Settings, Projects/Orders stubs); **TanStack Virtual**
  virtualizes the inventory table to the 10k-part target. Radix primitives
  (dialog/tabs/popover/toast) styled entirely with the shared tokens carry
  accessibility. The signature **stock-state gauge** (`StockGauge`,
  `{available, reserved, checkedOut, unit, lowThreshold?, size}` → segmented
  green/violet/cyan bar + amber low tick) is reused inline in the table, as a
  labeled panel in the part-detail header, and aggregated on the Dashboard. Part
  detail follows the **inspector-drawer pattern**: one shared `PartDetail` body
  renders both a right-hand slide-over (the fast path from an inventory row) and
  the full-page `/inventory/$partId` route. `dev_seed` (debug-only) populates a
  scratch dev DB. Every screen was **live-verified by driving the real Tauri
  WebView2** over CDP (`--remote-debugging-port`, Playwright `connectOverCDP`)
  with `PrintWindow`/screenshots against seeded data, not only vitest. See
  `docs/ui.md`.
- **Projects & BOMs** (`inventory-db::{projects,bom,build}`, Phase 4): a real
  project lifecycle (planned/active/completed/archived) and a bill of
  materials (`bom_items`/`bom_substitutes`, migration 0006) sit entirely over
  the Phase 2a ledger machinery — reserve-BOM, release-BOM, and
  build-from-BOM are each just a computed `Vec<LedgerOp>` handed to the
  existing `apply_group`/`reverse_group`, the same atomic all-or-nothing
  transaction-group path every other ledger mutation (receive, consume,
  transfer) already uses. No new transaction path was introduced. Per-line
  reserved/consumed are derived from the ledger on every read rather than
  stored, mirroring how `validate.rs` reconciles `part_stock`. The desktop
  **Projects** feature (list, detail, BOM editor, reserve/build-review) reads
  and writes exclusively through the generated typed `commands.*` +
  `src/hooks/projects.ts`, the identical pattern every other Phase 3 screen
  follows — see `docs/ui.md`.
- **Import parsing** (`inventory-import`, Phase 5a — the "Upload → Extract"
  half of spec §10): pure, testable parsing behind an `InvoiceParser`
  trait (`fn parse(&self, bytes: &[u8]) -> Result<ParsedInvoice, ImportError>`),
  with no `rusqlite`, DB access, or network in the crate at all — DigiKey
  CSV/XLSX/PDF parsers turn supplier order files into a `ParsedInvoice`
  (order metadata + `ParsedLine`s, money as exact micros, quantities as
  milli, every field's original extracted text preserved). PDF table
  reconstruction goes through a `PdfTextSource` abstraction so DigiKey's
  row/column logic is unit-tested against committed positioned-token JSON
  fixtures rather than a live `pdfium-render` extraction (feature-gated,
  off by default). Persistence (`inventory-db::imports::store_import`,
  migration 0007) preserves the original file bytes verbatim in the
  existing content-addressed attachments store plus each line's raw JSON,
  and detects likely duplicate re-imports by attachment hash or order/
  invoice/shipment number — but makes NO inventory mutation: no
  `part_stock`/ledger write happens until 5b's matching + atomic commit.
  See `docs/parsers.md` for the full parsing architecture and
  `docs/known-limitations.md` for what's deferred (OCR, live-pdfium
  validation).
