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

## PDF text extraction (pdfium)

`inventory-import` extracts positioned text from supplier PDFs (DigiKey PO
Acknowledgements) via [`pdfium-render`](https://docs.rs/pdfium-render), a
Rust wrapper around Google's `pdfium` C++ library. Every extracted token is
normalized to one coordinate convention regardless of source: **origin
top-left, `y` increases downward, units are PDF points** (1/72 inch). See
`crates/inventory-import/src/pdf/mod.rs` (`PositionedToken`) and
`src/pdf/text_source.rs` (`PdfTextSource`, `PdfiumTextSource`) for the full
contract and the pdfium-side coordinate-flip math.

**Feature-gated, off by default.** `pdfium-render` is an optional dependency
behind the `pdfium` Cargo feature (`cargo build -p inventory-import
--features pdfium`). `pdfium-render`'s default configuration links pdfium
*dynamically* at runtime (via `libloading`), not statically, so enabling the
feature never requires the native library at **build** time — only calling
`PdfiumTextSource::new()` needs it present at **run** time. The crate's
default build/test (`pdfium` off) needs nothing pdfium-related at all.

**Obtaining `pdfium.dll`:** pdfium is not vendored in this repo (its
license/size make that impractical). Download a prebuilt binary for your
platform — see pdfium-render's "usage" docs for current prebuilt-binary
sources (e.g. the `bblanchon/pdfium-binaries` GitHub releases) — and place
the platform library file (`pdfium.dll` on Windows, `libpdfium.so` on
Linux, `libpdfium.dylib` on macOS) somewhere `PdfiumTextSource` can find it:

1. Set the `PDFIUM_DLL_DIR` environment variable to the directory containing
   it, **or**
2. Rely on the system library search path (`PATH` on Windows, the shared
   library search path elsewhere) — e.g. drop it next to the app's `.exe`.

If neither resolves, `PdfiumTextSource::new()` returns
`ImportError::Pdf("pdfium library unavailable: ...")` rather than panicking.

**Unit tests never need the DLL.** All PDF-reconstruction logic
(`crate::digikey::pdf`, Tasks 8/9) is tested against a committed
**positioned-token JSON fixture**
(`crates/inventory-import/tests/fixtures/digikey_po_100353602.tokens.json`),
not a live extraction — mirroring the units-engine shared-fixture pattern
used elsewhere in this repo. That fixture was generated once, on the
authoring machine, from a sanitized private sample PDF via
`samples/digikey/tools/dump_tokens.py` (PyMuPDF, a separate Python
dependency) rather than through `PdfiumTextSource` itself, because a bound
`pdfium.dll` is not available in this dev/CI environment. PyMuPDF's own word
extraction already normalizes to the same top-left/`y`-down/points
convention, so the fixture is interchangeable with a live
`PdfiumTextSource` extraction as far as the reconstruction logic is
concerned. `cargo test -p inventory-import` (default features) exercises
that fixture; it does not build or run `PdfiumTextSource`.
