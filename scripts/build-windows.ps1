# Veldir v0.2.0 - build script for Windows PowerShell 5.1 / PowerShell 7
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Error "Rust/Cargo non presente. Installa Rust 1.95+ (MSVC) da https://rustup.rs/"
    exit 1
}
Write-Host "== Veldir: format, check, test, build ==" -ForegroundColor Cyan
cargo fmt --all
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
cargo check --all-targets
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
cargo test --all-targets
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
cargo build --release
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
$exe = Join-Path (Get-Location) 'target\release\veldir.exe'
if (-not (Test-Path $exe)) { Write-Error "Eseguibile non trovato: $exe"; exit 1 }
Get-FileHash $exe -Algorithm SHA256 | Format-List
Write-Host "BUILD OK: $exe" -ForegroundColor Green
