# Appends a PDF.js-shaped incremental revision to a copy of a "held source" PDF,
# so the Rust preflight can be exercised against real third-party structures.
# Reads only the file the caller names and writes only inside -Out.
param(
  [Parameter(Mandatory = $true)][string]$Source,
  [string]$Out = ".cache/pdf-structure"
)
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force -Path $Out | Out-Null
$enc = [System.Text.Encoding]::GetEncoding(28591)
$bytes = [System.IO.File]::ReadAllBytes($Source)
$text = $enc.GetString($bytes)

# The last startxref offset, as a reader would use it.
$at = $text.LastIndexOf('startxref')
$rest = $text.Substring($at + 9).Trim()
$offset = [int]($rest -split '\s+')[0]

# The trailer dictionary that starts at that offset (after an xref keyword).
$tail = $text.Substring($offset)
$trailerAt = $tail.IndexOf('trailer')
if ($trailerAt -lt 0) { throw "no classic trailer at the last startxref: $Source" }
$dictStart = $tail.IndexOf('<<', $trailerAt)
$dictEnd = $tail.IndexOf('>>', $dictStart) + 2
$trailer = $tail.Substring($dictStart, $dictEnd - $dictStart)
$size = [int]([regex]::Match($trailer, '/Size\s+(\d+)').Groups[1].Value)
$root = [int]([regex]::Match($trailer, '/Root\s+(\d+)').Groups[1].Value)
$id = [regex]::Match($trailer, '/ID\s*\[[^\]]*\]').Value

# One added object, then the revision's xref table and trailer, formatted as the
# writer under test formats them.
$base = $bytes.Length
$newObject = "$size 0 obj`n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents $($size + 1) 0 R >>`nendobj`n"
$contentBody = "BT /F1 12 Tf 72 700 Td (appended) Tj ET"
$content = "$($size + 1) 0 obj`n<< /Length $($contentBody.Length) >>`nstream`n$contentBody`nendstream`nendobj`n"
$body = $newObject + $content
$objAt = $base
$contentAt = $base + $newObject.Length
$xrefAt = $base + $body.Length
$table = "xref`n$size 2`n{0:0000000000} 00000 n `n{1:0000000000} 00000 n `n" -f $objAt, $contentAt
$trailerOut = "trailer`n<</Prev $offset/Size $($size + 2)/Root $root 0 R$id>>`nstartxref`n$xrefAt`n%%EOF`n"
$appended = [byte[]]($bytes + $enc.GetBytes($body + $table + $trailerOut))

$name = [System.IO.Path]::GetFileNameWithoutExtension($Source)
$dest = Join-Path $Out ("appended-" + $name + ".pdf")
[System.IO.File]::WriteAllBytes($dest, $appended)
"{0} bytes={1} -> {2} bytes={3} size={4} root={5} prev={6} id={7}" -f `
  (Split-Path $Source -Leaf), $bytes.Length, (Split-Path $dest -Leaf), $appended.Length, $size, $root, $offset, [bool]$id
