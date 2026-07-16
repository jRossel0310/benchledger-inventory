# Phase 3 UI — Design Direction

Grounding for the desktop workflow screens (Dashboard, Inventory, Quick actions, Part detail, Bin browser, History). The Phase 1 token system (dark graphite, non-pastel saturated accents, stock-state colors) is fixed; this doc sets layout, typography, and the signature device so the screens read as one intentional "bench instrument," not a generic dark SaaS dashboard.

## Thesis

The app is a **bench instrument for one engineer's parts library**, not a consumer app. Every screen answers, in this order: *what do I have, where is it, act on it in one keystroke.* The characteristic artifact is a component identified by its normalized spec (`10 kΩ · 0603 · 1% · ¼W`) and its physical location (`bin A12`) — so identifiers and quantities are the hero, and they are rendered like test-equipment readouts.

## Typography (system stacks only — the app is self-contained, no web fonts)

- **Instrument face (data):** monospace system stack — `ui-monospace, 'Cascadia Code', 'Cascadia Mono', 'SF Mono', Consolas, monospace`. Used for everything that is an identifier or measurement: part IDs, MPNs, supplier SKUs, bin labels, quantities, normalized spec values, timestamps. `font-variant-numeric: tabular-nums` so columns align. This is what makes the tables read like an instrument rather than a spreadsheet.
- **UI face (prose/labels):** the existing `'Segoe UI', system-ui, sans-serif` for section labels, buttons, descriptions, empty states.
- **Scale is dense:** table rows 30px, 13px data / 12px labels, generous letter-spacing on small caps eyebrow labels only. No oversized headings — a pro tool doesn't shout.

## Layout

- **Persistent left rail** (sections: Dashboard, Inventory, Bins, Projects, Orders, History, Settings) — narrow, icon+label, keyboard-navigable.
- **Top command bar** always showing the global search input and a hint that `Ctrl+K` opens the command palette. Search is never more than a glance away (spec requirement).
- **Primary surface is a dense data table** (Inventory) with hairline borders, zebra-free rows, inline row actions on hover, and virtualized rows for the 10k-part target.
- **Part detail is a right-hand inspector drawer** (like a properties panel in an EDA/CAD tool) that slides over the table — you inspect a part without losing your place in the list. Full-page part detail exists too (deep link), but the inspector is the fast path.
- **Ctrl+K command palette** is the keyboard-first spine: fuzzy over the quick actions (add stock, consume, reserve, check out, return, create part, import) and over parts/bins. This is the "one keystroke" promise made real.

## Signature device: the stock-state gauge

A compact horizontal **segmented bar** rendered per part, showing the available / reserved / checked-out split in the three stock-state token colors (green / violet / cyan), like a fuel gauge or a spectrum readout. It appears in the inventory table (a slim inline gauge), in the part-detail header (a larger labeled gauge), and aggregated on the dashboard. It encodes the exact thing this app is about — where each physical unit currently lives — as one glanceable instrument reading, and it ties the whole product together visually. Low-stock parts get an amber tick on the gauge's baseline.

Everything else stays quiet and disciplined so the gauge and the monospaced readouts carry the personality. One bold device, executed well; no decorative gradients, no card-bubble palette.

## Motion (minimal, reduced-motion respected)

- Command palette and inspector drawer: fast fade+slide (~120ms).
- Stock gauge animates its segment widths when a transaction changes stock (a receive visibly grows the green segment) — the one place motion tells a true story.
- Nothing else animates.

## Copy voice

Plain, active, instrument-like. Actions name their effect (`Add stock`, not `Submit`; the toast says `Received 10`). Empty states invite the next action (`No parts yet — press Ctrl+K to create one or import an order`). Errors state what happened, whether data is safe, and the fix (already the domain-error contract from Phase 2).

## Technical approach for the plan

- **TanStack Router** (multi-screen now; deferred from Phase 1) + **TanStack Query** wrapping the specta-generated `commands.*` from `apps/desktop/src/bindings.gen.ts` — every screen reads/writes through the typed commands, never ad-hoc invoke.
- **TanStack Virtual** for the inventory table at scale.
- **Radix primitives** (dialog, popover, dropdown, toast) styled entirely with the shared tokens — accessibility without imposed visual style.
- All color via `var(--color-*)`; stylelint already forbids raw colors. The two font stacks become two new shared tokens (`--font-data`, `--font-ui`).
- Verified by driving the real Tauri window (the `run`/`verify` loop), not only vitest — a UI phase must be seen working.
