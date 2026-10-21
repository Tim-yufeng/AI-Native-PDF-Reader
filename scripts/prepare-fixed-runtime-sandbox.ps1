# Prepares one manual Windows Sandbox run of the bundled fixed-runtime package.
# Host-side only: it copies the built installer and discardable samples into the
# read-only mapped folder, writes the short acceptance steps and clears the
# previous run's results folder. It installs nothing on the host, touches no
# registry key and changes no Windows feature.
$root = Split-Path -Parent $PSScriptRoot
$packageDirectory = Join-Path $root '.cache\m8-fixed-runtime-package'
$resultsDirectory = Join-Path $root '.cache\m8-fixed-runtime-results'
$packageName = 'AI-Native PDF Reader_0.2.0_fixed-runtime-x64-setup.exe'
$bundle = Join-Path $root '.cache\tauri-target\release\bundle\nsis\AI-Native PDF Reader_0.2.0_x64-setup.exe'

New-Item -ItemType Directory -Force -Path $packageDirectory, $resultsDirectory | Out-Null

if (-not (Test-Path -LiteralPath $bundle)) { throw "the fixed-runtime bundle is missing: $bundle" }
$bundleInfo = Get-Item -LiteralPath $bundle
$packagePath = Join-Path $packageDirectory $packageName
Copy-Item -LiteralPath $bundle -Destination $packagePath -Force

# Synthetic, discardable samples only: the checked-in fixture set first, then the
# previously assembled sandbox copy for files that are generated rather than
# checked in. No personal courseware is ever copied here.
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

$hash = (Get-FileHash -LiteralPath $packagePath -Algorithm SHA256).Hash.ToLower()
Set-Content -LiteralPath (Join-Path $packageDirectory 'package-sha256.txt') -Value $hash -Encoding ASCII

$steps = @(
  'AI-Native PDF Reader 0.2.0 — bundled fixed-version WebView2 package, manual acceptance'
  ''
  'What this package changes: it carries Microsoft Fixed Version WebView2 155.0.4283.45 (x64)'
  'inside its own install directory, so the reader never needs the host/sandbox Evergreen'
  'runtime registration and never runs a WebView2 installer.'
  ''
  '1) Install (the installer runs in this sandbox only; nothing is installed on the host):'
  ('   "' + $packagePath.Replace($packageDirectory, 'C:\M8ReadOnly\Package') + '"')
  '   The window is silent-capable: add /S for a silent install if you prefer.'
  ''
  '2) Confirm the runtime landed in the sandbox-local install directory (not a mapped host path):'
  '   %LOCALAPPDATA%\Programs\AI-Native PDF Reader\webview2-fixed-runtime\msedgewebview2.exe'
  ''
  '3) Launch the reader from the Start menu or that folder and open a sample:'
  '   C:\M8ReadOnly\Package\plain.pdf     (also geometry.pdf, freetext-control.pdf, matrix-4page.pdf)'
  ''
  '4) Acceptance to observe:'
  '   - the window renders the reader UI (not a WebView2 error dialog, not a blank window);'
  '   - Save As (MuPDF.js writer) now starts: this build checks its own wasm engine and pinned'
  '     font bytes against the compiled-in lock, which the 0.2.0 build resolved from a'
  '     build-machine path and could never satisfy once installed;'
  '   - Save As produces a new file you can reopen, and Ctrl+S saves and reopens with the edits intact.'
  ''
  '   If the writer is still refused, copy the exact message text: a writer_resource_mismatch here'
  '   would mean the installed bytes differ from the locked ones, which is the one case to report back.'
  ''
  '5) Keep evidence in C:\M8Results (mapped to .cache\m8-fixed-runtime-results on the host):'
  '   screenshots, saved PDFs and any error text. The sandbox is discarded when it closes.'
  ''
  'Not covered here: the host Evergreen runtime state, any registry registration of the reader'
  'runtime, and the 0x80040902 investigation, which this round deliberately stops.'
) -join [Environment]::NewLine
Set-Content -LiteralPath (Join-Path $packageDirectory 'ACCEPTANCE-STEPS.txt') -Value $steps -Encoding UTF8

# Clear earlier results so the next run cannot be confused with a previous one.
Get-ChildItem -LiteralPath $resultsDirectory -File -ErrorAction SilentlyContinue |
  Where-Object { $_.Name -in @('guest-run.log', 'guest-result.json', 'guest-result.txt', 'guest-app-window.png') -or $_.Name -like '*.png' } |
  Remove-Item -Force -ErrorAction SilentlyContinue

[pscustomobject]@{
  package = $packagePath
  packageBytes = (Get-Item -LiteralPath $packagePath).Length
  bundleModified = $bundleInfo.LastWriteTime.ToString('o')
  packageSha256 = $hash
  readOnlyMapping = $packageDirectory
  resultsMapping = $resultsDirectory
  wsb = (Join-Path $root 'src-tauri\installer\m8-fixed-runtime-manual.wsb')
  launch = 'WindowsSandbox.exe ' + (Join-Path $root 'src-tauri\installer\m8-fixed-runtime-manual.wsb')
} | Format-List
