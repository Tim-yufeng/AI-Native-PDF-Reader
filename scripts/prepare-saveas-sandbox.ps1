# Project-only material preparation. Does not install or launch anything, modify
# Windows configuration, or delete/overwrite earlier acceptance evidence.
[CmdletBinding()]
param([switch]$CheckOnly, [switch]$OriginalSave)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$root = [IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
$version = if ($OriginalSave) { '0.2.3' } else { '0.2.1' }
$kind = if ($OriginalSave) { 'save' } else { 'saveas' }
$config = Get-Content -LiteralPath (Join-Path $root 'src-tauri\tauri.conf.json') -Raw | ConvertFrom-Json
if ($config.version -ne $version) { throw 'Application version does not match this pinned acceptance entry.' }
$packageDirectory = Join-Path $root ".cache\m8-$kind-$version-package"
$installerName = "AI-Native PDF Reader_${version}_${kind}-fixed-runtime-x64-setup.exe"
$bundle = Join-Path $root ".cache\tauri-target\release\bundle\nsis\AI-Native PDF Reader_${version}_x64-setup.exe"
$wsb = Join-Path $root "src-tauri\installer\m8-$kind-$version-manual.wsb"

function Assert-ProjectPath([string]$Path, [switch]$MustExist) {
    $full = [IO.Path]::GetFullPath($Path)
    if (-not $full.StartsWith($root + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'A preparation path escaped this project.' }
    $current = $full
    while ($current.Length -gt $root.Length) {
        if (Test-Path -LiteralPath $current) {
            $item = Get-Item -LiteralPath $current -Force
            if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'Linked preparation paths are refused.' }
        }
        $current = Split-Path -Parent $current
    }
    if ($MustExist -and -not (Test-Path -LiteralPath $full -PathType Leaf)) { throw "Required project material is missing: $full" }
    return $full
}
function Hash-File([string]$Path) { return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant() }
function Assert-SameOrAbsent([string]$Path, [string]$ExpectedHash) {
    $null = Assert-ProjectPath $Path
    if (Test-Path -LiteralPath $Path) {
        if (-not (Test-Path -LiteralPath $Path -PathType Leaf) -or (Hash-File $Path) -ne $ExpectedHash) {
            throw "Existing acceptance material differs; it was retained and will not be overwritten: $Path"
        }
    }
}
function Hash-Bytes([byte[]]$Bytes) {
    $sha = [Security.Cryptography.SHA256]::Create()
    try { return ([BitConverter]::ToString($sha.ComputeHash($Bytes))).Replace('-', '').ToLowerInvariant() }
    finally { $sha.Dispose() }
}
function Write-NewOrSame([string]$Path, [byte[]]$Bytes) {
    Assert-SameOrAbsent $Path (Hash-Bytes $Bytes)
    if (Test-Path -LiteralPath $Path) { return }
    $stream = [IO.File]::Open($Path, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::Read)
    try { $stream.Write($Bytes, 0, $Bytes.Length); $stream.Flush($true) }
    finally { $stream.Dispose() }
}

$null = Assert-ProjectPath $bundle -MustExist
$null = Assert-ProjectPath $wsb -MustExist
$null = Assert-ProjectPath $packageDirectory
[xml]$sandboxConfig = Get-Content -LiteralPath $wsb -Raw
$mappings = @($sandboxConfig.Configuration.MappedFolders.MappedFolder)
if ($mappings.Count -ne 1 -or $mappings[0].HostFolder -ne $packageDirectory -or $mappings[0].SandboxFolder -ne 'C:\M8ReadOnly\Package' -or $mappings[0].ReadOnly -ne 'true') {
    throw 'The Sandbox must have exactly one pinned read-only project mapping.'
}
foreach ($setting in @('vGPU', 'Networking', 'AudioInput', 'VideoInput', 'PrinterRedirection', 'ClipboardRedirection')) {
    if ($sandboxConfig.Configuration.$setting -ne 'Disable') { throw 'A Sandbox isolation setting drifted.' }
}
if ($sandboxConfig.Configuration.ProtectedClient -ne 'Enable' -or $sandboxConfig.Configuration.MemoryInMB -ne '4096') { throw 'The pinned Sandbox protection or memory setting drifted.' }
if ($sandboxConfig.Configuration.LogonCommand.Command -ne 'cmd.exe /k chcp 65001 & type "C:\M8ReadOnly\Package\ACCEPTANCE-STEPS.txt"') { throw 'The Sandbox logon command may only display the fixed acceptance steps.' }
$bundleInfo = Get-Item -LiteralPath $bundle
if ($bundleInfo.Length -lt 1MB) { throw 'The NSIS bundle appears incomplete.' }
$metadata = [Diagnostics.FileVersionInfo]::GetVersionInfo($bundle)
# NSIS records VIProductVersion in the fixed version fields; the optional
# ProductVersion string is absent in the reviewed template.
if ($metadata.FileVersion -ne $version -or $metadata.ProductMajorPart -ne 0 -or $metadata.ProductMinorPart -ne 2 -or $metadata.ProductBuildPart -ne ([version]$version).Build -or $metadata.ProductPrivatePart -ne 0) { throw 'The NSIS version does not match the pinned acceptance entry.' }
$installerHash = Hash-File $bundle
$sources = [ordered]@{ $installerName = $bundle }
foreach ($name in @('plain.pdf', 'geometry.pdf', 'freetext-control.pdf', 'matrix-4page.pdf')) {
    $fixture = Join-Path $root "tests\fixtures\g1\$name"
    $cached = Join-Path $root ".cache\m8-mupdf-package\$name"
    $source = if (Test-Path -LiteralPath $fixture -PathType Leaf) { $fixture } else { $cached }
    $null = Assert-ProjectPath $source -MustExist
    $bytes = [IO.File]::ReadAllBytes($source)
    if ($bytes.Length -lt 8 -or [Text.Encoding]::ASCII.GetString($bytes, 0, 5) -ne '%PDF-') { throw "Invalid synthetic PDF material: $name" }
    $sources[$name] = $source
}

$steps = @"
AI-Native PDF Reader 0.2.1 — 仅另存为的 Sandbox 人工验收候选

本入口不会自动安装或启动。此包携带固定 WebView2，不下载运行库。
Sandbox 已关闭网络、剪贴板及设备重定向，宿主仅映射只读安装包和合成样例。
本轮不回传文件到宿主，不清理旧测试材料。尚未完成的步骤不是 Pass。

1. 仅在 Sandbox 内，将 C:\M8ReadOnly\Package 整个复制到 guest 本地桌面。
   双击该本地副本中的 $installerName 安装，不在宿主安装。
2. 启动后确认窗口标题含 0.2.1；用 guest 本地 plain.pdf 测试，不用私人课件。
3. 分别只加文本框“中文 α + β ≤ 10”、只加高亮、只加笔画，以及三者混合。
   每组结束编辑后点击空白，再“另存为批注副本”，选择 guest 本地新文件名。
   打开新副本，检查外观并改字/位置，再另存一个新文件；关闭重开检查仍正确。
4. 同样检查 geometry.pdf（旋转）、freetext-control.pdf（已有批注）与 matrix-4page.pdf。
   原文和已有批注应保留；确认文本仍可编辑，不是图片或整页扁平化。
5. 再次另存时取消原生保存对话框：不应误报成功，当前批注及原 PDF 应保留。
   选择已经存在的文件名时，取消覆盖确认应无变化；即使确认覆盖，应用也必须
   拒绝覆盖并提示换新文件名，既有文件不能被改写。正常另存请选择全新文件名。
6. Ctrl+S 原件保存仍禁用：应明确提示另存为，不能写原 PDF或清除未保存状态。
7. 用 guest 的 Edge 手动打开导出 PDF 检查中文、数学、高亮与笔画显示。
   Edge/桌面人工结果请分别记录；自动检查不能代替这一步。

如失败，请截图包含 0.2.1 标题、完整底部错误、当前操作；无需输入长命令。
文件校验见 package-sha256.txt。退出 Sandbox 会丢弃 guest 内的测试副本。
"@ -replace "`r?`n", "`r`n"
if ($OriginalSave) {
$steps = @"
AI-Native PDF Reader 0.2.3 — Ctrl+S Sandbox 人工验收候选

只在 Sandbox 内安装。本入口仅显示步骤，不自动安装或启动应用。
网络、剪贴板与设备重定向关闭，宿主只读映射安装包和合成样例。

1. 将 C:\M8ReadOnly\Package 复制到 guest 本地桌面，运行其中的 $installerName。
   窗口标题应含 0.2.3；不要在宿主安装，不要用私人 PDF。
2. 打开 guest 本地 plain.pdf，添加“中文 α + β ≤ 10”、高亮和笔画。
   结束编辑后按 Ctrl+S，应显示“已保存”，标签未保存标识消失。
3. 关闭并重开同一原件，改成“修改后 中文 β ≥ 20”并移动，Ctrl+S，关闭重开核对。
   不应出现另存对话框；原文字、批注外观与再次编辑能力均须保留。
4. 用 geometry.pdf / freetext-control.pdf / matrix-4page.pdf 检查已有批注、旋转和多页。
   在合成历史中新增一条 Mock 消息，再保存、退出重开，核对历史与阅读位置。
5. 原文件旁 .ainative-save-… 目录保留保存前和新版完整 PDF，不自动删除。
   遇到未完成保存提示，选择“恢复新版副本”或“恢复保存前副本”，只创建新文件。
   “保留，稍后处理”不修改原件或删除材料。恢复不自动覆盖原件。
6. 外部替换原路径、文件只读或被占用时必须拒绝保存；现有编辑和外部版本须保留。
   不支持的保护/表单/PDF 1.5 以下或超过 16 MB 文件应明确拒绝并建议另存。
7. 回归“另存为批注副本”，仅选新文件名，重开改字后再次另存。
8. guest Edge 打开输出，核对中文、数学、高亮、笔画；真实桌面结果由用户确认。

如失败，截取标题版本、完整底部错误及操作即可，不必输入命令。
退出 Sandbox 会丢弃 guest 数据；不回传、不清理宿主恢复材料。
SHA-256 见 package-sha256.txt。无真实 AI 调用；正式公开发布未批准。
"@ -replace "`r?`n", "`r`n"
}
$utf8 = [Text.UTF8Encoding]::new($false)
$readmeBytes = $utf8.GetBytes($steps + "`r`n")
$hashBytes = [Text.Encoding]::ASCII.GetBytes("$installerHash  $installerName`r`n")

# Preflight all destinations before writing anything. Different existing files,
# linked paths and directories are refused; repeated preparation is idempotent.
$allowedNames = @($sources.Keys) + @('ACCEPTANCE-STEPS.txt', 'package-sha256.txt')
if (Test-Path -LiteralPath $packageDirectory) {
    foreach ($entry in Get-ChildItem -LiteralPath $packageDirectory -Force) {
        if ($entry.PSIsContainer -or $entry.Name -notin $allowedNames -or ($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw 'The acceptance directory contains unexpected material; it was retained and will not be mapped.'
        }
    }
}
foreach ($name in $sources.Keys) { Assert-SameOrAbsent (Join-Path $packageDirectory $name) (Hash-File $sources[$name]) }
Assert-SameOrAbsent (Join-Path $packageDirectory 'ACCEPTANCE-STEPS.txt') (Hash-Bytes $readmeBytes)
Assert-SameOrAbsent (Join-Path $packageDirectory 'package-sha256.txt') (Hash-Bytes $hashBytes)
if (-not $CheckOnly) {
    $null = New-Item -ItemType Directory -Path $packageDirectory -Force
    foreach ($name in $sources.Keys) {
        $target = Join-Path $packageDirectory $name
        if (Test-Path -LiteralPath $target) { continue }
        $sourceStream = [IO.File]::OpenRead($sources[$name])
        try {
            $output = [IO.File]::Open($target, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::Read)
            try { $sourceStream.CopyTo($output); $output.Flush($true) } finally { $output.Dispose() }
        } finally { $sourceStream.Dispose() }
        if ((Hash-File $target) -ne (Hash-File $sources[$name])) { throw 'Copied material did not match its source; it was retained for inspection.' }
    }
    Write-NewOrSame (Join-Path $packageDirectory 'ACCEPTANCE-STEPS.txt') $readmeBytes
    Write-NewOrSame (Join-Path $packageDirectory 'package-sha256.txt') $hashBytes
}
[pscustomobject]@{ version = $version; package = (Join-Path $packageDirectory $installerName); packageBytes = $bundleInfo.Length; packageSha256 = $installerHash; sandbox = $wsb; prepared = (-not $CheckOnly); hostSystemChanged = $false; sandboxStarted = $false } | Format-List
