# Release gate (D-033, D-038): the update as the program runs it on her computer. An earlier
# version is installed silently; then the new installer runs exactly as `install_update`
# starts it (`/P /UPDATE /R`: passive, update mode, relaunch) over it, while the installed app
# is still running, as it is when she clicks "להפעיל מחדש ולעדכן" and the installer's
# RestartManager step closes it. Afterwards the registry must say the new version, the
# program in place must be the new one, the relaunched app must be running, and the vault
# left beside it must be byte for byte as it was. Then it is uninstalled.
#
# The earlier version is the newest published installer of the public releases repository
# (no secret: it is public). When it cannot be fetched (no network, no release yet), the same
# installer is installed twice, which still exercises update mode, the close of the running
# app and the relaunch, but not a version change: the step says so and does not fail on it.
# Only invented data (tests/fixtures/vaults). Runs after windows-clean-install.ps1, which
# leaves the account without the program but with the kept vault in place.
param(
    [Parameter(Mandatory = $true)] [string] $Installer,
    [string] $ReleasesRepo = "orliber/einat-vault-releases"
)
$ErrorActionPreference = "Stop"

$winConf = Join-Path $PSScriptRoot "..\apps\desktop\src-tauri\tauri.windows.conf.json"
$product = (Get-Content $winConf -Raw -Encoding utf8 | ConvertFrom-Json).productName
if (-not $product) { throw "no productName in $winConf" }
$version = (Get-Content (Join-Path $PSScriptRoot "..\Cargo.toml") | Select-String -Pattern '^version = "([^"]+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
if (-not $version) { throw "no version in Cargo.toml" }
$installDir = Join-Path $env:LOCALAPPDATA $product
$data = Join-Path $env:LOCALAPPDATA "il.diagnosticvault.desktop"
$vault = Join-Path $data "vault"
$uninstKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\$product"
if (Test-Path $installDir) { throw "the program is still installed in $installDir (run windows-clean-install.ps1 first)" }
if (-not (Test-Path $vault)) { throw "no kept vault in $vault (windows-clean-install.ps1 leaves one)" }
$Installer = (Resolve-Path $Installer).Path

$hash = { Get-ChildItem $vault -File | Sort-Object Name | ForEach-Object { "$($_.Name) $((Get-FileHash $_.FullName).Hash)" } }
$installedVersion = {
    if (Test-Path $uninstKey) { (Get-ItemProperty $uninstKey).DisplayVersion } else { $null }
}
$exeOf = { Get-ChildItem $installDir -Filter *.exe | Where-Object { $_.Name -ne "uninstall.exe" } | Select-Object -First 1 }

# 1. The earlier version: the newest published installer, or this one again.
$tmp = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { $env:TEMP }
$before = Join-Path $tmp "DiagnosticVault-Setup-before.exe"
$sameVersion = $false
try {
    $url = "https://github.com/$ReleasesRepo/releases/latest/download/DiagnosticVault-Setup.exe"
    Invoke-WebRequest -Uri $url -OutFile $before -MaximumRedirection 5 -TimeoutSec 300
    $head = [IO.File]::ReadAllBytes($before)[0..1]
    if ([Text.Encoding]::ASCII.GetString($head) -ne "MZ") { throw "not a Windows program" }
    Write-Host "earlier version: the newest published installer ($((Get-Item $before).Length) bytes)"
} catch {
    Write-Host "::notice::the published installer could not be fetched ($($_.Exception.Message)); the same installer is installed twice, so the version does not change"
    Copy-Item $Installer $before -Force
    $sameVersion = $true
}

# 2. The earlier version installed silently for this user, started beside the kept vault.
$setup = Start-Process -FilePath $before -ArgumentList "/S" -Wait -PassThru
if ($setup.ExitCode -ne 0) { throw "the earlier installer ended with $($setup.ExitCode)" }
$exe = & $exeOf
if (-not $exe) { throw "nothing was installed in $installDir" }
$was = & $installedVersion
Write-Host "installed $was at $($exe.FullName)"
$old = Start-Process -FilePath $exe.FullName -PassThru
Start-Sleep -Seconds 8
if ($old.HasExited) { throw "the earlier version closed by itself (exit $($old.ExitCode))" }
# The vault as the earlier version left it (a start may write beside it; the update must not).
$untouched = & $hash

# 3. The update, as the program starts it: the installer closes the running app
#    (RestartManager, forced in passive mode), replaces the program and opens it again.
$update = Start-Process -FilePath $Installer -ArgumentList "/P", "/UPDATE", "/R" -Wait -PassThru
if ($update.ExitCode -ne 0) { throw "the update installer ended with $($update.ExitCode)" }
for ($i = 0; $i -lt 30 -and -not $old.HasExited; $i++) { Start-Sleep -Seconds 1 }
if (-not $old.HasExited) { throw "the earlier version was still running after the update" }
$now = & $installedVersion
if ($now -ne $version) { throw "after the update the registry says $now, not $version" }
if (-not $sameVersion -and $was -eq $now) { throw "the version did not change: $was before, $now after" }
$exe = & $exeOf
if (-not $exe) { throw "the update left no program in $installDir" }
$fileVersion = (Get-Item $exe.FullName).VersionInfo.ProductVersion
if ($fileVersion -and -not $fileVersion.StartsWith($version)) { throw "the program in place is $fileVersion, not $version" }
if (Compare-Object $untouched (& $hash)) { throw "the update changed the vault" }

# 4. The relaunched app (`/R`) is running from the install folder and stays up.
$new = $null
for ($i = 0; $i -lt 30 -and -not $new; $i++) {
    Start-Sleep -Seconds 1
    $new = Get-Process | Where-Object { $_.Path -and $_.Path -ieq $exe.FullName } | Select-Object -First 1
}
if (-not $new) { throw "the new version was not opened again after the update" }
Start-Sleep -Seconds 8
if ($new.HasExited) { throw "the new version closed by itself after the relaunch (exit $($new.ExitCode))" }
Write-Host "update: $was -> $now, the new version is running (pid $($new.Id))"
Stop-Process -Id $new.Id -Force
Get-Process | Where-Object { $_.Path -like "$installDir*" } | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 3
$crashes = Join-Path $data "crash-log.txt"
if ((Test-Path $crashes) -and (Get-Item $crashes).Length -gt 0) {
    throw "the app wrote a crash: $(Get-Content $crashes -Raw)"
}

# 5. Uninstalled, the vault left as it was.
$kept = & $hash
Start-Process -FilePath (Join-Path $installDir "uninstall.exe") -ArgumentList "/S" -Wait | Out-Null
for ($i = 0; $i -lt 60 -and (Test-Path $exe.FullName); $i++) { Start-Sleep -Seconds 1 }
if (Test-Path $exe.FullName) { throw "the uninstaller left the program in place" }
if (Compare-Object $kept (& $hash)) { throw "the uninstall changed the vault" }
Write-Host "update over an installed version: ok ($(if ($sameVersion) { 'same version twice' } else { "$was -> $now" }), closed, replaced, relaunched, uninstalled, vault kept)"
