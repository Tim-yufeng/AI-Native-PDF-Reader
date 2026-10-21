# Project-local input preparation only. Does not start Sandbox or modify the host.
param([switch]$ResumePreparation)
$ErrorActionPreference='Stop'
$root=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$out=Join-Path $root '.cache/worker-cli-lab/input'
function Plain([string]$path) {
    $full=[IO.Path]::GetFullPath($path)
    if (-not $full.StartsWith($root+'\',[StringComparison]::OrdinalIgnoreCase)) { throw 'Outside project' }
    while (-not (Test-Path -LiteralPath $full)) {$full=[IO.Path]::GetDirectoryName($full)}
    $item=Get-Item -LiteralPath $full -Force
    while ($null -ne $item) {if(($item.Attributes-band [IO.FileAttributes]::ReparsePoint)-ne 0){throw 'Linked input'};$item=if($item-is[IO.FileInfo]){$item.Directory}else{$item.Parent}}
}
Plain $out
$names=@('runner.exe','node.exe','matrix-source.pdf','matrix-candidate.pdf','guest.ps1','query.ps1','verify-pdf.mjs','pdf.mjs','pdf.worker.mjs','scope.json','inputs.json')
if(Test-Path -LiteralPath $out){
    if(-not $ResumePreparation){throw 'Existing inputs preserved; explicit controlled refresh required'}
    foreach($item in Get-ChildItem -LiteralPath $out -Force){Plain $item.FullName;if($item.PSIsContainer-or $names-cnotcontains$item.Name){throw 'Unknown lab input preserved'}}
}
New-Item -ItemType Directory -Path $out -Force | Out-Null
$files=@{
 'runner.exe'=(Join-Path $root '.cache/tauri-target/debug/ai-native-pdf-reader.exe')
 'matrix-source.pdf'=(Join-Path $root 'tests/work/writer/xref-fixed/matrix-source.pdf')
 'matrix-candidate.pdf'=(Join-Path $root 'tests/work/writer/xref-fixed/matrix-candidate.pdf')
 'guest.ps1'=(Join-Path $root 'scripts/worker-cli-guest.ps1')
 'query.ps1'=(Join-Path $root 'scripts/worker-cli-query.ps1')
 'verify-pdf.mjs'=(Join-Path $root 'scripts/verify-worker-cli-pdf.mjs')
 'pdf.mjs'=(Join-Path $root 'node_modules/pdfjs-dist/legacy/build/pdf.mjs')
 'pdf.worker.mjs'=(Join-Path $root 'node_modules/pdfjs-dist/legacy/build/pdf.worker.mjs')
}
foreach($name in $files.Keys){Plain $files[$name];Copy-Item -LiteralPath $files[$name] -Destination (Join-Path $out $name)}
# Existing trusted OS-installed Node, copied once; no download or installation.
$node='C:\Program Files\nodejs\node.exe'
if(-not(Test-Path -LiteralPath $node)){throw 'Approved existing Node unavailable; no install permitted'}
Copy-Item -LiteralPath $node -Destination (Join-Path $out 'node.exe')
$scope=@{scope='ai-native-pdf-reader/guest-cli/v1';authorizeGuestOnlySetup=$true;workerPermissions='unchanged-production-LPAC';gui=$false}
[IO.File]::WriteAllText((Join-Path $out 'scope.json'),($scope|ConvertTo-Json),[Text.UTF8Encoding]::new($false))
$inventory=@(Get-ChildItem -LiteralPath $out -File|Where-Object {$_.Name-cne'inputs.json'}|Sort-Object Name|ForEach-Object {@{name=$_.Name;bytes=$_.Length;sha256=(Get-FileHash -LiteralPath $_.FullName).Hash.ToLowerInvariant()}})
[IO.File]::WriteAllText((Join-Path $out 'inputs.json'),(@{schema=1;files=$inventory}|ConvertTo-Json -Depth 4),[Text.UTF8Encoding]::new($false))
$xml=@"
<Configuration>
 <vGPU>Disable</vGPU><Networking>Disable</Networking><AudioInput>Disable</AudioInput><VideoInput>Disable</VideoInput>
 <ProtectedClient>Enable</ProtectedClient><PrinterRedirection>Disable</PrinterRedirection><ClipboardRedirection>Disable</ClipboardRedirection><MemoryInMB>4096</MemoryInMB>
 <MappedFolders>
  <MappedFolder><HostFolder>$out</HostFolder><SandboxFolder>C:\WriterReadOnly\Input</SandboxFolder><ReadOnly>true</ReadOnly></MappedFolder>
  <MappedFolder><HostFolder>$root\.cache\python-worker</HostFolder><SandboxFolder>C:\WriterReadOnly\Runtime</SandboxFolder><ReadOnly>true</ReadOnly></MappedFolder>
 </MappedFolders>
</Configuration>
"@
[IO.File]::WriteAllText((Join-Path $root '.cache/worker-cli-lab/config.wsb'),$xml,[Text.UTF8Encoding]::new($false))
@{status='prepared';inputBytes=(Get-ChildItem -LiteralPath $out -File|Measure-Object -Property Length -Sum).Sum;files=$inventory.Count;installerGenerated=$false;hostChanges=$false}|ConvertTo-Json -Compress
