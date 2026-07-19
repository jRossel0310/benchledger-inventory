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
  renders read-only state (inventory table + search, part detail, bins,
  projects — hash-routed, Phase 6). No write paths exist. See the
  "Public snapshot + publishing" section below and `docs/publishing.md`.
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
- **Import matching, review, and atomic commit** (`inventory-db::
  {import_match,import_review,import_commit}`, Phase 5b — the "Match ->
  Review -> Confirm" half of spec §10): `match_import`/`build_import_review`
  are entirely read-only — matching delegates to the existing 7-level
  `find_matches` (no matching logic duplicated) and the review layer folds
  each part line's top match into a default `ProposedAction` plus the
  SHIPPED (never ordered) receive quantity. **Inventory is untouched until
  Confirm.** `commit_import` is the pipeline's one mutation and reuses Phase
  4's atomic-group machinery rather than inventing a new one: it opens ONE
  `rusqlite` transaction, creates any needed parts/variants/listings through
  new in-tx helpers (`create_part_in_tx`/`add_variant_in_tx`/
  `add_supplier_listing_in_tx`, extracted from the existing public
  `create_part`/`add_variant`/`add_supplier_listing` so their behavior is
  byte-identical), collects every line's shipped-quantity `Receive` op and
  applies them all via `build_group_in_tx` (the same primitive
  `build_from_bom` uses), writes `price_history`, and records matching
  decisions as `part_aliases` — all inside that one transaction, so any
  failure rolls back the entire commit and the import stays `parsed`.
  `reverse_import` mirrors it: `reverse_group_in_tx` (extracted from
  `reverse_group` the same way) plus the `imports.status='reversed'` flip run
  together in one transaction, the same build-from-BOM atomicity pattern.
  The commands layer (`apps/desktop/src-tauri/src/commands.rs`) is thin
  wrappers over these — `parse_and_store_import`, `get_import_review`,
  `list_imports`/`list_import_lines`, `commit_import`, `reverse_import` — and
  `src/hooks/imports.ts` follows the Phase 3/4 TanStack Query hook pattern,
  with `commit`/`reverse` invalidating the same broad ledger surface
  `useReverseGroup` does (per-part stock/transactions, search, dashboard,
  recent activity, history) plus the import-specific keys. See
  `docs/imports.md` for the full Match -> Review -> Confirm flow.
- **Enrichment** (`inventory-enrich` + `inventory-db::enrichment`, Phase
  5c — spec §11 enrichment / §5 provenance / §16 redaction, ADR #2/#3): a new
  `inventory-enrich` crate holds the domain model and provider chain,
  entirely independent of the database — pure data in, `Enrichment` out, no
  `rusqlite`, no network beyond the one provider that needs it. The
  `EnrichmentProvider` trait (`fn enrich(&self, input: &EnrichInput) ->
  Result<Option<Enrichment>, EnrichError>`) is implemented by
  `DescriptionParser` (always available, fully offline — parses a
  DigiKey-style catalog description like `"RES 10K OHM 1% 1/4W 0603"` into
  category/package/identity-attribute candidates via `inventory-core`'s unit
  engine and package normalizer, every candidate `source = inferred` and
  confidence < 1) and `DigiKeyClient` (the DigiKey Product Information V4
  API — OAuth2 client-credentials, sandbox/production toggle, on-disk
  response cache). `run_chain` runs an ordered list of providers
  (`[&digikey, &description]`, highest-priority first) against one
  `EnrichInput` and merges candidates first-seen-key-wins — DigiKey's value
  for a key beats the description parser's guess for the same key; a
  provider that errors is skipped and logged as a chain note, never aborts
  the rest of the chain; a provider with nothing to add returns `Ok(None)`,
  which is normal, not a failure (an unconfigured DigiKey silently
  contributes nothing rather than erroring the chain).

  `inventory-db::enrichment` (Task 5) is the compare-and-apply layer:
  `Database::enrich_part_preview(part_id, cache_dir)` builds an `EnrichInput`
  from the part's preferred variant + description + category, runs the
  chain, and diffs each resulting candidate against the part's CURRENT value
  and that field's recorded `field_provenance` source (migration 0009) —
  writing nothing. Each diff is a `FieldDiff{key, current, proposed, source,
  current_source, requires_review}`; `requires_review` is set when the
  field's current source is `manual` (a human typed it in deliberately) OR
  the candidate itself is `inferred` and the field already has a value (a
  low-confidence guess must never silently replace something already there,
  confirmed or not). Nothing is ever auto-applied — the caller (eventually
  a UI diff screen, 5d) reviews the diff and hands back only the approved
  keys. `Database::apply_enrichment(part_id, applied)` writes every approved
  field in ONE transaction — dispatching on the same `field_key` scheme the
  candidates use (`variant.*` on the preferred manufacturer variant,
  `attr.*` through the same `set_attribute_in_tx` validation path a
  user-typed value goes through, `description`/`category` on `parts`) —
  upserts `field_provenance` for each, and updates `parts.metadata_complete`
  (monotonically). All-or-nothing: any failure rolls the whole apply back.

  Secrets (`inventory-core::secrets`, Task 1) are the single read/write path
  to the DigiKey Client ID/Secret in the OS credential store (`keyring`,
  Windows Credential Manager) — never SQLite, `settings`, logs, or a
  fixture; the OAuth access token itself lives only in an in-memory
  `RefCell` on `DigiKeyClient`, refreshed on expiry, never written to disk.
  The commands layer (`apps/desktop/src-tauri/src/commands.rs`, Task 6)
  exposes `enrich_part_preview`, `apply_enrichment`, `get_digikey_status`
  (a `bool` + the environment string — never the secret), and
  `set_digikey_environment` (the sandbox/production `settings` toggle);
  `src/hooks/enrichment.ts` follows the Phase 3/4/5b TanStack Query pattern,
  with `useEnrichmentPreview` deliberately lazy (`enabled` defaults `false`)
  since a preview does real network I/O. See `docs/enrichment.md` for the
  full pipeline, DigiKey app setup, and the cache.
- **Public snapshot + publishing** (`inventory-sync`, Phase 6 — spec
  §12/§13's public half): the `inventory-sync` crate owns everything between
  the local DB and the public GitHub repo.
  - **Snapshot builder** (`src/snapshot.rs`): `build_snapshot` reads
    non-archived parts/bins/projects through the existing public `Database`
    methods and assembles a serde struct tree that is deliberately narrower
    than the schema — private notes, prices, imports, provenance, credentials,
    and archived records have no field to land in (the denylist +
    planted-value test in `tests/snapshot.rs` is the backstop, not the only
    safeguard). `to_canonical_json` is byte-stable (sorted collections,
    2-space indent, LF, one trailing newline); `content_digest` is the SHA-256
    of the form *without* `published_at`, so an unchanged inventory has an
    unchanged digest.
  - **GitHub client** (`src/github.rs`): a `GitHubApi` trait (`get_file` /
    `put_file` against a `RepoRef`) with a `ReqwestGitHub` Contents-API
    implementation (Bearer token held in a no-Debug holder, 404-on-GET folded
    to `Ok(None)`, typed `GitHubError` whose Display strings are fixed
    classifications — never a response body) and an in-memory `MockGitHub`
    for hermetic tests, mirroring the DigiKey-client pattern. The token lives
    only in Windows Credential Manager
    (`inventory-core::secrets`, entry `ElectronicsInventory-GitHub`).
  - **Publish orchestration** (`src/publish.rs`): `publish_snapshot` = build →
    digest → compare `app_state.last_published_digest` → `Unchanged` (zero
    network calls) or: set the `pending_publish` marker *pre-upload* (kill-safe),
    render with a fresh `published_at`, GET the remote sha, PUT, record
    digest + `last_published_at`, clear the marker. Config comes from
    `settings` (`publish_owner`/`publish_repo`/`publish_branch`/`publish_path`/
    `publish_vercel_url`); runtime state from migration 0010's `app_state`.
    Commands (`get_publish_status`, `set_publish_config`, `set_github_token`
    (write-only), `test_github_connection` (fixed strings), `publish_now`,
    `retry_pending_publish`) + `hooks/publish.ts` follow the established
    thin-wrapper/TanStack pattern.
  - **Close flow** (`src-tauri/src/close_flow.rs` + `ClosePublishDialog.tsx`):
    every window close is intercepted and publishes first — success/unchanged
    exits, failure or a 20s timeout offers Retry / Close anyway, a pending
    marker + quiet startup retry make every path lossless, and a 30s
    wedged-frontend grace force-exits a webview that can't run the dialog.
    See `docs/publishing.md` for the full flow, setup, and troubleshooting.
