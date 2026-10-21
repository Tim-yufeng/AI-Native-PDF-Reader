# Content-free structural diagnostic for the two persistent failures.
#
# It never prints document text, annotations or any other content. It reports
# only sizes, digests, offsets, xref form and which structural rule refuses the
# save. Run it and paste the output; nothing else is read from the file.
#
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts/diagnose-save-structure.ps1 `
#     -Original "D:\path\to\the-opened-copy.pdf" [-Edited "D:\path\to\saved-copy.pdf"]
#
# -Original is the file the reader had open. -Edited is optional: the output of
# "另存为批注副本" from that same session, when you have one.
param(
  [Parameter(Mandatory = $true)][string]$Original,
  [string]$Edited = ''
)
$ErrorActionPreference = 'Stop'
$enc = [System.Text.Encoding]::GetEncoding(28591)

function Get-Digest([byte[]]$bytes) {
  $sha = [System.Security.Cryptography.SHA256]::Create()
  try { return ([BitConverter]::ToString($sha.ComputeHash($bytes)) -replace '-', '').ToLower() } finally { $sha.Dispose() }
}
function Get-Latin([byte[]]$bytes, [int]$from, [int]$count) {
  if ($from -lt 0 -or $from -ge $bytes.Length) { return '' }
  $n = [Math]::Min($count, $bytes.Length - $from)
  return $enc.GetString($bytes, $from, $n)
}
function Find-Last([string]$text, [string]$needle) { return $text.LastIndexOf($needle, [StringComparison]::Ordinal) }
function Count-All([string]$text, [string]$needle) {
  $n = 0; $at = 0
  while (($at = $text.IndexOf($needle, $at, [StringComparison]::Ordinal)) -ge 0) { $n++; $at += $needle.Length }
  return $n
}
# The same last-startxref rule the preflight uses: the final startxref must be
# terminal and its offset must land on a real xref keyword.
function Get-LastXref([byte[]]$bytes) {
  $text = $enc.GetString($bytes)
  $at = Find-Last $text 'startxref'
  if ($at -lt 0) { return @{ ok = $false; rule = 'T1-no-startxref'; detail = 'no startxref marker anywhere' } }
  $tail = $text.Substring($at)
  $parts = ($tail -split '\s+') | Where-Object { $_ -ne '' }
  if ($parts.Count -lt 3 -or $parts[0] -ne 'startxref' -or $parts[2] -ne '%%EOF') {
    return @{ ok = $false; rule = 'T2-startxref-not-terminal'; detail = "tokens after startxref: $($parts.Count); last token: $($parts[-1])" }
  }
  $offset = 0
  if (-not [int]::TryParse($parts[1], [ref]$offset)) { return @{ ok = $false; rule = 'T3-offset-not-numeric'; detail = "offset token: $($parts[1])" } }
  if ($offset -le 0 -or $offset -ge $bytes.Length) { return @{ ok = $false; rule = 'T4-offset-out-of-range'; detail = "offset $offset of $($bytes.Length) bytes" } }
  $at2 = Get-Latin $bytes $offset 5
  return @{ ok = $true; offset = $offset; kind = $(if ($at2.StartsWith('xref')) { 'table' } else { 'stream' }); at = $at2 }
}
# The trailer dictionary that belongs to a classic xref table at that offset.
function Get-RootRef([byte[]]$bytes, [int]$offset) {
  $tail = Get-Latin $bytes $offset ([Math]::Min(8192, $bytes.Length - $offset))
  if ($tail.StartsWith('xref')) {
    $t = $tail.IndexOf('trailer')
    if ($t -lt 0) { return @{ ok = $false; detail = 'no trailer keyword after the last xref table' } }
    $tail = $tail.Substring($t + 7)
  }
  $dictAt = $tail.IndexOf('<<')
  if ($dictAt -lt 0) { return @{ ok = $false; detail = 'no trailer dictionary at the last startxref' } }
  $dictEnd = $tail.IndexOf('>>', $dictAt)
  if ($dictEnd -lt 0) { return @{ ok = $false; detail = 'unterminated trailer dictionary' } }
  $dict = $tail.Substring($dictAt, $dictEnd - $dictAt + 2)
  $m = [regex]::Match($dict, '/Root\s+(\d+)\s+(\d+)\s+R')
  $keys = [regex]::Matches($dict, '/([A-Za-z0-9]+)') | ForEach-Object { $_.Groups[1].Value } | Sort-Object -Unique
  $size = [regex]::Match($dict, '/Size\s+(\d+)')
  $prev = [regex]::Match($dict, '/Prev\s+(\d+)')
  return @{
    ok = $true
    root = $(if ($m.Success) { "$($m.Groups[1].Value) $($m.Groups[2].Value) R" } else { 'MISSING' })
    keys = ($keys -join ',')
    size = $(if ($size.Success) { [int]$size.Groups[1].Value } else { -1 })
    prev = $(if ($prev.Success) { [int]$prev.Groups[1].Value } else { -1 })
    bytes = $dict.Length
  }
}

Write-Output "=== save-structure diagnostic (no document content is read or printed) ==="
$paths = @{ Original = $Original }
if ($Edited) { $paths['Edited'] = $Edited }
$data = @{}
foreach ($name in $paths.Keys) {
  $path = $paths[$name]
  if (-not (Test-Path -LiteralPath $path)) { Write-Output "$name`: MISSING at $path"; continue }
  $bytes = [System.IO.File]::ReadAllBytes($path)
  $data[$name] = $bytes
  $header = Get-Latin $bytes 0 9
  $tailText = Get-Latin $bytes ([Math]::Max(0, $bytes.Length - 48)) 48
  $xref = Get-LastXref $bytes
  Write-Output ""
  Write-Output "[$name]"
  Write-Output ("  path        : {0}" -f (Resolve-Path -LiteralPath $path).Path)
  Write-Output ("  bytes       : {0}" -f $bytes.Length)
  Write-Output ("  sha256      : {0}" -f (Get-Digest $bytes))
  Write-Output ("  header      : {0}" -f $header)
  Write-Output ("  tail(48)    : {0}" -f ($tailText -replace "`r", '\r' -replace "`n", '\n'))
  $all = $enc.GetString($bytes)
  Write-Output ("  markers     : startxref={0} trailer={1} eof={2} objstm={3} xrefstream={4}" -f `
      (Count-All $all 'startxref'), (Count-All $all 'trailer'), (Count-All $all '%%EOF'), (Count-All $all '/ObjStm'), (Count-All $all '/XRef'))
  if ($xref.ok) {
    Write-Output ("  last xref   : kind={0} offset={1} at={2}" -f $xref.kind, $xref.offset, ($xref.at -replace "`r", '\r' -replace "`n", '\n'))
    $root = Get-RootRef $bytes $xref.offset
    if ($root.ok) {
      Write-Output ("  trailer     : root={0} size={1} prev={2} bytes={3}" -f $root.root, $root.size, $root.prev, $root.bytes)
      Write-Output ("  trailer keys: {0}" -f $root.keys)
    } else { Write-Output ("  trailer     : REFUSED - {0}" -f $root.detail) }
  } else { Write-Output ("  last xref   : REFUSED - rule {0} ({1})" -f $xref.rule, $xref.detail) }
}

# The two rules that decide whether an original save can even be attempted.
if ($data.ContainsKey('Original')) {
  $old = $data['Original']
  Write-Output ""
  Write-Output "=== the rules that decide an original-save attempt (G1..G4) ==="
  if ($data.ContainsKey('Edited')) {
    $new = $data['Edited']
    Write-Output ("  G1 size grows            : {0} ({1} -> {2})" -f ($new.Length -gt $old.Length), $old.Length, $new.Length)
    $prefix = $true; $diffAt = -1
    if ($new.Length -ge $old.Length) {
      for ($i = 0; $i -lt $old.Length; $i++) { if ($new[$i] -ne $old[$i]) { $prefix = $false; $diffAt = $i; break } }
    } else { $prefix = $false; $diffAt = -2 }
    Write-Output ("  G2 new starts with old   : {0}{1}" -f $prefix, $(if ($prefix) { '' } else { " (first differing byte: $(if ($diffAt -ge 0) { $diffAt } else { 'edited file is shorter' }))" }))
    $oldX = Get-LastXref $old; $newX = Get-LastXref $new
    if ($oldX.ok -and $newX.ok) {
      $oldR = Get-RootRef $old $oldX.offset; $newR = Get-RootRef $new $newX.offset
      Write-Output ("  G3 root object unchanged : {0} (old={1} new={2})" -f ($oldR.ok -and $newR.ok -and $oldR.root -eq $newR.root), $(if ($oldR.ok) { $oldR.root } else { 'n/a' }), $(if ($newR.ok) { $newR.root } else { 'n/a' }))
      Write-Output ("  G4 new revision chains   : prev={0} old xref offset={1} match={2}" -f $(if ($newR.ok) { $newR.prev } else { 'n/a' }), $oldX.offset, $(if ($newR.ok) { $newR.prev -eq $oldX.offset } else { $false }))
      $chain = 0; $offset = $newX.offset; $seen = @{}
      while ($chain -lt 64 -and -not $seen.ContainsKey($offset)) {
        $seen[$offset] = $true
        $r = Get-RootRef $new $offset
        if (-not $r.ok -or $r.prev -lt 0) { break }
        $offset = $r.prev; $chain++
      }
      Write-Output ("  G5 chain length to base  : {0} (limit 64)" -f $chain)
    } else { Write-Output "  G3/G4/G5                 : cannot evaluate (a startxref rule already refused)" }
  } else {
    Write-Output "  (no -Edited file given, so only the held source was inspected)"
  }
}

# The two other failures seen in the same session, reported without any content.
Write-Output ""
Write-Output "=== side facts for the in-place explanation failure ==="
foreach ($name in $data.Keys) {
  $bytes = $data[$name]
  Write-Output ("  [{0}] encrypted={1} has-Encrypt-key={2} has-acroform={3}" -f $name, `
      ($enc.GetString($bytes).Contains('/Encrypt')), ($enc.GetString($bytes).Contains('/Encrypt')), ($enc.GetString($bytes).Contains('/AcroForm')))
}
Write-Output ""
Write-Output "Nothing above is document content. Report the whole output."
