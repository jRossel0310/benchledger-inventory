# Decision log

| Date | Decision | Why |
|---|---|---|
| 2026-07-14 | Rust core owns all domain/DB logic; React is thin | Integrity at the DB; one implementation; cargo-testable (spec ADR 1) |
| 2026-07-14 | Quantities: fixed-point integer x1000 | Exact; continuous units supported (spec ADR 4) |
| 2026-07-14 | ULIDs for IDs | Stable, sortable, deterministic exports (spec ADR 5) |
| 2026-07-14 | `PRAGMA user_version` + embedded numbered SQL migrations | Minimal, transactional, testable |
| 2026-07-14 | Redaction at the log-writer layer | Secrets cannot reach disk regardless of call site |
| 2026-07-14 | Hand-written TS bindings for the single Phase 1 command | tauri-specta adopted in Phase 2 when the command surface grows (spec ADR 11) |
| 2026-07-14 | TanStack Router deferred to Phase 3 | Phase 1 has one screen; YAGNI |
| 2026-07-14 | rusqlite backup feature + validated serde deserialization for Quantity | Review findings during Tasks 4-5 (see .superpowers/sdd reports) |
| 2026-07-14 | Web app uses Vite appType 'mpa' | Preview SPA fallback masked missing-snapshot 404; forward risk documented in vite.config.ts |
