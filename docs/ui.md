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
| `/` | **Dashboard** | Summary cards (each links into a filtered inventory view), an aggregate stock-state gauge, a recent-activity feed with safe reversal, and a publish/backup status strip. Cards and gauge only render meaningfully with data — empty on a fresh DB. |
| `/inventory?q=` | **Inventory browser** | The primary dense, virtualized table. `q` is the single source of search state (top command bar, the screen's own box, filter chips, and saved views all read/write it). Row click / Enter opens the part-detail inspector drawer; hover row actions run quick-action flows. |
| `/inventory/new` | **Create part** | The category-adaptive part form (create mode). Static segment, matched ahead of `$partId`. |
| `/inventory/$partId` | **Part detail (full page)** | Deep-link / back-button-friendly standalone view. Shares the `PartDetail` body with the inspector drawer. |
| `/inventory/$partId/edit` | **Edit part** | The same category-adaptive form in edit mode. |
| `/bins` | **Bin browser** | Bins grouped by `bin_label` with part counts plus an "Unassigned" bucket; selecting a bin filters the inventory table to it; rename / reassign with a non-blocking occupancy warning. |
| `/history?groupId=` | **History** | The full, filtered ledger. Grouped transactions render under one expandable header; single/group reversal, restore-archived-part, and a Phase 5 "view original import" stub live here. Optional `groupId` deep-links to one group. |
| `/settings` | **Settings** | Placeholder: hosts the read-only Phase 1 application-status panel; real preferences arrive later. |
| `/projects` | **Projects (stub)** | Present rail item; the panel states what Phase 4 will add. Not a dead button. |
| `/orders` | **Orders (stub)** | Present rail item; the panel states what Phase 5 (imports) will add. The palette's "Import order" action routes here. |

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
