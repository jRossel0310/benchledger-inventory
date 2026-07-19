# Desktop UI

The `apps/desktop` React app: the "bench instrument" workflow surface over the
Phase 2 typed command surface. Design direction:
`docs/superpowers/specs/2026-07-16-phase-3-ui-design-direction.md`. This doc is
the screen map, the full keyboard reference, the stock-gauge legend, how the UI
talks to the backend, and the `dev_seed` workflow.

## Shell

`AppShell` (`src/app/AppShell.tsx`) is the root route's component and wraps every
screen: a persistent left rail, a top command bar, and a routed `<Outlet/>`.

- **Left rail** — Dashboard, Inventory, Bins, Projects, Orders, History,
  Settings. Icon + label, active-route highlighting.
- **Top command bar** — one global search input (`Search parts, bins, MPNs…`)
  plus a `Ctrl K` affordance. Typing here navigates to `/inventory` and writes
  the route's `q` search param (debounced ~200ms, `replace` so refining a search
  doesn't spam history); clearing the box flushes immediately. While on
  `/inventory` the box reflects that route's current `q` back, so a Dashboard
  card link like `?q=low stock` shows up here too. Search is never more than a
  glance away.
- Mounted once in the shell so they work from any route: `<CommandPalette/>`
  (Ctrl+K), `<QuickActionProvider/>` (the shared quick-action dialog), and
  `<PartInspectorProvider/>` (the part-detail drawer).

## Screen map

Routes are code-based (`src/app/routes.tsx`), all children of the shell root.

| Route | Screen | Notes |
|---|---|---|
| `/` | **Dashboard** | Summary cards (each links into a filtered inventory view), an aggregate stock-state gauge, a recent-activity feed with safe reversal, and the publish/backup status strip (the publishing line is live since Phase 6 — see "Publish status card" below). Cards and gauge only render meaningfully with data — empty on a fresh DB. |
| `/inventory?q=` | **Inventory browser** | The primary dense, virtualized table. `q` is the single source of search state (top command bar, the screen's own box, filter chips, and saved views all read/write it). Row click / Enter opens the part-detail inspector drawer; hover row actions run quick-action flows. |
| `/inventory/new` | **Create part** | The category-adaptive part form (create mode). Static segment, matched ahead of `$partId`. |
| `/inventory/$partId` | **Part detail (full page)** | Deep-link / back-button-friendly standalone view. Shares the `PartDetail` body with the inspector drawer. |
| `/inventory/$partId/edit` | **Edit part** | The same category-adaptive form in edit mode. |
| `/bins` | **Bin browser** | Bins grouped by `bin_label` with part counts plus an "Unassigned" bucket; selecting a bin filters the inventory table to it; rename / reassign with a non-blocking occupancy warning. |
| `/history?groupId=` | **History** | The full, filtered ledger. Grouped transactions render under one expandable header; single/group reversal, restore-archived-part, and a Phase 5 "view original import" stub live here. Optional `groupId` deep-links to one group. |
| `/settings` | **Settings** | Read-only Phase 1 application-status panel, the DigiKey enrichment section (credentials, environment, test connection), and the Publishing section (repo config, token, test connection, publish now). See "Settings" below. |
| `/projects` | **Projects list** | Every project (`useProjectsFull`), filterable by lifecycle status; row click/Enter opens the project. |
| `/projects/$projectId` | **Project detail** | Header, lifecycle status control, editable build quantity, the BOM editor, and the reserve/build-from-BOM actions. See "Projects and BOMs" below. |
| `/orders` | **Orders & imports** | Every persisted import (`OrdersList`), newest first, with an always-visible upload dropzone above the table — never hidden behind a click-to-reveal toggle. Row click/Enter opens `/orders/$importId`. The palette's "Import order" action routes here. |
| `/orders/$importId` | **Import review** | Match → Review → Confirm for one import: order summary, duplicate-import warning, the per-line table, and the commit/reverse bar. See "Orders & Imports" below. |

### Part detail: inspector drawer vs. full page

`PartDetail` (`src/features/part/PartDetail.tsx`) is the shared body used by two
callers that differ only in chrome:

- **`PartInspector`** — a right-hand slide-over drawer (Radix `Dialog` styled as
  a properties panel) opened from an inventory row so you inspect a part without
  losing your place in the list. This is the fast path.
- **`PartDetailPage`** — the `/inventory/$partId` full-page route, reached via
  the drawer's "Open full page" link or a deep link.

Header: category, name, identity-attribute specs and bin (all `--font-data`), a
panel-size `StockGauge`, the four quantity figures, and primary actions (Add
stock / Consume / Reserve / Check out, each opening the shared quick-action
dialog). Below the header, Radix `Tabs` switch between eight sections —
**Overview, Specifications, Dimensions, Variants, Supplier listings,
Transactions, Attachments, Metadata** — each owning its own data fetch so an
unopened tab never fires a query.

## Projects and BOMs

`src/features/projects/` (Phase 4 Task 6/7, spec "Projects and BOMs"):
`/projects` lists every project and `/projects/$projectId` is the detail +
BOM editor + build flow, over `src/hooks/projects.ts`'s query/mutation hooks
(the same generated-`commands.*` pattern every other screen uses).

- **Projects list (`ProjectsList.tsx`)** — `useProjectsFull`, a `DataTable`
  with Name / Status (a `StatusChip`) / Build qty / BOM line count columns,
  behind an All/Planned/Active/Completed/Archived tab bar
  (`role="tablist"`). Row click or Enter opens `/projects/$projectId`. "New
  project" opens an inline (non-modal) create form — name, description,
  build quantity, repo/doc link, notes — and navigates straight to the
  created project.
- **Project detail (`ProjectDetail.tsx`)** — header with name/description, a
  `StatusChip`, a status `SelectField` (planned/active/completed/archived,
  any transition allowed; entering `completed` stamps `completed_at`,
  leaving it clears that timestamp), and an inline build-quantity
  `NumberField` that commits on blur — the BOM's Needed/Missing columns
  recompute server-side from the new `build_quantity` the moment the query
  invalidates, so nothing here redoes that math client-side. Repo/doc link,
  created/completed dates, and notes follow. "Edit fields" (name/
  description/repo link/notes as one batch), "Duplicate" (copies the project
  and its whole BOM structure onto a new `planned` project — no
  reservations or transactions carry over), and "Archive" are inline panels/
  actions rather than dialogs, so the BOM table stays visible underneath.
- **Status chips (`StatusChip.tsx`)** use dedicated `color-status-{planned,
  active,completed,archived}` tokens — deliberately distinct from the
  stock-state tokens (`color-stock-available`/`reserved`/`checked-out`/
  `low`), since a project's lifecycle status and a part's physical-stock
  split are unrelated axes that would be confusing to color the same way.
- **The BOM table (`BomTable.tsx`)** — the spec's seven columns, computed
  entirely server-side and rendered in `--font-data` mono: **Part**
  (display name + reference designators), **Per build**, **Needed** (total
  required = per build × build quantity), **Available**, **Reserved**,
  **Consumed**, and **Missing** (highlighted amber when positive). No
  inline `StockGauge` per row — the gauge's axis (available/reserved/
  checked-out, a part's *global* stock split) doesn't match a BOM line's
  axis (this *project's* draw against that stock plus how much of the
  build is done), so the seven columns cover the spec's requirement
  directly instead of overloading the gauge. "Add part" is a two-step
  dialog (search a part via the same `cmdk` search pattern the quick-action
  flows use, then fill in quantity per build / reference designators /
  required / notes); each row's `⋯` menu opens "Edit line" (also manages
  substitute parts) or "Remove"; row click/Enter opens the part inspector.
- **Reserve / release / build (on `ProjectDetail`)** — "Reserve available
  parts" reserves `min(needed, available)` for every required line as one
  atomic transaction group (partial reservation is the normal, expected
  outcome, not a failure) and toasts the exact line count reserved;
  "Release reservations" releases everything currently held for the
  project, also as one group. "Build from BOM" opens the **build review**
  (`BuildReview.tsx`), a dry run of `plan_build` shown before anything
  commits: every BOM line lists what will happen right now — "Consume
  reserved X" (unconditional), a checkbox "Draw Y from available" (the only
  gated control — checking it adds the line to `build_from_bom`'s approved
  list), "Check out Z" for reusable-equipment lines
  (`usage_behavior = usually_checked_out`), and "Short by M" plus an "Unmet
  required" badge for a line still short — flagged, never blocking Confirm
  ("build what can be built, flag the rest"). A summary strip totals lines/
  consuming-reserved/drawing-available/checkouts/unmet-required, and a
  footer note states the atomicity guarantee: "Committed as one transaction
  — reversible from History." Confirming calls `build_from_bom`; an
  approved line that turns out short fails the *whole* group (nothing
  partially applied) and the dialog stays open showing the error. A
  successful build auto-activates a `planned` project and appears in
  History as a group, reversible from there like any other (reversing it
  restores the BOM's reserved/consumed columns too).

## Orders & Imports

`src/features/orders/` (Phase 5d Tasks 2-4, spec §10's Upload -> Extract ->
Match -> Review -> Confirm pipeline): `/orders` lists every persisted import
and `/orders/$importId` is the review + commit/reverse screen, over
`src/hooks/imports.ts`'s query/mutation hooks — the same generated-
`commands.*` pattern every other screen uses. `docs/imports.md`'s "Using the
import UI" section walks the same flow end to end from the user's side.

- **Orders list (`OrdersList.tsx`)** — `useImports` (newest first,
  server-side), a `DataTable` with Imported / Supplier / Order # / Format /
  Lines / Total / Status (an `ImportStatusChip`) columns. `UploadImport` is
  always rendered above the table, never behind a toggle — an empty list is
  exactly the moment a first-time user needs the upload control most.
  Row click/Enter opens `/orders/$importId`.
- **Upload (`UploadImport.tsx`)** — a drag-drop zone plus a file picker for
  one PDF/CSV/XLSX at a time (a multi-file drop only uploads the first
  entry). Reads the file's bytes in the webview and calls
  `parse_and_store_import`; the backend only persists an `ImportRecord` once
  parsing actually succeeds, so a failed upload leaves nothing in the list.
  Success navigates straight to the new import's review screen.
- **Import review (`ImportReview.tsx`)** — header (supplier + order number,
  an `ImportStatusChip`), then:
  - **Duplicate warning** — a prominent but non-blocking alert when
    `duplicate_of` is non-empty ("this looks like an order already on
    file"), with links to the prior import(s). It never gates commit — only
    warns.
  - **Summary block** — the financial rows (subtotal/shipping/tax/tariff/
    total), a line count, how many lines will actually receive stock, and a
    backorder count (only shown when non-zero).
  - **Line table (`ReviewLineTable.tsx`)** — a plain, non-virtualized
    `<table>` (rows have variable height — a warning line, a two-line item
    cell — so `DataTable`'s fixed-row-height virtualization doesn't fit):
    line #, item identity, Ordered / **Shipped** (highlighted — this is the
    actual receive quantity, never ordered) / Backordered, unit price, the
    top match verdict, the current decision, target/draft bin, and a
    "Change…" trigger. Non-`part` lines (fee/tariff/no_charge/unknown)
    render greyed with a kind badge and no action editor — they never
    create inventory.
  - **Actions (`LineActionEditor.tsx`)** — a popover per part line: pick one
    of the backend's suggested matches directly, "Match other part…" or
    "Add as variant to…" (a `cmdk` search over `useSearch`), "Create new
    part" (opens the draft dialog below), or "Skip".
  - **Create-from-line dialog (`CreateFromLineDialog.tsx`)** — completes a
    `create_new` draft: part fields (name, category, description, quantity
    unit, usage behavior, bin, low-stock threshold, notes), the manufacturer
    variant (manufacturer, MPN), and the supplier listing (SKU, unit price,
    packaging). The bin field warns (not blocks) if the typed bin already
    holds parts — the same warn-not-block convention as elsewhere. Once
    display name and category are both set, the row's "Draft incomplete"
    flag clears to "Edit draft".
  - **Bin column** — the target part's *current* bin for `add_stock`/
    `add_as_variant`, the draft's own `bin_label` for `create_new` (editable
    only through the dialog above — one entry point, not two that could
    drift), an em dash for `skip` and non-part lines.
  - **Commit bar** — summary counts (receives / new parts / new variants /
    skipped / non-inventory) computed from the exact same `decisions` map
    sent to `commit_import`, so the numbers shown can never drift from what
    committing actually does. "Commit import" is disabled while any
    `create_new` draft is still incomplete. A successful commit toasts
    "Received N lines" and freezes every editor in the table (the import is
    no longer `parsed`).
  - **Reverse** — once `committed`, a "Reverse import" button (confirmed via
    a dialog mirroring `GroupRow.tsx`'s reverse-group confirmation) undoes
    the whole receive group as one transaction. Parts the commit created are
    never deleted — they stay on file at zero stock, same as any other
    reversal in this app.

### Enrichment diff dialog

`EnrichmentDiffDialog.tsx` opens from `PartDetail`'s "Refresh product data"
button (part-detail header actions) — the dialog is only ever mounted once
that button is clicked, so the DigiKey network call it triggers never fires
just because a part-detail screen happens to be open. It shows every
proposed field as a current -> proposed row with an `include` checkbox; a
protected row (the field's current source is `manual`, or a low-confidence
`inferred` candidate would overwrite an existing value) additionally needs
its own explicit confirmation checkbox — "Overwrite manually-set value" or
"Accept inferred over existing" — before it can be applied. See
`docs/enrichment.md`'s "UI: the diff dialog" section for the full
acknowledgement-rule/backend-enforcement pairing, select-all scope, and the
image strip.

## Settings

`/settings` (`SettingsPage.tsx`) hosts the Phase 1 read-only application-
status panel plus the DigiKey enrichment section (`DigiKeySettings.tsx`,
Phase 5d Task 6): status (configured / current environment), a credentials
form (Client ID + Client Secret, doubling as "Replace" once configured, with
a confirmed "Remove"), a sandbox/production environment toggle, and "Test
connection". See `docs/enrichment.md`'s "UI: Settings" section for exactly
what each control does and doesn't do (the credentials form is write-only —
nothing is ever displayed back, not even masked).

### Publishing (`PublishSettings.tsx`, Phase 6)

The only UI surface that ever collects the GitHub publish token. Five pieces,
in reading order:

- **Status card** — configured / repo / last published / pending / Vercel
  URL from `usePublishStatus()` (the `PublishStatus` type has no field that
  could carry the token). A pending-publish warning row carries its own
  **Retry** button (`useRetryPendingPublish`); either outcome refetches
  status so the warning clears or persists honestly.
- **Repository form** — owner + repo (required), branch and snapshot path
  (blank submits `""`; the backend stores the real defaults `main` /
  `apps/web/public/inventory.snapshot.json` — the defaults appear as
  placeholders only), optional Vercel URL (display-only convenience).
- **Token form** — `type="password"`, write-only (`set_github_token`
  returns nothing), cleared from local state the instant the save succeeds;
  nothing ever re-populates it. "Remove" confirms via dialog and is
  idempotent server-side. DOM-wide tests assert the token never appears in
  the rendered document, masked or otherwise.
- **Test connection** — one read-only probe; the result is one of the
  backend's fixed strings (see `docs/publishing.md`'s troubleshooting
  table), never a response body.
- **Publish now** — toasts "Published" / "Already up to date"; a failure
  toasts the error and refetches status (the backend has just set the
  pending marker, and the status card + Dashboard card show it).

### Publish status card (`PublishStatusCard.tsx`, Dashboard)

The publishing line of the Dashboard's "Publish & backup" panel. Honest
simplification (per the Phase 6 plan): the app has no change-tracking, so
"unpublished changes" cannot be detected — the only proof of up-to-dateness
is `publish_now` returning `unchanged`, which the card never runs. It shows
exactly what is known: "Publishing not configured" (+ Settings link),
"Configured — nothing published yet.", "Last published <ts>", or the one
warning-toned state "Publish pending — will retry on launch".

### Close-time publish dialog (`ClosePublishDialog.tsx`, in `AppShell`)

Closing the window publishes first (see `docs/publishing.md` for the full
flow): the Rust close guard prevents every close request and emits an event;
this dialog fetches a *fresh* publish status (never the query cache) —
unconfigured closes immediately with no dialog flash; otherwise a
non-dismissable "Publishing before close…" runs `publish_now`. Success or
"already up to date" exits; a typed failure or 20s timeout offers **Retry**
/ **Close anyway** with the honest copy "Publish failed — it will retry next
launch. Your local data is safe." (true on every path — the pending marker
is set server-side before the upload starts). A quiet startup retry
(`useStartupPublishRetry`, StrictMode-proof) picks the marker up next
launch; the Rust side force-exits if the frontend is wedged for 30s past
the first close request.

## The web companion app (`apps/web`, Phase 6)

A static, read-only SPA (React + Vite, no router library — hash routing in
`src/router.ts`) that renders the published `inventory.snapshot.json`. No
auth, no write API, no edit controls; a banner reads "Read-only inventory
snapshot — last published <timestamp>", and a missing snapshot renders the
"No snapshot published yet" empty state. Dark theme only this phase; all
colors come from the shared tokens, identifiers/quantities in `--font-data`.

| Route | View |
|---|---|
| `#/` | **Inventory** — dense table (part / category / key specs / available / reserved / checked-out / bin), a 3-segment stock bar, amber "Low" badge, quantity-unit-aware whole numbers, plus the search box. |
| `#/part/<id>` | **Part detail** — header + stock figures, About, Specifications (attribute display values), Dimensions, Variants (manufacturer/MPN/lifecycle/datasheet links), supplier part numbers (no prices — they are not in the snapshot), tags, project links; unknown id → not-found panel with a back link. |
| `#/bins` | **Bins** — per-bin contents plus an "Unassigned" bucket; every part cross-links. |
| `#/projects` | **Projects** — status chips, build quantity, associated parts. |

**Search** (`searchSnapshot.ts`) reuses the shared query grammar
(`@ei/shared`'s `parseQuery`/`parseWithKind` — the fixture-locked TS twins of
the Rust parsers) over the snapshot: free text (AND, case-insensitive, quotes
stripped), `bin:`/`category:` exact, numeric/range filters on
`available`/`reserved`/`checked_out`/`stock`, unit-normalized attribute
filters (`voltage:>=25V`, `capacitance:10nF..1uF` — `0.1uF` matches `100nF`),
dimension names, `has:datasheet`/`has:dimensions`, and `is:low`/`low stock`.
**Unsupported-filter honesty**: a filter the snapshot can't answer
(`project:`, `is:archived` — excluded by construction, unknown keys or
unparseable values) is *ignored* (it never restricts results) and surfaced as
a visible "unsupported filter" chip rather than silently dropped.

## Keyboard shortcuts

| Keys | Where | Effect |
|---|---|---|
| `Ctrl+K` / `Cmd+K` | Anywhere | Toggle the command palette. A global `keydown` listener in `CommandPalette` owns this; it works from any route. |
| `Esc` | Palette / inspector drawer / any quick-action or confirm dialog | Close it. All are Radix/cmdk dialogs, so Esc, focus-trap, and overlay dismissal come for free. |
| `↑` / `↓` | Command palette | Move between palette items (cmdk; `loop` wraps at the ends). |
| `Enter` | Command palette | Run the highlighted action / open the highlighted part / bin. |
| `Home` / `End` | Command palette | Jump to first / last item (cmdk). |
| `↑` / `↓` | Inventory table (and any `DataTable`) | Move the active row; the list auto-scrolls to keep it in view. |
| `Enter` | Inventory table | Activate the active row → open its part-detail inspector. |
| `Enter` | Quick-action dialog fields / new-project field | Submit the action / confirm the new project. |
| `Enter` or `,` | Tag input (part form) | Commit the typed tag. |
| `Backspace` | Tag input, when the box is empty | Remove the last tag. |
| `Esc` / native clear (`×`) | Top command-bar search (`type="search"`) | Clear the search (flushes immediately, not debounced). |

Accessibility floor: visible keyboard focus (`--color-focus-ring`), every action
reachable by keyboard, and `prefers-reduced-motion` disables the gauge and
drawer transitions (`StockGauge.css`, `PartInspector.css`, `DataTable.css`,
`Toast.css`).

## The stock-state gauge

The signature device (`src/components/StockGauge.tsx`): a compact horizontal
segmented bar showing a part's available / reserved / checked-out split, drawn
edge-to-edge as percentages of *current stock* (available + reserved +
checked-out). It appears as a slim inline gauge in the inventory table, a larger
labeled `panel` gauge in the part-detail header, and aggregated on the
Dashboard.

| Segment | Meaning | Token |
|---|---|---|
| **Green** | Available (unreserved, on hand) | `--color-stock-available` |
| **Violet** | Reserved against a project | `--color-stock-reserved` |
| **Cyan** | Checked out (left the shelf) | `--color-stock-checked-out` |
| **Amber tick** | Low-stock marker on the baseline — shown only when a `low_stock_threshold` is set and `available` is below it | `--color-stock-low` |

The `panel` size renders a per-segment legend ("5 available, 3 reserved, 1
checked out"); the `inline` size shows the available figure as a compact label.
An empty part reads "0 in stock". The bar is `role="img"` with an accessible
label spelling out all three states. All quantities are milli-units; the gauge
does exact integer arithmetic and only converts to a percentage for width.

## How the UI talks to the backend

The UI **only** calls the generated typed commands — never raw `invoke`:

```
React component
  → TanStack Query hook (src/hooks/inventory.ts: useParts, usePart, useStock,
      useInventorySearch, useHistory, useDashboardSummary, useApplyLedgerOp,
      useReverseTransaction, useReverseGroup, …)
  → commands.* (src/lib/commands.ts re-exports the generated `commands`)
  → bindings.gen.ts  (tauri-specta generated)
  → Tauri IPC → Rust commands.rs → Database
```

- `src/lib/commands.ts` wraps each call in `unwrap<T>(Result<T, CommandError>)`,
  which throws the `CommandError` on the error variant so TanStack Query sees a
  rejected promise. `CommandError { code, message }` is surfaced in the UI's own
  voice: the `message` is shown, the `code` selects a recovery hint
  (`errorHint`). A raw code string is never shown alone.
- Every mutation goes through a TanStack Query mutation that invalidates the
  affected query keys and fires a toast naming the effect (`Received 10`,
  `Reserved 5 for Bench PSU Rebuild`) — the design voice: actions name their
  effect, never "Submitted successfully".
- `bindings.gen.ts` is generated (`EXPORT_BINDINGS=1; cargo test -p
  electronics-inventory export_bindings`) and committed. **A drift-detecting
  test (`export_bindings`, run with the env unset in the phase gate) fails if
  the committed file and the current command surface disagree**, so the typed
  contract cannot silently rot. Adding a Rust command means: add it to
  `commands.rs` + `collect_commands!`, regenerate, and commit the bindings.

## The `dev_seed` workflow (debug-only)

`dev_seed` populates a scratch dev database with a representative dataset (~46
parts across the built-in categories, real stock via the ledger, two projects
with reservations/checkouts, some variants/listings/dimensions/low-stock
thresholds, one archived part, and one grouped receive) so the data-driven
screens have something to render. It goes through the same repos/ledger the real
commands use — it can never construct a state the app couldn't reach — and is
idempotent (no-ops once any part exists).

- **Debug-only.** The `dev_seed` Tauri command's real body is
  `#[cfg(debug_assertions)]`; a release build compiles a stub that returns an
  error. There is one `collect_commands!` list for every profile (the body is
  gated, not the command entry), so the drift test still covers it. It will
  never touch a user's production database.
- **No UI trigger** — it is invoked programmatically during dev/verification, not
  from a button.
- **Isolate the data dir.** Point `ELECTRONICS_INVENTORY_DATA_DIR` at a scratch
  directory (never `%APPDATA%`) before launching, so the seed can't touch a real
  library.

Typical live-verification loop (the technique Phase 3 Tasks 3–10 used):

1. `pnpm --filter @ei/desktop tauri build --debug --no-bundle` (a plain
   `tauri build` embeds `frontendDist` even for a debug build).
2. Launch `target/debug/electronics-inventory.exe` with
   `ELECTRONICS_INVENTORY_DATA_DIR=<scratch>` and
   `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333`.
3. Connect to the WebView2 over CDP (Playwright's `chromium.connectOverCDP`),
   seed by calling the command in the page —
   `window.__TAURI_INTERNALS__.invoke('dev_seed')` — then reload so the first
   render/query sees the fresh data.
4. Drive the real DOM and capture screenshots; clean up the scratch dir after.
