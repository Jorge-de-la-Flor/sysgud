param(
    [ValidateRange(0, 900)][int]$WaitSeconds = 900,
    [switch]$NoOpen
)
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot -Parent)
$pairArguments = @('scripts/connect-telegram.py', '--wait', "$WaitSeconds")
if (-not $NoOpen) { $pairArguments += '--open' }
python @pairArguments
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
Write-Host 'Cuenta vinculada. Iniciando API y Telegram...'
& "$PSScriptRoot/start.ps1"
exit $LASTEXITCODE
