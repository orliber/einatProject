# Release gate (F7, R8): the installer as she runs it, on a Windows account that never had the
# app. A vault kept from the newest earlier version goes where the app keeps hers; the
# installer runs silently for this user only; the installed app must start and stay up with
# that vault in place and write no crash; and the uninstaller must remove the program and
# leave her vault byte for byte. Only invented data (tests/fixtures/vaults).
param([Parameter(Mandatory = $true)] [string] $Installer)
$ErrorActionPreference = "Stop"

$product = "Diagnostic Vault"
$data = Join-Path $env:LOCALAPPDATA "il.diagnosticvault.desktop"
$vault = Join-Path $data "vault"
$installDir = Join-Path $env:LOCALAPPDATA $product
foreach ($p in @($installDir, $data)) {
    if (Test-Path $p) { throw "not a clean account: $p already exists" }
}

# 1. The newest kept vault, decoded from its letters (each half-byte is a-p).
$kept = Get-ChildItem (Join-Path $PSScriptRoot "..\tests\fixtures\vaults") -Directory |
    Sort-Object { [version]$_.Name } | Select-Object -Last 1
New-Item -ItemType Directory -Path $vault -Force | Out-Null
foreach ($f in Get-ChildItem $kept.FullName -Filter *.txt) {
    $text = (Get-Content $f.FullName -Raw) -replace '\s', ''
    $bytes = New-Object byte[] ($text.Length / 2)
    for ($i = 0; $i -lt $bytes.Length; $i++) {
        $bytes[$i] = (([int]$text[2 * $i] - 97) -shl 4) -bor ([int]$text[2 * $i + 1] - 97)
    }
    [IO.File]::WriteAllBytes((Join-Path $vault $f.BaseName), $bytes)
}
Write-Host "kept vault $($kept.Name): $((Get-ChildItem $vault -File).Count) files"

# 2. Silent install, for this user only (no administrator).
$setup = Start-Process -FilePath $Installer -ArgumentList "/S" -Wait -PassThru
if ($setup.ExitCode -ne 0) { throw "the installer ended with $($setup.ExitCode)" }
$exe = Get-ChildItem $installDir -Filter *.exe | Where-Object { $_.Name -ne "uninstall.exe" } | Select-Object -First 1
if (-not $exe) { throw "nothing was installed in $installDir" }
Write-Host "installed $($exe.FullName)"

# 3. It starts and stays up beside the old vault, opens it, takes 500 Hebrew characters typed
#    in its real window (F1), and writes no crash.
$app = Start-Process -FilePath $exe.FullName -PassThru
Start-Sleep -Seconds 10
if ($app.HasExited) { throw "the app closed by itself (exit $($app.ExitCode))" }
& powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "windows-typing.ps1") -ProcessId $app.Id
if ($LASTEXITCODE -ne 0) { throw "typing in the window failed (see above)" }
if ($app.HasExited) { throw "the app closed by itself (exit $($app.ExitCode))" }
Stop-Process -Id $app.Id -Force
Get-Process | Where-Object { $_.Path -like "$installDir*" } | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 3
$crashes = Join-Path $data "crash-log.txt"
if ((Test-Path $crashes) -and (Get-Item $crashes).Length -gt 0) {
    throw "the app wrote a crash: $(Get-Content $crashes -Raw)"
}

# 4. The uninstaller removes the program and leaves her vault as it was (as the app left it:
#    opening it wrote to its log).
$before = Get-ChildItem $vault -File | Sort-Object Name | ForEach-Object { "$($_.Name) $((Get-FileHash $_.FullName).Hash)" }
Start-Process -FilePath (Join-Path $installDir "uninstall.exe") -ArgumentList "/S" -Wait | Out-Null
for ($i = 0; $i -lt 60 -and (Test-Path $exe.FullName); $i++) { Start-Sleep -Seconds 1 }
if (Test-Path $exe.FullName) { throw "the uninstaller left the program in place" }
$after = Get-ChildItem $vault -File | Sort-Object Name | ForEach-Object { "$($_.Name) $((Get-FileHash $_.FullName).Hash)" }
if (Compare-Object $before $after) { throw "the vault changed: $($before -join ', ') / $($after -join ', ')" }
Write-Host "clean install: ok (installed, opened a $($kept.Name) vault and typed in it, uninstalled, vault kept)"
