# Reusable fixed CLI lab. Only our newly created Sandbox ID may be changed/stopped.
param([switch]$CheckOnly)
$ErrorActionPreference='Stop'
$root=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$lab=Join-Path $root '.cache/worker-cli-lab'
$cli=Join-Path $env:LOCALAPPDATA 'Microsoft\WindowsApps\wsb.exe'
function Require([bool]$ok,[string]$why){if(-not $ok){throw $why}}
function Plain([string]$path){$item=Get-Item -LiteralPath $path -Force;while($null-ne$item){Require (($item.Attributes-band[IO.FileAttributes]::ReparsePoint)-eq 0) 'Linked input refused';$item=if($item-is[IO.FileInfo]){$item.Directory}else{$item.Parent}}}
function Validate([xml]$xml){
 $doc=$xml.DocumentElement;Require ($doc.Name-ceq'Configuration'-and $doc.Attributes.Count-eq 0 -and $doc.SelectNodes('./*').Count-eq 9) 'Unexpected configuration'
 $expected=@{vGPU='Disable';Networking='Disable';AudioInput='Disable';VideoInput='Disable';ProtectedClient='Enable';PrinterRedirection='Disable';ClipboardRedirection='Disable';MemoryInMB='4096'}
 foreach($name in $expected.Keys){$nodes=$doc.SelectNodes("./$name");Require ($nodes.Count-eq 1 -and $nodes[0].Attributes.Count-eq 0 -and $nodes[0].SelectNodes('./*').Count-eq 0 -and $nodes[0].InnerText-ceq$expected[$name]) 'Unsafe Sandbox option'}
 $sets=$doc.SelectNodes('./MappedFolders');Require ($sets.Count-eq 1-and $sets[0].Attributes.Count-eq 0-and $sets[0].SelectNodes('./*').Count-eq 2) 'Unexpected mapping set'
 $maps=$sets[0].SelectNodes('./MappedFolder');Require ($maps.Count-eq 2) 'Unexpected mappings'
 foreach($i in 0,1){
  $m=$maps[$i];Require ($m.Attributes.Count-eq 0-and $m.SelectNodes('./*').Count-eq 3) 'Unexpected mapping fields'
  foreach($name in @('HostFolder','SandboxFolder','ReadOnly')){$fields=$m.SelectNodes("./$name");Require ($fields.Count-eq 1-and $fields[0].Attributes.Count-eq 0-and $fields[0].SelectNodes('./*').Count-eq 0) 'Duplicate/nested mapping field'}
  $mappedHost=if($i-eq 0){Join-Path $lab 'input'}else{Join-Path $root '.cache/python-worker'};$guest=if($i-eq 0){'C:\WriterReadOnly\Input'}else{'C:\WriterReadOnly\Runtime'}
  Require ($m.SelectSingleNode('./HostFolder').InnerText-ieq$mappedHost-and $m.SelectSingleNode('./SandboxFolder').InnerText-ceq$guest-and $m.SelectSingleNode('./ReadOnly').InnerText-ceq'true') 'Mapping is not the fixed read-only set';Plain $mappedHost
 }
}
Plain (Join-Path $lab 'config.wsb')
$settings=[Xml.XmlReaderSettings]::new();$settings.DtdProcessing=[Xml.DtdProcessing]::Prohibit;$settings.XmlResolver=$null
$reader=[Xml.XmlReader]::Create((Join-Path $lab 'config.wsb'),$settings)
try{$xml=[Xml.XmlDocument]::new();$xml.XmlResolver=$null;$xml.Load($reader)}finally{$reader.Dispose()}
Validate $xml
$inventory=Get-Content -LiteralPath (Join-Path $lab 'input/inputs.json') -Raw | ConvertFrom-Json
$allowed=@('runner.exe','node.exe','matrix-source.pdf','matrix-candidate.pdf','guest.ps1','query.ps1','verify-pdf.mjs','pdf.mjs','pdf.worker.mjs','scope.json')
Require ($inventory.files.Count-eq $allowed.Count) 'Inventory differs'
$entries=@(Get-ChildItem -LiteralPath (Join-Path $lab 'input') -Force)
Require ($entries.Count-eq ($allowed.Count+1)) 'Unknown input files'
foreach($entry in $inventory.files){Require ($allowed-ccontains$entry.name) 'Unknown input';$path=Join-Path $lab ('input/'+$entry.name);Plain $path;Require ((Get-Item -LiteralPath $path).Length-eq$entry.bytes-and (Get-FileHash -LiteralPath $path).Hash-ieq$entry.sha256) 'Input identity differs'}
foreach($option in @('Networking','ClipboardRedirection','ProtectedClient')){$bad=[xml]$xml.OuterXml;$bad.Configuration.$option=if($option-ceq'ProtectedClient'){'Disable'}else{'Enable'};$rejected=$false;try{Validate $bad}catch{$rejected=$true};Require $rejected 'Dangerous variant accepted'}
$bad=[xml]$xml.OuterXml;$bad.Configuration.MappedFolders.MappedFolder[0].ReadOnly='false';$rejected=$false;try{Validate $bad}catch{$rejected=$true};Require $rejected 'Writable mapping accepted'
foreach($field in @('ReadOnly','HostFolder','SandboxFolder')){$bad=[xml]$xml.OuterXml;$map=$bad.SelectSingleNode('/Configuration/MappedFolders/MappedFolder');$duplicate=$map.SelectSingleNode("./$field").CloneNode($true);$map.AppendChild($duplicate)|Out-Null;$rejected=$false;try{Validate $bad}catch{$rejected=$true};Require $rejected 'Duplicate mapping accepted'}
$bad=[xml]$xml.OuterXml;$map=$bad.SelectSingleNode('/Configuration/MappedFolders/MappedFolder');$map.AppendChild($bad.CreateElement('Extra'))|Out-Null;$rejected=$false;try{Validate $bad}catch{$rejected=$true};Require $rejected 'Unknown mapping field accepted'
if($CheckOnly){@{status='PASS';checks='fixed inputs/config plus 8 dangerous variants';sandboxStarted=$false}|ConvertTo-Json -Compress;exit 0}
Require (Test-Path -LiteralPath $cli) 'Existing Sandbox CLI missing; no install permitted'
function Call-Cli([string[]]$arguments){$text=(& $cli @arguments 2>$null|Out-String);try{return ($text|ConvertFrom-Json)}catch{throw 'Sandbox CLI did not return its documented JSON result'}}
function Exit-Code([string]$id,[string]$command){$data=Call-Cli @('exec','--id',$id,'--run-as','System','--command',$command,'--raw');$property=$data.PSObject.Properties['ExitCode'];Require ($null-ne$property) 'Sandbox CLI exit-code field unavailable';return [int64]$property.Value}
$id=[Guid]::NewGuid().ToString();$record=Join-Path $lab ('run-'+$id+'.json');$created=$false
$before=Call-Cli @('list','--raw');Require (-not (($before|ConvertTo-Json -Depth 6)-match[Regex]::Escape($id))) 'Session ID already present'
$evidence=@{sandboxId=$id;status='BLOCKED';network=$false;clipboard=$false;protectedClient=$true;hostMappingsReadOnly=$true;hostProfileCreated=$false;hostAclChanged=$false;guiOperated=$false;installerGenerated=$false;scope='ordinary guest user -> production LPAC -> independent PDF.js; not GUI/Edge/Adobe'}
try {
 $created=$true
 $start=Call-Cli @('start','--id',$id,'--config',$xml.OuterXml,'--raw')
 $list=Call-Cli @('list','--raw');Require (($list|ConvertTo-Json -Depth 6)-match[Regex]::Escape($id)) 'New Sandbox ID not confirmed'
 $evidence['start']=$start
 $smoke=Exit-Code $id 'cmd.exe /d /c exit 42';Require ($smoke-eq 42) 'Sandbox return-code protocol not confirmed';$evidence['exitProtocolConfirmed']=$true
 $code=Exit-Code $id 'powershell.exe -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File C:\WriterReadOnly\Input\guest.ps1'
 $evidence['executionExit']=$code
 $diagnostics=@{}
 foreach($field in @('gate','stage','reason','windows','worker','validation')){$diagnostics[$field]=Exit-Code $id ("powershell.exe -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File C:\WriterReadOnly\Input\query.ps1 -Field "+$field)}
 $evidence['diagnostics']=$diagnostics
 if($code-eq 0 -and $diagnostics.gate-eq 0 -and $diagnostics.validation-eq 0){
    $evidence.status='PASS';$evidence['outputBytes']=Exit-Code $id 'powershell.exe -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File C:\WriterReadOnly\Input\query.ps1 -Field bytes'
    $digest='';foreach($n in 0..7){$part=Exit-Code $id ("powershell.exe -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File C:\WriterReadOnly\Input\query.ps1 -Field hash"+$n);$digest+=('{0:x8}'-f ($part-band 4294967295))};$evidence['outputSha256']=$digest
 }
}catch{$evidence['controllerFailure']='environment-or-command-protocol';throw}
finally {
 if($created){try{$stop=Call-Cli @('stop','--id',$id,'--raw');$remaining=Call-Cli @('list','--raw');$gone=-not (($remaining|ConvertTo-Json -Depth 6)-match[Regex]::Escape($id));$evidence['sandboxStopped']=$gone;Require $gone 'Owned Sandbox cleanup unconfirmed'}catch{$evidence.status='BLOCKED';$evidence['sandboxStopped']=$false}}
 [IO.File]::WriteAllText($record,($evidence|ConvertTo-Json -Depth 8),[Text.UTF8Encoding]::new($false))
 $evidence|ConvertTo-Json -Depth 8
}
if($evidence.status-cne'PASS'){exit 1}
