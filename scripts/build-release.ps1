# Builds the Release executable for one purpose, using the same toolchain the
# project's desktop script uses. -Purpose lab is the hidden feature-gated App
# lab, which cannot be packaged; -Purpose release is the ordinary build that
# every installer must be made from.
param(
  [ValidateSet('lab', 'release')][string]$Purpose = 'release',
  [switch]$Quiet
)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$rust = 'D:/DevTools/Rust'
$toolchain = '1.98.1-x86_64-pc-windows-msvc'
$vswhere = 'C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe'
if (-not (Test-Path $vswhere)) { throw 'Visual Studio locator is missing; do not install tools automatically' }
$vs = (& $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath)
$vs = ("$vs").Trim()
if (-not $vs) { throw 'No Visual Studio C++ toolset was found; do not install tools automatically' }
$dev = Join-Path $vs 'Common7\Tools\VsDevCmd.bat'
foreach ($line in (& cmd.exe /d /c "call `"$dev`" -no_logo -host_arch=x64 -arch=x64 && set")) {
  $i = $line.IndexOf('=')
  if ($i -gt 0) { [Environment]::SetEnvironmentVariable($line.Substring(0, $i), $line.Substring($i + 1), 'Process') }
}
$env:CARGO_HOME = "$rust/cargo"
$env:RUSTUP_HOME = "$rust/rustup"
$env:RUSTUP_TOOLCHAIN = $toolchain
$env:CARGO_TARGET_DIR = Join-Path $root '.cache/tauri-target'
$env:CARGO_BUILD_JOBS = '4'
$env:TEMP = Join-Path $root '.cache/tmp'
$env:TMP = $env:TEMP
New-Item -ItemType Directory -Force -Path $env:TEMP | Out-Null

# A stale hard-linked artifact from the other purpose would be launched by
# mistake, so the leaf is removed first and the build must recreate it.
$leaf = Join-Path $env:CARGO_TARGET_DIR 'release/deps/ai_native_pdf_reader.exe'
Remove-Item $leaf -Force -ErrorAction SilentlyContinue

# The lab needs Tauri's custom protocol; without it `tauri::is_dev()` is compiled
# true and the lab entry point exits before doing anything.
$features = if ($Purpose -eq 'lab') { 'save-chain-lab,tauri/custom-protocol' } else { 'real-provider,tauri/custom-protocol' }
$cargo = "$rust/cargo/bin/cargo.exe"
# Cargo writes progress to stderr; only the exit code and the marker decide
# success, so the quiet path must not turn progress into a terminating error.
$previous = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
if ($Quiet) {
  & $cargo build --manifest-path (Join-Path $root 'src-tauri/Cargo.toml') --locked --release --features $features 2>&1 |
    Select-String -Pattern 'Finished|^error' | ForEach-Object { $_.Line }
} else {
  & $cargo build --manifest-path (Join-Path $root 'src-tauri/Cargo.toml') --locked --release --features $features
}
$code = $LASTEXITCODE
$ErrorActionPreference = $previous
if ($code -ne 0) { throw "cargo build failed with exit code $code" }

$exe = Join-Path $env:CARGO_TARGET_DIR 'release/ai-native-pdf-reader.exe'
if (-not (Test-Path $exe)) { throw "the build produced no executable at $exe" }
$bytes = [System.IO.File]::ReadAllBytes($exe)
# Windows PowerShell 5.1 has no Latin1 encoding, so the marker is matched as
# bytes. A Release build with `strip = true` legitimately drops the lab's marker
# literal, so the lab is identified by its behaviour instead.
$markerBytes = [System.Text.Encoding]::ASCII.GetBytes('AI_NATIVE_SAVE_CHAIN_LAB_ONLY_v1')
$marker = $false
for ($i = 0; $i -le $bytes.Length - $markerBytes.Length; $i++) {
  if ($bytes[$i] -eq $markerBytes[0]) {
    $same = $true
    for ($j = 1; $j -lt $markerBytes.Length; $j++) { if ($bytes[$i + $j] -ne $markerBytes[$j]) { $same = $false; break } }
    if ($same) { $marker = $true; break }
  }
}
if ($Purpose -eq 'release' -and $marker) { throw 'the ordinary build must not contain the hidden lab' }
"{0} build: {1} bytes, marker={2}, {3}" -f $Purpose, $bytes.Length, $marker, $exe
