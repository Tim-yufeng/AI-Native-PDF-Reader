# Prepares one automated Windows Sandbox run of the offline WebView2 validation
# package. Host-side only: it copies files into the read-only mapped folder and
# clears the previous run's result files from the results folder. It never
# installs anything on the host and never changes host registry or features.
$root = Split-Path -Parent $PSScriptRoot
$packageDirectory = Join-Path $root '.cache\m8-offline-package'
$resultsDirectory = Join-Path $root '.cache\m8-offline-results'
$packageName = 'AI-Native PDF Reader_0.2.0_offline-x64-setup.exe'
$bundle = Join-Path $root '.cache\tauri-target\release\bundle\nsis\AI-Native PDF Reader_0.2.0_x64-setup.exe'

New-Item -ItemType Directory -Force -Path $packageDirectory, $resultsDirectory | Out-Null

if (-not (Test-Path -LiteralPath $bundle)) { throw "the offline validation bundle is missing: $bundle" }
Copy-Item -LiteralPath $bundle -Destination (Join-Path $packageDirectory $packageName) -Force

# Guest test script and its launcher.
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'guest-verify-offline-package.ps1') -Destination (Join-Path $packageDirectory 'guest-verify.ps1') -Force
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'guest-verify-offline-package.cmd') -Destination (Join-Path $packageDirectory 'guest-verify.cmd') -Force

# Synthetic, discardable samples only. The first source is the checked-in fixture
# set; the second is the previously assembled sandbox copy for files that are
# generated rather than checked in.
foreach ($name in @('plain.pdf', 'geometry.pdf', 'freetext-control.pdf', 'matrix-4page.pdf')) {
  $fixture = Join-Path $root "tests\fixtures\g1\$name"
  $cached = Join-Path $root ".cache\m8-mupdf-package\$name"
  if (Test-Path -LiteralPath $fixture) {
    Copy-Item -LiteralPath $fixture -Destination (Join-Path $packageDirectory $name) -Force
  } elseif (Test-Path -LiteralPath $cached) {
    Copy-Item -LiteralPath $cached -Destination (Join-Path $packageDirectory $name) -Force
  } else {
    Write-Warning "sample not found and not copied: $name"
  }
}

# The guest script compares the transported package against this hash.
$packagePath = Join-Path $packageDirectory $packageName
$hash = (Get-FileHash -LiteralPath $packagePath -Algorithm SHA256).Hash.ToLower()
Set-Content -LiteralPath (Join-Path $packageDirectory 'package-sha256.txt') -Value $hash -Encoding ASCII

# Clear earlier results so the next run cannot be confused with a previous one.
Get-ChildItem -LiteralPath $resultsDirectory -File -ErrorAction SilentlyContinue |
  Where-Object { $_.Name -in @('guest-run.log', 'guest-result.json', 'guest-result.txt', 'guest-app-window.png') -or $_.Name -like 'edgeupdate-*.log' } |
  Remove-Item -Force -ErrorAction SilentlyContinue

[pscustomobject]@{
  package = $packagePath
  packageBytes = (Get-Item -LiteralPath $packagePath).Length
  packageSha256 = $hash
  readOnlyMapping = $packageDirectory
  resultsMapping = $resultsDirectory
  wsb = (Join-Path $root 'src-tauri\installer\m8-offline-package.wsb')
  launch = 'WindowsSandbox.exe ' + (Join-Path $root 'src-tauri\installer\m8-offline-package.wsb')
} | Format-List
