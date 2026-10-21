# Read-only policy check. Never starts Sandbox, an installer, or a desktop app.
$ErrorActionPreference = 'Stop'
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$packageRoot = Join-Path $projectRoot '.cache/m7-writer-probe-package'
$fixtureRoot = Join-Path $projectRoot 'tests/fixtures/g1'
function Assert-Safe([bool]$condition, [string]$reason) {
    if (-not $condition) { throw $reason }
}
function Assert-PlainPath([string]$path) {
    $current = Get-Item -LiteralPath $path -Force
    while ($null -ne $current) {
        Assert-Safe (($current.Attributes -band [IO.FileAttributes]::ReparsePoint) -eq 0) 'Linked mapping or ancestor refused'
        $current = if ($current -is [IO.FileInfo]) { $current.Directory } else { $current.Parent }
    }
}
function Test-Config([xml]$xml) {
    $root = $xml.DocumentElement
    Assert-Safe ($root.Name -ceq 'Configuration' -and $root.Attributes.Count -eq 0) 'Unexpected root'
    $expected = @{
        vGPU='Disable'; Networking='Disable'; AudioInput='Disable'; VideoInput='Disable'
        ProtectedClient='Enable'; PrinterRedirection='Disable'; ClipboardRedirection='Disable'; MemoryInMB='4096'
    }
    Assert-Safe ($root.SelectNodes('./*').Count -eq 9) 'Unexpected, duplicated or automatic command element'
    foreach ($key in $expected.Keys) {
        $nodes = $root.SelectNodes("./$key")
        Assert-Safe ($nodes.Count -eq 1) 'Missing/duplicate option'
        Assert-Safe ($nodes[0].Attributes.Count -eq 0 -and $nodes[0].SelectNodes('./*').Count -eq 0) 'Nested option refused'
        Assert-Safe ($nodes[0].InnerText -ceq $expected[$key]) "Unsafe option: $key"
    }
    $mapping = $root.SelectSingleNode('./MappedFolders')
    Assert-Safe ($null -ne $mapping -and $mapping.Attributes.Count -eq 0 -and $mapping.SelectNodes('./*').Count -eq 2) 'Mapping set differs'
    $allowed = @($packageRoot,$fixtureRoot)
    $guests = @('C:\M7ReadOnly\Package','C:\M7ReadOnly\Samples')
    $folders = $mapping.SelectNodes('./MappedFolder')
    Assert-Safe ($folders.Count -eq 2) 'Unknown mapping element'
    for ($i=0; $i -lt 2; $i++) {
        $node=$folders[$i]
        Assert-Safe ($node.Attributes.Count -eq 0 -and $node.SelectNodes('./*').Count -eq 3) 'Mapping fields differ'
        foreach ($name in @('HostFolder','SandboxFolder','ReadOnly')) {
            $fields=$node.SelectNodes("./$name")
            Assert-Safe ($fields.Count -eq 1 -and $fields[0].Attributes.Count -eq 0 -and $fields[0].SelectNodes('./*').Count -eq 0) 'Unexpected mapping field'
        }
        Assert-Safe ([IO.Path]::GetFullPath($node.HostFolder) -ieq $allowed[$i]) 'Unexpected host folder; no inspection performed'
        Assert-Safe ($node.SandboxFolder -ceq $guests[$i] -and $node.ReadOnly -ceq 'true') 'Writable or redirected mapping refused'
    }
}
$settings=[Xml.XmlReaderSettings]::new()
$settings.DtdProcessing=[Xml.DtdProcessing]::Prohibit
$settings.XmlResolver=$null
$reader=[Xml.XmlReader]::Create((Join-Path $projectRoot 'src-tauri/installer/m7-isolated.wsb'),$settings)
try { $config=[Xml.XmlDocument]::new();$config.XmlResolver=$null;$config.Load($reader) } finally { $reader.Dispose() }
Test-Config $config
Assert-PlainPath $packageRoot
Assert-PlainPath $fixtureRoot
$packageName='AI-Native PDF Reader_0.1.8_x64-setup.exe'
$packages=@{
    'AI-Native PDF Reader_0.1.5_x64-setup.exe'='f7931faa9194609ea37cac2beeb72db8f70f1d237ae1db5b8180716dea7edccf'
    'AI-Native PDF Reader_0.1.6_x64-setup.exe'='79af05bef615c2e05a01972c1d36c141f4c59a7d6bfc93028781f4b492a7fc8f'
    'AI-Native PDF Reader_0.1.7_x64-setup.exe'='3dcd0caf2f9a8b231cb9b826db8efa1e8c55f14d8acfe20f96cc66301ff3dcff'
    'AI-Native PDF Reader_0.1.8_x64-setup.exe'='89e65f47ccc786f1b91f5fa2c3603b91bba2028804b8c3bd3a8d4c632ae6b24e'
}
$entries=@(Get-ChildItem -LiteralPath $packageRoot -Force)
$diagnostics=@{
    'diagnose-writer-profile.ps1'='b1f99ea5b3002371984e036fba05c506f80c021b52d248ab15a79940c1fdffee'
    'diagnose-writer-profile.cmd'='f237e8767d7993d33809d02eebe7b3a4239c3081b8b4f588c7887425c829aefd'
}
$packageNames=@($packages.Keys)+@($diagnostics.Keys)
Assert-Safe ($entries.Count -eq $packageNames.Count) 'Package share contains unexpected content'
foreach ($entry in $entries) {
    Assert-Safe (-not $entry.PSIsContainer -and $packageNames -ccontains $entry.Name) 'Unknown package share file'
    Assert-PlainPath $entry.FullName
    if ($packages.ContainsKey($entry.Name)) {
        Assert-Safe ((Get-FileHash -LiteralPath $entry.FullName).Hash -ieq $packages[$entry.Name]) 'Package changed; re-review before guest execution'
    }
    if ($diagnostics.ContainsKey($entry.Name)) {
        Assert-Safe ((Get-FileHash -LiteralPath $entry.FullName).Hash -ieq $diagnostics[$entry.Name]) 'Read-only diagnostic changed; re-review before guest execution'
        $source=Join-Path $PSScriptRoot $entry.Name;Assert-PlainPath $source
        Assert-Safe ((Get-FileHash -LiteralPath $source).Hash -ieq $diagnostics[$entry.Name]) 'Diagnostic share differs from reviewed source'
    }
}
$packageHash=(Get-FileHash -LiteralPath (Join-Path $packageRoot $packageName) -Algorithm SHA256).Hash
Assert-Safe ($packageHash -ieq $packages[$packageName]) 'Package changed; re-review before guest execution'
$names=@('plain.pdf','geometry.pdf','freetext-control.pdf')
$fixtureEntries=@(Get-ChildItem -LiteralPath $fixtureRoot -Force)
$allowedNames=@('manifest.json')+$names
Assert-Safe ($fixtureEntries.Count -eq $allowedNames.Count) 'Fixture share contents differ'
foreach ($entry in $fixtureEntries) {
    Assert-Safe (-not $entry.PSIsContainer -and $allowedNames -ccontains $entry.Name) 'Unexpected fixture entry'
    Assert-PlainPath $entry.FullName
}
$manifestPath=Join-Path $fixtureRoot 'manifest.json'
Assert-PlainPath $manifestPath
Assert-Safe ((Get-FileHash -LiteralPath $manifestPath -Algorithm SHA256).Hash -ieq 'ddb38c52ea6facfa10353ca5b5aedea748cccf97d41f1cf1c9d6575225686ed9') 'Control manifest changed; re-review before guest execution'
$manifest=Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
Assert-Safe ($manifest.files.Count -eq 3) 'Fixture manifest differs'
foreach ($name in $names) {
    $records=@($manifest.files | Where-Object name -CEQ $name)
    Assert-Safe ($records.Count -eq 1) 'Fixture manifest differs'
    $path=Join-Path $fixtureRoot $name;Assert-PlainPath $path
    Assert-Safe ((Get-FileHash -LiteralPath $path).Hash -ieq $records[0].sha256 -and (Get-Item -LiteralPath $path).Length -eq $records[0].bytes) 'Control bytes differ'
}
$rejected=0
foreach ($kind in @('network','write-map','whole-project','auto-command')) {
    $bad=$config.CloneNode($true)
    switch ($kind) {
        network { $bad.Configuration.Networking='Enable' }
        write-map { $bad.Configuration.MappedFolders.MappedFolder[0].ReadOnly='false' }
        whole-project { $bad.Configuration.MappedFolders.MappedFolder[0].HostFolder=$projectRoot }
        auto-command { $node=$bad.CreateElement('LogonCommand');$node.InnerText='cmd.exe';[void]$bad.DocumentElement.AppendChild($node) }
    }
    $denied=$false;try { Test-Config $bad } catch { $denied=$true }
    Assert-Safe $denied 'Unsafe configuration accepted';$rejected++
}
[pscustomobject]@{Status='PASS';Scope='static configuration and trusted input metadata only';UnsafeVariantsRejected=$rejected
    Networking='Disabled';WritableMappings=0;AutomaticCommands=0;RequestedMemoryMiB=4096;PackageSha256=$packageHash
    ReadOnlyDiagnosticFiles=2
    SandboxStarted=$false;InstallationPerformed=$false;HostSystemChanges=0} | ConvertTo-Json
