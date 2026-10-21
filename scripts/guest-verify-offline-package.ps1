# Guest-only verification for the offline WebView2 validation package.
#
# This script runs INSIDE the disposable Windows Sandbox guest, started by
# src-tauri/installer/m8-offline-package.wsb. Boundaries kept by construction:
#   * the guest has no network (the .wsb keeps Networking disabled);
#   * the host is reachable only through one read-only package mapping and one
#     results mapping, so nothing here can write to the host outside C:\M8Results;
#   * nothing is installed on the host: the runtime and the reader are installed
#     in the guest and discarded with it.
# It reports facts only. It never asserts a pass it did not observe.

param(
  [string]$ResultsDirectory = 'C:\M8Results',
  [string]$PackageDirectory = 'C:\M8ReadOnly\Package',
  [string]$SetupName = 'AI-Native PDF Reader_0.2.0_offline-x64-setup.exe',
  [int]$SetupTimeoutSeconds = 420,
  [int]$LaunchObservationSeconds = 30
)

$ErrorActionPreference = 'Continue'
$ProgressPreference = 'SilentlyContinue'

$logPath = Join-Path $ResultsDirectory 'guest-run.log'
$jsonPath = Join-Path $ResultsDirectory 'guest-result.json'
$textPath = Join-Path $ResultsDirectory 'guest-result.txt'

function Write-Log {
  param([string]$Message)
  $line = '{0} {1}' -f (Get-Date).ToString('HH:mm:ss'), $Message
  Write-Host $line
  try { Add-Content -LiteralPath $logPath -Value $line -Encoding UTF8 } catch { }
}

function Read-RegistryValue {
  param([string]$Path, [string]$Name)
  try { return [string](Get-ItemProperty -LiteralPath $Path -Name $Name -ErrorAction Stop).$Name } catch { return '' }
}

$runtimeKeyWow6432 = 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
$runtimeKeyNative = 'HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
$runtimeKeyUser = 'HKCU:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
$edgeWebViewRoot = 'C:\Program Files (x86)\Microsoft\EdgeWebView\Application'
$defaultRuntimeLocation = 'C:\Program Files (x86)\Microsoft\EdgeWebView\Application'

function Get-RuntimeState {
  $folders = @()
  $details = @()
  if (Test-Path -LiteralPath $edgeWebViewRoot) {
    foreach ($directory in Get-ChildItem -LiteralPath $edgeWebViewRoot -Directory -ErrorAction SilentlyContinue) {
      $folders += $directory.Name
      $engine = Join-Path $directory.FullName 'msedgewebview2.exe'
      $files = @(Get-ChildItem -LiteralPath $directory.FullName -Recurse -File -ErrorAction SilentlyContinue).Count
      $details += ('{0}(files={1},engine={2})' -f $directory.Name, $files, (Test-Path -LiteralPath $engine))
    }
  }
  $pvWow6432 = Read-RegistryValue $runtimeKeyWow6432 'pv'
  $pvNative = Read-RegistryValue $runtimeKeyNative 'pv'
  $pvUser = Read-RegistryValue $runtimeKeyUser 'pv'
  $location = Read-RegistryValue $runtimeKeyWow6432 'location'
  $source = 'wow6432'
  $pv = $pvWow6432
  if (-not $pv) { $pv = $pvNative; $source = 'native' }
  if (-not $pv) { $pv = $pvUser; $location = Read-RegistryValue $runtimeKeyUser 'location'; $source = 'hkcu' }
  if (-not $pv) { $source = 'none' }
  if (-not $location) { $location = $defaultRuntimeLocation }
  $engineForRegistration = ''
  if ($pv) { $engineForRegistration = Test-Path -LiteralPath (Join-Path (Join-Path $location $pv) 'msedgewebview2.exe') }
  [ordered]@{
    pvWow6432 = $pvWow6432
    pvNative = $pvNative
    pvUser = $pvUser
    location = $location
    resolvedSource = $source
    resolvedPv = $pv
    engineForRegisteredVersion = $engineForRegistration
    applicationFolders = ($folders -join ',')
    applicationDetails = ($details -join ' | ')
  }
}

function Get-WindowInventory {
  param([int]$TargetProcessId)
  $items = @()
  # The delegate runs outside this function's local scope, so the target pid and
  # the collected rows are kept in the script scope on purpose.
  $script:targetProcessId = $TargetProcessId
  $script:windowItems = @()
  try {
    [void](Add-Type -Namespace GuestVerify -Name Windows -MemberDefinition @'
[StructLayout(LayoutKind.Sequential)] public struct INTUNUSED { public int Unused; }
[DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr p);
public delegate bool EnumProc(IntPtr h, IntPtr p);
[DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
[DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr h, System.Text.StringBuilder text, int count);
[DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassNameW(IntPtr h, System.Text.StringBuilder text, int count);
[DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
'@ -PassThru -ErrorAction Stop)
  } catch {
    if ($_.Exception.Message -notlike '*already exists*') { Write-Log "window helper compile failed: $($_.Exception.Message)" }
  }
  try {
    $callback = [GuestVerify.Windows+EnumProc] {
      param([IntPtr]$handle, [IntPtr]$parameter)
      [uint32]$windowPid = 0
      [void][GuestVerify.Windows]::GetWindowThreadProcessId($handle, [ref]$windowPid)
      if ($windowPid -eq $script:targetProcessId -and [GuestVerify.Windows]::IsWindowVisible($handle)) {
        $title = New-Object System.Text.StringBuilder -ArgumentList 512
        [void][GuestVerify.Windows]::GetWindowTextW($handle, $title, 512)
        $class = New-Object System.Text.StringBuilder -ArgumentList 256
        [void][GuestVerify.Windows]::GetClassNameW($handle, $class, 256)
        $script:windowItems += ('{0}|{1}' -f $class.ToString(), $title.ToString())
      }
      return $true
    }
    [void][GuestVerify.Windows]::EnumWindows($callback, [IntPtr]::Zero)
    $items = $script:windowItems
  } catch {
    Write-Log "window enumeration failed: $($_.Exception.Message)"
  }
  return $items
}

function Save-Screenshot {
  param([string]$Path)
  try {
    Add-Type -AssemblyName System.Windows.Forms -ErrorAction Stop
    Add-Type -AssemblyName System.Drawing -ErrorAction Stop
    $bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $bitmap = New-Object System.Drawing.Bitmap -ArgumentList $bounds.Width, $bounds.Height
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $graphics.CopyFromScreen($bounds.Location, [System.Drawing.Point]::Empty, $bounds.Size)
    $bitmap.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
    $graphics.Dispose(); $bitmap.Dispose()
    Write-Log "screenshot saved: $Path"
  } catch {
    Write-Log "screenshot failed: $($_.Exception.Message)"
  }
}

$result = [ordered]@{}
$shortcuts = @()

try {
  New-Item -ItemType Directory -Force -Path $ResultsDirectory | Out-Null
  Write-Log 'guest verification started'

  # Watchdog: the guest shuts itself down even if a step hangs, so the window
  # cannot stay open on the user's desktop.
  Start-Process -FilePath 'cmd.exe' -ArgumentList '/c timeout /t 1200 /nobreak >nul & shutdown /s /t 5' -WindowStyle Hidden

  $identity = [System.Security.Principal.WindowsIdentity]::GetCurrent()
  $principal = New-Object System.Security.Principal.WindowsPrincipal $identity
  $os = Get-CimInstance Win32_OperatingSystem -ErrorAction SilentlyContinue
  $groups = (& whoami /groups) -join ' '
  $policy = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System'
  $result.startedAt = (Get-Date).ToString('o')
  $result.guestUser = $identity.Name
  $result.isAdministrator = $principal.IsInRole([System.Security.Principal.WindowsBuiltInRole]::Administrator)
  $result.elevated = ($groups -match 'S-1-16-12288')
  $result.uacEnableLua = Read-RegistryValue $policy 'EnableLUA'
  $result.uacConsentPromptBehaviorAdmin = Read-RegistryValue $policy 'ConsentPromptBehaviorAdmin'
  $result.osVersion = if ($os) { $os.Version } else { '' }
  $result.osBuild = if ($os) { $os.BuildNumber } else { '' }
  Write-Log ("user={0} admin={1} elevated={2} enableLua={3} consentPrompt={4} build={5}" -f $result.guestUser, $result.isAdministrator, $result.elevated, $result.uacEnableLua, $result.uacConsentPromptBehaviorAdmin, $result.osBuild)

  # Package transport check: the guest must see the exact artifact that was built.
  $setupPath = Join-Path $PackageDirectory $SetupName
  $package = [ordered]@{ name = $SetupName; path = $setupPath; present = (Test-Path -LiteralPath $setupPath) }
  if ($package.present) {
    $file = Get-Item -LiteralPath $setupPath
    $package.size = $file.Length
    $package.sha256 = (Get-FileHash -LiteralPath $setupPath -Algorithm SHA256).Hash.ToLower()
    $expectedPath = Join-Path $PackageDirectory 'package-sha256.txt'
    if (Test-Path -LiteralPath $expectedPath) {
      $package.expectedSha256 = (Get-Content -LiteralPath $expectedPath -Raw).Trim().ToLower()
      $package.hashMatches = ($package.sha256 -eq $package.expectedSha256)
    }
  }
  $result.package = $package
  Write-Log ("package present={0} size={1} hashMatches={2}" -f $package.present, $package.size, $package.hashMatches)
  if (-not $package.present) { throw "the validation package is not visible in $PackageDirectory" }

  $result.runtimeBefore = Get-RuntimeState
  Write-Log ("runtime before: pv={0} @{1} source={2} engineForRegisteredVersion={3}" -f $result.runtimeBefore.resolvedPv, $result.runtimeBefore.location, $result.runtimeBefore.resolvedSource, $result.runtimeBefore.engineForRegisteredVersion)
  Write-Log ("runtime folders: {0}" -f $result.runtimeBefore.applicationDetails)

  # The package itself must supply the runtime: this is the behaviour under test.
  # The wait is bounded because a hidden elevation prompt must not hang the guest.
  $installStart = Get-Date
  Write-Log 'running the validation package silently (/S)'
  $setupProcess = Start-Process -FilePath $setupPath -ArgumentList '/S' -PassThru
  $deadline = (Get-Date).AddSeconds($SetupTimeoutSeconds)
  while (-not $setupProcess.HasExited -and (Get-Date) -lt $deadline) { Start-Sleep -Seconds 5 }
  $result.install = [ordered]@{
    durationSeconds = [math]::Round(((Get-Date) - $installStart).TotalSeconds, 1)
    timedOut = (-not $setupProcess.HasExited)
  }
  if ($setupProcess.HasExited) {
    $result.install.setupExitCode = $setupProcess.ExitCode
    Write-Log ("setup exit code {0} after {1}s" -f $setupProcess.ExitCode, $result.install.durationSeconds)
  } else {
    Write-Log ("setup still running after {0}s; capturing the guest screen" -f $result.install.durationSeconds)
    Save-Screenshot -Path (Join-Path $ResultsDirectory 'guest-setup-timeout.png')
  }

  $result.runtimeAfter = Get-RuntimeState
  Write-Log ("runtime after: pv={0} @{1} source={2} engineForRegisteredVersion={3}" -f $result.runtimeAfter.resolvedPv, $result.runtimeAfter.location, $result.runtimeAfter.resolvedSource, $result.runtimeAfter.engineForRegisteredVersion)
  Write-Log ("runtime folders after: {0}" -f $result.runtimeAfter.applicationDetails)

  # The embedded installer is extracted to %TEMP% by the NSIS script; its presence
  # proves the embedded payload was carried and executed, not that it succeeded.
  $tempInstaller = Join-Path $env:TEMP 'MicrosoftEdgeWebView2RuntimeInstaller.exe'
  $result.embeddedPayload = [ordered]@{
    extractedPath = $tempInstaller
    present = (Test-Path -LiteralPath $tempInstaller)
    size = if (Test-Path -LiteralPath $tempInstaller) { (Get-Item -LiteralPath $tempInstaller).Length } else { 0 }
  }
  Write-Log ("embedded payload extracted={0} size={1}" -f $result.embeddedPayload.present, $result.embeddedPayload.size)

  $edgeUpdateLogs = @()
  $edgeUpdateDirectory = 'C:\ProgramData\Microsoft\EdgeUpdate\Log'
  if (Test-Path -LiteralPath $edgeUpdateDirectory) {
    foreach ($file in Get-ChildItem -LiteralPath $edgeUpdateDirectory -File -ErrorAction SilentlyContinue) {
      $target = Join-Path $ResultsDirectory ('edgeupdate-' + $file.Name)
      Copy-Item -LiteralPath $file.FullName -Destination $target -Force -ErrorAction SilentlyContinue
      $edgeUpdateLogs += ('{0} ({1} bytes)' -f $file.Name, $file.Length)
    }
  }
  $result.edgeUpdateLogsCopied = $edgeUpdateLogs
  $result.edgeUpdatePath = [ordered]@{
    wow6432 = Read-RegistryValue 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate' 'path'
    native = Read-RegistryValue 'HKLM:\SOFTWARE\Microsoft\EdgeUpdate' 'path'
    hkcu = Read-RegistryValue 'HKCU:\SOFTWARE\Microsoft\EdgeUpdate' 'path'
  }
  $result.edgeBrowserFolders = if (Test-Path -LiteralPath 'C:\Program Files (x86)\Microsoft\Edge\Application') { @(Get-ChildItem -LiteralPath 'C:\Program Files (x86)\Microsoft\Edge\Application' -Directory -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Name) -join ',' } else { '' }

  # Reader installation result.
  $installDir = Join-Path $env:LOCALAPPDATA 'Programs\AI-Native PDF Reader'
  $appExe = Join-Path $installDir 'ai-native-pdf-reader.exe'
  $uninstallEntry = Get-ItemProperty -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\io.github.ainativepdfreader' -ErrorAction SilentlyContinue
  $result.reader = [ordered]@{
    installDirectory = $installDir
    executablePresent = (Test-Path -LiteralPath $appExe)
    executableVersion = if (Test-Path -LiteralPath $appExe) { (Get-Item -LiteralPath $appExe).VersionInfo.FileVersion } else { '' }
    installedFiles = if (Test-Path -LiteralPath $installDir) { @(Get-ChildItem -LiteralPath $installDir -Recurse -File -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName.Substring($installDir.Length + 1) }) -join ',' } else { '' }
    uninstallDisplayVersion = if ($uninstallEntry) { $uninstallEntry.DisplayVersion } else { '' }
  }
  Write-Log ("reader exe present={0} version={1}" -f $result.reader.executablePresent, $result.reader.executableVersion)

  # Launch observation: a rendered window plus running msedgewebview2.exe children
  # is the only in-guest evidence that the reader found a usable runtime.
  if (-not $result.reader.executablePresent) {
    $result.launch = [ordered]@{ verdict = 'reader-not-installed' }
  } else {
    $sampleSource = Join-Path $PackageDirectory 'plain.pdf'
    $sampleCopy = Join-Path ([Environment]::GetFolderPath('Desktop')) 'guest-plain.pdf'
    Copy-Item -LiteralPath $sampleSource -Destination $sampleCopy -Force -ErrorAction SilentlyContinue
    $stderrFile = Join-Path $ResultsDirectory 'guest-app-stderr.txt'
    $before = @(Get-Process -Name 'msedgewebview2' -ErrorAction SilentlyContinue).Count
    Write-Log ("msedgewebview2 processes before launch: {0}" -f $before)
    $appProcess = Start-Process -FilePath $appExe -ArgumentList ('"' + $sampleCopy + '"') -PassThru -RedirectStandardError $stderrFile -RedirectStandardOutput (Join-Path $ResultsDirectory 'guest-app-stdout.txt')
    $windows = @()
    $engineProcesses = 0
    $enginePaths = ''
    $deadline = (Get-Date).AddSeconds($LaunchObservationSeconds)
    while ((Get-Date) -lt $deadline) {
      Start-Sleep -Seconds 3
      $engines = @(Get-Process -Name 'msedgewebview2' -ErrorAction SilentlyContinue)
      $engineProcesses = $engines.Count
      if ($engineProcesses -gt 0) { $enginePaths = (@($engines | ForEach-Object { $_.Path }) -join ' ; ') }
      $windows = @(Get-WindowInventory -TargetProcessId $appProcess.Id)
      if ($engineProcesses -gt 0 -and $windows.Count -gt 0) { break }
    }
    Save-Screenshot -Path (Join-Path $ResultsDirectory 'guest-app-window.png')
    $alive = $null -ne (Get-Process -Id $appProcess.Id -ErrorAction SilentlyContinue)
    $dialogWindows = @($windows | Where-Object { $_.StartsWith('#32770|') })
    $renderWindows = @($windows | Where-Object { $_ -notlike '#32770|*' })
    $result.launch = [ordered]@{
      sampleCopy = $sampleCopy
      observedSeconds = $LaunchObservationSeconds
      appProcessAlive = $alive
      msedgewebview2Processes = $engineProcesses
      msedgewebview2Paths = $enginePaths
      windowInventory = ($windows -join ' ; ')
      errorDialogWindows = $dialogWindows.Count
      nonDialogWindows = $renderWindows.Count
      stderrFile = $stderrFile
      verdict = if ($engineProcesses -gt 0 -and $renderWindows.Count -gt 0) { 'webview2-rendered' } elseif ($dialogWindows.Count -gt 0) { 'error-dialog-only' } elseif ($alive) { 'process-alive-without-webview2' } else { 'process-exited' }
    }
    Write-Log ("launch verdict: {0} (engineProcesses={1} windows={2})" -f $result.launch.verdict, $engineProcesses, $windows.Count)
    Stop-Process -Id $appProcess.Id -Force -ErrorAction SilentlyContinue
    Start-Sleep -Seconds 2
    Get-Process -Name 'msedgewebview2' -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
  }
} catch {
  $result.error = $_.Exception.Message
  Write-Log ("error: {0}" -f $_.Exception.Message)
} finally {
  $result.finishedAt = (Get-Date).ToString('o')
  $summary = @()
  $summary += 'AI-Native PDF Reader offline WebView2 guest verification'
  $summary += 'user: ' + $result.guestUser + ' (elevated: ' + $result.elevated + ', enableLua: ' + $result.uacEnableLua + ')'
  $summary += 'package hash matches expected: ' + $result.package.hashMatches
  $summary += 'setup exit code: ' + $result.install.setupExitCode + ' (timedOut: ' + $result.install.timedOut + ')'
  $summary += 'runtime before: pv=' + $result.runtimeBefore.resolvedPv + ' engineForRegisteredVersion=' + $result.runtimeBefore.engineForRegisteredVersion
  $summary += 'runtime after: pv=' + $result.runtimeAfter.resolvedPv + ' engineForRegisteredVersion=' + $result.runtimeAfter.engineForRegisteredVersion
  $summary += 'embedded payload extracted: ' + $result.embeddedPayload.present + ' (' + $result.embeddedPayload.size + ' bytes)'
  $summary += 'reader exe present: ' + $result.reader.executablePresent
  $summary += 'launch verdict: ' + $result.launch.verdict
  if ($result.launch.msedgewebview2Paths) { $summary += 'webview2 engine paths: ' + $result.launch.msedgewebview2Paths }
  if ($result.error) { $summary += 'error: ' + $result.error }
  $text = ($summary -join [Environment]::NewLine)
  try { Set-Content -LiteralPath $textPath -Value $text -Encoding UTF8 } catch { Write-Log 'failed to write the text summary' }
  try { ($result | ConvertTo-Json -Depth 6) | Set-Content -LiteralPath $jsonPath -Encoding UTF8 } catch { Write-Log 'failed to write the JSON result' }
  Write-Log 'guest verification finished; shutting the guest down'
  Start-Process -FilePath 'shutdown.exe' -ArgumentList '/s /t 20' -WindowStyle Hidden
}
