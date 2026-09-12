param([switch]$Check)
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot -Parent)
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    $localTools = Join-Path (Get-Location) 'target/validation-tools/activate.ps1'
    if (Test-Path -LiteralPath $localTools) { . $localTools }
    else { throw 'Instala Rust estable y reinicia la terminal.' }
}
if ($Check) { cargo run --locked -- --check }
else { cargo run --locked }
exit $LASTEXITCODE
