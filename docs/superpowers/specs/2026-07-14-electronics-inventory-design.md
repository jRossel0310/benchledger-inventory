# Electronics Inventory — Design Specification

- **Date:** 2026-07-14
- **Status:** Approved by user (enrichment approach, core architecture, and overall design confirmed in brainstorming session)
- **Authoritative requirements:** The user's full requirements document (delivered with the initial build request) governs product behavior. This spec condenses those requirements and records the engineering decisions layered on top. Where this spec is silent, the original requirements text wins.

---

## 1. Overview

A production-quality, single-user personal electronics inventory system:

- A **native Windows desktop application** (Tauri 2) — the only place data is edited.
- A **public read-only website** (Vite React SPA on Vercel) rendering a published snapshot.
- A **local SQLite database** as the sole source of truth, stored in `%APPDATA%\ElectronicsInventory\`.
- **GitHub public publishing** of a deterministic snapshot into this repository (which also hosts the web app), and a **separate private GitHub backup repository** holding a complete restorable export.
- **DigiKey invoice importing** (PDF/CSV/XLSX) with matching, automatic metadata **enrichment**, review, atomic commit, and reversal.
- **Projects and BOMs**, a **transaction ledger**, **history and reversal**, **local backups**, **restore**, **recovery mode**, a **first-run wizard**, **tests**, **documentation**, and **PowerShell launchers**.

Single user, single physical storage system, free-form bin labels (multiple parts may share a bin — warn, never block).

## 2. Goals and non-goals

### Goals
Everything in the Definition of Done (§23). No placeholder screens, no dead buttons, no mock integrations.

### Explicit exclusions (do not build)
Multi-user accounts, roles/permissions, teams, multiple physical locations, warehouse management, shopping lists, purchase orders, mark-as-ordered workflow, supplier cart integration, accounting, tax management, native mobile app, cloud-hosted writable database, remote editing, full KiCad plugin integration, estimated/approximate quantities. No placeholder screens for excluded features.

## 3. Architecture

### 3.1 Decision: Rust core (user-approved)

All domain logic — ledger, invariants, unit normalization, duplicate matching, import pipeline, enrichment orchestration, snapshot export, backup/publish — lives in Rust. React is a thin UI invoking typed Tauri commands. The web SPA consumes the published snapshot plus a small shared TS package. Rationale: transactional integrity enforced next to the database, one implementation of money-critical logic, fast `cargo test` coverage. No persistent local HTTP server; direct Tauri commands and native SQLite only.

### 3.2 Repository layout

```
apps/
  desktop/                 Tauri 2 application
    src/                   React + TypeScript + Vite UI
    src-tauri/             Rust binary: command bindings, window lifecycle, recovery mode
  web/                     Read-only Vite React SPA (Vercel)
    public/inventory.snapshot.json    default published snapshot path
packages/
  shared/                  TS: design tokens, snapshot schema types, formatting,
                           search-query parser (web twin), shared query-test fixtures
crates/
  inventory-core/          Domain: parts, categories, attributes, dimensions, units,
                           ledger, invariants, matching, search query model
  inventory-db/            rusqlite integration, numbered migrations, FTS5, repositories
  inventory-import/        Parser trait + DigiKey PDF/CSV/XLSX adapters, OCR fallback
  inventory-sync/          Snapshot export, backup format, GitHub client (trait + real impl),
                           restore, local backups
samples/digikey/           Sanitized fixtures (committed) ; private/ (gitignored originals)
scripts/                   PowerShell launchers + shortcut installer
docs/                      Architecture, schema, parser, backup, GitHub setup, build,
                           troubleshooting, recovery, known limitations
docs/superpowers/specs/    This spec and future specs
```

Cargo workspace at the root; pnpm workspace for `apps/*` and `packages/*`.

### 3.3 Technology choices

| Concern | Choice | Rationale |
|---|---|---|
| Desktop shell | Tauri 2 | Required; native window, no localhost workflow |
| DB access | rusqlite (bundled SQLite), WAL, `foreign_keys=ON` | In-process, transactional, no server |
| Migrations | Numbered SQL files embedded in `inventory-db`, applied in order with schema-version table | Simple, testable, versioned |
| TS bindings | specta + tauri-specta generate TS types/commands from Rust | UI cannot drift from core |
| UI framework | React 18 + TypeScript + Vite | Required |
| Routing/data/tables | TanStack Router, Query, Virtual | Type-safe routes, cache mgmt, virtualized 10k-row tables |
| Component primitives | Radix UI primitives + custom token-styled components | Accessibility without imposed visual style |
| Styling | CSS custom properties generated from `packages/shared` tokens; CSS modules | Central parametric theme, shared desktop/web |
| PDF text extraction | pdfium (via `pdfium-render`, DLL shipped with app) | Positional text → reliable column reconstruction |
| OCR fallback | Windows built-in OCR (`windows` crate, `Windows.Media.Ocr`) on pdfium-rendered bitmaps | No Tesseract bundling; Windows-only app |
| CSV / XLSX | `csv` crate / `calamine` | Standard, robust |
| HTTP | `reqwest` | GitHub + enrichment fetches |
| GitHub access | Thin REST client behind a `GitHubApi` trait | Fully mockable failure matrix in tests |
| Secrets | Windows Credential Manager via `keyring` crate | Native, simpler than Stronghold; per-purpose entries |
| Logging | `tracing` + rolling file appender + redaction layer | Rotating logs, secrets never written |
| Rust tests | cargo test + fixture files; `insta` for snapshot-style assertions where useful | |
| TS tests | Vitest (shared + components), Playwright (web SPA) | |
| Desktop E2E | Scripted scenarios as Rust integration tests through the full command layer; WebDriver (`tauri-driver` + Edge WebDriver) UI smoke tests where tooling permits | Deterministic coverage even if WebDriver flakes |

## 4. Data model

### 4.1 On-disk layout (production data — never inside the repo or build dirs)

```
%APPDATA%\ElectronicsInventory\
  inventory.sqlite         source of truth
  attachments/<hash>.<ext> content-addressed attachment store
  cache/                   enrichment response cache
  logs/                    rotating logs
  pending-sync/            pending publish/backup markers + last-published digests
  local-backups/           timestamped .sqlite copies + retention metadata
  settings.json            non-secret app settings (repos, paths, preferences)
```

Rebuilds, branch switches, deleted build artifacts, or replaced executables never touch this directory. Dev/test runs use a separate data directory (`ELECTRONICS_INVENTORY_DATA_DIR` override) so seed data never contaminates production data.

### 4.2 Schema domains (principal tables)

- **Parts:** `parts` (canonical), `manufacturer_variants`, `supplier_listings`, `part_tags`.
- **Categories/attributes:** `categories`, `attribute_defs`, `category_attributes`, `part_attribute_values` (original text + normalized numeric value + canonical unit), `attribute_choices`.
- **Dimensions:** `dimension_sets`, `dimensions` (name, value, display unit, normalized mm/g, group, source, notes, measured date), attachable photo/drawing.
- **CAD/docs:** `part_cad_links` (kind: kicad_symbol | kicad_footprint | datasheet | step | model3d | cad | pinout | repo | doc; URL or attachment ref; variant-preferred footprint flag). Multiple footprints per part allowed.
- **Stock:** `part_stock` (available/reserved/checked_out aggregates + lifetime received/consumed, all `CHECK (x >= 0)`), `transactions` (append-only ledger), `transaction_groups`.
- **Projects:** `projects`, `bom_items`, `bom_substitutes`, `project_checkouts`.
- **Imports:** `imports`, `import_files` (hash, original bytes preserved in attachments), `import_lines` (raw extracted fields JSON preserved), `price_history`.
- **Matching memory:** `part_aliases` (supplier SKU / MPN → part), `equivalence_decisions` (approved/rejected pairs), `equivalence_families`.
- **Search:** FTS5 virtual table + triggers keeping it in sync.
- **Meta:** `schema_version`, `app_state` (pending flags, last publish digests), `saved_views`, `settings` (authoritative inventory-level preferences — units, currency, thresholds, theme — included in backups). The on-disk `settings.json` holds only machine-local bootstrap config the app needs before the DB opens: data directory, window state, repo configuration.
- **Attachments:** `attachments` (content hash, ext, size, kind, source).

Stable IDs: ULIDs generated in Rust (sortable, stable across exports; deterministic ordering for backups).

### 4.3 Quantities

Fixed-point integers, ×1000 (`quantity_milli INTEGER`). Each part has a `quantity_unit` (`each` default, `m`, `ft`, …). For discrete units (`each`) the domain layer rejects any quantity that is not a whole multiple of 1000; continuous units accept fractions. Display renders whole numbers for `each`. Exact only — no estimates anywhere.

### 4.4 Inventory states and ledger

States: **Available**, **Reserved**, **Checked out** (current stock = sum of the three). **Consumed** is historical only. Transaction types: receive, reserve, release_reservation, check_out, return, consume_available, consume_reserved, consume_checked_out, adjust_up, adjust_down, transfer_reservation, reverse. Each row records part, quantity, from/to state, project, BOM item, group, timestamp, note, related import, reversed-transaction ref.

Every stock change is a ledger transaction — quantity is never directly edited. Multi-part actions (import commit, reserve BOM, build from BOM, reversals of these) run in one SQLite transaction as a **transaction group**: all-or-nothing. Corrections create reversing transactions; history is never deleted.

### 4.5 Invariant enforcement (three layers)

1. SQL `CHECK` constraints on `part_stock` (no negatives) and triggers verifying aggregate updates accompany ledger inserts.
2. Rust domain layer computes every stock delta from the ledger operation — no caller-supplied aggregates.
3. A validator recomputes aggregates from the full ledger and compares: run at startup (quiet), before backup/restore swap, in recovery mode, and in tests.

## 5. Parts model

**Canonical part** (the inventory unit — functionally/mechanically interchangeable): stable ID, display name, suggested generated name, category, description, tags, bin label (free-form), default usage behavior (usually_consumed | usually_checked_out | ask), low-stock threshold, preferred reorder qty (metadata only), preferred variant, public notes, **private/local-only notes**, created/modified timestamps, archived flag, metadata-completeness state, per-field provenance where practical (invoice | digikey | manufacturer | datasheet | inferred | manual | measured).

**Manufacturer variant:** manufacturer, MPN, description, package, datasheet URL, product URL, lifecycle status, preferred flag, notes, optional preferred footprint. All approved variants pool into the canonical part's quantity.

**Supplier listing (per variant):** supplier, supplier SKU, product URL, packaging type, typical order qty, last unit price, last purchase date. SKUs/MPNs drive import matching.

**Categories:** the full built-in taxonomy from the requirements (passives, semiconductors, interconnect/electromech, modules/reusables, mechanical/misc — ~70 templates) ships as seed data flagged `built_in`, each with curated typed attributes (resistor, capacitor, MOSFET, op amp, connector fields per requirements). Users can create/duplicate categories, add custom attributes, choose types/units, mark searchable/filterable/identity-defining, reorder, and hide unused built-in fields. Attribute data types: text, number, number+unit, boolean, single choice, multi choice, range, URL.

**Dimensions** are separate from footprints: overall/body/mounting groups plus arbitrary custom named measurements, each with value, display unit, normalized value, source (manufacturer | datasheet | supplier | measured | estimated), notes, optional date; photo/sketch attachable to a dimension set.

## 6. Units and normalization

A Rust unit engine parses electronics notation (`10k`, `4k7`, `3V3`, `0R`, `1u`, `100n`, `1/4 W`, `0603`, `1608 metric`, unicode µ/Ω variants) into `(normalized SI value, canonical unit)` while retaining the original text for display. Equivalences (`0.1 µF` = `100 nF` = `100000 pF`; `1/4 W` = `0.25 W`; imperial/metric package codes) compare equal. Normalized values feed search, filters, matching, and identity comparison. The web SPA needs only the **query-side** parser (TS twin in `packages/shared`); both parsers are tested against one shared JSON fixture of cases so they cannot drift.

## 7. Duplicate matching and equivalence

Verdicts: exact match / probable equivalent / similar but materially different / no match — always with a human-readable explanation.

- **Passives:** auto-combinable when identity-defining attributes match (resistor: resistance, package, tolerance, power; capacitor: capacitance, dielectric, voltage, package, tolerance; inductor: inductance, package, current, tolerance).
- **Actives/ICs:** never silently merged. Suggest aggressively; merge only via approved equivalence family, matching package/pinout, known alias, or explicit user approval.

User actions: add stock to existing, add as variant, merge canonical parts, split, keep separate, mark not-equivalent. All decisions persist (`equivalence_decisions`, `part_aliases`) so suggestions don't reappear. Live duplicate detection during part entry calls the same Rust matcher via a Tauri command.

## 8. Search

SQLite FTS5 indexes names, categories, descriptions, tags, manufacturers, MPNs, supplier SKUs, bins, attribute labels + formatted values, projects, public notes. A structured query layer parses operators — `project:`, `bin:`, `has:dimensions|datasheet|footprint`, `available:<10`, `voltage:>=25V`, `capacitance:10nF..1uF`, `height:<10mm`, `low stock`, `is:archived` — combining FTS candidates with SQL predicates over normalized attribute values. Same query grammar on desktop (Rust) and web (TS, over snapshot data).

## 9. Desktop application

**Sections:** Dashboard, Inventory, Bin browser, Projects, Orders & Imports, History, Settings. Global search and `Ctrl+K` quick-action palette everywhere. No shopping/ordering workflows anywhere (low-stock and shortages are display-only).

- **Dashboard:** summary cards (available units/parts, reserved, checked out, low stock, active projects, metadata review, unbinned), recent activity with safe reverse, and explicit publish/backup status (published, local changes unpublished, publishing, failed, backup pending/failed, offline).
- **Inventory browser:** dense virtualized table (part, category, key specs, available, reserved, checked out, bin, low-stock), inline actions (add stock, consume, reserve, check out, more), the full filter set from requirements, saved views (all, by category, by bin, low stock, recently used, archived, unassigned bin, metadata incomplete), bulk actions (assign bin, add to BOM, archive, export, edit category, tags).
- **Quick actions (`Ctrl+K`):** add stock, consume, reserve, release, check out, return, create part, import — keyboard-first, show remaining quantities before confirm, seconds-fast add-stock flow (search → qty → confirm creates a receive transaction).
- **Add/edit part:** category-adaptive form; basic info, typed category specs, dimensions, CAD/docs, variants + supplier listings, and live duplicate detection with the §7 action set.
- **Part detail:** header (name, category, key specs, bin, four quantity figures, low-stock), primary actions, and sections: overview, specifications, dimensions, projects, variants, supplier listings, purchase history, transaction history, attachments, CAD/KiCad, provenance. "Refresh product data" re-runs enrichment with a compare-and-apply diff view that never overwrites manual values without review.
- **Bin browser:** all bins, contents per bin, unassigned parts, create/rename bins, assign parts, occupied-bin warning (never a block).
- **Projects:** statuses planned/active/completed/archived; fields per requirements; BOM items (part, qty per build, total = per-build × build qty, refdes, required/optional, notes, substitutes, reserved, consumed) with columns per-build/needed/available/reserved/consumed/missing; actions: add part, import BOM, reserve, release, **build from BOM** (review screen listing every transaction, then one atomic group: consume reserved, consume available only when explicitly approved, check out reusables, skip optionals), associate checkouts, duplicate project/BOM, archive.
- **History:** filter by date/type/part/project/import/group/adjustment; grouped actions shown together; reverse transaction, reverse group, view original import, correct matching decision, restore archived part.
- **Settings:** preferences, theme, GitHub public + backup repos, credentials (test/replace/remove), publish/backup now + status + retry, local backup retention, data directory info.

**Theming:** primitive palette + semantic tokens (`--color-bg-app`, `--color-stock-available`, `--color-warning`, …) defined once in `packages/shared`, emitted as CSS custom properties for both apps. Default: dark graphite/near-black, high-contrast off-white text, saturated non-pastel accents (strong amber warning, clear red error, strong green success), dense tables, restrained decoration. Light theme equally non-pastel. Dark/light/system, presets, live preview, import/export. No hardcoded colors in components — enforced by a stylelint rule banning raw color literals outside `packages/shared` token files.

## 10. DigiKey import pipeline

Workflow: **Upload → Extract → Match → Enrich → Review → Assign bins → Confirm.** Inventory is untouched until Confirm; cancellable until then.

- **Files:** drag-drop + picker; original preserved in attachments; SHA-256 hash; duplicate detection by hash + invoice/order/shipment numbers + supplier + line signature — warn and link to prior import; explicit confirm required to reimport.
- **Parsers:** `InvoiceParser` trait; DigiKey PDF, CSV, XLSX adapters (future suppliers slot in). PDF: pdfium positional text → column/table reconstruction → DigiKey-specific rules (`PART:`/`DESC:` items, `MFG : <manufacturer> / <MPN>`, ordered/available/backordered columns, TARIFF sub-rows, ECCN/HTSUS/ROHS noise lines, repeated per-page headers, totals block, `WEB ORDER ID`) → OCR only for scanned PDFs → manual correction UI when confidence is low. Raw extracted fields preserved per line. The provided sample (PO Acknowledgement 100353602, 2 pages, 6 lines) becomes the first sanitized fixture; parsers must tolerate reasonable layout variation, not just this file.
- **Extraction targets:** order metadata (supplier, invoice/order/shipment numbers, dates, currency, subtotal/shipping/tax/total) and line metadata (DigiKey PN, MPN, manufacturer, description, ordered/shipped/backordered qty, unit/extended price, packaging, customer reference, lot/origin when present).
- **Preview:** summary (numbers, dates, line count, total qty, financials, warnings, backorders); PDF shown beside parsed data; selecting a line highlights comparison.
- **Matching order:** exact supplier SKU → exact MPN → known alias → exact normalized identity → strong equivalent suggestion → similar suggestion → create new. Every proposal carries an explanation.
- **Review table:** per line — item, qty, match result, explanation, proposed action, status. Actions: add stock to match, add as variant, match other part, create new, correct extracted values, ignore, mark non-inventory (shipping/fees/promo), split across parts.
- **Quantities:** use shipped/received, never blindly ordered (ordered 10 / shipped 8 / backordered 2 → receive 8); multi-shipment orders never double-count; replacement shipments detectable and reviewable.
- **Commit:** one atomic group creating import record, file ref, new parts, variants, listings, receive transactions, price history, bin assignments, matching decisions. Fully reversible as a group. Post-import line correction = reverse wrong part + receive correct part + update alias rules + preserved history.

## 11. Enrichment pipeline (user-approved: DigiKey API + fallbacks)

`EnrichmentProvider` trait, ordered pipeline: (1) known local supplier listing, (2) **DigiKey Product Information API** (OAuth2 client-credentials; user registers a free app at developer.digikey.com and enters Client ID/Secret in Settings; tokens in Credential Manager), (3) DigiKey product-page metadata, (4) manufacturer page, (5) datasheet extraction, (6) description parsing (always available — parses e.g. "IC OPAMP GP 2 CIRCUIT 8DIP" into category/specs/package), (7) manual review.

Retrieves: manufacturer, MPN, description, category, package, mounting, electrical specs, dimensions, datasheet/product URLs, images, lifecycle, packaging, CAD/symbol/footprint/3D links. Responses cached in `cache/` keyed by provider+part so repeat imports don't refetch. Per-field provenance recorded; inferred identity-defining fields highlighted for confirmation; trusted manual values never overwritten without review. Enrichment failure never blocks import completion — records flagged metadata-incomplete. Re-runnable from part detail (fetch → compare → apply selected). Limitations (API not yet configured, page-scrape fragility) documented in `docs/known-limitations.md`.

## 12. Public snapshot and web app

- **Snapshot:** deterministic JSON at `apps/web/public/inventory.snapshot.json` (configurable) — stable ordering, stable IDs, no volatile fields except the publication timestamp; unchanged inventory ⇒ byte-identical output ⇒ no commit. Includes: canonical parts, public descriptions/notes, attributes, quantities (available/reserved/checked-out), bins, public project associations, dimensions, variants, supplier part numbers, datasheet/product links, publication timestamp. Excludes (test-enforced): credentials, local paths, private notes, raw invoices, purchase prices (unless a setting opts in), logs, backup config, secret metadata, internal-only IDs, deleted records.
- **Web app:** static Vite React SPA on Vercel, loads the snapshot, no auth, no write API, no edit controls. Search (shared query grammar), filters, inventory table, part detail pages, quantities, bins, projects, dimensions, specs, variants, datasheet links, low-stock status. Banner: "Read-only inventory snapshot — last published <timestamp>". Vercel redeploys on snapshot commits.

## 13. GitHub publishing, backup, close-time flow

- **Client:** `GitHubApi` trait + reqwest implementation. Snapshot publish via Contents API (updates only the snapshot file, preserves everything else). Backup via Git Data API — blobs → one tree → **one commit** → ref update — atomic multi-file backups.
- **Public publish config:** owner, repo, branch, snapshot path, optional Vercel URL, publish-on-close, optional auto-publish delay, last success + version.
- **Backup repo (private, separate):** deterministic human-readable structure per requirements — `backup/manifest.json` (format version, schema version, app version, timestamp, record counts, checksums, latest txn ID, attachment count, completion state), `parts/variants/supplier-listings/transactions/projects/boms/imports/dimensions/custom-fields/settings.json`, `attachments/index.json` + `<hash>.<ext>` (deduped), `schema/schema-version.json`. Include/exclude attachments setting. Secrets never enter any export.
- **Close flow (window close = real exit, no tray):** commit pending edits → export snapshot + backup → compare digests with last published → publish changed artifacts (public + backup in parallel) → exit. Timeout with Retry / Close-anyway; failures set separate pending-publish / pending-backup markers; local data is never at risk from network/token/GitHub failures.
- **Startup checks (quiet):** DB readable → unfinished-shutdown detection → migrations (with backup) → invariant validation → pending publish/backup retry when online → newer-remote-backup detection (show local vs remote, review/restore/keep — never auto-replace) → schema-newer-than-app guard (read-only + recovery options).

## 14. Local backups, restore, recovery, first run

- **Local backups:** timestamped SQLite copies before migrations, restores, large imports, large reversals, merges, repairs; plus one auto backup per active day. Retention defaults 30 daily; configurable by count/age/disk.
- **Restore (GitHub or local):** fetch/validate manifest, checksums, format + schema versions → show counts/details → download → restore into a **temp database** → restore attachments → migrate if needed → validate invariants + counts → safety-copy existing DB → swap only after validation. Historical commit selection with backup metadata shown. Existing data never destroyed before the replacement validates.
- **First-run wizard:** (1) new inventory / restore GitHub / restore local → (2) data directory (default appdata, verify writable) → (3) preferences (metric + mm default, USD, low-stock default, bin naming suggestion — labels stay free-form, theme) → (4) public repo config + test connection → (5) backup repo config + test connection. Both GitHub steps skippable; app can initialize expected files in empty repos without touching unrelated files.
- **Recovery mode** (`electronics-inventory.exe --recovery`, independent of the normal UI): DB health inspection, manual backup, restore local/GitHub, rebuild indexes, export readable JSON, migration status, open logs, reset credentials, regenerate snapshot, repair known-safe consistency issues, open DB read-only.

## 15. Migrations

Numbered, versioned, embedded SQL (+ Rust hooks where needed); each has upgrade logic, validation, automated tests against representative prior schemas, pre-migration safety backup, and clear errors. Tracks current DB version vs app-supported version: older → backup + migrate; equal → continue; newer → refuse unsafe writes, offer read-only/recovery.

## 16. Logging and error handling

`tracing` with rotating file logs per area (startup, DB, migrations, imports, parsing, enrichment, publishing, backups, restores, recovery, panics). Redaction layer strips tokens, Authorization headers, secrets. Significant user-facing errors present: plain-language explanation, whether local data is safe, what the app does next, recommended action, expandable technical details, correlation ID. Never raw `Request failed: 422`.

## 17. Launchers and build workflow

PowerShell scripts in `scripts/` + an installer script creating Start Menu/Desktop shortcuts:

1. **Electronics Inventory** — launch the built production executable directly (no dev servers, no terminals).
2. **— Rebuild** — locate repo, verify tools (rustup/cargo, node, pnpm, WebView2), restore deps only on lockfile change, rebuild only on source change, preserve production data, launch, write a readable build log, surface the log on failure. Never auto-pulls git.
3. **— Open Project** — open repo in VS Code.
4. **— Recovery** — launch `--recovery`.

Plus a standard `tauri build` Windows production build and documented installer creation (NSIS/MSI via Tauri bundler). Auto-update is out of scope (GitHub Releases noted as a future path).

## 18. Testing strategy

- **Domain (Rust):** every ledger operation, negative-prevention, atomic groups, reversal (single + group), rollback on failure, archived behavior, invariants.
- **Units (Rust + TS twin, shared fixture):** 10k/10 kΩ/10000 Ω; 0.1 µF/100 nF/100000 pF; 0603/1608 metric; 1/4 W/0.25 W; 3V3/3.3 V; etc.
- **Matching:** exact passives, cross-manufacturer identity, missing fields, differing voltage/dielectric/package, same-name-different-package ICs, approved/rejected equivalents, remembered decisions.
- **Import (sanitized fixtures):** single/multi-page, repeated headers, wrapped descriptions, partial shipment, backorder, cut tape/reel, fee rows, no-charge, missing MPN, duplicate invoice, same order different shipment, known SKU, known MPN + new SKU, new variant, unmatched, malformed PDF, text PDF, scanned fallback, CSV, XLSX, reversal, line correction.
- **Backup/restore:** full round-trip of every entity; missing file, corrupt JSON, bad checksum, missing attachment, old schema, newer schema, interrupted restore, existing DB, historical restore, duplicate hashes, empty and large inventories.
- **Snapshot:** inclusion list, strict exclusion of secrets/private data, byte-determinism.
- **GitHub (mocked trait):** success paths, no-change skip, partial failures both directions, invalid token, missing permission/branch, timeout, rate limit, concurrent publish, close-during-publish, startup retry, remote-file-changed, newer-remote detection.
- **Migrations:** every migration against representative prior schemas.
- **E2E (scripted scenarios via full command layer; WebDriver smoke where feasible):** add/consume/reverse (30→40→35→40), reserve→build, checkout→return, full import→reverse, close-sync with simulated network failure and startup retry.
- **Performance:** generated dataset at 10k parts / 30k variants / 50k listings / 250k transactions / 1k projects / 100k BOM rows; assert responsive list/search/detail behavior; long tasks show progress and stay cancellable pre-commit.

## 19. Performance

Virtualized tables, FTS5 + covering indexes, paginated queries, batched writes, background tasks (Tauri async commands + events for progress), progress reporting for exports/backups/imports/enrichment. Common actions feel immediate at the §18 dataset scale.

## 20. Environment prerequisites and open user actions

Machine state today: Node 22 ✓, npm ✓, Python ✓, pdftotext (MiKTeX) ✓ — **Rust/cargo missing, pnpm missing**. Phase 1 begins by installing rustup + MSVC build tools and enabling pnpm (corepack). WebView2 ships with Windows 11.

Open user actions (none block development): register the free DigiKey API app and enter credentials in Settings; create the public GitHub repo (this repo's remote) and a private backup repo; connect Vercel to the public repo.

## 21. Key decisions (ADR summary)

| # | Decision | Choice | Why |
|---|---|---|---|
| 1 | Domain logic location | Rust core; thin React UI | Integrity at the DB, one implementation, cargo-testable (user-approved) |
| 2 | Enrichment source | DigiKey official API + fallback chain | Best data quality; degrades gracefully (user-approved) |
| 3 | Secrets | Windows Credential Manager (`keyring`) | Native OS store, simpler than Stronghold |
| 4 | Quantities | Fixed-point integer ×1000 | Exact; supports continuous units |
| 5 | IDs | ULIDs | Stable, sortable, deterministic exports |
| 6 | PDF engine | pdfium + Windows OCR fallback | Positional text for tables; no Tesseract |
| 7 | Backup commits | Git Data API single-commit trees | Atomic multi-file backup |
| 8 | Web app | Static SPA reading snapshot | No server/auth surface; deterministic; Vercel-native |
| 9 | Search | FTS5 + normalized-attribute predicates; shared query grammar | Robust, offline, same UX both apps |
| 10 | Sample privacy | Raw invoices gitignored; sanitized fixtures committed | Public repo must not leak name/address |
| 11 | Type safety across IPC | specta-generated TS bindings | UI cannot drift from Rust API |

## 22. Phase plan

Phases 1–9 exactly as the requirements define (Foundation → Inventory domain → Desktop workflows → Projects/BOMs → Imports/enrichment → Public web/publishing → Backup/restore/recovery → Developer workflow → Hardening). Each phase gate: tests pass, format + static analysis clean, affected apps build, docs updated, decisions recorded, data compatibility preserved. Phases are ordering, not permission to stop early. The writing-plans step turns this spec into the detailed per-phase implementation plan.

## 23. Definition of done

The full Definition of Done from the requirements applies verbatim: working desktop app, UI, DB + migrations, ledger, parts/variants, categories/attributes, dimensions, projects/BOMs, PDF/CSV/XLSX import, enrichment, review + reversal, public web app, deterministic snapshot, GitHub publish + separate backup, close-time sync, restore + historical restore, local backups, recovery mode, startup wizard, theme system, launchers, Windows production build, automated tests, isolated seed data, sanitized fixtures, and the complete documentation set (architecture, schema, parsers, backup format, GitHub setup, build, troubleshooting, recovery, known limitations, how to supply more sample invoices). A feature counts as complete only when it works through DB, domain, UI, error handling, and tests.
