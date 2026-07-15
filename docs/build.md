# Build Prerequisites

Verified working toolchain for this repository (Windows 11):

| Tool | Install | Verify |
|---|---|---|
| Node.js 22+ | winget install OpenJS.NodeJS.LTS | `node --version` |
| pnpm 9+ | `npm install -g pnpm` | `pnpm --version` |
| Rust stable (MSVC) | winget install Rustlang.Rustup, then `rustup default stable` | `cargo --version` |
| MSVC Build Tools | winget install Microsoft.VisualStudio.2022.BuildTools with the VCTools workload | vswhere reports an installation path |
| WebView2 runtime | Preinstalled on Windows 11 | registry key `HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}` |

## Building

| Action | Command |
|---|---|
| Everything (gate) | `powershell -File scripts\verify.ps1` |
| Desktop dev | `pnpm --filter @ei/desktop exec tauri dev` |
| Desktop debug build | `pnpm --filter @ei/desktop exec tauri build --debug` |
| Desktop release + NSIS installer | `pnpm --filter @ei/desktop exec tauri build` |
| Web | `pnpm --filter @ei/web build` (output `apps/web/dist/`) |
| Web e2e | `pnpm --filter @ei/web e2e` |
