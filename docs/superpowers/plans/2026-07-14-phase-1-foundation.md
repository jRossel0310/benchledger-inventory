# Phase 1: Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stand up the monorepo (Cargo + pnpm workspaces), a building Tauri 2 desktop shell, a read-only web shell, SQLite with a versioned migration system, logging with redaction, shared design tokens, and working test frameworks on every layer.

**Architecture:** Rust core crates own all domain/DB logic; the Tauri app binds them as typed commands; React UIs are thin. `packages/shared` holds design tokens and snapshot types consumed by both apps. Spec: `docs/superpowers/specs/2026-07-14-electronics-inventory-design.md`.

**Tech Stack:** Tauri 2, Rust (stable MSVC), rusqlite (bundled SQLite), tracing, React 18 + TypeScript + Vite, TanStack Router/Query, Vitest, Playwright, pnpm workspaces, PowerShell scripts.

## Global Constraints

- Platform: Windows 11; scripts are PowerShell 5.1-compatible; the executor shell is PowerShell (`&&` is NOT available — chain with `;`).
- Production data lives in `%APPDATA%\ElectronicsInventory\`; dev/test data goes to a temp dir via the `ELECTRONICS_INVENTORY_DATA_DIR` env override. Nothing under the repo or `target/`/`dist/` ever stores user data.
- Quantities are fixed-point integers ×1000 (`Quantity`, milli-units); discrete units reject fractions; negatives are impossible by construction.
- IDs are ULID strings.
- All new SQLite tables use `STRICT`; connections set `journal_mode=WAL`, `foreign_keys=ON`, `busy_timeout=5000`.
- Database schema newer than the app ⇒ refuse write access with a typed error (`DbError::NewerSchema`).
- Secrets (tokens, Authorization headers) never appear in SQLite, logs, exports, or source — the logging layer redacts known token shapes.
- No hardcoded colors outside `packages/shared` token files (stylelint-enforced: `color-no-hex` + disallowed color functions in apps).
- Default theme: dark graphite, non-pastel, high-contrast (see spec §9 token names).
- No placeholder screens or dead buttons: UI sections appear only in the phase that makes them real. Phase 1 desktop UI = app shell + Dashboard status panel only.
- TypeScript everywhere on the frontend, `strict: true`.
- Rust: `cargo fmt` clean, `cargo clippy --workspace --all-targets -- -D warnings` clean.
- Node 22 is already installed. Rust and pnpm are NOT installed yet (Task 1 installs them).
- Package names: npm scope `@ei/*` (`@ei/shared`, `@ei/desktop`, `@ei/web`); crates `inventory-core`, `inventory-db`, `inventory-import`, `inventory-sync`; Tauri binary crate `electronics-inventory`.
- Commit after every task; messages in imperative mood.

---

### Task 1: Toolchain bootstrap (Rust, MSVC, pnpm)

**Files:**
- Create: `docs/build.md`

**Interfaces:**
- Consumes: nothing (first task).
- Produces: working `cargo`, `rustc`, `pnpm` on PATH; `docs/build.md` prerequisites doc. All later tasks assume these commands exist.

- [ ] **Step 1: Check what is already present**

Run:
```powershell
foreach ($c in 'cargo','rustc','pnpm','node') { $g = Get-Command $c -ErrorAction SilentlyContinue; if ($g) { Write-Output "$c => $($g.Source)" } else { Write-Output "$c => MISSING" } }
```
Expected: `node` present; `cargo`, `rustc`, `pnpm` MISSING (skip any install step below whose tool is already present).

- [ ] **Step 2: Check for MSVC C++ build tools (Rust's linker needs them)**

Run:
```powershell
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
if (Test-Path $vswhere) { & $vswhere -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -latest -property installationPath } else { Write-Output "vswhere missing => Build Tools not installed" }
```
Expected: an installation path (Build Tools present) or a message that they're missing.

- [ ] **Step 3: Install MSVC Build Tools if missing**

Run (takes 10–20 minutes; requires elevation — if winget fails with an elevation error, rerun in an elevated PowerShell):
```powershell
winget install --id Microsoft.VisualStudio.2022.BuildTools --override "--add Microsoft.VisualStudio.Workload.VCTools --includeRecommended --passive --norestart" --accept-source-agreements --accept-package-agreements
```
Expected: exit code 0. Re-run Step 2 to confirm an installation path appears.

- [ ] **Step 4: Install rustup (stable MSVC toolchain)**

Run:
```powershell
winget install --id Rustlang.Rustup --accept-source-agreements --accept-package-agreements
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"   # current session; new shells get it automatically
rustup default stable
cargo --version; rustc --version
```
Expected: `cargo 1.x` and `rustc 1.x (stable)` version lines print.

- [ ] **Step 5: Install pnpm (user-scope, no elevation)**

Run:
```powershell
npm install -g pnpm
pnpm --version
```
Expected: a pnpm 9+ version number.

- [ ] **Step 6: Verify WebView2 runtime (ships with Windows 11)**

Run:
```powershell
Get-ItemProperty 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}' -ErrorAction SilentlyContinue | Select-Object -ExpandProperty pv
```
Expected: a WebView2 version string (e.g. `1xx.x.xxxx.xx`). If empty, install with `winget install --id Microsoft.EdgeWebView2Runtime`.

- [ ] **Step 7: Write `docs/build.md`**

```markdown
# Build Prerequisites

Verified working toolchain for this repository (Windows 11):

| Tool | Install | Verify |
|---|---|---|
| Node.js 22+ | winget install OpenJS.NodeJS.LTS | `node --version` |
| pnpm 9+ | `npm install -g pnpm` | `pnpm --version` |
| Rust stable (MSVC) | winget install Rustlang.Rustup, then `rustup default stable` | `cargo --version` |
| MSVC Build Tools | winget install Microsoft.VisualStudio.2022.BuildTools with the VCTools workload | vswhere reports an installation path |
| WebView2 runtime | Preinstalled on Windows 11 | registry key `HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}` |

Build commands live in later sections as the workspace grows.
```

- [ ] **Step 8: Commit**

```powershell
git add docs/build.md; git commit -m "Document verified build prerequisites"
```

---

### Task 2: Monorepo workspaces and crate stubs

**Files:**
- Create: `Cargo.toml` (workspace root), `rust-toolchain.toml`, `pnpm-workspace.yaml`, `package.json` (root), `tsconfig.base.json`, `.editorconfig`, `.prettierrc.json`, `rustfmt.toml`
- Create: `crates/inventory-core/Cargo.toml`, `crates/inventory-core/src/lib.rs`
- Create: `crates/inventory-db/Cargo.toml`, `crates/inventory-db/src/lib.rs`
- Create: `crates/inventory-import/Cargo.toml`, `crates/inventory-import/src/lib.rs`
- Create: `crates/inventory-sync/Cargo.toml`, `crates/inventory-sync/src/lib.rs`

**Interfaces:**
- Consumes: toolchain from Task 1.
- Produces: `cargo test --workspace` and `pnpm install` both succeed; crate names `inventory-core/db/import/sync` referenced by all later tasks; root scripts `pnpm -r test`, `pnpm -r build`.

- [ ] **Step 1: Root Cargo workspace**

`Cargo.toml`:
```toml
[workspace]
resolver = "2"
members = [
    "crates/inventory-core",
    "crates/inventory-db",
    "crates/inventory-import",
    "crates/inventory-sync",
]

[workspace.package]
version = "0.1.0"
edition = "2021"

[workspace.dependencies]
thiserror = "2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
ulid = "1"
rusqlite = { version = "0.32", features = ["bundled"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
tracing-appender = "0.2"
regex = "1"
tempfile = "3"
```

`rust-toolchain.toml`:
```toml
[toolchain]
channel = "stable"
```

`rustfmt.toml`:
```toml
edition = "2021"
```

- [ ] **Step 2: Four crate stubs**

`crates/inventory-core/Cargo.toml`:
```toml
[package]
name = "inventory-core"
version.workspace = true
edition.workspace = true

[dependencies]
thiserror.workspace = true
serde.workspace = true
ulid.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
tracing-appender.workspace = true
regex.workspace = true

[dev-dependencies]
tempfile.workspace = true
```

`crates/inventory-core/src/lib.rs`:
```rust
//! Domain core: parts, quantities, units, ledger, matching. Grows per phase.
```

`crates/inventory-db/Cargo.toml`:
```toml
[package]
name = "inventory-db"
version.workspace = true
edition.workspace = true

[dependencies]
inventory-core = { path = "../inventory-core" }
rusqlite.workspace = true
thiserror.workspace = true
tracing.workspace = true

[dev-dependencies]
tempfile.workspace = true
```

`crates/inventory-db/src/lib.rs`:
```rust
//! SQLite integration and versioned migrations.
```

`crates/inventory-import/Cargo.toml`:
```toml
[package]
name = "inventory-import"
version.workspace = true
edition.workspace = true

[dependencies]
inventory-core = { path = "../inventory-core" }
thiserror.workspace = true
```

`crates/inventory-import/src/lib.rs`:
```rust
//! Supplier invoice parsers (DigiKey PDF/CSV/XLSX). Implemented in Phase 5.
```

`crates/inventory-sync/Cargo.toml`:
```toml
[package]
name = "inventory-sync"
version.workspace = true
edition.workspace = true

[dependencies]
inventory-core = { path = "../inventory-core" }
thiserror.workspace = true
serde.workspace = true
serde_json.workspace = true
```

`crates/inventory-sync/src/lib.rs`:
```rust
//! Snapshot export, GitHub publish/backup, restore. Implemented in Phases 6-7.
```

- [ ] **Step 3: Verify the Rust workspace builds and tests**

Run: `cargo test --workspace`
Expected: compiles; `running 0 tests` for each crate; exit 0.

- [ ] **Step 4: pnpm workspace + root configs**

`pnpm-workspace.yaml`:
```yaml
packages:
  - "apps/*"
  - "packages/*"
```

`package.json` (root):
```json
{
  "name": "electronics-inventory-monorepo",
  "private": true,
  "scripts": {
    "test": "pnpm -r test",
    "build": "pnpm -r build",
    "lint:css": "stylelint \"apps/**/src/**/*.css\" \"packages/**/src/**/*.css\" --allow-empty-input"
  },
  "devDependencies": {
    "prettier": "^3.3.0",
    "stylelint": "^16.8.0"
  }
}
```

`tsconfig.base.json`:
```json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "ESNext",
    "moduleResolution": "bundler",
    "strict": true,
    "noUncheckedIndexedAccess": true,
    "forceConsistentCasingInFileNames": true,
    "skipLibCheck": true,
    "esModuleInterop": true,
    "resolveJsonModule": true,
    "jsx": "react-jsx"
  }
}
```

`.editorconfig`:
```ini
root = true

[*]
charset = utf-8
end_of_line = lf
insert_final_newline = true
indent_style = space
indent_size = 2

[*.rs]
indent_size = 4
```

`.prettierrc.json`:
```json
{ "singleQuote": true, "trailingComma": "all", "printWidth": 100 }
```

- [ ] **Step 5: Verify pnpm workspace**

Run: `pnpm install`
Expected: lockfile `pnpm-lock.yaml` created, exit 0 (no packages yet besides root devDeps).

- [ ] **Step 6: Commit**

```powershell
git add -A; git commit -m "Scaffold Cargo and pnpm workspaces with crate stubs"
```

---

### Task 3: Shared design tokens package (`@ei/shared`)

**Files:**
- Create: `packages/shared/package.json`, `packages/shared/tsconfig.json`, `packages/shared/vitest.config.ts`
- Create: `packages/shared/src/tokens/palette.ts`, `packages/shared/src/tokens/semantic.ts`, `packages/shared/src/tokens/css.ts`, `packages/shared/src/index.ts`
- Create: `packages/shared/src/tokens/css.test.ts`
- Create: `.stylelintrc.json` (root)

**Interfaces:**
- Consumes: workspace from Task 2.
- Produces: `@ei/shared` exports `palette`, `themes: Record<'dark'|'light', SemanticTheme>`, `generateCssVariables(theme: ThemeName): string`, `SEMANTIC_TOKEN_NAMES: readonly string[]`, type `ThemeName = 'dark' | 'light'`. Desktop (Task 7) and web (Task 8) import these.

- [ ] **Step 1: Package scaffolding**

`packages/shared/package.json`:
```json
{
  "name": "@ei/shared",
  "version": "0.1.0",
  "private": true,
  "type": "module",
  "main": "src/index.ts",
  "types": "src/index.ts",
  "scripts": {
    "test": "vitest run",
    "build": "tsc --noEmit"
  },
  "devDependencies": {
    "typescript": "^5.5.0",
    "vitest": "^2.0.0"
  }
}
```

`packages/shared/tsconfig.json`:
```json
{ "extends": "../../tsconfig.base.json", "include": ["src"] }
```

`packages/shared/vitest.config.ts`:
```ts
import { defineConfig } from 'vitest/config';
export default defineConfig({ test: { environment: 'node' } });
```

- [ ] **Step 2: Write the failing test**

`packages/shared/src/tokens/css.test.ts`:
```ts
import { describe, expect, it } from 'vitest';
import { generateCssVariables, SEMANTIC_TOKEN_NAMES, themes } from '../index';

describe('design tokens', () => {
  it('defines every semantic token in both themes', () => {
    for (const theme of ['dark', 'light'] as const) {
      for (const name of SEMANTIC_TOKEN_NAMES) {
        expect(themes[theme][name], `${theme}/${name}`).toMatch(/^#[0-9a-f]{6}$/);
      }
    }
  });

  it('emits one CSS custom property per semantic token', () => {
    const css = generateCssVariables('dark');
    for (const name of SEMANTIC_TOKEN_NAMES) {
      expect(css).toContain(`--${name}:`);
    }
  });

  it('covers the token names the spec requires', () => {
    const required = [
      'color-bg-app', 'color-bg-panel', 'color-bg-elevated', 'color-border',
      'color-text-primary', 'color-text-secondary', 'color-text-muted',
      'color-action-primary', 'color-action-hover', 'color-focus-ring',
      'color-stock-available', 'color-stock-reserved', 'color-stock-checked-out',
      'color-stock-low', 'color-warning', 'color-error', 'color-success',
    ];
    for (const name of required) expect(SEMANTIC_TOKEN_NAMES).toContain(name);
  });
});
```

- [ ] **Step 3: Run test to verify it fails**

Run: `pnpm --filter @ei/shared install; pnpm --filter @ei/shared test`
Expected: FAIL — cannot resolve `../index` exports.

- [ ] **Step 4: Implement tokens**

`packages/shared/src/tokens/palette.ts` (primitive palette — the ONLY file in the repo allowed to contain raw colors):
```ts
/** Primitive palette. Dark graphite base, saturated non-pastel accents. */
export const palette = {
  graphite950: '#111214',
  graphite900: '#17181b',
  graphite850: '#1d1f23',
  graphite800: '#232529',
  graphite700: '#2e3138',
  graphite300: '#a6adbb',
  graphite200: '#c7ccd6',
  offWhite: '#eef0f4',
  paper: '#f4f5f7',
  paperPanel: '#ffffff',
  paperElevated: '#eceef2',
  ink900: '#181a1e',
  ink600: '#3f4450',
  ink400: '#666d7c',
  blue500: '#2f6fed',
  blue400: '#4f86f0',
  green500: '#1f9d55',
  amber500: '#e08a00',
  red500: '#d63333',
  violet500: '#7a5af8',
  cyan500: '#0e9db8',
} as const;
```

`packages/shared/src/tokens/semantic.ts`:
```ts
import { palette } from './palette';

export const SEMANTIC_TOKEN_NAMES = [
  'color-bg-app', 'color-bg-panel', 'color-bg-elevated', 'color-border',
  'color-text-primary', 'color-text-secondary', 'color-text-muted',
  'color-action-primary', 'color-action-hover', 'color-focus-ring',
  'color-stock-available', 'color-stock-reserved', 'color-stock-checked-out',
  'color-stock-low', 'color-warning', 'color-error', 'color-success',
] as const;

export type SemanticTokenName = (typeof SEMANTIC_TOKEN_NAMES)[number];
export type SemanticTheme = Record<SemanticTokenName, string>;
export type ThemeName = 'dark' | 'light';

export const themes: Record<ThemeName, SemanticTheme> = {
  dark: {
    'color-bg-app': palette.graphite950,
    'color-bg-panel': palette.graphite900,
    'color-bg-elevated': palette.graphite850,
    'color-border': palette.graphite700,
    'color-text-primary': palette.offWhite,
    'color-text-secondary': palette.graphite200,
    'color-text-muted': palette.graphite300,
    'color-action-primary': palette.blue500,
    'color-action-hover': palette.blue400,
    'color-focus-ring': palette.blue400,
    'color-stock-available': palette.green500,
    'color-stock-reserved': palette.violet500,
    'color-stock-checked-out': palette.cyan500,
    'color-stock-low': palette.amber500,
    'color-warning': palette.amber500,
    'color-error': palette.red500,
    'color-success': palette.green500,
  },
  light: {
    'color-bg-app': palette.paper,
    'color-bg-panel': palette.paperPanel,
    'color-bg-elevated': palette.paperElevated,
    'color-border': palette.graphite200,
    'color-text-primary': palette.ink900,
    'color-text-secondary': palette.ink600,
    'color-text-muted': palette.ink400,
    'color-action-primary': palette.blue500,
    'color-action-hover': palette.blue400,
    'color-focus-ring': palette.blue500,
    'color-stock-available': palette.green500,
    'color-stock-reserved': palette.violet500,
    'color-stock-checked-out': palette.cyan500,
    'color-stock-low': palette.amber500,
    'color-warning': palette.amber500,
    'color-error': palette.red500,
    'color-success': palette.green500,
  },
};
```

`packages/shared/src/tokens/css.ts`:
```ts
import { SEMANTIC_TOKEN_NAMES, themes, type ThemeName } from './semantic';

/** Emit `:root` CSS custom properties for a theme. Deterministic order. */
export function generateCssVariables(theme: ThemeName): string {
  const lines = SEMANTIC_TOKEN_NAMES.map((name) => `  --${name}: ${themes[theme][name]};`);
  return `:root {\n${lines.join('\n')}\n}\n`;
}
```

`packages/shared/src/index.ts`:
```ts
export { palette } from './tokens/palette';
export {
  SEMANTIC_TOKEN_NAMES,
  themes,
  type SemanticTheme,
  type SemanticTokenName,
  type ThemeName,
} from './tokens/semantic';
export { generateCssVariables } from './tokens/css';
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `pnpm --filter @ei/shared test`
Expected: 3 tests PASS.

- [ ] **Step 6: Stylelint guard (root)**

`.stylelintrc.json`:
```json
{
  "rules": {
    "color-no-hex": true,
    "function-disallowed-list": ["rgb", "rgba", "hsl", "hsla", "oklch", "color-mix"]
  }
}
```
Run: `pnpm install; pnpm lint:css`
Expected: exit 0 (no CSS files yet; `--allow-empty-input` covers that).

- [ ] **Step 7: Commit**

```powershell
git add -A; git commit -m "Add shared design token package with stylelint color guard"
```

---

### Task 4: Quantity and ID primitives in `inventory-core`

**Files:**
- Create: `crates/inventory-core/src/quantity.rs`, `crates/inventory-core/src/id.rs`
- Modify: `crates/inventory-core/src/lib.rs`

**Interfaces:**
- Consumes: crate stub from Task 2.
- Produces: `inventory_core::quantity::{Quantity, QuantityUnit, QuantityError}` with `Quantity::from_whole(i64) -> Result<Quantity, QuantityError>`, `Quantity::from_milli(i64, QuantityUnit) -> Result<Quantity, QuantityError>`, `as_milli() -> i64`, `checked_add/checked_sub -> Result<Quantity, QuantityError>`, `Quantity::ZERO`, `Quantity::SCALE = 1000`; `inventory_core::id::new_id() -> String` (ULID). Phase 2's ledger consumes these exact signatures.

- [ ] **Step 1: Write the failing tests**

Append to `crates/inventory-core/src/quantity.rs` (create file with tests referencing the not-yet-written types):
```rust
// (implementation goes above; tests below)
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_quantities_scale_by_1000() {
        assert_eq!(Quantity::from_whole(30).unwrap().as_milli(), 30_000);
    }

    #[test]
    fn negative_quantities_are_rejected() {
        assert_eq!(Quantity::from_whole(-1), Err(QuantityError::Negative));
        assert_eq!(
            Quantity::from_milli(-5, QuantityUnit::Meter),
            Err(QuantityError::Negative)
        );
    }

    #[test]
    fn discrete_units_reject_fractions() {
        assert_eq!(
            Quantity::from_milli(1500, QuantityUnit::Each),
            Err(QuantityError::FractionalDiscrete)
        );
        assert!(Quantity::from_milli(2000, QuantityUnit::Each).is_ok());
    }

    #[test]
    fn continuous_units_accept_fractions() {
        assert_eq!(Quantity::from_milli(1500, QuantityUnit::Meter).unwrap().as_milli(), 1500);
    }

    #[test]
    fn subtraction_cannot_go_negative() {
        let a = Quantity::from_whole(3).unwrap();
        let b = Quantity::from_whole(5).unwrap();
        assert_eq!(a.checked_sub(b), Err(QuantityError::Negative));
        assert_eq!(b.checked_sub(a).unwrap(), Quantity::from_whole(2).unwrap());
    }

    #[test]
    fn addition_detects_overflow() {
        let max = Quantity::from_milli(i64::MAX - (i64::MAX % 1000), QuantityUnit::Meter).unwrap();
        assert_eq!(max.checked_add(Quantity::from_whole(1).unwrap()), Err(QuantityError::Overflow));
    }
}
```

`crates/inventory-core/src/id.rs`:
```rust
/// Generate a new ULID string (26 chars, Crockford base32, lexicographically sortable).
pub fn new_id() -> String {
    ulid::Ulid::new().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_26_char_ulids_and_unique() {
        let a = new_id();
        let b = new_id();
        assert_eq!(a.len(), 26);
        assert_ne!(a, b);
        assert!(ulid::Ulid::from_string(&a).is_ok());
    }
}
```

`crates/inventory-core/src/lib.rs`:
```rust
//! Domain core: parts, quantities, units, ledger, matching. Grows per phase.
pub mod id;
pub mod quantity;
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p inventory-core`
Expected: compile error — `Quantity` not defined.

- [ ] **Step 3: Implement `Quantity`**

Top of `crates/inventory-core/src/quantity.rs` (above the tests):
```rust
//! Exact fixed-point quantities: milli-units (x1000). No floats, no negatives.

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Quantity(i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuantityUnit {
    Each,
    Meter,
    Foot,
}

impl QuantityUnit {
    pub fn is_discrete(self) -> bool {
        matches!(self, QuantityUnit::Each)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum QuantityError {
    #[error("quantity cannot be negative")]
    Negative,
    #[error("discrete quantities must be whole units")]
    FractionalDiscrete,
    #[error("quantity overflow")]
    Overflow,
}

impl Quantity {
    pub const ZERO: Quantity = Quantity(0);
    pub const SCALE: i64 = 1000;

    pub fn from_milli(milli: i64, unit: QuantityUnit) -> Result<Self, QuantityError> {
        if milli < 0 {
            return Err(QuantityError::Negative);
        }
        if unit.is_discrete() && milli % Self::SCALE != 0 {
            return Err(QuantityError::FractionalDiscrete);
        }
        Ok(Quantity(milli))
    }

    pub fn from_whole(units: i64) -> Result<Self, QuantityError> {
        if units < 0 {
            return Err(QuantityError::Negative);
        }
        units
            .checked_mul(Self::SCALE)
            .map(Quantity)
            .ok_or(QuantityError::Overflow)
    }

    pub fn as_milli(self) -> i64 {
        self.0
    }

    pub fn checked_add(self, other: Quantity) -> Result<Quantity, QuantityError> {
        self.0.checked_add(other.0).map(Quantity).ok_or(QuantityError::Overflow)
    }

    pub fn checked_sub(self, other: Quantity) -> Result<Quantity, QuantityError> {
        let v = self.0 - other.0;
        if v < 0 {
            Err(QuantityError::Negative)
        } else {
            Ok(Quantity(v))
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p inventory-core`
Expected: 7 tests PASS.

- [ ] **Step 5: Commit**

```powershell
git add crates; git commit -m "Add exact fixed-point Quantity and ULID id primitives"
```

---

### Task 5: SQLite integration and versioned migrations (`inventory-db`)

**Files:**
- Create: `crates/inventory-db/src/database.rs`, `crates/inventory-db/migrations/0001_create_settings.sql`
- Modify: `crates/inventory-db/src/lib.rs`
- Test: `crates/inventory-db/tests/migrations.rs`

**Interfaces:**
- Consumes: crate stub (Task 2).
- Produces: `inventory_db::{Database, DbError, SUPPORTED_SCHEMA_VERSION}` with `Database::open_and_migrate(db_path: &Path, backup_dir: &Path) -> Result<Database, DbError>`, `Database::schema_version(&self) -> Result<u32, DbError>`, `Database::conn(&self) -> &rusqlite::Connection`. Task 7's app state and all Phase 2 repositories consume these.

- [ ] **Step 1: Write the failing integration tests**

`crates/inventory-db/tests/migrations.rs`:
```rust
use inventory_db::{Database, DbError, SUPPORTED_SCHEMA_VERSION};

fn temp_dirs() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("inventory.sqlite");
    let backups = dir.path().join("local-backups");
    std::fs::create_dir_all(&backups).unwrap();
    (dir, db, backups)
}

#[test]
fn fresh_database_migrates_to_latest() {
    let (_g, db_path, backups) = temp_dirs();
    let db = Database::open_and_migrate(&db_path, &backups).unwrap();
    assert_eq!(db.schema_version().unwrap(), SUPPORTED_SCHEMA_VERSION);
    // settings table exists and is usable
    db.conn()
        .execute("INSERT INTO settings (key, value) VALUES ('theme', 'dark')", [])
        .unwrap();
    let v: String = db
        .conn()
        .query_row("SELECT value FROM settings WHERE key = 'theme'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(v, "dark");
}

#[test]
fn reopening_is_idempotent() {
    let (_g, db_path, backups) = temp_dirs();
    drop(Database::open_and_migrate(&db_path, &backups).unwrap());
    let db = Database::open_and_migrate(&db_path, &backups).unwrap();
    assert_eq!(db.schema_version().unwrap(), SUPPORTED_SCHEMA_VERSION);
    let count: i64 = db
        .conn()
        .query_row("SELECT COUNT(*) FROM schema_migrations", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, SUPPORTED_SCHEMA_VERSION as i64);
}

#[test]
fn required_pragmas_are_active() {
    let (_g, db_path, backups) = temp_dirs();
    let db = Database::open_and_migrate(&db_path, &backups).unwrap();
    let journal: String = db
        .conn()
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(journal.to_lowercase(), "wal");
    let fk: i64 = db.conn().query_row("PRAGMA foreign_keys", [], |r| r.get(0)).unwrap();
    assert_eq!(fk, 1);
}

#[test]
fn newer_schema_is_refused() {
    let (_g, db_path, backups) = temp_dirs();
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.pragma_update(None, "user_version", 999).unwrap();
    }
    let err = Database::open_and_migrate(&db_path, &backups).unwrap_err();
    match err {
        DbError::NewerSchema { found, supported } => {
            assert_eq!(found, 999);
            assert_eq!(supported, SUPPORTED_SCHEMA_VERSION);
        }
        other => panic!("expected NewerSchema, got {other:?}"),
    }
}

#[test]
fn existing_file_gets_pre_migration_backup() {
    let (_g, db_path, backups) = temp_dirs();
    {
        // simulate an existing (old, un-migrated) database file
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch("CREATE TABLE legacy (x INTEGER)").unwrap();
    }
    drop(Database::open_and_migrate(&db_path, &backups).unwrap());
    let backup_files: Vec<_> = std::fs::read_dir(&backups).unwrap().collect();
    assert_eq!(backup_files.len(), 1, "expected exactly one pre-migration backup");
}

#[test]
fn fresh_database_creates_no_backup() {
    let (_g, db_path, backups) = temp_dirs();
    drop(Database::open_and_migrate(&db_path, &backups).unwrap());
    assert_eq!(std::fs::read_dir(&backups).unwrap().count(), 0);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p inventory-db`
Expected: compile error — `Database` not defined.

- [ ] **Step 3: Implement migration SQL and `Database`**

`crates/inventory-db/migrations/0001_create_settings.sql`:
```sql
CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) STRICT;
```

`crates/inventory-db/src/database.rs`:
```rust
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;

/// Highest schema version this build of the application understands.
pub const SUPPORTED_SCHEMA_VERSION: u32 = 1;

/// Ordered embedded migrations: (version, name, sql).
const MIGRATIONS: &[(u32, &str, &str)] = &[(
    1,
    "create_settings",
    include_str!("../migrations/0001_create_settings.sql"),
)];

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("database schema v{found} is newer than this app supports (v{supported}); refusing write access")]
    NewerSchema { found: u32, supported: u32 },
    #[error("migration {version} ({name}) failed")]
    Migration {
        version: u32,
        name: String,
        #[source]
        source: rusqlite::Error,
    },
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub struct Database {
    conn: Connection,
}

impl Database {
    /// Open the database, apply pragmas, and run any pending migrations.
    /// If the file already existed and migrations are pending, a safety copy
    /// is written into `backup_dir` first (via SQLite's online backup API).
    pub fn open_and_migrate(db_path: &Path, backup_dir: &Path) -> Result<Self, DbError> {
        let existed = db_path.exists();
        let conn = Connection::open(db_path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_millis(5000))?;

        let current = schema_version_of(&conn)?;
        if current > SUPPORTED_SCHEMA_VERSION {
            return Err(DbError::NewerSchema {
                found: current,
                supported: SUPPORTED_SCHEMA_VERSION,
            });
        }

        let pending: Vec<_> = MIGRATIONS.iter().filter(|(v, _, _)| *v > current).collect();
        if !pending.is_empty() {
            if existed {
                write_safety_backup(&conn, backup_dir, current)?;
            }
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS schema_migrations (
                    version    INTEGER PRIMARY KEY,
                    name       TEXT NOT NULL,
                    applied_at TEXT NOT NULL
                 ) STRICT",
            )?;
            for (version, name, sql) in pending {
                apply_migration(&conn, *version, name, sql)?;
            }
        }

        Ok(Database { conn })
    }

    pub fn schema_version(&self) -> Result<u32, DbError> {
        schema_version_of(&self.conn)
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }
}

fn schema_version_of(conn: &Connection) -> Result<u32, DbError> {
    let v: u32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    Ok(v)
}

fn apply_migration(conn: &Connection, version: u32, name: &str, sql: &str) -> Result<(), DbError> {
    let wrap = |source| DbError::Migration { version, name: name.to_string(), source };
    conn.execute_batch("BEGIN").map_err(wrap)?;
    let result = (|| {
        conn.execute_batch(sql)?;
        conn.execute(
            "INSERT INTO schema_migrations (version, name, applied_at)
             VALUES (?1, ?2, datetime('now'))",
            rusqlite::params![version, name],
        )?;
        conn.pragma_update(None, "user_version", version)?;
        Ok(())
    })();
    match result {
        Ok(()) => conn.execute_batch("COMMIT").map_err(wrap),
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(wrap(e))
        }
    }
}

fn write_safety_backup(conn: &Connection, backup_dir: &Path, from_version: u32) -> Result<(), DbError> {
    std::fs::create_dir_all(backup_dir)?;
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let dest_path = backup_dir.join(format!("pre-migration-v{from_version}-{stamp}.sqlite"));
    let mut dest = Connection::open(&dest_path)?;
    let backup = rusqlite::backup::Backup::new(conn, &mut dest)?;
    backup.run_to_completion(64, std::time::Duration::from_millis(50), None)?;
    Ok(())
}
```

`crates/inventory-db/src/lib.rs`:
```rust
//! SQLite integration and versioned migrations.
mod database;
pub use database::{Database, DbError, SUPPORTED_SCHEMA_VERSION};
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p inventory-db`
Expected: 6 tests PASS.

- [ ] **Step 5: Commit**

```powershell
git add crates; git commit -m "Add SQLite database with versioned migrations and safety backups"
```

---

### Task 6: Logging with secret redaction (`inventory-core::logging`) and data-dir resolution (`inventory-core::paths`)

**Files:**
- Create: `crates/inventory-core/src/logging.rs`, `crates/inventory-core/src/paths.rs`
- Modify: `crates/inventory-core/src/lib.rs`

**Interfaces:**
- Consumes: crate from Task 4.
- Produces: `inventory_core::logging::{redact(&str) -> String, init(log_dir: &Path) -> std::io::Result<tracing_appender::non_blocking::WorkerGuard>}`; `inventory_core::paths::{resolve_data_dir(env_override: Option<&str>, appdata: Option<&str>) -> Result<PathBuf, PathsError>, ensure_layout(&Path) -> std::io::Result<DataLayout>}` where `DataLayout { root, attachments, cache, logs, pending_sync, local_backups: PathBuf }`. Task 7 wires these into app startup.

- [ ] **Step 1: Write the failing tests**

Tests inside `crates/inventory-core/src/logging.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_github_classic_tokens() {
        let msg = "publish failed for token ghp_abcdefghijklmnopqrstuvwxyz012345 on repo";
        let out = redact(msg);
        assert!(!out.contains("ghp_abcdefghijklmnopqrstuvwxyz012345"));
        assert!(out.contains("[REDACTED]"));
    }

    #[test]
    fn redacts_fine_grained_tokens_and_bearer_headers() {
        let out = redact("Authorization: Bearer github_pat_11ABCDEFG0123456789_abcdefghij");
        assert!(!out.contains("github_pat_"));
        assert!(out.contains("[REDACTED]"));
    }

    #[test]
    fn redacts_client_secrets() {
        let out = redact("request client_secret=SuP3rS3cretValue123 sent");
        assert!(!out.contains("SuP3rS3cretValue123"));
    }

    #[test]
    fn leaves_normal_text_alone() {
        assert_eq!(redact("received 30 x 10k resistor"), "received 30 x 10k resistor");
    }

    #[test]
    fn log_file_is_written_with_redaction_applied() {
        let dir = tempfile::tempdir().unwrap();
        {
            let (writer, guard) = file_writer(dir.path()).unwrap();
            let subscriber = tracing_subscriber::fmt()
                .with_writer(writer)
                .with_ansi(false)
                .finish();
            tracing::subscriber::with_default(subscriber, || {
                tracing::info!("startup with ghp_abcdefghijklmnopqrstuvwxyz012345");
            });
            drop(guard); // flush
        }
        let entries: Vec<_> = std::fs::read_dir(dir.path()).unwrap().flatten().collect();
        assert!(!entries.is_empty(), "expected a log file");
        let content = std::fs::read_to_string(entries[0].path()).unwrap();
        assert!(content.contains("[REDACTED]"));
        assert!(!content.contains("ghp_abcdefghijklmnopqrstuvwxyz012345"));
    }
}
```

Tests inside `crates/inventory-core/src/paths.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_override_wins() {
        let dir = resolve_data_dir(Some("C:\\custom\\data"), Some("C:\\Users\\x\\AppData\\Roaming")).unwrap();
        assert_eq!(dir, std::path::PathBuf::from("C:\\custom\\data"));
    }

    #[test]
    fn defaults_to_appdata_subfolder() {
        let dir = resolve_data_dir(None, Some("C:\\Users\\x\\AppData\\Roaming")).unwrap();
        assert_eq!(dir, std::path::PathBuf::from("C:\\Users\\x\\AppData\\Roaming\\ElectronicsInventory"));
    }

    #[test]
    fn missing_both_is_an_error() {
        assert!(resolve_data_dir(None, None).is_err());
    }

    #[test]
    fn ensure_layout_creates_all_subdirs() {
        let dir = tempfile::tempdir().unwrap();
        let layout = ensure_layout(dir.path()).unwrap();
        for p in [&layout.attachments, &layout.cache, &layout.logs, &layout.pending_sync, &layout.local_backups] {
            assert!(p.is_dir(), "{p:?} should exist");
        }
        assert_eq!(layout.root, dir.path());
    }
}
```

Update `crates/inventory-core/src/lib.rs`:
```rust
//! Domain core: parts, quantities, units, ledger, matching. Grows per phase.
pub mod id;
pub mod logging;
pub mod paths;
pub mod quantity;
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p inventory-core`
Expected: compile error — `redact` / `resolve_data_dir` not defined.

- [ ] **Step 3: Implement logging and paths**

Top of `crates/inventory-core/src/logging.rs`:
```rust
//! Rotating file logging with secret redaction. Secrets must never reach disk.

use std::io::Write;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};

static SECRET_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        Regex::new(r"ghp_[A-Za-z0-9]{20,}").unwrap(),
        Regex::new(r"github_pat_[A-Za-z0-9_]{20,}").unwrap(),
        Regex::new(r"(?i)bearer\s+[^\s]+").unwrap(),
        Regex::new(r"(?i)(client_secret|api_key|token|password)\s*[=:]\s*[^\s]+").unwrap(),
    ]
});

/// Replace anything that looks like a credential with `[REDACTED]`.
pub fn redact(input: &str) -> String {
    let mut out = input.to_string();
    for re in SECRET_PATTERNS.iter() {
        out = re.replace_all(&out, "[REDACTED]").into_owned();
    }
    out
}

struct RedactingWriter<W: Write>(W);

impl<W: Write> Write for RedactingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let text = String::from_utf8_lossy(buf);
        self.0.write_all(redact(&text).as_bytes())?;
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}

/// Build a non-blocking, daily-rolling, redacting writer for `log_dir`.
pub fn file_writer(log_dir: &Path) -> std::io::Result<(NonBlocking, WorkerGuard)> {
    std::fs::create_dir_all(log_dir)?;
    let appender = tracing_appender::rolling::daily(log_dir, "app.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(RedactingWriter(appender));
    Ok((non_blocking, guard))
}

/// Install the global subscriber writing to `log_dir`. Keep the guard alive
/// for the life of the process or buffered lines are lost.
pub fn init(log_dir: &Path) -> std::io::Result<WorkerGuard> {
    let (writer, guard) = file_writer(log_dir)?;
    let subscriber = tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .finish();
    // Ignore the error if a subscriber is already set (e.g. tests).
    let _ = tracing::subscriber::set_global_default(subscriber);
    Ok(guard)
}
```

Note: `RedactingWriter` wraps the *rolling appender itself* (not the non-blocking fan-out), so every line is redacted regardless of buffering. `tracing_subscriber::fmt().with_env_filter(...)` requires the `env-filter` feature, already enabled in the workspace dependency.

Top of `crates/inventory-core/src/paths.rs`:
```rust
//! Application data directory resolution and layout. Pure functions: callers
//! pass in environment values so this stays unit-testable.

use std::path::{Path, PathBuf};

pub const APP_DIR_NAME: &str = "ElectronicsInventory";
pub const ENV_OVERRIDE: &str = "ELECTRONICS_INVENTORY_DATA_DIR";

#[derive(Debug, thiserror::Error)]
pub enum PathsError {
    #[error("no data directory available: set {ENV_OVERRIDE} or ensure %APPDATA% exists")]
    NoDataDir,
}

/// Resolve the data directory: explicit override beats %APPDATA% default.
pub fn resolve_data_dir(
    env_override: Option<&str>,
    appdata: Option<&str>,
) -> Result<PathBuf, PathsError> {
    if let Some(over) = env_override.filter(|s| !s.trim().is_empty()) {
        return Ok(PathBuf::from(over));
    }
    if let Some(base) = appdata.filter(|s| !s.trim().is_empty()) {
        return Ok(Path::new(base).join(APP_DIR_NAME));
    }
    Err(PathsError::NoDataDir)
}

#[derive(Debug, Clone)]
pub struct DataLayout {
    pub root: PathBuf,
    pub attachments: PathBuf,
    pub cache: PathBuf,
    pub logs: PathBuf,
    pub pending_sync: PathBuf,
    pub local_backups: PathBuf,
}

/// Create the standard subdirectory layout beneath `root` (idempotent).
pub fn ensure_layout(root: &Path) -> std::io::Result<DataLayout> {
    let layout = DataLayout {
        root: root.to_path_buf(),
        attachments: root.join("attachments"),
        cache: root.join("cache"),
        logs: root.join("logs"),
        pending_sync: root.join("pending-sync"),
        local_backups: root.join("local-backups"),
    };
    for dir in [
        &layout.root,
        &layout.attachments,
        &layout.cache,
        &layout.logs,
        &layout.pending_sync,
        &layout.local_backups,
    ] {
        std::fs::create_dir_all(dir)?;
    }
    Ok(layout)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p inventory-core`
Expected: all tests PASS (7 from Task 4 + 9 new).

- [ ] **Step 5: Commit**

```powershell
git add crates; git commit -m "Add redacting file logging and data-directory resolution"
```

---

### Task 7: Tauri 2 desktop shell (`apps/desktop`)

**Files:**
- Create: `apps/desktop/package.json`, `apps/desktop/tsconfig.json`, `apps/desktop/vite.config.ts`, `apps/desktop/vitest.config.ts`, `apps/desktop/index.html`
- Create: `apps/desktop/src/main.tsx`, `apps/desktop/src/App.tsx`, `apps/desktop/src/theme.css.ts`, `apps/desktop/src/shell.css`, `apps/desktop/src/features/dashboard/StatusPanel.tsx`, `apps/desktop/src/features/dashboard/StatusPanel.test.tsx`, `apps/desktop/src/bindings.ts`
- Create: `apps/desktop/src-tauri/Cargo.toml`, `apps/desktop/src-tauri/build.rs`, `apps/desktop/src-tauri/tauri.conf.json`, `apps/desktop/src-tauri/src/main.rs`, `apps/desktop/src-tauri/src/app.rs`, `apps/desktop/src-tauri/icons/` (generated)
- Modify: `Cargo.toml` (root — add `apps/desktop/src-tauri` to members)

**Interfaces:**
- Consumes: `@ei/shared` tokens (Task 3), `inventory_db::Database` (Task 5), `inventory_core::{logging, paths}` (Task 6).
- Produces: a launchable native window titled "Electronics Inventory"; Tauri command `app_status() -> AppStatus { appVersion: string; schemaVersion: number; dataDir: string }` exposed to TS via `apps/desktop/src/bindings.ts` (`export async function appStatus(): Promise<AppStatus>`); Rust `app::AppInit::initialize(env_override, appdata) -> Result<AppInit, InitError>` reused by recovery mode in Phase 7.

- [ ] **Step 1: Frontend package scaffolding**

`apps/desktop/package.json`:
```json
{
  "name": "@ei/desktop",
  "version": "0.1.0",
  "private": true,
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "tsc --noEmit && vite build",
    "test": "vitest run",
    "tauri": "tauri"
  },
  "dependencies": {
    "@ei/shared": "workspace:*",
    "@tanstack/react-query": "^5.51.0",
    "@tauri-apps/api": "^2.0.0",
    "react": "^18.3.1",
    "react-dom": "^18.3.1"
  },
  "devDependencies": {
    "@tauri-apps/cli": "^2.0.0",
    "@testing-library/react": "^16.0.0",
    "@types/react": "^18.3.0",
    "@types/react-dom": "^18.3.0",
    "@vitejs/plugin-react": "^4.3.0",
    "jsdom": "^25.0.0",
    "typescript": "^5.5.0",
    "vite": "^6.0.0",
    "vitest": "^2.0.0"
  }
}
```
(TanStack Router is introduced in Phase 3 when there is more than one screen; Phase 1 has a single Dashboard view — YAGNI.)

`apps/desktop/tsconfig.json`:
```json
{ "extends": "../../tsconfig.base.json", "include": ["src"], "compilerOptions": { "types": ["vite/client"] } }
```

`apps/desktop/vite.config.ts`:
```ts
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 5173, strictPort: true },
});
```

`apps/desktop/vitest.config.ts`:
```ts
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vitest/config';

export default defineConfig({
  plugins: [react()],
  test: { environment: 'jsdom' },
});
```

`apps/desktop/index.html`:
```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>Electronics Inventory</title>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
```

Run: `pnpm install`
Expected: dependencies resolve, exit 0.

- [ ] **Step 2: Write the failing frontend test**

`apps/desktop/src/features/dashboard/StatusPanel.test.tsx`:
```tsx
import { render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue({
    appVersion: '0.1.0',
    schemaVersion: 1,
    dataDir: 'C:\\Users\\x\\AppData\\Roaming\\ElectronicsInventory',
  }),
}));

import { StatusPanel } from './StatusPanel';

describe('StatusPanel', () => {
  it('renders app status returned by the app_status command', async () => {
    render(<StatusPanel />);
    await waitFor(() => {
      expect(screen.getByText(/0\.1\.0/)).toBeTruthy();
      expect(screen.getByText(/schema v1/i)).toBeTruthy();
      expect(screen.getByText(/ElectronicsInventory/)).toBeTruthy();
    });
  });
});
```

Run: `pnpm --filter @ei/desktop test`
Expected: FAIL — `./StatusPanel` does not exist.

- [ ] **Step 3: Implement bindings, theme injection, shell, and StatusPanel**

`apps/desktop/src/bindings.ts` (hand-written for the single Phase 1 command; replaced by specta-generated bindings in Phase 2 when the command surface grows — if `tauri-specta@2.0.0-rc` resolves cleanly at that point, per spec ADR #11):
```ts
import { invoke } from '@tauri-apps/api/core';

export interface AppStatus {
  appVersion: string;
  schemaVersion: number;
  dataDir: string;
}

export async function appStatus(): Promise<AppStatus> {
  return invoke<AppStatus>('app_status');
}
```

`apps/desktop/src/theme.css.ts`:
```ts
import { generateCssVariables, type ThemeName } from '@ei/shared';

/** Inject semantic token CSS variables into the document (idempotent). */
export function applyTheme(theme: ThemeName): void {
  const id = 'ei-theme-vars';
  let el = document.getElementById(id) as HTMLStyleElement | null;
  if (!el) {
    el = document.createElement('style');
    el.id = id;
    document.head.appendChild(el);
  }
  el.textContent = generateCssVariables(theme);
}
```

`apps/desktop/src/shell.css`:
```css
* {
  box-sizing: border-box;
}
body {
  margin: 0;
  background: var(--color-bg-app);
  color: var(--color-text-primary);
  font-family: 'Segoe UI', system-ui, sans-serif;
  font-size: 14px;
}
.shell {
  display: grid;
  grid-template-columns: 220px 1fr;
  height: 100vh;
}
.sidebar {
  background: var(--color-bg-panel);
  border-right: 1px solid var(--color-border);
  padding: 12px;
}
.sidebar h1 {
  font-size: 15px;
  margin: 4px 8px 16px;
}
.nav-item {
  display: block;
  padding: 8px 10px;
  border-radius: 4px;
  color: var(--color-text-secondary);
  background: var(--color-bg-elevated);
}
.content {
  padding: 20px;
  overflow: auto;
}
.panel {
  background: var(--color-bg-panel);
  border: 1px solid var(--color-border);
  border-radius: 6px;
  padding: 16px;
  max-width: 560px;
}
.panel dt {
  color: var(--color-text-muted);
  font-size: 12px;
  margin-top: 10px;
}
.panel dd {
  margin: 2px 0 0;
}
```

`apps/desktop/src/features/dashboard/StatusPanel.tsx`:
```tsx
import { useEffect, useState } from 'react';
import { appStatus, type AppStatus } from '../../bindings';

export function StatusPanel() {
  const [status, setStatus] = useState<AppStatus | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    appStatus().then(setStatus, (e: unknown) => setError(String(e)));
  }, []);

  if (error) {
    return <div className="panel">Could not load application status: {error}</div>;
  }
  if (!status) {
    return <div className="panel">Loading…</div>;
  }
  return (
    <dl className="panel">
      <dt>Application version</dt>
      <dd>{status.appVersion}</dd>
      <dt>Database</dt>
      <dd>schema v{status.schemaVersion}</dd>
      <dt>Data directory</dt>
      <dd>{status.dataDir}</dd>
    </dl>
  );
}
```

`apps/desktop/src/App.tsx`:
```tsx
import { StatusPanel } from './features/dashboard/StatusPanel';

export function App() {
  return (
    <div className="shell">
      <aside className="sidebar">
        <h1>Electronics Inventory</h1>
        <span className="nav-item">Dashboard</span>
      </aside>
      <main className="content">
        <StatusPanel />
      </main>
    </div>
  );
}
```

`apps/desktop/src/main.tsx`:
```tsx
import React from 'react';
import ReactDOM from 'react-dom/client';
import { App } from './App';
import { applyTheme } from './theme.css';
import './shell.css';

applyTheme('dark');
ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
```

Run: `pnpm --filter @ei/desktop test`
Expected: 1 test PASS.

- [ ] **Step 4: Rust side — app initialization module with tests**

`apps/desktop/src-tauri/Cargo.toml`:
```toml
[package]
name = "electronics-inventory"
version.workspace = true
edition.workspace = true

[build-dependencies]
tauri-build = { version = "2", features = [] }

[dependencies]
inventory-core = { path = "../../../crates/inventory-core" }
inventory-db = { path = "../../../crates/inventory-db" }
tauri = { version = "2", features = [] }
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
tracing.workspace = true
tracing-appender.workspace = true

[dev-dependencies]
tempfile.workspace = true
```

`apps/desktop/src-tauri/build.rs`:
```rust
fn main() {
    tauri_build::build()
}
```

`apps/desktop/src-tauri/src/app.rs`:
```rust
//! Application startup: resolve data dir, ensure layout, init logging, open DB.

use std::sync::Mutex;

use inventory_core::paths::{ensure_layout, resolve_data_dir, DataLayout, PathsError};
use inventory_db::{Database, DbError};

#[derive(Debug, thiserror::Error)]
pub enum InitError {
    #[error(transparent)]
    Paths(#[from] PathsError),
    #[error(transparent)]
    Db(#[from] DbError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub struct AppInit {
    pub layout: DataLayout,
    pub db: Database,
}

impl AppInit {
    /// Resolve directories and open the database. Pure inputs for testability;
    /// `main` passes real env values.
    pub fn initialize(env_override: Option<&str>, appdata: Option<&str>) -> Result<Self, InitError> {
        let root = resolve_data_dir(env_override, appdata)?;
        let layout = ensure_layout(&root)?;
        let db = Database::open_and_migrate(&layout.root.join("inventory.sqlite"), &layout.local_backups)?;
        Ok(AppInit { layout, db })
    }
}

/// Shared Tauri state.
pub struct AppState {
    pub layout: DataLayout,
    pub db: Mutex<Database>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    pub app_version: String,
    pub schema_version: u32,
    pub data_dir: String,
}

pub fn status_of(state: &AppState, app_version: &str) -> Result<AppStatus, DbError> {
    let db = state.db.lock().expect("db mutex poisoned");
    Ok(AppStatus {
        app_version: app_version.to_string(),
        schema_version: db.schema_version()?,
        data_dir: state.layout.root.display().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_creates_layout_and_database() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("data");
        let init = AppInit::initialize(Some(root.to_str().unwrap()), None).unwrap();
        assert!(root.join("inventory.sqlite").exists());
        assert!(root.join("logs").is_dir());
        assert_eq!(init.db.schema_version().unwrap(), inventory_db::SUPPORTED_SCHEMA_VERSION);
    }

    #[test]
    fn status_reports_version_and_data_dir() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("data");
        let init = AppInit::initialize(Some(root.to_str().unwrap()), None).unwrap();
        let state = AppState { layout: init.layout, db: Mutex::new(init.db) };
        let status = status_of(&state, "0.1.0").unwrap();
        assert_eq!(status.app_version, "0.1.0");
        assert_eq!(status.schema_version, 1);
        assert!(status.data_dir.ends_with("data"));
    }
}
```

`apps/desktop/src-tauri/src/main.rs`:
```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;

use app::{status_of, AppInit, AppState, AppStatus};

#[tauri::command]
fn app_status(state: tauri::State<'_, AppState>, app: tauri::AppHandle) -> Result<AppStatus, String> {
    let version = app.package_info().version.to_string();
    status_of(&state, &version).map_err(|e| e.to_string())
}

fn main() {
    let env_override = std::env::var("ELECTRONICS_INVENTORY_DATA_DIR").ok();
    let appdata = std::env::var("APPDATA").ok();
    let init = AppInit::initialize(env_override.as_deref(), appdata.as_deref())
        .expect("failed to initialize application data directory and database");

    let _log_guard = inventory_core::logging::init(&init.layout.logs)
        .expect("failed to initialize logging");
    tracing::info!("application starting");

    tauri::Builder::default()
        .manage(AppState { layout: init.layout, db: std::sync::Mutex::new(init.db) })
        .invoke_handler(tauri::generate_handler![app_status])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

`apps/desktop/src-tauri/tauri.conf.json`:
```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "Electronics Inventory",
  "version": "0.1.0",
  "identifier": "com.jacob.electronics-inventory",
  "build": {
    "beforeDevCommand": "pnpm --filter @ei/desktop dev",
    "devUrl": "http://localhost:5173",
    "beforeBuildCommand": "pnpm --filter @ei/desktop build",
    "frontendDist": "../dist"
  },
  "app": {
    "windows": [
      {
        "title": "Electronics Inventory",
        "width": 1280,
        "height": 800,
        "minWidth": 960,
        "minHeight": 600
      }
    ],
    "security": { "csp": "default-src 'self'; style-src 'self' 'unsafe-inline'" }
  },
  "bundle": {
    "active": true,
    "targets": ["nsis"],
    "icon": ["icons/icon.ico"]
  }
}
```

Generate icons (Tauri requires an .ico; use the CLI's generator against any square PNG — create a plain one first):
```powershell
Add-Type -AssemblyName System.Drawing
$bmp = New-Object System.Drawing.Bitmap 512,512
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.Clear([System.Drawing.Color]::FromArgb(255,47,111,237))
$bmp.Save("apps\desktop\src-tauri\app-icon.png", [System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose(); $bmp.Dispose()
# Note: --filter exec runs with cwd apps/desktop, so the path is relative to that.
pnpm --filter @ei/desktop exec tauri icon src-tauri/app-icon.png
```
Expected: `apps/desktop/src-tauri/icons/` populated including `icon.ico`.

Add the member to the root `Cargo.toml`:
```toml
members = [
    "crates/inventory-core",
    "crates/inventory-db",
    "crates/inventory-import",
    "crates/inventory-sync",
    "apps/desktop/src-tauri",
]
```

- [ ] **Step 5: Run the Rust tests**

Run: `cargo test -p electronics-inventory`
Expected: 2 tests PASS (initialize + status).

- [ ] **Step 6: Verify the full app builds and launches**

Run: `pnpm --filter @ei/desktop exec tauri build --debug`
Expected: build succeeds; `target\debug\electronics-inventory.exe` exists.

Launch smoke test (uses an isolated data dir; kills the app after it demonstrably starts):
```powershell
$env:ELECTRONICS_INVENTORY_DATA_DIR = "$env:TEMP\ei-smoke-test"
$p = Start-Process "target\debug\electronics-inventory.exe" -PassThru
Start-Sleep -Seconds 10
$alive = -not $p.HasExited
Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
Remove-Item Env:\ELECTRONICS_INVENTORY_DATA_DIR
if ($alive -and (Test-Path "$env:TEMP\ei-smoke-test\inventory.sqlite")) { Write-Output "SMOKE OK" } else { Write-Error "SMOKE FAILED" }
```
Expected: `SMOKE OK` — the process stayed up and created its database in the isolated data dir (production `%APPDATA%` untouched).

- [ ] **Step 7: Commit**

```powershell
git add -A; git commit -m "Add Tauri desktop shell with status command, themed UI, and DB startup"
```

---

### Task 8: Read-only web shell (`apps/web`)

**Files:**
- Create: `apps/web/package.json`, `apps/web/tsconfig.json`, `apps/web/vite.config.ts`, `apps/web/vitest.config.ts`, `apps/web/index.html`, `apps/web/playwright.config.ts`
- Create: `apps/web/src/main.tsx`, `apps/web/src/App.tsx`, `apps/web/src/snapshot.ts`, `apps/web/src/snapshot.test.ts`, `apps/web/src/web.css`
- Create: `apps/web/e2e/smoke.spec.ts`
- Create: `packages/shared/src/snapshot.ts`
- Modify: `packages/shared/src/index.ts`

**Interfaces:**
- Consumes: `@ei/shared` tokens (Task 3).
- Produces: `@ei/shared` gains `SnapshotHeader { formatVersion: number; publishedAt: string; partCount: number }` and `parseSnapshotHeader(json: unknown): SnapshotHeader | null` (Phase 6's full snapshot schema extends this header — the field names here are final). Web app renders the published-state banner; `loadSnapshot(fetchImpl): Promise<SnapshotState>` where `SnapshotState = { kind: 'none' } | { kind: 'loaded'; header: SnapshotHeader } | { kind: 'invalid' }`.

- [ ] **Step 1: Shared snapshot header type + failing test**

`packages/shared/src/snapshot.ts`:
```ts
/** Header fields of the published snapshot. Phase 6 extends the full schema;
 * these field names are final. */
export interface SnapshotHeader {
  formatVersion: number;
  publishedAt: string; // ISO-8601 UTC
  partCount: number;
}

export function parseSnapshotHeader(json: unknown): SnapshotHeader | null {
  if (typeof json !== 'object' || json === null) return null;
  const o = json as Record<string, unknown>;
  if (
    typeof o.formatVersion === 'number' &&
    typeof o.publishedAt === 'string' &&
    typeof o.partCount === 'number'
  ) {
    return { formatVersion: o.formatVersion, publishedAt: o.publishedAt, partCount: o.partCount };
  }
  return null;
}
```

Append to `packages/shared/src/index.ts`:
```ts
export { parseSnapshotHeader, type SnapshotHeader } from './snapshot';
```

`apps/web/src/snapshot.test.ts`:
```ts
import { describe, expect, it } from 'vitest';
import { loadSnapshot } from './snapshot';

const ok = { formatVersion: 1, publishedAt: '2026-07-14T00:00:00Z', partCount: 42 };

function fakeFetch(status: number, body?: unknown) {
  return async () =>
    ({ ok: status === 200, status, json: async () => body }) as Response;
}

describe('loadSnapshot', () => {
  it('returns none when the snapshot is missing (404)', async () => {
    expect(await loadSnapshot(fakeFetch(404))).toEqual({ kind: 'none' });
  });

  it('returns loaded with the parsed header', async () => {
    const state = await loadSnapshot(fakeFetch(200, ok));
    expect(state).toEqual({ kind: 'loaded', header: ok });
  });

  it('returns invalid for malformed JSON shape', async () => {
    expect(await loadSnapshot(fakeFetch(200, { nope: true }))).toEqual({ kind: 'invalid' });
  });
});
```

- [ ] **Step 2: Web package scaffolding, run test to verify it fails**

`apps/web/package.json`:
```json
{
  "name": "@ei/web",
  "version": "0.1.0",
  "private": true,
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "tsc --noEmit && vite build",
    "preview": "vite preview --port 4173 --strictPort",
    "test": "vitest run",
    "e2e": "playwright test"
  },
  "dependencies": {
    "@ei/shared": "workspace:*",
    "react": "^18.3.1",
    "react-dom": "^18.3.1"
  },
  "devDependencies": {
    "@playwright/test": "^1.46.0",
    "@types/react": "^18.3.0",
    "@types/react-dom": "^18.3.0",
    "@vitejs/plugin-react": "^4.3.0",
    "typescript": "^5.5.0",
    "vite": "^6.0.0",
    "vitest": "^2.0.0"
  }
}
```

`apps/web/tsconfig.json`:
```json
{ "extends": "../../tsconfig.base.json", "include": ["src", "e2e"], "compilerOptions": { "types": ["vite/client"] } }
```

`apps/web/vite.config.ts`:
```ts
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

export default defineConfig({ plugins: [react()] });
```

`apps/web/vitest.config.ts`:
```ts
import { defineConfig } from 'vitest/config';
export default defineConfig({ test: { environment: 'node', include: ['src/**/*.test.ts'] } });
```

`apps/web/index.html`:
```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>Electronics Inventory — Read-only</title>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
```

Run: `pnpm install; pnpm --filter @ei/web test`
Expected: FAIL — `./snapshot` does not exist.

- [ ] **Step 3: Implement snapshot loader and app**

`apps/web/src/snapshot.ts`:
```ts
import { parseSnapshotHeader, type SnapshotHeader } from '@ei/shared';

export const SNAPSHOT_URL = '/inventory.snapshot.json';

export type SnapshotState =
  | { kind: 'none' }
  | { kind: 'loaded'; header: SnapshotHeader }
  | { kind: 'invalid' };

export async function loadSnapshot(
  fetchImpl: (url: string) => Promise<Response> = (url) => fetch(url),
): Promise<SnapshotState> {
  let res: Response;
  try {
    res = await fetchImpl(SNAPSHOT_URL);
  } catch {
    return { kind: 'none' };
  }
  if (!res.ok) return { kind: 'none' };
  let body: unknown;
  try {
    body = await res.json();
  } catch {
    return { kind: 'invalid' };
  }
  const header = parseSnapshotHeader(body);
  return header ? { kind: 'loaded', header } : { kind: 'invalid' };
}
```

`apps/web/src/web.css`:
```css
* {
  box-sizing: border-box;
}
body {
  margin: 0;
  background: var(--color-bg-app);
  color: var(--color-text-primary);
  font-family: 'Segoe UI', system-ui, sans-serif;
  font-size: 14px;
}
.banner {
  background: var(--color-bg-panel);
  border-bottom: 1px solid var(--color-border);
  padding: 10px 20px;
  color: var(--color-text-secondary);
}
.main {
  padding: 24px 20px;
}
.empty {
  border: 1px solid var(--color-border);
  background: var(--color-bg-panel);
  border-radius: 6px;
  padding: 24px;
  max-width: 560px;
}
```

`apps/web/src/App.tsx`:
```tsx
import { useEffect, useState } from 'react';
import { loadSnapshot, type SnapshotState } from './snapshot';

export function App() {
  const [state, setState] = useState<SnapshotState | null>(null);

  useEffect(() => {
    loadSnapshot().then(setState);
  }, []);

  return (
    <>
      <header className="banner">
        Read-only inventory snapshot
        {state?.kind === 'loaded' && <> — last published {state.header.publishedAt}</>}
      </header>
      <main className="main">
        {state === null && <p>Loading…</p>}
        {state?.kind === 'none' && (
          <div className="empty">
            <h2>No snapshot published yet</h2>
            <p>The desktop application has not published an inventory snapshot to this site.</p>
          </div>
        )}
        {state?.kind === 'invalid' && (
          <div className="empty">
            <h2>Snapshot could not be read</h2>
            <p>The published snapshot file is not in a recognized format.</p>
          </div>
        )}
        {state?.kind === 'loaded' && (
          <div className="empty">
            <h2>{state.header.partCount} parts published</h2>
            <p>Inventory browsing arrives with the Phase 6 snapshot schema.</p>
          </div>
        )}
      </main>
    </>
  );
}
```

`apps/web/src/main.tsx`:
```tsx
import { generateCssVariables } from '@ei/shared';
import React from 'react';
import ReactDOM from 'react-dom/client';
import { App } from './App';
import './web.css';

const style = document.createElement('style');
style.textContent = generateCssVariables('dark');
document.head.appendChild(style);

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
```

- [ ] **Step 4: Run unit tests, then build**

Run: `pnpm --filter @ei/web test; pnpm --filter @ei/web build`
Expected: 3 tests PASS; `apps/web/dist/` produced.

- [ ] **Step 5: Playwright smoke test**

`apps/web/playwright.config.ts`:
```ts
import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './e2e',
  use: { baseURL: 'http://localhost:4173' },
  webServer: {
    command: 'pnpm preview',
    port: 4173,
    reuseExistingServer: true,
  },
});
```

`apps/web/e2e/smoke.spec.ts`:
```ts
import { expect, test } from '@playwright/test';

test('shows read-only banner and empty state when no snapshot exists', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByText('Read-only inventory snapshot')).toBeVisible();
  await expect(page.getByText('No snapshot published yet')).toBeVisible();
});
```

Run:
```powershell
pnpm --filter @ei/web exec playwright install chromium
pnpm --filter @ei/web e2e
```
Expected: 1 e2e test PASS.

- [ ] **Step 6: Commit**

```powershell
git add -A; git commit -m "Add read-only web shell with snapshot loader and e2e smoke test"
```

---

### Task 9: Phase gate — verify script, docs, decisions

**Files:**
- Create: `scripts/verify.ps1`, `docs/architecture.md`, `docs/decisions.md`, `README.md`
- Modify: `docs/build.md`

**Interfaces:**
- Consumes: everything above.
- Produces: `scripts/verify.ps1` — the single phase-gate command every later phase reruns; seeded ADR log; architecture doc.

- [ ] **Step 1: Verify script**

`scripts/verify.ps1`:
```powershell
# Phase gate: run every check. Fails fast with a clear section name.
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')

function Invoke-Step {
    param([string]$Name, [scriptblock]$Body)
    Write-Host "=== $Name ===" -ForegroundColor Cyan
    & $Body
    if ($LASTEXITCODE -ne 0) { Write-Host "FAILED: $Name" -ForegroundColor Red; exit 1 }
}

Invoke-Step 'rustfmt'      { cargo fmt --all -- --check }
Invoke-Step 'clippy'       { cargo clippy --workspace --all-targets -- -D warnings }
Invoke-Step 'cargo tests'  { cargo test --workspace }
Invoke-Step 'ts tests'     { pnpm -r test }
Invoke-Step 'ts builds'    { pnpm -r build }
Invoke-Step 'stylelint'    { pnpm lint:css }

Write-Host 'ALL CHECKS PASSED' -ForegroundColor Green
```

- [ ] **Step 2: Run it and fix anything it finds**

Run: `powershell -File scripts\verify.ps1`
Expected: `ALL CHECKS PASSED`. If rustfmt or clippy complain, apply `cargo fmt --all` / fix the lints and rerun until green.

- [ ] **Step 3: Documentation**

`README.md`:
```markdown
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
```

`docs/architecture.md`:
```markdown
# Architecture

See the spec for full detail. Summary of what exists after Phase 1:

- **Rust core** (`crates/*`): all domain and persistence logic. The UI never
  computes stock or touches SQLite directly.
- **Desktop** (`apps/desktop`): React UI over typed Tauri commands. Startup:
  resolve data dir (`ELECTRONICS_INVENTORY_DATA_DIR` override, else
  `%APPDATA%\ElectronicsInventory`) → ensure layout → init redacting logging →
  open + migrate SQLite → serve `app_status`.
- **Web** (`apps/web`): static SPA that loads `/inventory.snapshot.json` and
  renders read-only state. No write paths exist.
- **Tokens** (`packages/shared`): primitive palette + semantic tokens emitted
  as CSS custom properties; stylelint forbids raw colors anywhere else.
- **Migrations**: numbered SQL embedded in `inventory-db`, applied in one
  transaction each, `PRAGMA user_version` tracks state, pre-migration safety
  backup via SQLite online backup API, newer-schema refusal.
- **Quantities**: exact fixed-point milli-units (`Quantity`, x1000).
```

`docs/decisions.md`:
```markdown
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
```

Append to `docs/build.md`:
```markdown
## Building

| Action | Command |
|---|---|
| Everything (gate) | `powershell -File scripts\verify.ps1` |
| Desktop dev | `pnpm --filter @ei/desktop exec tauri dev` |
| Desktop debug build | `pnpm --filter @ei/desktop exec tauri build --debug` |
| Desktop release + NSIS installer | `pnpm --filter @ei/desktop exec tauri build` |
| Web | `pnpm --filter @ei/web build` (output `apps/web/dist/`) |
| Web e2e | `pnpm --filter @ei/web e2e` |
```

- [ ] **Step 4: Commit**

```powershell
git add -A; git commit -m "Add phase gate verify script and foundation documentation"
```

---

## Plan self-review notes (kept for the record)

- **Spec coverage (Phase 1 scope):** repository structure (T2), shared domain package (T3, T4), Tauri application (T7), React desktop shell (T7), read-only web shell (T8), SQLite integration (T5), migration system (T5), logging (T6), theme tokens (T3), test framework (T3–T8: vitest, cargo test, Playwright). Data-dir safety and env override (T6, T7). Phase gate (T9).
- **Deferred with rationale, not placeholders:** TanStack Router (Phase 3, one screen today), tauri-specta (Phase 2, one command today), full snapshot schema (Phase 6, header type is final).
- **Type consistency:** `AppStatus { appVersion, schemaVersion, dataDir }` matches Rust `#[serde(rename_all = "camelCase")]` struct; `SnapshotHeader` field names identical in shared package and web loader; crate/package names uniform.
