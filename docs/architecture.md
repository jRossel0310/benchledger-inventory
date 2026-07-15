# Architecture

See the spec for full detail. Summary of what exists after Phase 1:

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
  renders read-only state. No write paths exist.
- **Tokens** (`packages/shared`): primitive palette + semantic tokens emitted
  as CSS custom properties; stylelint forbids raw colors anywhere else.
- **Migrations**: numbered SQL embedded in `inventory-db`, applied in one
  transaction each, `PRAGMA user_version` tracks state, pre-migration safety
  backup via SQLite online backup API, newer-schema refusal.
- **Quantities**: exact fixed-point milli-units (`Quantity`, x1000).
