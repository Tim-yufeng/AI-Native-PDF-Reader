param([switch]$SelfTest)
# Read-only, application-specific Windows Sandbox diagnostic. No profile setup,
# ACL changes, network calls, credential access, report files, or disk compilation.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$profileName = 'io.github.ainativepdfreader.writer.prototype'

function Get-PrincipalClass([string]$sid, [string]$user, [string]$package) {
    if ($sid -ceq $user) { return 'USER' }
    if ($sid -ceq 'S-1-5-18') { return 'SYSTEM' }
    if ($sid -ceq 'S-1-5-32-544') { return 'ADMIN' }
    if ($sid -ceq 'S-1-3-0') { return 'CREATOR_OWNER' }
    if ($sid -ceq $package) { return 'OWN_PACKAGE' }
    if ($sid -ceq 'S-1-15-2-1') { return 'ALL_PACKAGES' }
    if ($sid -ceq 'S-1-15-2-2') { return 'RESTRICTED_PACKAGES' }
    if ($sid.StartsWith('S-1-15-2-', [StringComparison]::Ordinal)) { return 'OTHER_PACKAGE' }
    if ($sid.StartsWith('S-1-15-3-', [StringComparison]::Ordinal)) { return 'CAPABILITY' }
    return 'FOREIGN'
}


function New-ReadOnlyNativeApi {
    # Reflection.Emit keeps the small P/Invoke declarations entirely in memory.
    $name = [Reflection.AssemblyName]::new('WriterProfileReadOnlyDiagnostic')
    $domain = [AppDomain]::CurrentDomain
    if ($null -ne $domain.GetType().GetMethod('DefineDynamicAssembly', [Type[]]@([Reflection.AssemblyName], [Reflection.Emit.AssemblyBuilderAccess]))) {
        $assembly = $domain.DefineDynamicAssembly($name, [Reflection.Emit.AssemblyBuilderAccess]::Run)
    } else {
        $assembly = [Reflection.Emit.AssemblyBuilder]::DefineDynamicAssembly($name, [Reflection.Emit.AssemblyBuilderAccess]::Run)
    }
    $module = $assembly.DefineDynamicModule($name.Name)
    $type = $module.DefineType('WriterProfileReadOnlyNative', [Reflection.TypeAttributes]'Public, Sealed, Abstract')
    $attributes = [Reflection.MethodAttributes]'Public, Static, PinvokeImpl'
    $specs = @(
        @('DeriveAppContainerSidFromAppContainerName', 'userenv.dll', [int], [Type[]]@([string], [IntPtr].MakeByRefType())),
        @('GetAppContainerFolderPath', 'userenv.dll', [int], [Type[]]@([string], [IntPtr].MakeByRefType())),
        @('FreeSid', 'advapi32.dll', [IntPtr], [Type[]]@([IntPtr])),
        @('CoTaskMemFree', 'ole32.dll', [void], [Type[]]@([IntPtr])))
    foreach ($spec in $specs) {
        $library = [IO.Path]::Combine([Environment]::SystemDirectory, $spec[1])
        $method = $type.DefinePInvokeMethod($spec[0], $library, $attributes,
            [Reflection.CallingConventions]::Standard, $spec[2], $spec[3],
            [Runtime.InteropServices.CallingConvention]::Winapi, [Runtime.InteropServices.CharSet]::Unicode)
        $method.SetImplementationFlags([Reflection.MethodImplAttributes]::PreserveSig)
    }
    return $type.CreateType()
}

function Get-PathKey([string]$path) {
    if ($path.StartsWith('\\?\', [StringComparison]::Ordinal)) { $path = $path.Substring(4) }
    return [IO.Path]::GetFullPath($path).TrimEnd('\')
}

function Assert-PlainDirectoryChain([string]$path) {
    $item = Get-Item -LiteralPath $path -Force -ErrorAction Stop
    while ($null -ne $item) {
        if ($item -isnot [IO.DirectoryInfo] -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw 'Directory chain is linked or not a directory.'
        }
        $item = $item.Parent
    }
}

function Get-SafeFailure([Management.Automation.ErrorRecord]$record) {
    # Only fixed exception categories and a numeric HRESULT leave this function.
    # Do not print FullyQualifiedErrorId, messages, paths, or raw error objects.
    $exception = $record.Exception
    for ($i = 0; $i -lt 4 -and $null -ne $exception.InnerException; $i++) {
        $exception = $exception.InnerException
    }
    $kind = switch ($exception.GetType().Name) {
        'UnauthorizedAccessException' { 'ACCESS_DENIED' }
        'ItemNotFoundException' { 'NOT_FOUND' }
        'DirectoryNotFoundException' { 'NOT_FOUND' }
        'FileNotFoundException' { 'NOT_FOUND' }
        'PropertyNotFoundException' { 'SCRIPT_PROPERTY_MISSING' }
        'MethodException' { 'SCRIPT_METHOD_ERROR' }
        'ArgumentException' { 'INVALID_DESCRIPTOR_OR_ARGUMENT' }
        'InvalidOperationException' { 'UNSUPPORTED_STATE' }
        default { 'OTHER_READ_FAILURE' }
    }
    return ('Error={0} HRESULT=0x{1:X8}' -f $kind, ([long]$exception.HResult -band 4294967295L))
}

function Show-Descriptor([string]$label, [string]$path, [string]$user, [string]$package) {
    $stage = 'CHAIN'
    try {
        Assert-PlainDirectoryChain $path
        $stage = 'READ_ACL'
        $security = Get-Acl -LiteralPath $path -ErrorAction Stop
        $stage = 'DECODE_ACL'
        $raw = [Security.AccessControl.RawSecurityDescriptor]::new($security.GetSecurityDescriptorBinaryForm(), 0)
        $stage = 'PRINT_ACL'
        $owner = if ($null -eq $raw.Owner) { 'MISSING' } else { Get-PrincipalClass $raw.Owner.Value $user $package }
        Write-Output ('{0}: Owner={1} Control=0x{2:X4}' -f $label, $owner, [int]$raw.ControlFlags)
        if ($null -eq $raw.DiscretionaryAcl) {
            Write-Output ($label + ': DACL=NULL (unsafe; no repair attempted)')
            return
        }
        $count = $raw.DiscretionaryAcl.Count
        Write-Output ('{0}: ACE count={1}' -f $label, $count)
        if ($count -gt 128) {
            Write-Output ($label + ': REFUSED excessive ACE count; no repair attempted')
            return
        }
        for ($i = 0; $i -lt $count; $i++) {
            $ace = $raw.DiscretionaryAcl[$i]
            $principal = 'UNPARSED'
            $mask = 'n/a'
            if ($ace -is [Security.AccessControl.KnownAce]) {
                $principal = Get-PrincipalClass $ace.SecurityIdentifier.Value $user $package
                $mask = '0x{0:X8}' -f ([long]$ace.AccessMask -band 4294967295L)
            }
            Write-Output ('  ACE {0}: Type={1} Flags=0x{2:X2} Mask={3} Principal={4}' -f
                $i, [int]$ace.AceType, [int]$ace.AceFlags, $mask, $principal)
        }
    } catch {
        # Never print exception messages, paths, SID strings, or raw SDDL.
        Write-Output ('{0}: READ_REFUSED Stage={1} {2}' -f $label, $stage, (Get-SafeFailure $_))
    }
}

if ($SelfTest) {
    $syntheticUser = 'S-1-5-21-10-20-30-1001'
    $syntheticPackage = 'S-1-15-2-11-12-13-14-15-16-17'
    $cases = @(
        @($syntheticUser, 'USER'), @('S-1-5-18', 'SYSTEM'), @('S-1-5-32-544', 'ADMIN'),
        @('S-1-3-0', 'CREATOR_OWNER'), @($syntheticPackage, 'OWN_PACKAGE'),
        @('S-1-15-2-1', 'ALL_PACKAGES'), @('S-1-15-2-2', 'RESTRICTED_PACKAGES'),
        @('S-1-15-2-21-22-23-24-25-26-27', 'OTHER_PACKAGE'),
        @('S-1-15-3-123', 'CAPABILITY'), @('S-1-5-21-40-50-60-2001', 'FOREIGN'),
        @('S-1-3-4', 'FOREIGN'), @('S-1-1-0', 'FOREIGN'))
    foreach ($case in $cases) {
        if ((Get-PrincipalClass $case[0] $syntheticUser $syntheticPackage) -cne $case[1]) {
            throw 'Synthetic principal classification failed.'
        }
    }
    # Exercise the actual directory/descriptor path under StrictMode, including
    # CLR DirectoryInfo.Parent objects that lack the provider PSIsContainer tag.
    Assert-PlainDirectoryChain $PSScriptRoot
    $directory = [IO.DirectoryInfo]::new($PSScriptRoot)
    if ($null -eq $directory.Parent -or $directory.Parent -isnot [IO.DirectoryInfo]) {
        throw 'Directory ancestor fixture is unavailable.'
    }
    $current = [Security.Principal.WindowsIdentity]::GetCurrent()
    try { $actualUser = $current.User.Value } finally { $current.Dispose() }
    $output = @(Show-Descriptor 'SELF_TEST' $PSScriptRoot $actualUser $syntheticPackage)
    if (-not ($output -match '^SELF_TEST: Owner=') -or $output -match 'READ_REFUSED') {
        throw 'Read-only project descriptor check failed.'
    }
    $fileRejected = $false
    try { Assert-PlainDirectoryChain $PSCommandPath } catch { $fileRejected = $true }
    if (-not $fileRejected) { throw 'A file was accepted as a directory.' }
    $simulatedFailure = [Management.Automation.ErrorRecord]::new(
        [UnauthorizedAccessException]::new('FAKE_PRIVATE_PATH_NOT_FOR_OUTPUT'),
        'FAKE_PRIVATE_SID_NOT_FOR_OUTPUT', [Management.Automation.ErrorCategory]::PermissionDenied, $null)
    $safe = Get-SafeFailure $simulatedFailure
    if ($safe -notmatch 'ACCESS_DENIED' -or $safe.Contains('FAKE_PRIVATE')) { throw 'Safe failure classification failed.' }
    Write-Output 'SELF_TEST PASS: 12 principal classes, actual project ancestor/ACL read, file rejection, safe failure output. No host profile accessed.'
    exit 0
}


# This gate prevents accidental host use; the diagnostic itself grants nothing.
# Use the token identity, not USERNAME, which a caller can override.
$identity = $null
try {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $guest = $identity.Name.Split('\')[-1] -ceq 'WDAGUtilityAccount'
    if (-not $guest) {
        Write-Output 'REFUSED: Run this read-only diagnostic inside Windows Sandbox only.'
        exit 2
    }
    $machine = Get-CimInstance -ClassName Win32_ComputerSystem -ErrorAction Stop
    if ($machine.Manufacturer -cne 'Microsoft Corporation' -or
        $machine.Model -cne 'Virtual Machine' -or -not $machine.HypervisorPresent) {
        Write-Output 'REFUSED: Windows Sandbox virtual-machine checks did not match.'
        exit 2
    }
    $userSid = $identity.User.Value
} catch {
    Write-Output 'REFUSED: Sandbox identity could not be verified.'
    exit 2
} finally {
    if ($null -ne $identity) { $identity.Dispose() }
}

Write-Output 'Writer profile diagnostic v2 - READ ONLY - no profile or permissions changed'
$native = $null
$sidPointer = [IntPtr]::Zero
$pathPointer = [IntPtr]::Zero
try {
    $native = New-ReadOnlyNativeApi
    $hr = $native::DeriveAppContainerSidFromAppContainerName($profileName, [ref]$sidPointer)
    if ($hr -lt 0 -or $sidPointer -eq [IntPtr]::Zero) {
        Write-Output ('Package identity query failed: HRESULT=0x{0:X8}' -f ([long]$hr -band 4294967295L))
        exit 1
    }
    $packageSid = [Security.Principal.SecurityIdentifier]::new($sidPointer).Value
    $hr = $native::GetAppContainerFolderPath($packageSid, [ref]$pathPointer)
    if ($hr -lt 0 -or $pathPointer -eq [IntPtr]::Zero) {
        Write-Output ('Profile folder query failed: HRESULT=0x{0:X8}' -f ([long]$hr -band 4294967295L))
        exit 1
    }
    $length = 0
    while ($length -lt 32768 -and [Runtime.InteropServices.Marshal]::ReadInt16($pathPointer, $length * 2) -ne 0) { $length++ }
    if ($length -eq 32768) { throw 'Profile path exceeds bound.' }
    $returned = [Runtime.InteropServices.Marshal]::PtrToStringUni($pathPointer, $length)
    $local = [Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)
    if ([string]::IsNullOrEmpty($local) -or $local -notmatch '^[A-Za-z]:\\' -or $local.Substring(2).Contains(':')) {
        throw 'Local application data must use a local drive path.'
    }
    $root = Join-Path (Join-Path $local 'Packages') $profileName
    $ac = Join-Path $root 'AC'
    $returnedKey = Get-PathKey $returned
    $mapping = if ([string]::Equals($returnedKey, (Get-PathKey $root), [StringComparison]::OrdinalIgnoreCase)) { 'ROOT' }
        elseif ([string]::Equals($returnedKey, (Get-PathKey $ac), [StringComparison]::OrdinalIgnoreCase)) { 'AC' }
        else { 'UNEXPECTED' }
    Write-Output ('System folder mapping=' + $mapping)
    if ($mapping -ceq 'UNEXPECTED') {
        Write-Output 'REFUSED unexpected profile location; no directory inspected.'
        exit 1
    }
    Show-Descriptor 'ROOT' $root $userSid $packageSid
    Show-Descriptor 'AC' $ac $userSid $packageSid
    Write-Output 'DONE. Screenshot this window. No files, profiles, or permissions were changed.'
} catch {
    Write-Output ('DIAGNOSTIC_FAILED: ' + (Get-SafeFailure $_) + '; no repair attempted.')
    exit 1
} finally {
    if ($null -ne $native -and $pathPointer -ne [IntPtr]::Zero) { $native::CoTaskMemFree($pathPointer) }
    if ($null -ne $native -and $sidPointer -ne [IntPtr]::Zero) { [void]$native::FreeSid($sidPointer) }
}
