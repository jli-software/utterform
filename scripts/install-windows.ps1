# Verify the NSIS setup before opening it; no elevation or forced app shutdown.
param(
    [string]$Version = 'v0.7.14',
    [switch]$VerifyOnly
)

$ErrorActionPreference = 'Stop'
if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT -or
    -not [Environment]::Is64BitOperatingSystem) {
    throw 'This installer requires 64-bit Windows.'
}
if ($Version -notmatch '^[A-Za-z0-9][A-Za-z0-9._-]*$') {
    throw 'Invalid release tag.'
}

$asset = 'utterform-windows-x86_64-setup.exe'
$base = "https://github.com/jli-software/utterform/releases/download/$Version"
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('utterform-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $scratch | Out-Null
try {
    $installer = Join-Path $scratch $asset
    $checksums = Join-Path $scratch 'SHA256SUMS.txt'
    Write-Host "Downloading Utterform $Version..."
    Invoke-WebRequest -UseBasicParsing "$base/$asset" -OutFile $installer
    Invoke-WebRequest -UseBasicParsing "$base/SHA256SUMS.txt" -OutFile $checksums
    $pattern = '^([a-fA-F0-9]{64})\s+\*?' + [regex]::Escape($asset) + '$'
    $entries = @(Get-Content $checksums | Where-Object { $_ -match $pattern })
    if ($entries.Count -ne 1) { throw 'Missing or ambiguous installer checksum.' }
    $expected = [regex]::Match($entries[0], $pattern).Groups[1].Value
    $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $installer).Hash
    if ($actual -ne $expected) { throw 'Installer checksum mismatch; installation refused.' }
    Write-Host "SHA-256 verified: Utterform $Version"
    if (-not $VerifyOnly) {
        Write-Host 'Quit Utterform before continuing in the setup window.'
        $setup = Start-Process -FilePath $installer -Wait -PassThru
        if ($setup.ExitCode -notin @(0, 3010)) {
            throw "Setup exited with code $($setup.ExitCode)."
        }
    }
} finally {
    Remove-Item -LiteralPath $scratch -Recurse -Force
}
