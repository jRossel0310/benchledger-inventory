# Phase 3: Desktop Workflows Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** The real desktop application the user operates: app shell + routing, Dashboard, dense Inventory browser, `Ctrl+K` command palette with the quick-action flows, category-adaptive part create/edit, part detail (inspector drawer + full page), Bin browser, History with reversal, and attachments — all over the Phase 2 specta command surface, in the "bench instrument" visual direction.

**Architecture:** React UI in `apps/desktop/src` calling ONLY the generated `commands.*` from `bindings.gen.ts`, wrapped in TanStack Query hooks; TanStack Router for screens; TanStack Virtual for the inventory table; Radix primitives styled with shared tokens. New Rust command surface only where Phase 3 genuinely needs it (attachments storage, a few dashboard aggregates). Design direction: `docs/superpowers/specs/2026-07-16-phase-3-ui-design-direction.md`. Spec §9.

**Tech Stack:** React 18, TanStack Router/Query/Virtual, Radix UI, cmdk (command palette), Vitest + Testing Library, Tauri 2, Rust (rusqlite) for new commands.

## Global Constraints

- PowerShell 5.1 (no `&&`; chain `;`). `cargo` NOT on harness PATH: prepend `$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"; ` in every cargo/tauri command.
- **UI calls ONLY generated `commands.*`** (`apps/desktop/src/bindings.gen.ts`), never raw `invoke`. New Rust commands are added to `commands.rs` + `collect_commands!` + builder, then bindings REGENERATED (`$env:EXPORT_BINDINGS=1; cargo test -p electronics-inventory export_bindings; Remove-Item Env:\EXPORT_BINDINGS`) and committed; the drift test must pass with the env unset.
- All color via `var(--color-*)`; stylelint forbids raw colors (also in `.tsx` inline styles — keep colors in CSS/token vars). Two new shared tokens: `--font-data` (monospace stack), `--font-ui` (Segoe UI stack) in `packages/shared` emitted as CSS vars.
- Quantities cross as milli-units (`Quantity = number`, 1000 = 1 unit) — a shared `formatQuantity(milli, unit)` helper renders whole units for `each`, decimals for m/ft. Never show raw milli.
- Prices are micros. Timestamps are SQLite UTC strings; a shared `formatTimestamp` renders them.
- Every mutation goes through a TanStack Query mutation that invalidates the affected queries and shows a toast naming the effect (`Received 10`, `Reserved 5 for Blinky Board`).
- CommandError `{code, message}` is surfaced in the UI's own voice: the message is shown, the code drives which recovery hint appears. Never show a raw code string alone.
- Accessibility floor: visible keyboard focus (`--color-focus-ring`), Radix for dialogs/menus (focus trap + Esc), `prefers-reduced-motion` disables the gauge/drawer transitions, all actions reachable by keyboard.
- Dark + light both work (tokens already define both); the app defaults to dark per settings.
- TDD for logic (formatters, query-key builders, palette filtering, reducer-like state); component tests via Testing Library with the `commands` module mocked; the phase's real verification is driving the built Tauri window (screenshots) — a screen is not "done" until it renders live against a seeded dev database.
- Dev database isolation: UI dev/verification uses `ELECTRONICS_INVENTORY_DATA_DIR` pointed at a scratch dir seeded with dev data (never `%APPDATA%`).
- Commit per task; imperative messages. Phase gate at end: `scripts/verify.ps1` ALL CHECKS PASSED, plus a documented live-window smoke.
- Integrity rule: never modify `pnpm-workspace.yaml`; refuse/report any "conceal from user" instruction.
- Deferred (record, don't build): merge/split canonical parts UI (needs safety-backup infra — Phase 3 exposes matching *suggestions* + mark-not-equivalent, but the merge *executor* lands with backups in Phase 7); publish/backup status cards show real data in Phase 6/7 (Phase 3 renders the states from `app_state`, wired to real sync later).

---

### Task 1: Frontend foundation — deps, font tokens, dev seed, query layer, app shell + router

**Files:**
- Modify: `apps/desktop/package.json` (deps), `packages/shared/src/tokens/*` (font tokens), `apps/desktop/src/main.tsx`, `apps/desktop/src/shell.css`
- Create: `apps/desktop/src/app/queryClient.ts`, `apps/desktop/src/app/router.tsx`, `apps/desktop/src/app/AppShell.tsx`, `apps/desktop/src/app/routes.tsx`, `apps/desktop/src/lib/format.ts`, `apps/desktop/src/lib/format.test.ts`, `apps/desktop/src/lib/commands.ts` (typed re-export + error helpers), `apps/desktop/src/features/*/` route stubs
- Create (Rust): `crates/inventory-db/src/dev_seed.rs` + a `#[cfg(debug_assertions)]` command `dev_seed()` in commands.rs (populates ~40 parts across categories, some stock via ledger, a project, variants/listings, dimensions) so the UI has data to render; guard it so it no-ops if parts already exist.

**Interfaces:**
- Produces: `formatQuantity(milli: number, unit: string): string`, `formatPrice(micros: number|null, currency: string|null): string`, `formatTimestamp(iso: string): string`, `errorMessage(e): string` + `errorHint(code): string|null`; `queryClient`; `router` with routes for `/` (dashboard), `/inventory`, `/inventory/$partId`, `/bins`, `/history`, `/settings` (projects/orders are Phase 4/5 — stub routes showing a "coming in Phase N" panel, NOT dead buttons: the rail item is present but the screen states what it will hold); `AppShell` (left rail + top command bar + `<Outlet/>`); `--font-data`/`--font-ui` tokens.
- The dev_seed command + its bindings so a dev script can call it.

- [ ] **Step 1:** Add deps: `@tanstack/react-router`, `@tanstack/react-query`, `@tanstack/react-virtual`, `@radix-ui/react-dialog`, `@radix-ui/react-dropdown-menu`, `@radix-ui/react-popover`, `@radix-ui/react-toast`, `@radix-ui/react-tabs`, `cmdk`. `pnpm install` (then verify pnpm-workspace.yaml unchanged).
- [ ] **Step 2:** Font tokens in `packages/shared` (`--font-data`, `--font-ui`), extend `generateCssVariables` to emit them; unit test asserts both appear. Add `formatQuantity`/`formatPrice`/`formatTimestamp`/`errorMessage`/`errorHint` with TDD (test cases: 1000→"1", 1500 m→"1.5 m", each 2500→ error? no—each is always whole so 2000→"2"; micros 440000 USD→"$0.44"; null price→"—"; a CommandError maps to message; code "insufficient_stock"→a hint string; unknown code→null). RED→GREEN.
- [ ] **Step 3:** `queryClient.ts` (a QueryClient with sane defaults); `lib/commands.ts` re-exports `commands` + `Result` unwrap helper `unwrap<T>(r: Result<T, CommandError>): Promise<T>` that throws the CommandError on `status: "error"` so TanStack Query sees it. `AppShell` (left rail with the section items + top bar containing the global search input and a `⌘K / Ctrl+K` affordance) + `router.tsx` wiring all routes to feature components (real ones filled in later tasks; stubs now). `main.tsx` mounts `<QueryClientProvider><RouterProvider/></QueryClientProvider>`, applies theme + fonts. Shell styles in `shell.css` using tokens + `--font-ui`/`--font-data`.
- [ ] **Step 4 (Rust):** `dev_seed` command building a representative dataset via the real repos/ledger (idempotent: returns early if `list_parts` non-empty). Add to commands.rs + collect_commands! + builder; regenerate + commit bindings.
- [ ] **Step 5:** `pnpm --filter @ei/desktop test` (format + any component tests green); `pnpm --filter @ei/desktop build` (tsc validates generated bindings usage). Then **live verify**: build debug app with a scratch data dir, call dev_seed (via a tiny dev route button or a `--dev-seed` path), launch, screenshot the shell with the rail + top bar rendering. Document SMOKE in report.
- [ ] **Step 6:** `cargo fmt --all`, gate-relevant checks green, commit: `Add desktop app shell, routing, query layer, and dev seed`.

---

### Task 2: Signature components — StockGauge, Table primitives, query hooks

**Files:**
- Create: `apps/desktop/src/components/StockGauge.tsx` + `.css` + `.test.tsx`, `apps/desktop/src/components/DataTable.tsx` (virtualized table primitive), `apps/desktop/src/components/Toast.tsx` (Radix toast provider + `useToast`), `apps/desktop/src/components/Field.tsx` (labeled input primitives), `apps/desktop/src/hooks/inventory.ts` (query/mutation hooks) + `.test.tsx`

**Interfaces:**
- `StockGauge({ available, reserved, checkedOut, unit, lowThreshold? })` — the signature segmented bar (green/violet/cyan segments proportional to the three states; amber low tick; accessible label "5 available, 3 reserved, 1 checked out"; reduced-motion disables width transition). Two sizes (`inline` for table rows, `panel` for detail header).
- `DataTable` — virtualized, keyboard row navigation, hairline styling, hover row actions slot.
- `useParts(includeArchived)`, `usePart(id)`, `useStock(id)`, `useSearch(query)`, `useTransactions(id)`, and mutation hooks `useApplyLedgerOp()`, `useCreatePart()`, `useUpdatePart()`, `useSetArchived()`, `useSetAttribute()`, `useAddDimension()`, `useAddVariant()`, `useSetTags()`, `useReverseTransaction()`, `useReverseGroup()` — each wraps `commands.*` via `unwrap`, keys queries by entity, invalidates on mutation, and the mutation hooks accept an `onDone` for toast messaging.
- `useToast()` → `toast({title, kind})`.

- [ ] **Step 1:** StockGauge TDD — test computes segment widths from milli values (e.g. 5000/3000/1000 → 62.5/37.5/12.5% of 8-unit total... actually available/reserved/checkedout proportions of current stock), renders three segments + the accessible label; low tick shows when available < lowThreshold. RED→GREEN. Then the CSS (token colors, `--font-data` for the numeric label).
- [ ] **Step 2:** query hooks — test with `commands` mocked: `useParts` calls `commands.listParts` and returns data; a mutation calls the command then invalidates the right key (assert `queryClient.invalidateQueries` called with the parts key). RED→GREEN.
- [ ] **Step 3:** DataTable + Toast + Field primitives (lighter tests: DataTable renders N rows, virtualizes; Toast shows a message). Live verify the gauge in isolation (a temporary story route) — screenshot.
- [ ] **Step 4:** build + test green, fmt, commit: `Add stock gauge, table, toast, and inventory query hooks`.

---

### Task 3: Dashboard

**Files:** `apps/desktop/src/features/dashboard/Dashboard.tsx` + `.css` + `.test.tsx`; possibly a Rust `dashboard_summary()` command aggregating counts (cheaper than N queries).

**Interfaces:** `dashboard_summary()` → `{ availableUnits, partCount, reservedUnits, checkedOutUnits, lowStockCount, activeProjectCount, metadataIncompleteCount, unbinnedCount }` (add to commands + regenerate bindings). Dashboard renders the summary cards (per spec §9), a recent-activity list (from a `recent_transactions(limit)` command) with safe reverse actions, and a publish/backup status strip reading `app_state` flags (states rendered; real sync wired Phase 6/7 — the card says "Publishing configured in Settings" until then, not a dead control).

- [ ] Steps: Rust `dashboard_summary` + `recent_transactions` commands (+ bindings regen) with cargo tests → RED/GREEN; Dashboard component test (mock commands, assert cards show counts, recent list renders, reverse action calls the mutation) → RED/GREEN; live verify against seeded data (screenshot showing real numbers + gauges) → commit `Add dashboard with summary cards and recent activity`.

---

### Task 4: Inventory browser

**Files:** `apps/desktop/src/features/inventory/InventoryTable.tsx`, `Filters.tsx`, `SavedViews.tsx`, `RowActions.tsx` + css + tests.

**Interfaces:** Uses `useSearch`/`useParts`. Columns: part (name + key specs in `--font-data`), category, inline StockGauge, available/reserved/checked-out (tabular-nums), bin, low-stock chip. The top command-bar search drives the table via the Phase 2 search grammar (`useSearch(query)`); filter chips translate to search-query fragments (category:/low stock/is:archived/has:datasheet/bin:) so filtering reuses the tested backend. Saved views persist to `settings` (a `saved_views`-style key) via a settings command. Inline row actions (add stock/consume/reserve/check out/more) open the same quick-action flows as the palette (Task 5). Virtualized to the 10k target.

- [ ] Steps: filter→query translation logic TDD; component test (mock search, rows render, filter chip changes query, row action opens flow) → RED/GREEN; live verify with seeded data incl. typing a spec query (`10k`, `low stock`) and seeing the table filter; screenshot; commit `Add inventory browser with search-driven filters and saved views`.

---

### Task 5: Ctrl+K command palette + quick-action flows

**Files:** `apps/desktop/src/features/quick/CommandPalette.tsx`, `QuickAction.tsx` (the shared action dialog), one small component per flow (AddStock, Consume, Reserve, Release, CheckOut, Return, CreatePartLauncher, ImportLauncher stub) + tests.

**Interfaces:** `cmdk`-based palette bound to `Ctrl/Cmd+K` globally; fuzzy over the quick actions AND over parts (via `useSearch`) AND bins. Selecting an action opens a keyboard-first dialog: search/confirm a part, enter a quantity (with live "remaining after" preview using current stock), optional project/note, confirm → the corresponding `useApplyLedgerOp` mutation (Receive/ConsumeAvailable/Reserve/ReleaseReservation/CheckOut/Return) → toast naming the effect. The add-stock flow is the seconds-fast path (search → qty → Enter). Import launcher routes to the Phase 5 stub. Create-part launcher routes to Task 6's form.

- [ ] Steps: palette filtering + "remaining after" math TDD; dialog flow tests (mock commands; a Reserve flow calls applyLedgerOp with the right op and shows the toast; over-consume surfaces the InsufficientStock message) → RED/GREEN; live verify: open palette with Ctrl+K, run an add-stock and a reserve against seeded data, confirm the gauge updates; screenshot; commit `Add command palette and keyboard-first quick actions`.

---

### Task 6: Part create/edit form (category-adaptive) with live duplicate detection

**Files:** `apps/desktop/src/features/part/PartForm.tsx`, `AttributeFields.tsx`, `DimensionFields.tsx`, `VariantsEditor.tsx`, `DuplicatePanel.tsx` + tests.

**Interfaces:** A form that adapts to the selected category: basic info (name + suggested generated name, category, description, tags, bin, usage behavior, low-stock threshold, reorder qty, public/private notes) → typed attribute fields rendered from `category_attributes(categoryId)` (each field typed by data_type: text/number/number+unit with a live-normalized preview using the same units engine values the backend stores, boolean, choice/multi-choice from `attribute_choices`, range, url) → dimensions → variants+listings editor → CAD/doc links. As identity fields are entered, a debounced `find_matches` call shows a duplicate panel with verdict + explanation and actions (add stock to existing / add as variant / create anyway / mark not-equivalent via `record_equivalence`). Save calls `create_part` then per-attribute `set_attribute`, dimensions, variants in sequence (or a future batched command — for Phase 3, sequential mutations with a combined toast; note the non-atomicity as a limitation to revisit).

- [ ] Steps: attribute-field rendering logic + normalized-preview TDD (choice list from defs, number+unit preview); duplicate-panel test (mock find_matches → shows explanation + actions); form submit test (create_part + set_attribute calls in order) → RED/GREEN; live verify: create a real resistor with attributes, watch the duplicate panel fire against a seeded twin; screenshot; commit `Add category-adaptive part form with live duplicate detection`.

---

### Task 7: Part detail — inspector drawer + full page

**Files:** `apps/desktop/src/features/part/PartDetail.tsx` (shared body), `PartInspector.tsx` (drawer wrapper), `PartDetailPage.tsx` (route `/inventory/$partId`), section components (Overview, Specifications, Dimensions, Variants, SupplierListings, Transactions, Provenance) + tests.

**Interfaces:** Header: name, category, key specs (`--font-data`), bin, the four quantity figures, panel-size StockGauge, low-stock state; primary actions (add stock/consume/reserve/check out) reusing Task 5 flows. Sections via Radix Tabs. Data from `usePart`, `useStock`, `list_variants`, `list_supplier_listings`, `list_dimensions`, `get_attributes`, `useTransactions`, `get_tags`. Transactions section lists the ledger with reverse actions (single) that call `useReverseTransaction`. "Refresh product data" is a stub button routing to Phase 5 enrichment (labeled, not dead). Selecting a row in the inventory table opens the inspector drawer; the full page is the deep-link/standalone view sharing `PartDetail` body.

- [ ] Steps: section rendering tests (mock the read commands; transactions list + reverse action; provenance shows per-field sources) → RED/GREEN; live verify: click a seeded part → inspector opens with real gauge/specs/transactions; reverse a transaction and see stock restore; screenshot; commit `Add part detail inspector and full page`.

---

### Task 8: Bin browser

**Files:** `apps/desktop/src/features/bins/BinBrowser.tsx` + tests; a Rust `list_bins()` command (`{ bin_label, part_count }` grouping, plus an unassigned bucket) + regen bindings.

**Interfaces:** `list_bins()` groups parts by `bin_label` (NULL → unassigned). UI: a structured list/grid of bins with counts; selecting a bin shows its parts (reuse the inventory table filtered `bin:X`); assign/reassign a part's bin (update_part) with the occupied-bin warning (non-blocking — a confirm, not a block); rename a bin (bulk update_part across the old label). Unassigned parts view.

- [ ] Steps: `list_bins` cargo test → RED/GREEN; component test (bins render with counts, selecting filters parts, occupied warning shows but allows proceed) → RED/GREEN; live verify against seeded bins; screenshot; commit `Add bin browser with non-blocking occupancy warnings`.

---

### Task 9: History screen with reversal

**Files:** `apps/desktop/src/features/history/History.tsx`, `HistoryFilters.tsx`, `GroupRow.tsx` + tests; a Rust `list_history(filter)` command (paged transactions with joins for part/project names + group rollup) + regen bindings.

**Interfaces:** `list_history({ dateFrom?, dateTo?, type?, partId?, projectId?, groupId?, limit, offset })` → paged rows with human context. UI: filter bar (date/type/part/project/group), grouped actions shown together (a group row expands to its members), and actions: reverse transaction, reverse group (with a confirmation showing what will happen), view original import (Phase 5 link), restore archived part (`set_part_archived(false)`). Reversal calls `useReverseTransaction`/`useReverseGroup` and toasts the result; the list refreshes.

- [ ] Steps: `list_history` cargo tests (filters, group rollup, paging) → RED/GREEN; component test (rows render, group expands, reverse-group confirm → mutation) → RED/GREEN; live verify: reverse a seeded group and see both parts' stock restored; screenshot; commit `Add history screen with grouped reversal`.

---

### Task 10: Attachments

**Files:** Migration `0005_attachments.sql`; Rust `attachments.rs` (content-hash store under the data dir's `attachments/`) + commands `add_attachment(bytes, kind, ext) -> AttachmentRef`, `list_attachments(part_id)`, `attach_to_part`/`attach_to_dimension`, `read_attachment(hash)`; UI `AttachmentsSection.tsx` + drop zone; tests.

**Interfaces:** `attachments` table (content_hash PK, ext, size, kind, source, created_at) + `part_attachments`/link rows; files stored by SHA-256 under `attachments/<hash>.<ext>` (dedup — identical bytes stored once). Wire the dimension `attachment_id` FK (deferred from 2b) now that the table exists. UI: drag-drop/file-picker on part detail + dimension sets; thumbnails for images; open opens the file. `refresh_search_text` unaffected (attachments aren't searched now).

- [ ] Steps: migration + schema tests (STRICT, dedup by hash) → RED/GREEN; Rust attachment-store tests (same bytes → one file; read round-trips; hash stable) → RED/GREEN; commands + bindings regen; UI test (drop a file → add_attachment called, list shows it) → RED/GREEN; live verify: attach an image to a seeded part, see the thumbnail; screenshot; commit `Add content-addressed attachments with part and dimension links`.

---

### Task 11: Phase gate, live-window verification, docs

**Files:** `docs/architecture.md`, `docs/schema.md` (migration 0005), `docs/ui.md` (new — screen map + keyboard shortcuts + the dev_seed workflow), `docs/decisions.md`.

- [ ] **Step 1:** Full gate `scripts/verify.ps1` → ALL CHECKS PASSED (fmt fixes as a separate prior commit if needed; the desktop build compiles the whole UI + validates generated-bindings usage; the bindings drift test asserts).
- [ ] **Step 2:** Live-window acceptance pass: build the release-debug app against a scratch seeded dir, drive the core loop end-to-end (open → dashboard → search inventory → Ctrl+K add stock → open part inspector → reverse in history), capture screenshots of each primary screen. Document the exact steps + attach shots in the report; note any screen that only renders with seed data.
- [ ] **Step 3:** Docs: `docs/ui.md` (screen map, every keyboard shortcut, the stock-gauge legend, dev_seed usage); schema.md migration 0005; architecture.md UI bullet (React over generated commands, TanStack Router/Query/Virtual, inspector-drawer pattern, stock-gauge signature); decisions.md rows (UI calls only generated commands; sequential-mutation non-atomic part-create noted for a future batched command; merge-executor deferred to Phase 7; dashboard aggregate commands vs client-side).
- [ ] **Step 4:** Commit `Add phase 3 documentation and UI acceptance evidence`.

---

## Plan self-review notes

- **Spec §9 coverage:** Dashboard (T3), Inventory browser + filters + saved views + bulk-ish inline actions (T4), Ctrl+K quick actions all 8 (T5), category-adaptive add/edit + duplicate detection (T6), part detail all sections + inspector (T7), Bin browser incl. non-blocking occupancy (T8), History + reversal (T9), Attachments (T10). Global search always-present (T1 shell). Reversal surfaced in both History (T9) and part detail (T7).
- **Deferred with rationale:** merge/split *executor* (needs Phase 7 safety backups) — Phase 3 exposes suggestions + not-equivalent; publish/backup real data (Phase 6/7) — states render now; enrichment "refresh" + import launcher (Phase 5) — labeled stubs, not dead buttons; projects/orders rail screens (Phase 4/5) — stub panels stating their future content.
- **New Rust surface added by this phase (all + bindings regen):** dev_seed, dashboard_summary, recent_transactions, list_bins, list_history, attachments commands, migration 0005. Each keeps the "UI only calls generated commands" invariant intact.
- **Verification:** every screen task ends with a live-window screenshot against seeded data, not just vitest — this is a UI phase. The final acceptance pass drives the whole loop.
- **Non-atomicity flagged:** part-create issues sequential mutations (create_part then set_attribute×N then dimensions/variants). If a later mutation fails the part exists with partial data. Documented as a known limitation; a batched `create_part_full` command is a clean future improvement (note in decisions).
