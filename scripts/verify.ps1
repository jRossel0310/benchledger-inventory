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
