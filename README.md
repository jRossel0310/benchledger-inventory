# Electronics Inventory

Single-user personal electronics inventory: a native Windows desktop app (Tauri 2)
with a local SQLite source of truth, plus a public read-only snapshot site.

- Spec: `docs/superpowers/specs/2026-07-14-electronics-inventory-design.md`
- Architecture: `docs/architecture.md`
- Build prerequisites: `docs/build.md`
- Decisions: `docs/decisions.md`

## Layout

| Path | What |
|---|---|
| `apps/desktop` | Tauri 2 desktop application (React UI + `src-tauri` binary) |
| `apps/web` | Read-only snapshot site (Vercel) |
| `packages/shared` | Design tokens, snapshot types shared by both UIs |
| `crates/inventory-core` | Domain logic |
| `crates/inventory-db` | SQLite + migrations |
| `crates/inventory-import` | Invoice parsers (Phase 5) |
| `crates/inventory-sync` | Snapshot/backup/publish (Phases 6-7) |

## Verify everything

    powershell -File scripts\verify.ps1
