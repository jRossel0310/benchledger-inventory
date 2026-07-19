# Phase 5d — Orders & Imports UI + Enrichment UI + Settings Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the complete 5a/5b/5c backend usable from the desktop app: the Orders & Imports screen (upload → extract → match → review → assign bins → confirm → reverse), the enrichment compare-and-apply diff UI with per-field approval, and the Settings DigiKey section (masked credentials, environment toggle, connection test).

**Architecture:** Pure UI phase over the existing typed command surface, plus one small backend task (T1) adding the credential-management commands the Settings screen needs and surfacing `chain.images` in `EnrichmentDiff`. Every screen follows the Phase 3 "bench instrument" patterns: dense tables, `--font-data` for identifiers/quantities/prices, token colors only, Radix primitives, TanStack Query hooks with complete invalidation, CDP live verification.

**Tech Stack:** React 18 + TypeScript strict, TanStack Router/Query, Radix UI, specta-generated `bindings.gen.ts`, Rust (keyring, commands.rs) for T1 only.

## Global Constraints

- Branch `phase-5d-import-ui` off main (0167c5a). Schema stays v9 — NO new migrations.
- Secrets: credentials cross IPC ONCE, write-only, immediately keyring'd; never echoed back, never in logs/state/DB; UI clears input fields after save; no command ever RETURNS a credential (spec §16, ADR 3).
- Enrichment trust rule: `acknowledge_review: true` is set ONLY from an explicit per-field user confirmation — never by bulk select-all (backend enforces via `EnrichmentReviewRequired`, UI must make the confirmation distinct).
- Import receive quantities are SHIPPED, never ordered (backend enforces; UI displays both).
- Inventory is untouched until Confirm; cancel/leave before commit loses nothing but the parse (spec §10).
- Money = micros, quantities = milli (`formatQuantity`; prices formatted via a shared `formatMicros` helper — add to `lib/format.ts` if absent).
- All color via `var(--color-*)` tokens (stylelint-enforced); data in `--font-data`; copy names its effect ("Commit import", toast "Received N lines").
- Every UI task: TDD component tests + live CDP screenshot verification against a scratch seeded data dir; `pnpm --filter @ei/desktop test` + `build` green; prettier/stylelint clean.
- Commit messages: sentence-case imperative, no prefixes (house style).

## File Map

- `apps/desktop/src-tauri/src/commands.rs` — T1: +3 commands (set/clear/test DigiKey credentials); `EnrichmentDiff.images` passthrough (in `crates/inventory-db/src/enrichment.rs`).
- `apps/desktop/src/hooks/enrichment.ts` — T1: +useSetDigiKeyCredentials/useClearDigiKeyCredentials/useTestDigiKeyConnection.
- `apps/desktop/src/features/orders/` — T2: `OrdersList.tsx` (replaces OrdersPage stub), `UploadImport.tsx`; T3: `ImportReview.tsx`, `ReviewLineTable.tsx`, `LineActionEditor.tsx`; T4: `CreateFromLineDialog.tsx`, bin column in ReviewLineTable, commit/reverse flows in ImportReview. Each + `.css` + `.test.tsx`.
- `apps/desktop/src/features/part/EnrichmentDiffDialog.tsx` — T5 (+ css + test); "Refresh product data" wiring in `PartDetail.tsx`.
- `apps/desktop/src/features/settings/DigiKeySettings.tsx` — T6 (+ css + test), wired into `SettingsPage.tsx`.
- `apps/desktop/src/app/routes.tsx` — T2: `/orders` → OrdersList, `/orders/$importId` → ImportReview.
- Docs (T7): `docs/ui.md`, `docs/imports.md`, `docs/enrichment.md`, `docs/known-limitations.md`.

---

### Task 1: Credential commands + enrichment images passthrough (backend)

**Files:** `apps/desktop/src-tauri/src/commands.rs`; `crates/inventory-core/src/secrets.rs` (only if a clear/delete fn is missing); `crates/inventory-enrich/src/digikey.rs` (token-fetch probe helper if needed); `crates/inventory-db/src/enrichment.rs` (images); regenerate `bindings.gen.ts`; `apps/desktop/src/hooks/enrichment.ts` + test.

**Interfaces (produces):**
- `set_digikey_credentials(client_id: String, client_secret: String) -> Result<(), CommandError>` — validates non-empty/trimmed, writes via the existing secrets module, returns NOTHING; params never logged (verify no tracing on this command's args; add `#[tracing::instrument(skip_all)]` if instrumented).
- `clear_digikey_credentials() -> Result<(), CommandError>` — deletes the keyring entries (missing entries = Ok, idempotent).
- `test_digikey_connection() -> Result<DigiKeyTestResult, CommandError>` where `DigiKeyTestResult { ok: bool, environment: String, message: String }` — loads credentials, attempts ONLY an OAuth token fetch against the configured environment (no product call), maps outcomes to fixed strings ("connected", "not configured", "rejected (check credentials/environment)", "network error/timeout") — never echoes response bodies or secrets. Reuse the client's existing token path via a small `pub fn probe_token(&self) -> Result<(), EnrichError>` on `DigiKeyClient` (respects the 15s/5s timeouts).
- `EnrichmentDiff` gains `pub images: Vec<String>` — populated in `enrich_part_preview` from `chain.images` (dedup; URLs only, display-only in 5d).
- Hooks: `useSetDigiKeyCredentials()` (mutation → invalidates `keys.digikeyStatus`), `useClearDigiKeyCredentials()` (same), `useTestDigiKeyConnection()` (mutation, no invalidation).

- [ ] TDD (Rust): set→status configured=true→clear→configured=false round-trip using the secrets module's test seam (mock store if that's how T1-5c tested; NEVER the real keyring in the gate); `test_digikey_connection` with no credentials → ok=false "not configured" (hermetic, no network — the not-configured path short-circuits before any request; the connected path is NOT gate-tested, it's live-verified); a serialization test asserting `DigiKeyTestResult` JSON never contains a secret-looking field; images: a canned chain with images → diff.images populated, deduped. Bindings regenerated, drift test green with `EXPORT_BINDINGS` unset. TS hook tests: invalidation of digikeyStatus on set/clear. GREEN `cargo test --workspace` + `pnpm --filter @ei/desktop test`; fmt/clippy/prettier. Commit `Add DigiKey credential commands and surface enrichment images`.

---

### Task 2: Orders & Imports list + upload

**Files:** `apps/desktop/src/features/orders/OrdersList.tsx` + `UploadImport.tsx` (+ css + tests); replace the `OrdersPage.tsx` stub; `apps/desktop/src/app/routes.tsx` (`/orders` → OrdersList, `/orders/$importId` → ImportReview placeholder until T3 lands — a minimal detail shell is fine).

**Interfaces:** Consumes `useImports()` (ImportRecord[]), `useParseImport({bytes, filename})` (bytes as number[]/Uint8Array per the hook's existing signature — READ it), `useReverseImport`. ImportRecord fields: supplier, order/invoice/shipment numbers, order_date, currency, source_format, status ('parsed'|'committed'|'reversed'), *_micros totals, line_count, created_at.

- OrdersList: dense table (created_at, supplier, order #, source format badge, line count, total (formatMicros + currency), status chip — token colors distinct per status), newest first (hook order), row click → `/orders/$importId`. Empty state invites upload.
- UploadImport: file picker + drag-drop (accept .pdf/.csv/.xlsx), reads bytes (`File.arrayBuffer()`), calls useParseImport, on success navigate to the review route; on parse error show the CommandError message plainly (data safe, nothing stored on failure — verify against backend behavior and word the copy accordingly). Duplicate detection surfaces AFTER parse in the review screen (backend puts `duplicate_of` on the review) — the upload itself never blocks.

- [ ] TDD: list renders records + status chips + navigates; upload calls parse with the file bytes and navigates on success; parse error shown. Live CDP: launch scratch dir, upload the REAL private sample PDF (`samples/digikey/private/…` — it stays local; screenshots go to scratchpad only, NEVER committed), see it appear in the list; screenshot list + upload states. GREEN; fmt/prettier/stylelint; commit `Add orders list and import upload`.

---

### Task 3: Import review — summary + line table + per-line actions

**Files:** `apps/desktop/src/features/orders/ImportReview.tsx`, `ReviewLineTable.tsx`, `LineActionEditor.tsx` (+ css + tests); wire `/orders/$importId`.

**Interfaces:** Consumes `useImportReview(importId)` → `ImportReview { import, lines: ImportReviewLine[], duplicate_of: ImportRecord[], total_receive_lines }`. `ImportReviewLine { line_id, line_number, kind, supplier_sku, mpn, manufacturer, description, receive_qty_milli (SHIPPED, null for non-part/zero), ordered_milli, backordered_milli, unit_price_micros, matches: MatchResult[] (part_id, display_name, verdict_kind, explanation, rank), proposed: ProposedAction, warning }`. Produces for T4: a `decisions: Map<ImportLineId, LineDecision>` state model (`{type:'add_stock',part_id} | {type:'create_new',draft,variant,listing} | {type:'add_as_variant',part_id,variant,listing} | {type:'skip'}`) owned by ImportReview and passed down.

- Summary header: order metadata + financial block (subtotal/shipping/tax/tariff/total via formatMicros), line counts, backorder warnings, and a prominent duplicate warning when `duplicate_of` is non-empty (link to the prior import; proceeding is allowed — warn-not-block, spec §10).
- ReviewLineTable: per line — line #, item (sku/mpn/manufacturer/description, `--font-data`), ordered/shipped/backordered quantities (shipped highlighted as what will be received), unit price, match verdict badge + explanation (the top match), current action, status. Non-part lines (fee/tariff/no_charge/unknown) shown greyed with their kind badge, no action editor (display-only; spec: mark non-inventory).
- LineActionEditor (per part line; opens inline or popover): choose among top matches (each with verdict + explanation), "match other part" (reuse the existing part-search-select pattern from BomTable/inventory), "create new part" (defers the full dialog to T4 — store `{type:'create_new'}` intent with a placeholder draft the T4 dialog fills), "skip". Default = backend `proposed`. Changing a decision updates the map + the row status.

- [ ] TDD: summary renders metadata/financials/duplicate warning; table renders all line kinds correctly (shipped-not-ordered as receive qty; non-part greyed); action editor switches decisions (asserted via the decisions map); default decisions come from `proposed`. Live CDP: open the real parsed invoice review, see 6 lines + matches; screenshot. GREEN; commit `Add import review screen with per-line actions`.

---

### Task 4: Import review — create-from-line dialog, bins, commit + reverse

**Files:** `apps/desktop/src/features/orders/CreateFromLineDialog.tsx` (+ css + test); extend `ReviewLineTable.tsx` (bin column) and `ImportReview.tsx` (commit bar, reverse action for committed imports).

**Interfaces:** Consumes `useCommitImport({importId, decisions})` → GroupRecord (atomic; invalidations owned by the hook — VERIFY its invalidation list covers parts/stock/search/dashboard/imports/history and extend the hook if the review finds gaps), `useReverseImport({importId, note})`, `useCategories`/PartForm helpers for the draft dialog, `useBins` (existing bins hook) for bin suggestions.

- CreateFromLineDialog: prefills `PartDraft` from the line (display_name from description, category default/picker, quantity_unit 'each', description; usage_behavior default 'usually_consumed'), `VariantDraft` (manufacturer, mpn), `ListingDraft` (supplier "DigiKey", supplier_sku, unit price micros, packaging) — user edits then saves the decision into the map. Bin label field with existing-bin suggestions + free-form (occupied-bin warn-not-block, mirroring the part form).
- Bin column: for add_stock/add_as_variant targets show the target part's current bin; for create_new show/edit the draft's bin.
- Commit bar: summary of what commit will do (N receives, M new parts, K variants, skipped/fee lines excluded), the atomic note ("committed as one transaction group — reversible from History"), Commit → useCommitImport(decisions) → success toast "Received N lines" + status flips to committed + navigate stays (header shows committed state + the group link). Errors: CommandError surfaced, nothing partial (backend guarantees).
- Reverse: on a committed import, "Reverse import" (confirm dialog stating full-group reversal; created parts survive at zero stock — per 5b behavior) → useReverseImport → status reversed.

- [ ] TDD: dialog prefill from a line; decisions map carries the edited drafts; commit calls the hook with the exact decisions map (asserted arg shape); commit disabled while any part line lacks a decision (defaults count as decisions); reverse flow calls hook. Live CDP: commit the real invoice into the scratch DB — watch stock appear (inventory table), then reverse it and watch it zero; screenshot review-before-commit, committed state, history group, post-reverse. GREEN; commit `Add import commit flow with bin assignment and reversal`.

---

### Task 5: Enrichment diff UI + part-detail entry point

**Files:** `apps/desktop/src/features/part/EnrichmentDiffDialog.tsx` (+ css + test); `PartDetail.tsx` — replace the Phase 3 "Refresh product data" stub button with the real entry point.

**Interfaces:** Consumes `useEnrichmentPreview(partId, {enabled})` (lazy — enable ONLY when the dialog opens), `useApplyEnrichment()` (`{partId, applied: AppliedField[]}`; `AppliedField { key, value, source, acknowledge_review }`), `useDigiKeyStatus()`. `EnrichmentDiff { part_id, diffs: FieldDiff[], notes, provider_summary, images }`; `FieldDiff { key, current, proposed, source, current_source, requires_review }`.

- Dialog: opens from "Refresh product data" → preview fetch runs (loading state; DigiKey-not-configured state shows guidance + link to Settings when `useDigiKeyStatus` says unconfigured — description-parser candidates still show). Per-field rows: key (readable label), current → proposed, source badge (digikey/description/inferred token colors), checkbox to include. **requires_review rows are visually distinct (warning accent) and their checkbox alone does NOT arm them: an explicit per-row "Overwrite manual value" / "Accept inferred over existing" confirmation control sets `acknowledge_review: true`.** No select-all across requires_review rows (select-all may exist but only covers unprotected rows — document in the component). Images: thumbnail strip/URL list (display-only). Notes/provider_summary in a collapsed footer.
- Apply: sends ONLY checked fields with their ack flags → success toast "Applied N fields" + dialog closes; `EnrichmentReviewRequired` error (if a race slips through) surfaced with plain copy. Provenance is recorded by the backend; the part detail's provenance tab reflects it (hook invalidation — verify).

- [ ] TDD: preview lazy (no fetch until open); unprotected field checked+applied → hook called with ack=false; requires_review field cannot reach the applied list without the distinct confirmation (assert both: checked-without-confirm excluded or blocked, confirmed → ack=true); not-configured state renders guidance; images render. Live CDP (production creds live on this machine): open a real part (from the committed import), Refresh product data → real DigiKey diff appears → apply a subset incl. one requires_review confirmation → part detail shows updated fields + provenance; screenshots of diff + confirmed state. GREEN; commit `Add enrichment diff dialog with review acknowledgement`.

---

### Task 6: Settings — DigiKey section

**Files:** `apps/desktop/src/features/settings/DigiKeySettings.tsx` (+ css + test); wire into `SettingsPage.tsx`.

**Interfaces:** Consumes `useDigiKeyStatus()`, `useSetDigiKeyCredentials()`, `useClearDigiKeyCredentials()`, `useTestDigiKeyConnection()`, `useSetDigiKeyEnvironment()`.

- Status card: configured yes/no, current environment. Credentials form: Client ID (text) + Client Secret (masked `type=password`), Save → useSetDigiKeyCredentials → **both fields cleared from component state on success** (and on unmount), toast "Credentials saved"; Replace = same form; Remove → confirm → useClearDigiKeyCredentials. NO display of stored values ever (status is only a boolean).
- Environment toggle: sandbox/production radio with plain-language copy: production apps (the normal registration) work only against production; sandbox requires separate sandbox credentials — a 401 on the wrong pairing is expected. Persist via useSetDigiKeyEnvironment.
- Test connection: button → useTestDigiKeyConnection → show ok/message inline (fixed strings from backend).

- [ ] TDD: save calls the command with entered values then clears the fields (assert state/DOM empty after); remove confirms + calls clear; env toggle persists; test-connection renders result; nothing ever renders a stored secret. Live CDP: open Settings, see configured=true (real machine state), run Test connection against production → "connected"; screenshot. GREEN; commit `Add DigiKey settings section`.

---

### Task 7: Phase gate + docs + full-loop acceptance

**Files:** `docs/ui.md` (Orders & Imports + enrichment dialog + settings sections), `docs/imports.md` (UI walkthrough addendum), `docs/enrichment.md` (UI section: diff dialog, ack semantics, settings), `docs/known-limitations.md` (images display-only; OCR still deferred; sandbox-creds note).

- [ ] Full gate `powershell -File scripts\verify.ps1` → ALL CHECKS PASSED (fmt-fix commit first if needed; real failures stop and get routed). Docs accurate to code. Live acceptance (CDP, scratch dir): the COMPLETE loop — upload real invoice → review → assign bin → commit → stock visible → enrich the part live → apply with one ack'd field → reverse the import → verify restoration; screenshot each stage to scratchpad. Commit `Add phase 5d documentation and acceptance evidence`.

---

## Plan self-review notes

- **Spec coverage:** §9 Orders & Imports screen (T2-T4), §9 Settings credentials test/replace/remove + env (T1/T6), §10 upload→…→confirm pipeline UI incl. duplicate warn-not-block, shipped-not-ordered display, non-inventory marking, atomic commit + reversal (T2-T4), §11 re-run from part detail + compare-and-apply diff never auto-overwriting manual (T5, backed by the 5c-enforced ack). Deferred, documented: OCR UI, correct-matching-decision UI (post-commit line correction beyond reverse — History already reverses; full correction workflow noted in known-limitations), images as attachments, quantity_unit display gap (carryover).
- **5d plan inputs consumed:** per-field ack UI (T5), images surfaced (T1/T5), set-credentials command + masked UI (T1/T6), production-only guidance (T6 copy + T5 not-configured state).
- **Type consistency:** LineDecision/ImportReviewLine/AppliedField/FieldDiff/DigiKeyTestResult field names verified against `bindings.gen.ts` and 5c code; T1 is the only bindings-changing task and runs first so T5/T6 build against regenerated types.
- **Secrets:** the only new IPC carrying a secret is `set_digikey_credentials` (inbound-only); T1 test asserts no secret-shaped field in any response type; UI clears fields; no logging of args.
- **Live verification uses the real private invoice + real production creds already on this machine — screenshots stay in the scratchpad, fixtures stay sanitized, nothing PII enters the repo.**
