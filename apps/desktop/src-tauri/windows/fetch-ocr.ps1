# Puts the local OCR engine next to the app before the Windows installer is built (D-048):
#   apps/desktop/src-tauri/ocr/tesseract.exe
#   apps/desktop/src-tauri/ocr/tessdata/{heb,eng,osd}.traineddata
#
# Tesseract is built from its upstream sources by vcpkg (static: one program, no DLLs), not
# downloaded as someone else's build. vcpkg's port files pin every source archive by SHA-512.
# The runner's own vcpkg is used: an old pinned vcpkg breaks when its build-tool downloads
# (MSYS2 packages) disappear from the mirrors. Its commit is printed for the record. The language data is checked
# against pinned SHA-256 hashes. Nothing here runs on her computer; it runs in CI only.
$ErrorActionPreference = 'Stop'

$TessdataTag = '4.1.0'
$Languages = @{
  'heb' = 'dbaa827aea6bc21215638447f17783a1004987c2d0bf5573d111fee397abdae5'
  'eng' = '8280aed0782fe27257a68ea10fe7ef324ca0f8d85bd2fd145d1c2b560bcb66ba'
  'osd' = '9cf5d576fcc47564f11265841e5ca839001e7e6f38ff7f7aacf46d15a96b00ff'
}

$dest = Join-Path $PSScriptRoot '..\ocr'
$data = Join-Path $dest 'tessdata'
New-Item -ItemType Directory -Force -Path $data | Out-Null

# 1. The engine.
$vcpkg = $env:VCPKG_INSTALLATION_ROOT
if (-not $vcpkg -or -not (Test-Path (Join-Path $vcpkg 'vcpkg.exe'))) { throw 'vcpkg not found on this runner' }
Write-Host "vcpkg commit: $(git -C $vcpkg rev-parse HEAD)"
& (Join-Path $vcpkg 'vcpkg.exe') install 'tesseract:x64-windows-static' --clean-after-build --disable-metrics
if ($LASTEXITCODE -ne 0) { throw 'tesseract build failed' }
$exe = Get-ChildItem -Recurse -Filter 'tesseract.exe' (Join-Path $vcpkg 'installed\x64-windows-static\tools') | Select-Object -First 1
if (-not $exe) { throw 'tesseract.exe not found after the build' }
Copy-Item $exe.FullName (Join-Path $dest 'tesseract.exe') -Force
& (Join-Path $dest 'tesseract.exe') --version

# 2. The language data (tessdata_best: the most accurate Hebrew).
foreach ($lang in $Languages.Keys) {
  $file = Join-Path $data "$lang.traineddata"
  $url = "https://raw.githubusercontent.com/tesseract-ocr/tessdata_best/$TessdataTag/$lang.traineddata"
  Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $file
  $hash = (Get-FileHash $file -Algorithm SHA256).Hash.ToLower()
  if ($hash -ne $Languages[$lang]) { throw "$lang.traineddata: hash $hash does not match the pinned one" }
}
Get-ChildItem -Recurse $dest | Select-Object FullName, Length
