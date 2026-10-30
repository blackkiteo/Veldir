param(
    [Parameter(Mandatory=$true)][string]$CertificateThumbprint,
    [Parameter(Mandatory=$true)][string]$TimestampUrl
)
$ErrorActionPreference = 'Stop'
$signTool = (Get-Command signtool.exe -ErrorAction Stop).Source
$releaseDir = Join-Path $PSScriptRoot '..\target\release'
$exe = Join-Path $releaseDir 'veldir.exe'
if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) { throw 'Compilare prima la release.' }
& $signTool sign /sha1 $CertificateThumbprint /fd SHA256 /tr $TimestampUrl /td SHA256 $exe
if ($LASTEXITCODE -ne 0) { throw 'Firma non riuscita; pacchetto non pubblicato.' }
& $signTool verify /pa /v $exe
if ($LASTEXITCODE -ne 0) { throw 'Verifica firma non riuscita.' }
Get-FileHash -LiteralPath $exe -Algorithm SHA256 | ForEach-Object {
    "$($_.Hash.ToLowerInvariant()) *veldir.exe" | Out-File (Join-Path $releaseDir 'SHA256SUMS.txt') -Encoding ascii
}
