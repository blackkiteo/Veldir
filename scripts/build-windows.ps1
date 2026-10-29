$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')
cargo fmt --all -- --check
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
cargo check --locked --all-targets
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
cargo test --locked --all-targets
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
cargo build --locked --release
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
Get-FileHash target/release/veldir.exe -Algorithm SHA256
