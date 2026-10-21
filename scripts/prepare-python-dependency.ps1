# One fixed official wheel, downloaded into the project only. No pip/install.
$ErrorActionPreference='Stop'
$projectRoot=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$lock=Get-Content -LiteralPath (Join-Path $PSScriptRoot 'python-worker-dependencies.json') -Raw | ConvertFrom-Json
if ($lock.schema -ne 1 -or $lock.packages.pypdf -cne '6.20.1' -or $lock.pypdfWheel.filename -cne 'pypdf-6.20.1-py3-none-any.whl') { throw 'Unexpected fixed dependency lock' }
$directory=Join-Path $projectRoot '.cache/python-wheels'
$cacheRoot=Join-Path $projectRoot '.cache'
$current=Get-Item -LiteralPath $(if (Test-Path -LiteralPath $cacheRoot) {$cacheRoot} else {$projectRoot}) -Force
while ($null -ne $current) {
    if (($current.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'Linked project ancestor refused' }
    $current=$current.Parent
}
if (Test-Path -LiteralPath $directory) {
    $entry=Get-Item -LiteralPath $directory -Force
    if (-not $entry.PSIsContainer -or ($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'Linked wheel cache refused' }
} else { [void](New-Item -ItemType Directory -Path $directory) }
$target=Join-Path $directory $lock.pypdfWheel.filename
if (-not (Test-Path -LiteralPath $target)) {
    $uri=[Uri]$lock.pypdfWheel.url
    if ($uri.Scheme -cne 'https' -or $uri.Host -cne 'files.pythonhosted.org') { throw 'Nonofficial dependency source refused' }
    Invoke-WebRequest -Uri $uri.AbsoluteUri -OutFile $target -MaximumRedirection 0 -TimeoutSec 60
}
$entry=Get-Item -LiteralPath $target -Force
if ($entry.PSIsContainer -or ($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0 -or $entry.Length -ne $lock.pypdfWheel.bytes -or (Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash -ine $lock.pypdfWheel.sha256) { throw 'Wheel differs; retained for inspection, not installed' }
[pscustomobject]@{Status='PASS';Version=$lock.packages.pypdf;Path=$target;Bytes=$entry.Length;Sha256=$lock.pypdfWheel.sha256;GlobalInstallation=$false} | ConvertTo-Json
