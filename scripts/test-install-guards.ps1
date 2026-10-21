param([string]$ProjectRoot = '')
$ErrorActionPreference = 'Stop'
if ($ProjectRoot -eq '') { $ProjectRoot = Join-Path $PSScriptRoot '..' }
$project = [IO.Path]::GetFullPath($ProjectRoot)
$work = [IO.Path]::GetFullPath((Join-Path $project ('tests/work/install-guards/' + [Guid]::NewGuid().ToString())))
if (-not $work.StartsWith($project + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Project-only test directory required.' }
[IO.Directory]::CreateDirectory($work) | Out-Null
$oldTemp = $env:TEMP
$oldTmp = $env:TMP
$env:TEMP = $work
$env:TMP = $work
try {
  Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using Microsoft.Win32.SafeHandles;
public static class InstallGuardProbe {
  [StructLayout(LayoutKind.Sequential)] struct Info {
    public uint Attributes, CreationLow, CreationHigh, AccessLow, AccessHigh, WriteLow, WriteHigh;
    public uint Volume, SizeHigh, SizeLow, Links, IndexHigh, IndexLow;
  }
  [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
  static extern SafeFileHandle CreateFileW(string name, uint access, uint share, IntPtr security, uint creation, uint flags, IntPtr template);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool GetFileInformationByHandle(SafeFileHandle handle, out Info info);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool WriteFile(SafeFileHandle handle, byte[] bytes, uint length, out uint written, IntPtr overlapped);
  [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] public static extern bool DeleteFileW(string name);
  [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] public static extern bool MoveFileExW(string source, string target, uint flags);
  [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] public static extern bool CreateHardLinkW(string link, string target, IntPtr security);
  [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] public static extern bool CreateSymbolicLinkW(string link, string target, uint flags);
  public static SafeFileHandle Guard(string path, bool directory, bool reserve) {
    var handle = CreateFileW(path, 0x81, 3, IntPtr.Zero, reserve ? 1u : 3u, 0x00200000u | (directory ? 0x02000000u : 0u), IntPtr.Zero);
    if (handle.IsInvalid) { var error=Marshal.GetLastWin32Error(); handle.Dispose(); throw new Win32Exception(error); }
    try {
      Info info; if (!GetFileInformationByHandle(handle, out info)) throw new Win32Exception(Marshal.GetLastWin32Error());
      if ((info.Attributes & 0x410u) != (directory ? 0x10u : 0u) || (!directory && info.Links != 1)) throw new InvalidDataException("Linked or incorrect-type program path refused.");
      return handle;
    } catch { handle.Dispose(); throw; }
  }
  public static SafeFileHandle AttributesOnly(string path) {
    var handle=CreateFileW(path, 0x80, 3, IntPtr.Zero, 3, 0x00200000, IntPtr.Zero);
    if (handle.IsInvalid) { var error=Marshal.GetLastWin32Error(); handle.Dispose(); throw new Win32Exception(error); }
    return handle;
  }
  public static void WriteLikeNsis(string path, string text) {
    using (var file = CreateFileW(path, 0x40000000, 1, IntPtr.Zero, 2, 0x80, IntPtr.Zero)) {
      if (file.IsInvalid) throw new Win32Exception(Marshal.GetLastWin32Error());
      var bytes=Encoding.UTF8.GetBytes(text); uint written;
      if (!WriteFile(file, bytes, (uint)bytes.Length, out written, IntPtr.Zero) || written != bytes.Length) throw new Win32Exception(Marshal.GetLastWin32Error());
    }
  }
  public static List<SafeFileHandle> PinChain(string path) {
    var parents=new List<string>(); var current=new DirectoryInfo(Path.GetFullPath(path));
    for(; current!=null; current=current.Parent) parents.Add(current.FullName);
    parents.Reverse(); var guards=new List<SafeFileHandle>();
    try { foreach(var parent in parents) guards.Add(Guard(parent, true, false)); return guards; }
    catch { foreach(var guard in guards) guard.Dispose(); throw; }
  }
}
'@
  $results = [Collections.Generic.List[object]]::new()
  function Expect-Refusal([string]$name, [scriptblock]$action) {
    $rejected = $false
    try { & $action } catch { $rejected = $true }
    if (-not $rejected) { throw "$name unexpectedly accepted." }
    $results.Add([pscustomobject]@{ test=$name; status='Pass' })
  }
  $metadataOnly = Join-Path $work 'attributes-only-control.bin'
  [IO.File]::WriteAllText($metadataOnly, 'disposable sharing control')
  $metadataGuard = [InstallGuardProbe]::AttributesOnly($metadataOnly)
  try {
    if (-not [InstallGuardProbe]::DeleteFileW($metadataOnly)) { throw 'READ_ATTRIBUTES-only control behaved differently; reassess sharing assumptions.' }
    $results.Add([pscustomobject]@{ test='READ_ATTRIBUTES-only negative control permits deletion'; status='Pass'; reason='This access mode is insufficient as a no-delete guard.' })
  } finally { $metadataGuard.Dispose() }
  $existing = Join-Path $work 'existing.bin'
  [IO.File]::WriteAllText($existing, 'old controlled file')
  $guard = [InstallGuardProbe]::Guard($existing, $false, $false)
  try {
    [InstallGuardProbe]::WriteLikeNsis($existing, 'new controlled bytes')
    if ([IO.File]::ReadAllText($existing) -ne 'new controlled bytes') { throw 'Existing guarded write did not match.' }
    if ([InstallGuardProbe]::DeleteFileW($existing)) { throw 'Guarded file was deleted.' }
    $errorCode = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
    if ($errorCode -ne 32) { throw "Unexpected guarded DeleteFile error $errorCode." }
    $results.Add([pscustomobject]@{ test='existing-leaf CREATE_ALWAYS compatibility and no-delete'; status='Pass'; blockedDeleteWin32=$errorCode })
  } finally { $guard.Dispose() }
  $reserved = Join-Path $work 'reserved.bin'
  $guard = [InstallGuardProbe]::Guard($reserved, $false, $true)
  try {
    [InstallGuardProbe]::WriteLikeNsis($reserved, 'reserved controlled bytes')
    if ([IO.File]::ReadAllText($reserved) -ne 'reserved controlled bytes') { throw 'Reserved guarded write did not match.' }
    $results.Add([pscustomobject]@{ test='CREATE_NEW reservation then CREATE_ALWAYS compatibility'; status='Pass' })
  } finally { $guard.Dispose() }
  $shortcutPath = Join-Path $work 'controlled-shortcut.lnk'
  $shell = New-Object -ComObject WScript.Shell
  $shortcut = $shell.CreateShortcut($shortcutPath)
  $shortcut.TargetPath = $existing
  $shortcut.WorkingDirectory = $work
  $guard = [InstallGuardProbe]::Guard($shortcutPath, $false, $true)
  try {
    $shortcut.Save()
    if ((Get-Item -LiteralPath $shortcutPath).Length -eq 0) { throw 'Guarded shortcut was not written.' }
    $results.Add([pscustomobject]@{ test='COM shortcut Save with reserved guarded leaf'; status='Pass' })
  } catch {
    $results.Add([pscustomobject]@{ test='COM shortcut Save with reserved guarded leaf'; status='Blocked'; reason='Shortcut persistence is incompatible with a retained read handle; do not weaken the guard.' })
  } finally {
    $guard.Dispose()
    [Runtime.InteropServices.Marshal]::ReleaseComObject($shortcut) | Out-Null
    [Runtime.InteropServices.Marshal]::ReleaseComObject($shell) | Out-Null
  }
  $ordinary = Join-Path $work 'ordinary'
  [IO.Directory]::CreateDirectory($ordinary) | Out-Null
  $guards = [InstallGuardProbe]::PinChain($ordinary)
  try {
    if ([InstallGuardProbe]::MoveFileExW($ordinary, (Join-Path $work 'renamed'), 0)) { throw 'Pinned directory was renamed.' }
    $errorCode = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
    if ($errorCode -ne 32) { throw "Unexpected guarded directory rename error $errorCode." }
    [IO.File]::WriteAllText((Join-Path $ordinary 'child.bin'), 'ordinary child')
    $results.Add([pscustomobject]@{ test='pinned parent chain blocks rename while child writing remains available'; status='Pass'; blockedRenameWin32=$errorCode })
  } finally { foreach ($handle in $guards) { $handle.Dispose() } }
  $hardlink = Join-Path $work 'hardlink.bin'
  if (-not [InstallGuardProbe]::CreateHardLinkW($hardlink, $existing, [IntPtr]::Zero)) { throw ([ComponentModel.Win32Exception]::new([Runtime.InteropServices.Marshal]::GetLastWin32Error())) }
  Expect-Refusal 'existing multiply linked leaf' { $h=[InstallGuardProbe]::Guard($hardlink, $false, $false); $h.Dispose() }
  if ([IO.File]::ReadAllText($existing) -ne 'new controlled bytes') { throw 'Hardlink refusal modified the control.' }
  $junction = Join-Path $work 'junction'
  New-Item -ItemType Junction -Path $junction -Target $ordinary | Out-Null
  Expect-Refusal 'junction directory' { $h=[InstallGuardProbe]::Guard($junction, $true, $false); $h.Dispose() }
  Expect-Refusal 'linked ancestor in directory chain' { $handles=[InstallGuardProbe]::PinChain($junction); foreach($h in $handles) { $h.Dispose() } }
  $symlink = Join-Path $work 'symlink.bin'
  if ([InstallGuardProbe]::CreateSymbolicLinkW($symlink, $reserved, 2)) {
    Expect-Refusal 'symbolic-link leaf' { $h=[InstallGuardProbe]::Guard($symlink, $false, $false); $h.Dispose() }
  } else {
    $results.Add([pscustomobject]@{ test='symbolic-link leaf'; status='Blocked'; win32=[Runtime.InteropServices.Marshal]::GetLastWin32Error(); reason='No privilege elevation or Developer Mode change is permitted.' })
  }
  Expect-Refusal 'directory supplied as leaf' { $h=[InstallGuardProbe]::Guard($ordinary, $false, $false); $h.Dispose() }
  Expect-Refusal 'CREATE_NEW refuses an existing file' { $h=[InstallGuardProbe]::Guard($reserved, $false, $true); $h.Dispose() }
  $report = [pscustomobject]@{ scope='synthetic native file APIs only; no installer, GUI, registry or ACL changes'; work=$work; tests=$results; atomicInstallationProved=$false; equalUserRaceImmunityProved=$false }
  $report | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $work 'results.json') -Encoding utf8
  $report | ConvertTo-Json -Depth 6
} finally {
  $env:TEMP = $oldTemp
  $env:TMP = $oldTmp
}
