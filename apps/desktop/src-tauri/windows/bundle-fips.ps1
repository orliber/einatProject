# Puts the FIPS 140-3 module next to the app in the Windows installer (D-047, D-091).
#
# On Windows the validated module (aws-lc-fips-sys) can only be a DLL, never linked into the
# program. `cargo test` finds it in the build folder, so every test passed; the installed app
# did not, and Windows stopped it before its first line with a "DLL not found" message that
# belongs to no window of the app (the clean install saw "the app shows no window"). This
# copies the DLL of the build just made and writes a config that adds it to the installer,
# beside DiagnosticVault.exe, where Windows looks first:
#
#   cargo build --release -p dv-desktop --features fips
#   apps/desktop/src-tauri/windows/bundle-fips.ps1
#   pnpm tauri build --bundles nsis --features fips --config src-tauri/tauri.fips.conf.json
$ErrorActionPreference = 'Stop'

$root = Resolve-Path (Join-Path $PSScriptRoot '..\..\..\..')
$lock = Get-Content (Join-Path $root 'Cargo.lock') -Raw
if ($lock -notmatch 'name = "aws-lc-fips-sys"\s+version = "([^"]+)"') { throw 'aws-lc-fips-sys is not in Cargo.lock' }
$version = $Matches[1] -replace '\.', '_'

# The build folder of this version (an older one can still sit in the CI cache).
$found = Get-ChildItem -Path (Join-Path $root 'target\release\build') -Directory -Filter 'aws-lc-fips-sys-*' |
    ForEach-Object { Get-ChildItem -Path $_.FullName -Recurse -File -Filter "aws_lc_fips_$($version)*crypto*.dll" } |
    Sort-Object LastWriteTime -Descending
if (-not $found) { throw "no aws_lc_fips_$($version)*crypto*.dll under target\release\build: build first (cargo build --release -p dv-desktop --features fips)" }
$dll = $found | Select-Object -First 1
Write-Host "FIPS module: $($dll.FullName) ($((Get-FileHash $dll.FullName -Algorithm SHA256).Hash.ToLower()))"

$dest = Join-Path $PSScriptRoot '..\fips'
New-Item -ItemType Directory -Force -Path $dest | Out-Null
Copy-Item $dll.FullName (Join-Path $dest $dll.Name) -Force

# Merged over tauri.conf.json and tauri.windows.conf.json; the OCR folder stays as it is there.
$conf = @{ bundle = @{ resources = [ordered]@{ 'ocr/' = 'ocr/'; "fips/$($dll.Name)" = $dll.Name } } }
$conf | ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8 (Join-Path $PSScriptRoot '..\tauri.fips.conf.json')
Get-Content (Join-Path $PSScriptRoot '..\tauri.fips.conf.json')
