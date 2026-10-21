# Existing project-local compiler only. Never installs or modifies a toolchain.
$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
function Assert-PlainProjectPath([string]$path) {
    $full = [IO.Path]::GetFullPath($path)
    if (-not $full.StartsWith($root + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Build path outside project' }
    while (-not (Test-Path -LiteralPath $full)) { $full = [IO.Path]::GetDirectoryName($full) }
    $item = Get-Item -LiteralPath $full -Force
    while ($null -ne $item) {
        if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'Linked build path refused' }
        $item = if ($item -is [IO.FileInfo]) { $item.Directory } else { $item.Parent }
    }
}
$out = Join-Path $root '.cache/worker-entry'
Assert-PlainProjectPath $out
New-Item -ItemType Directory -Force -Path $out | Out-Null
$env:TEMP = Join-Path $root '.cache/tmp/worker-entry'
$env:TMP = $env:TEMP
Assert-PlainProjectPath $env:TEMP
New-Item -ItemType Directory -Force -Path $env:TEMP | Out-Null
$vs = (& 'C:/Program Files (x86)/Microsoft Visual Studio/Installer/vswhere.exe' -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath).Trim()
if (-not $vs) { throw 'Existing MSVC unavailable; no installation permitted' }
$init = Join-Path $vs 'Common7/Tools/VsDevCmd.bat'
function Compile([string]$source, [string]$name, [string]$options) {
    $exe = Join-Path $out $name
    $obj = Join-Path $out ($name + '.obj')
    $prefix = Join-Path $out $name
    foreach ($path in @((Join-Path $root $source),$exe,$obj,($prefix+'.lib'),($prefix+'.exp'))) { Assert-PlainProjectPath $path }
    $linkExtra = if ($name.EndsWith('.dll')) { ' /NOENTRY kernel32.lib' } else { '' }
    $command = 'call "' + $init + '" -no_logo -host_arch=x64 -arch=x64 >nul && cl.exe /nologo /W4 /WX /O2 /MT /GS /guard:cf /std:c11 /DUNICODE /D_UNICODE /Fo"' + $obj + '" /Fe"' + $exe + '" ' + $options + ' "' + (Join-Path $root $source) + '" /link /DYNAMICBASE /NXCOMPAT /HIGHENTROPYVA /INCREMENTAL:NO /GUARD:CF /IMPLIB:"' + $prefix + '.lib"' + $linkExtra
    & $env:ComSpec /d /c $command
    if ($LASTEXITCODE -ne 0) { throw 'Project entry compilation failed' }
}
Compile 'worker/windows_entry.c' 'python.exe' ''
Compile 'tests/native/worker_entry_stub.c' 'stub.dll' '/LD'
Compile 'tests/native/worker_entry_stub.c' 'no-entry.dll' '/DSTUB_WITHOUT_ENTRY /LD'
Assert-PlainProjectPath (Join-Path $out 'identity.json')
$identity = @{
    schema=1; sourceSha256=(Get-FileHash -LiteralPath (Join-Path $root 'worker/windows_entry.c')).Hash.ToLowerInvariant()
    exeSha256=(Get-FileHash -LiteralPath (Join-Path $out 'python.exe')).Hash.ToLowerInvariant()
    compiler='Existing MSVC 2026; /MT /GS /guard:cf; imports independently checked'
}
[IO.File]::WriteAllText((Join-Path $out 'identity.json'), ($identity | ConvertTo-Json), [Text.UTF8Encoding]::new($false))
[pscustomobject]@{Status='PASS';Output=$out;Runtime='static MSVC; dependency inventory must also pass';SystemChanges=0} | ConvertTo-Json -Compress
