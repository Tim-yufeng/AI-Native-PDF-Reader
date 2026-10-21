# Synthetic, disposable structural fixtures. Never reads or writes user files.
# Emits third-party-shaped PDFs directly so the Rust incremental preflight can be
# exercised on structures our own writer never makes: classic xref tables with
# /Info and /ID, an older header, genuine second and third revisions, and a
# canonical compressed xref stream as the final revision.
param([string]$Out = ".cache/pdf-structure")
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force -Path $Out | Out-Null
$enc = [System.Text.Encoding]::GetEncoding(28591)   # latin1, byte preserving

function New-Pdf {
  param([object[]]$Objects, [int]$Root = 1, [int]$Info = 0, [string]$Header = "%PDF-1.5",
        [int]$Prev = -1, [int]$Size = 0, [byte[]]$Prefix = $null)
  $bytes = New-Object System.Collections.Generic.List[byte]
  if ($Prefix) { $bytes.AddRange($Prefix) } else { $bytes.AddRange($enc.GetBytes($Header + "`n")) }
  $base = $bytes.Count
  $offsets = @{}
  foreach ($item in $Objects) {
    $offsets[[int]$item[0]] = $bytes.Count
    $bytes.AddRange($enc.GetBytes("$($item[0]) 0 obj`n"))
    $bytes.AddRange([byte[]]$item[1])
    $bytes.AddRange($enc.GetBytes("`nendobj`n"))
  }
  $xrefAt = $bytes.Count
  $highest = ($offsets.Keys | Measure-Object -Maximum).Maximum
  $total = if ($Size -gt 0) { $Size } else { $highest + 1 }
  $bytes.AddRange($enc.GetBytes("xref`n0 $total`n"))
  $bytes.AddRange($enc.GetBytes("0000000000 65535 f `n"))
  for ($n = 1; $n -lt $total; $n++) {
    if ($offsets.ContainsKey($n)) { $bytes.AddRange($enc.GetBytes(("{0:0000000000} 00000 n `n" -f $offsets[$n]))) }
    else { $bytes.AddRange($enc.GetBytes("0000000000 65535 f `n")) }
  }
  $trailer = "trailer`n<< /Size $total /Root $Root 0 R"
  if ($Info -gt 0) { $trailer += " /Info $Info 0 R" }
  $trailer += " /ID [<0123456789ABCDEF0123456789ABCDEF> <FEDCBA9876543210FEDCBA9876543210>]"
  if ($Prev -ge 0) { $trailer += " /Prev $Prev" }
  $trailer += " >>`nstartxref`n$xrefAt`n%%EOF`n"
  $bytes.AddRange($enc.GetBytes($trailer))
  return @{ Bytes = $bytes.ToArray(); Xref = $xrefAt }
}

function Get-ContentStream([string]$Text) {
  $enc.GetBytes("<< /Length $($Text.Length) >>`nstream`n$Text`nendstream")
}

function Get-BaseObjects([int]$Pages = 2) {
  $kids = (0..($Pages - 1) | ForEach-Object { $n = $_ * 2; "$($n + 5) 0 R" }) -join ' '
  $list = New-Object System.Collections.Generic.List[object]
  $list.Add(@(1, $enc.GetBytes("<< /Type /Catalog /Pages 2 0 R >>")))
  $list.Add(@(2, $enc.GetBytes("<< /Type /Pages /Kids [$kids] /Count $Pages >>")))
  $list.Add(@(3, $enc.GetBytes("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")))
  $list.Add(@(4, $enc.GetBytes("<< /Producer (synthetic structure fixture) /Title (structure) >>")))
  for ($i = 0; $i -lt $Pages; $i++) {
    $pageObj = [int]$i * 2 + 5
    $contentObj = [int]$i * 2 + 6
    $pageNo = [int]$i + 1
    $list.Add(@($pageObj, $enc.GetBytes("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents $contentObj 0 R >>")))
    $list.Add(@($contentObj, (Get-ContentStream "BT /F1 12 Tf 72 700 Td (Page $pageNo) Tj ET")))
  }
  return $list.ToArray()
}

# 1. Current-style xref table with /Info and /ID.
$one = New-Pdf -Objects (Get-BaseObjects) -Info 4
[System.IO.File]::WriteAllBytes((Join-Path $Out 'v15-xref-table.pdf'), $one.Bytes)

# 2. Older header, same structure.
$two = New-Pdf -Objects (Get-BaseObjects) -Info 4 -Header "%PDF-1.4"
[System.IO.File]::WriteAllBytes((Join-Path $Out 'v14-xref-table.pdf'), $two.Bytes)

# 3. Genuine second revision: one added page chained by /Prev. Same /Root and /ID.
$rev2 = New-Pdf -Objects @(
  @(9, $enc.GetBytes("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 10 0 R >>")),
  @(10, (Get-ContentStream "BT /F1 12 Tf 72 700 Td (Added) Tj ET")),
  @(2, $enc.GetBytes("<< /Type /Pages /Kids [5 0 R 7 0 R 9 0 R] /Count 3 >>"))
) -Size 11 -Prev $one.Xref -Prefix $one.Bytes
[System.IO.File]::WriteAllBytes((Join-Path $Out 'v15-incremental-twice.pdf'), $rev2.Bytes)

# 4. Third revision on top of the second.
$rev3 = New-Pdf -Objects @(
  @(11, $enc.GetBytes("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 12 0 R >>")),
  @(12, (Get-ContentStream "BT /F1 12 Tf 72 700 Td (Third) Tj ET")),
  @(2, $enc.GetBytes("<< /Type /Pages /Kids [5 0 R 7 0 R 9 0 R 11 0 R] /Count 4 >>"))
) -Size 13 -Prev $rev2.Xref -Prefix $rev2.Bytes
[System.IO.File]::WriteAllBytes((Join-Path $Out 'v15-incremental-thrice.pdf'), $rev3.Bytes)

# 5. Canonical compressed xref stream as the final revision over revision one.
$count = 11
$entries = New-Object System.Collections.Generic.List[byte]
$entries.AddRange([byte[]]@(0, 0, 0, 255))
for ($n = 1; $n -lt $count; $n++) { $entries.AddRange([byte[]]@(0, 0, 0, 0)) }
# Windows PowerShell lacks ZLibStream, so emit the two-byte zlib header and a
# raw deflate body. A tolerant zlib reader accepts it; the Adler-32 trailer is
# not required by the reader under test.
$ms = New-Object System.IO.MemoryStream
$z = New-Object System.IO.Compression.DeflateStream($ms, [System.IO.Compression.CompressionMode]::Compress, $true)
$z.Write($entries.ToArray(), 0, $entries.Count); $z.Dispose()
$raw = [byte[]](@(0x78, 0x9C) + $ms.ToArray()); $ms.Dispose()
$streamAt = $one.Bytes.Length
$meta = "<< /Type /XRef /Size $($count + 1) /W [1 2 1] /Index [0 $($count + 1)] /Root 1 0 R /Info 4 0 R " +
        "/ID [<0123456789ABCDEF0123456789ABCDEF> <FEDCBA9876543210FEDCBA9876543210>] /Prev $($one.Xref) /Length $($raw.Length) >>`nstream`n"
$tail = New-Object System.Collections.Generic.List[byte]
$tail.AddRange($one.Bytes)
$tail.AddRange($enc.GetBytes("$count 0 obj`n$meta"))
$tail.AddRange($raw)
$tail.AddRange($enc.GetBytes("`nendstream`nendobj`nstartxref`n$streamAt`n%%EOF`n"))
[System.IO.File]::WriteAllBytes((Join-Path $Out 'v15-xref-stream-final.pdf'), $tail.ToArray())

Get-ChildItem $Out -Filter *.pdf | ForEach-Object {
  $blob = [System.IO.File]::ReadAllBytes($_.FullName)
  $text = $enc.GetString($blob)
  "{0,-30} bytes={1,7} header={2} startxref={3} trailer={4} xrefstream={5}" -f `
    $_.Name, $blob.Length, $enc.GetString($blob[0..8]), `
    ([regex]::Matches($text, 'startxref')).Count, ([regex]::Matches($text, 'trailer')).Count, ([regex]::Matches($text, '/XRef')).Count
}
