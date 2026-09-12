$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot -Parent)
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    $localTools = Join-Path (Get-Location) 'target/validation-tools/activate.ps1'
    if (Test-Path -LiteralPath $localTools) { . $localTools }
    else { throw 'Instala Rust estable y reinicia la terminal.' }
}
cargo fmt --all --check
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
cargo test --workspace --locked
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
cargo clippy --workspace --locked --all-targets --all-features -- -D warnings
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
python scripts/check-boundaries.py
exit $LASTEXITCODE
