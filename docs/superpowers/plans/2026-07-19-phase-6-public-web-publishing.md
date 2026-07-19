# Phase 6 — Public Snapshot + Web Companion + GitHub Publishing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A deterministic, privacy-safe public snapshot exported from the local DB, published to GitHub via a mocked-testable client (live publish after the user creates the repo), and a read-only Vercel web SPA that renders it with the shared search grammar.

**Architecture:** Snapshot building + GitHub publishing live in the `inventory-sync` crate behind a `GitHubApi` trait (hermetic tests; reqwest impl for production). Publish state (last-published digest, pending-publish marker) lives in a new `app_state` DB table (migration 0010) — the spec's `pending-sync/` directory is reserved for Phase 7's backup markers; the public-publish digest belongs with the DB it describes. The web SPA consumes the snapshot statically; search parity comes from TS twins of the units + query-grammar parsers in `packages/shared`, locked to the Rust implementations by shared JSON fixtures.

**Tech Stack:** Rust (serde_json canonical serialization, reqwest blocking + rustls, keyring via inventory-core secrets), TS/React (Vite, the existing @ei/web stub), shared fixtures, Vercel static hosting.

## Global Constraints

- Branch `phase-6-public-web` off main (e228881). One new migration: 0010 (`app_state`), schema v10.
- **Snapshot exclusions are test-enforced (spec §12):** credentials, local paths, private notes, raw invoices, purchase prices (unless a future setting opts in — no setting in Phase 6), logs, backup config, secret metadata, internal-only IDs, deleted/archived-internal records. A denylist test scans the serialized snapshot for forbidden keys AND for known-private seeded values.
- **Determinism (spec §12):** stable ordering (ULID sort), stable field order, no volatile fields except `published_at`. The publish digest is computed over the snapshot WITH `published_at` stripped — unchanged inventory ⇒ unchanged digest ⇒ publish skipped ⇒ the committed file's bytes never churn.
- **Secrets:** GitHub token only in Windows Credential Manager (reuse the 5c `secrets.rs` pattern: write-only IPC, fixed-string errors, redacted Debug, never in DB/logs/snapshot/exports). Publish config (owner/repo/branch/path/Vercel URL) is NOT secret → `settings` table.
- Web app: NO auth, NO write API, NO edit controls; banner "Read-only inventory snapshot — last published <timestamp>". Token-only colors (stylelint covers `apps/**`); `--font-data` for identifiers/quantities.
- Money/quantities in the snapshot: quantities as milli + unit (render via the TS units helper); NO prices at all.
- All gate tests hermetic (mock GitHubApi, no network). Live publish is an orchestrator-run step AFTER the user creates the public repo + connects Vercel (do not block on it).
- PowerShell 5.1 harness: chain `;`, cargo via `$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"; `.
- Commit messages sentence-case imperative, no prefixes.

## File Map

- `crates/inventory-db/migrations/0010_app_state.sql` + `src/app_state.rs` — T1.
- `crates/inventory-sync/src/{lib,snapshot}.rs` — T1 (builder); `src/{github,publish}.rs` — T3/T4.
- `packages/shared/fixtures/unit-cases.json` (extend) + `query-cases.json` (new, generated) — T2.
- `packages/shared/src/{units,query,snapshot}.ts` (+tests) — T2 (snapshot.ts rewritten full-schema).
- `crates/inventory-core/src/secrets.rs` — T3 (add GitHub-token entry fns, same pattern).
- `apps/desktop/src-tauri/src/commands.rs` + `hooks/publish.ts` — T4; `features/settings/PublishSettings.tsx` + Dashboard card — T5; close-flow in `src-tauri` (window-event) + `features/shell` dialog — T6.
- `apps/web/src/` — T7 (App/table/search), T8 (detail/bins/projects/routing), `apps/web/vercel.json` — T9.
- Docs (T9): `docs/publishing.md` (new), `docs/architecture.md`, `docs/schema.md`, `docs/ui.md`, `docs/known-limitations.md`.

---

### Task 1: Migration 0010 (app_state) + deterministic snapshot builder

**Files:** `crates/inventory-db/migrations/0010_app_state.sql`; `crates/inventory-db/src/app_state.rs` (+lib.rs); `crates/inventory-sync/src/snapshot.rs` (+lib.rs, Cargo.toml deps: inventory-db, inventory-core, serde/serde_json, sha2); tests `crates/inventory-sync/tests/snapshot.rs`, `crates/inventory-db/tests/schema.rs`/`migrations.rs` additions.

**Interfaces (produces):**
- `0010_app_state.sql`: `CREATE TABLE app_state (key TEXT PRIMARY KEY, value TEXT NOT NULL, updated_at TEXT NOT NULL DEFAULT (datetime('now'))) STRICT;` — keys used this phase: `last_published_digest`, `pending_publish` ('1' present/absent), `last_published_at`. `Database::get_app_state(key) -> Result<Option<String>>`, `set_app_state(key, value)`, `clear_app_state(key)`. SUPPORTED_SCHEMA_VERSION → 10; migration test v9→v10.
- `snapshot::build_snapshot(db: &Database) -> Result<Snapshot, SyncError>` — reads parts (non-archived), variants, supplier listings (NO prices — listing includes supplier, SKU, product_url, packaging ONLY), attributes (label + display value + normalized), dimensions, bins, tags, projects (name/status/description only — no notes) + BOM part associations, stock (available/reserved/checked_out milli + unit), low-stock flags. `Snapshot` is a serde struct tree with `#[serde(serialize_with)]`/BTreeMap ordering such that serialization is canonical: all arrays sorted by ULID (or name for bins/tags), all maps key-sorted.
- `snapshot::to_canonical_json(snapshot: &Snapshot, published_at: Option<&str>) -> String` — `published_at: None` → the digest form (field omitted); `Some(ts)` → the publish form. 2-space indent, trailing newline, LF endings (byte-stable).
- `snapshot::content_digest(snapshot: &Snapshot) -> String` — sha256 hex of the digest form.

- [ ] TDD: migration tests (table, STRICT, upsert round-trip, v9→v10 w/ backup). Snapshot: seed a rich DB (dev_seed + extras incl. a part with private_notes, a supplier listing with a price, an archived part) → build → (a) **exclusion test**: serialized JSON contains NO `private_notes`/`price`/`micros`/`token`/`client_id`/path-like (`C:\\`)/archived part's id — key-denylist AND seeded-value scan; (b) **determinism test**: build twice from the same DB → identical bytes; mutate stock → digest changes; same content + different `published_at` → same `content_digest`; (c) inclusion spot-checks (a part's attributes/dimensions/variants/stock present + correctly shaped). `cargo test -p inventory-sync -p inventory-db` green; fmt/clippy. Commit `Add app_state migration and deterministic snapshot builder`.

---

### Task 2: TS twins — units (query-side) + query grammar + full snapshot schema

**Files:** `packages/shared/fixtures/unit-cases.json` (EXTEND: add `5m`, `5m5`, `500m`, `Ω` U+2126 ohm-sign cases — the Rust `units.rs` fixture test must still pass: run `cargo test -p inventory-core` after editing; if a new case exposes a Rust gap, fix Rust to match the documented normalization, noting it); NEW `packages/shared/fixtures/query-cases.json` (generated: write a small Rust test-helper in `crates/inventory-core/src/search.rs` tests that serializes `parse_query` outputs for a curated input list — commit the generated JSON; a Rust test asserts the fixture matches current parser output so the two can never drift); `packages/shared/src/units.ts`, `query.ts`, rewrite `snapshot.ts` (full schema), `index.ts` exports; vitest tests for each; `apps/web/src/snapshot.ts` updated to the new parse.

**Interfaces (produces):**
- `parseUnitValue(text: string): { value: number; canonicalUnit: string } | null` — query-side only (parse `10k`, `4k7`, `0.1uF`, `3V3`, `1/4W`, `0603`, µ/Ω unicode variants) matching Rust normalization; tested against EVERY unit-cases.json case (parse + reject lists).
- `parseQuery(input: string): ParsedQuery` — TS mirror of `inventory-core::search::{ParsedQuery, RawFilter, FilterOp, QueryFlags}` (same field names camelCased consistently — document the mapping in the file header); tested against every query-cases.json case.
- `parseSnapshot(body: unknown): Snapshot | null` — full-schema validation (replaces header-only `parseSnapshotHeader`, which stays as a thin wrapper for compatibility); `Snapshot` TS types mirroring T1's Rust shapes exactly.

- [ ] TDD: fixture-driven tests both sides (Rust fixture test extended cases pass; TS units/query tests iterate the fixtures — a fixture case count assertion prevents silent truncation); snapshot parse round-trip on a T1-generated sample (commit a small `packages/shared/fixtures/sample-snapshot.json` GENERATED by a Rust test from seeded data — sanitized by construction since the builder excludes private data; a Rust test regenerates + compares to prevent drift). `pnpm --filter @ei/shared test` + `cargo test -p inventory-core` green. Commit `Add TS unit and query parsers with shared fixtures and full snapshot schema`.

---

### Task 3: GitHub client — trait, reqwest impl, token storage

**Files:** `crates/inventory-sync/src/github.rs`; `crates/inventory-core/src/secrets.rs` (add `store_github_token/load_github_token/clear_github_token` — same keyring service pattern, entry name `ElectronicsInventory-GitHub`); Cargo.toml (reqwest blocking + rustls-tls + json, workspace-consistent with inventory-enrich's); tests `crates/inventory-sync/tests/github.rs` (mock trait) + secrets tests (mock store).

**Interfaces (produces):**
- `trait GitHubApi { fn get_file(&self, cfg: &RepoRef, path: &str) -> Result<Option<RemoteFile>, GitHubError>; fn put_file(&self, cfg: &RepoRef, path: &str, content: &[u8], message: &str, prev_sha: Option<&str>) -> Result<PutOutcome, GitHubError>; }` with `RepoRef { owner, repo, branch }`, `RemoteFile { sha: String, content: Vec<u8> }`, `PutOutcome { new_sha: String }`.
- `GitHubError` (thiserror): `Auth` (401/403 bad token), `NotFound` (repo/branch missing), `Conflict` (sha mismatch — remote changed), `RateLimited`, `Network(String — fixed classification, NEVER the response body)`, `Api(u16)`. Display strings fixed; a test asserts a planted token never appears in any error Display/Debug.
- `ReqwestGitHub::new(token: String) -> Self` — Contents API impl: GET `/repos/{owner}/{repo}/contents/{path}?ref={branch}` (base64 decode), PUT with `sha` when updating; `Authorization: Bearer` header; 15s/5s timeouts (mirror the DigiKey client); token held in-memory only, no Debug derive.

- [ ] TDD: trait-level tests with a `MockGitHub` (in-memory map): put-new, put-update w/ sha, conflict on stale sha, auth error propagation. Reqwest impl compiles; its LIVE behavior is deferred to the orchestrator's live-publish step (no network tests in the gate — mirror the DigiKey probe pattern). Secrets round-trip via the mock store; no-secret-in-errors test. `cargo test -p inventory-sync -p inventory-core` green; clippy/fmt. Commit `Add GitHub client trait and token storage`.

---

### Task 4: Publish orchestration + commands + hooks

**Files:** `crates/inventory-sync/src/publish.rs`; `apps/desktop/src-tauri/src/commands.rs` (+CommandError arms); regenerate bindings; `apps/desktop/src/hooks/publish.ts` (+test); tests `crates/inventory-sync/tests/publish.rs`.

**Interfaces (produces):**
- `PublishConfig` read from settings keys `publish_owner`/`publish_repo`/`publish_branch` (default `main`)/`publish_path` (default `apps/web/public/inventory.snapshot.json`)/`publish_vercel_url` (optional display-only).
- `publish::publish_snapshot(db: &mut Database, api: &dyn GitHubApi) -> Result<PublishOutcome, SyncError>`: build → `content_digest` → if digest == `app_state.last_published_digest` → `PublishOutcome::Unchanged` (NO network call); else render publish form with fresh `published_at` → get remote sha → put → update `last_published_digest`+`last_published_at`, clear `pending_publish` → `PublishOutcome::Published { digest }`. Any failure → set `pending_publish` → typed error (config missing → `NotConfigured`; auth/network/conflict mapped from GitHubError).
- Commands: `get_publish_status() -> PublishStatus { configured: bool, repo: Option<String>, last_published_at: Option<String>, pending: bool, vercel_url: Option<String> }` (never the token); `set_publish_config(owner, repo, branch, path, vercel_url)`; `set_github_token(token)` (write-only, trimmed, typed reject on empty); `clear_github_token()`; `test_github_connection() -> GitHubTestResult { ok, message }` (fixed strings: probe = GET the configured repo's branch ref or the snapshot path; "not configured"/"connected"/"rejected — check token"/"repo or branch not found"/"network error or timeout"); `publish_now() -> PublishOutcome` (serialized enum: published/unchanged); `retry_pending_publish() -> Option<PublishOutcome>` (no-op None when no pending marker or not configured — the quiet startup path).
- Hooks: `usePublishStatus()`, `useSetPublishConfig()`, `useSetGitHubToken()`, `useClearGitHubToken()`, `useTestGitHubConnection()`, `usePublishNow()`, `useRetryPendingPublish()` — mutations invalidate `keys.publishStatus`.

- [ ] TDD (hermetic — MockGitHub injected; find how commands construct the api: a small factory fn in commands that live code points at ReqwestGitHub and tests can exercise at the publish.rs level): unchanged-skip (no api calls — assert via mock call-count), publish-updates-state, failure-sets-pending, retry-clears-pending on success, NotConfigured path, digest stability across publish forms. Command drift green; hooks tests. `cargo test --workspace` + `pnpm --filter @ei/desktop test` green. Commit `Add snapshot publish orchestration and commands`.

---

### Task 5: Desktop publish UI — Settings section + Dashboard status

**Files:** `apps/desktop/src/features/settings/PublishSettings.tsx` (+css+test), wired into SettingsPage; Dashboard publish-status card (`features/dashboard/` — extend the existing card grid).

**Interfaces:** Consumes T4's hooks. Mirrors `DigiKeySettings` patterns exactly: masked token entry (clear-after-save, DOM-wide no-secret test), repo config form (owner/repo/branch/path/vercel url; save → useSetPublishConfig), Test connection (fixed-string results), "Publish now" (pending state → outcome toast: "Published" / "Already up to date"), status display (configured, last published, pending-publish warning with a Retry button → useRetryPendingPublish). Dashboard card: publish status per spec §9 (published/unpublished-changes/publishing/failed/pending — derive from PublishStatus + mutation states; keep it honest: without change-tracking, show last-published time + pending flag; "unpublished changes" detection = `publish_now` returning Unchanged proves up-to-date, otherwise unknown — display "Last published <ts>" + pending/failed states only; note the simplification in the component doc).

- [ ] TDD: token clear-after-save + DOM-wide secret absence; config save; test-connection render; publish-now outcomes; pending + retry; dashboard card states. `pnpm --filter @ei/desktop test` + build green; stylelint/prettier. Live CDP (scratch dir): configure a FAKE repo, Test connection → clean "rejected/not found" handling, Publish now → typed failure sets pending + card shows it, Retry visible; screenshots. Commit `Add publish settings and dashboard status`.

---

### Task 6: Close-time publish flow

**Files:** `apps/desktop/src-tauri/src/` (main.rs/lib.rs window-close handling + a `close_flow` module); `apps/desktop/src/features/shell/ClosePublishDialog.tsx` (+css+test) + AppShell wiring; commands additions (`begin_close_publish` etc. — design below); tests both sides.

**Interfaces:**
- Rust: intercept `WindowEvent::CloseRequested` → `api.prevent_close()` + emit `close-publish-requested` to the frontend ONCE (guard re-entry). Frontend dialog drives: if publish not configured OR nothing pending to do → immediately call `finalize_close()` command → `app.exit(0)`. Else show "Publishing before close…" → call `publish_now` (the T4 command; it already sets pending on failure) with a 20s frontend timeout → on success/Unchanged → `finalize_close()`; on failure/timeout → dialog offers **Retry** / **Close anyway** (close-anyway leaves the pending marker — honest copy: "Publish failed — it will retry next launch. Your local data is safe.") → `finalize_close()`.
- `finalize_close()` command: `app_handle.exit(0)` (nothing else — local data is already durable; the spec's "commit pending edits" is satisfied vacuously since all edits are synchronous transactions).
- Startup retry (quiet): on app start the frontend (AppShell mount) calls `useRetryPendingPublish` once, silently; success clears the pending state (Dashboard card updates), failure stays quiet (card still shows pending).

- [ ] TDD: Rust close-guard unit-testable pieces kept thin (the event wiring is glue; test the frontend logic hard): dialog state machine (configured/unconfigured, success, Unchanged, failure→Retry/Close-anyway, timeout), startup retry fire-once. Live CDP: with a fake repo configured, close the window → dialog appears → failure path → Close anyway → app exits (verify process gone) → relaunch → quiet retry attempted + pending card; with publishing unconfigured → close exits immediately. Screenshots. `cargo test --workspace` + `pnpm --filter @ei/desktop test` + build green. Commit `Add close-time publish flow with retry and pending markers`.

---

### Task 7: Web SPA — snapshot load, inventory table, search, banner

**Files:** `apps/web/src/` — rewrite `App.tsx` (layout: banner + search + table), `snapshot.ts` (already updated T2 — extend as needed), NEW `Inventory.tsx`, `searchSnapshot.ts` (+tests each), `web.css` (tokens via @ei/shared `generateCssVariables` — check how the desktop injects them and mirror).

**Interfaces:**
- `searchSnapshot(snapshot: Snapshot, query: string): PartSummary[]` — pure TS: `parseQuery` (T2) → free-text match over name/category/description/tags/manufacturer/MPN/SKU/bin (case-insensitive substring; no FTS — snapshot scale is fine), filters: `bin:`, `project:`, `is:archived` excluded by construction, `available:<10` style numeric ops on stock, attribute filters via `parseUnitValue` on normalized values (`voltage:>=25V`, `capacitance:10nF..1uF`), `has:datasheet` etc. per the grammar's RawFilter keys — implement the subset the snapshot data supports; unsupported filter keys → ignored with a visible "unsupported filter" chip (honest, not silent).
- Inventory table: dense, `--font-data`, columns part/category/key specs/available/reserved/checked-out/bin/low-stock; stock gauge OPTIONAL (reuse nothing from desktop — web has no component lib; a simple segmented bar div matching token colors is enough); virtualization unnecessary below ~2k rows — plain render with a perf note.
- Banner: "Read-only inventory snapshot — last published {published_at}" + empty state ("No snapshot published yet") when load fails/none.

- [ ] TDD (vitest in @ei/web — check its test setup; add if the stub lacks one, mirroring @ei/desktop's config): searchSnapshot cases (free text, bin filter, numeric stock filter, unit-normalized attribute filter incl. equivalent forms 0.1uF==100nF, unsupported-filter chip data), table rendering from a fixture snapshot (the T2 sample-snapshot.json), banner/empty states. `pnpm --filter @ei/web test` + `build` green. Visual check: `pnpm --filter @ei/web dev` + Playwright/CDP screenshot with the sample snapshot in `public/`. Commit `Add web inventory table with snapshot search`.

---

### Task 8: Web SPA — part detail, bins, projects, routing

**Files:** `apps/web/src/` — `PartDetail.tsx`, `Bins.tsx`, `Projects.tsx`, `router.ts` (+tests, css).

**Interfaces:** Hash-based routing (`#/`, `#/part/<id>`, `#/bins`, `#/projects`) — sidesteps the known `appType: 'mpa'` deep-link 404 issue (progress note) with zero server config; Vercel serves one index.html regardless. Part detail: header (name/category/bin/stock figures), specs (attributes with display values), dimensions, variants (manufacturer/MPN/lifecycle/datasheet links), supplier part numbers (NO prices — they're not in the snapshot), tags, public description/notes, project associations. Bins: bin list with contents + unassigned. Projects: list with status + part associations. All read-only; every part reference cross-links.

- [ ] TDD: router parse/format; detail renders every section from the sample snapshot (assert a known attribute/dimension/variant); bins grouping; projects associations; unknown part id → not-found panel + back link. Build + tests green. Screenshot each view via dev server. Commit `Add web part detail, bins, and projects views`.

---

### Task 9: Vercel config + gate + docs (+ live-publish handshake point)

**Files:** `apps/web/vercel.json` (static build config: `buildCommand` `pnpm --filter @ei/web build`, `outputDirectory` `apps/web/dist`, SPA rewrite to /index.html as belt-and-braces though hash routing needs none); `docs/publishing.md` (NEW: how the snapshot works — determinism, digest skip, exclusions; GitHub setup: create the PUBLIC repo, generate a fine-grained PAT with Contents read/write on that repo only, paste in Settings; Vercel: import the repo, framework Vite, the vercel.json; close-flow + pending retry semantics; troubleshooting table); `docs/architecture.md` (sync crate section), `docs/schema.md` (0010, v10), `docs/ui.md` (publish settings, dashboard card, close dialog), `docs/known-limitations.md` (no unpublished-changes detection — dashboard shows last-published only; prices excluded with no opt-in setting yet; web search is substring not FTS).

- [ ] Full gate `powershell -File scripts\verify.ps1` → ALL CHECKS PASSED (verify @ei/web tests are IN the gate script — if verify.ps1 doesn't run the web workspace, ADD it: tests + build). Docs accurate. **Live-publish handshake:** report to the orchestrator that the phase is gate-complete and the live step needs: (1) the public GitHub repo created, (2) a fine-grained PAT (Contents RW on that repo) stored via Settings, (3) Vercel connected. The orchestrator asks the user; live verify then = configure real repo in Settings → Test connection → Publish now → snapshot file appears in the repo → Vercel deploys → the web app renders the real inventory. Do NOT block the phase gate on this. Commit `Add phase 6 documentation and Vercel config`.

---

## Plan self-review notes

- **Spec §12 coverage:** deterministic snapshot + exclusions test-enforced (T1), configurable path via publish_path (T4), web SPA full feature list (T7/T8), banner (T7), Vercel redeploy-on-commit (T9 — inherent). §13 public side: GitHubApi trait + reqwest (T3), Contents-API single-file publish preserving everything else (T3/T4), publish config (T4/T5), close flow with Retry/Close-anyway + pending markers + startup quiet retry (T6), token never in exports (T1 exclusion test + T3 secrets pattern). §13 backup repo (Git Data API), §14 restore, newer-remote detection → Phase 7 (per the phase ordering: "Backup/restore/recovery"); close-flow runs backup in parallel per spec only once Phase 7 adds it — T6's design leaves an obvious seam (the dialog drives publish_now; Phase 7 adds backup_now beside it). Auto-publish delay setting deferred with the dashboard "unpublished changes" simplification (documented, known-limitations).
- **Fixture-locked twins:** unit-cases.json extended (5m/5m5/500m, U+2126 — the recorded Phase 6 input), query-cases.json generated FROM Rust and drift-tested on the Rust side, consumed by TS tests — parsers cannot diverge silently. The sample snapshot is likewise generated-and-drift-tested.
- **Type consistency:** Snapshot shapes defined once in T1 (Rust) mirrored in T2 (TS) and consumed by T7/T8; PublishOutcome/PublishStatus/GitHubTestResult defined in T4 and consumed by T5/T6; RepoRef/GitHubError defined in T3 consumed by T4.
- **Privacy:** the ONLY committed snapshot-like artifact is generated from seeded (synthetic) data by the builder that itself enforces exclusions; the denylist test scans both key names and seeded private values. PII fixtures policy unchanged.
- **Live dependency isolated:** exactly one step (T9's handshake) needs the user; everything else hermetic, mirroring the DigiKey pattern that worked in 5c/5d.
