# Command-line probe of the cached NSIS System DLL only. No installer, window,
# registry write or ACL modification. All synthetic files/outputs stay in project.
$ErrorActionPreference='Stop'
$projectRoot=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
if ([IntPtr]::Size -ne 4) {
    $probeTemp=Join-Path $projectRoot 'tests/work/m7-directory/.tmp'
    [void][IO.Directory]::CreateDirectory($probeTemp)
    $priorTemp=$env:TEMP;$priorTmp=$env:TMP
    try {
        $env:TEMP=$probeTemp;$env:TMP=$probeTemp
        & "$env:SystemRoot\SysWOW64\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $PSCommandPath
        $probeExit=$LASTEXITCODE
    } finally {$env:TEMP=$priorTemp;$env:TMP=$priorTmp}
    exit $probeExit
}
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
$lab=Join-Path $projectRoot ('tests/work/m7-directory/'+[Guid]::NewGuid().ToString())
[void][IO.Directory]::CreateDirectory($lab)
$template=Get-Content -LiteralPath (Join-Path $projectRoot 'src-tauri/installer/reader.nsi') -Raw
$normalize=[regex]::Match($template,"System::Call '(kernel32::GetFullPathNameW[^']+)'").Groups[1].Value
$attributes=[regex]::Match($template,"System::Call '(kernel32::GetFileAttributesW[^']+)'").Groups[1].Value
$first=[regex]::Match($template,"System::Call '(kernel32::FindFirstFileW[^']+)'").Groups[1].Value
$next=[regex]::Match($template,"System::Call '(kernel32::FindNextFileW[^']+)'").Groups[1].Value
$close=[regex]::Match($template,"System::Call '(kernel32::FindClose[^']+\?e)'").Groups[1].Value
$name=[regex]::Match($template,'System::Call ''(\*\$8\(&v44,\s*&w260 \.r1\))''').Groups[1].Value
$pinDirectory=[regex]::Match($template,'(?s)Function \$\{PREFIX\}PinDirectory\r?\n(.*?)FunctionEnd').Groups[1].Value
$pinOpen=[regex]::Match($pinDirectory,"System::Call '(kernel32::CreateFileW[^']+)'").Groups[1].Value
$pinInformation=[regex]::Match($pinDirectory,"System::Call '(kernel32::GetFileInformationByHandle[^']+)'").Groups[1].Value
$pinDescriptor=[regex]::Match($pinDirectory,'System::Call ''(\*\$GuardBuffer\(i \.r0\))''').Groups[1].Value
foreach($call in @($normalize,$attributes,$first,$next,$close,$name,$pinOpen,$pinInformation,$pinDescriptor)) {if (-not $call) {throw 'Native source call missing'}}
# Validate the actual cached LogicLib expansion, not a C# numeric comparison
# standing in for the installer's string/integer dispatch.
$logicPath=Join-Path $projectRoot '.cache/tauri-target/.tauri/NSIS/Include/LogicLib.nsh'
$logic=Get-Content -LiteralPath $logicPath -Raw
$stringUnequal=[regex]::Match($logic,'(?s)!macro _!= _a _b _t _f\s+(.*?)!macroend').Groups[1].Value.Trim()
$stringEqual=[regex]::Match($logic,'(?s)!macro _== _a _b _t _f\s+(.*?)!macroend').Groups[1].Value.Trim()
$integerUnequal=[regex]::Match($logic,'(?s)!macro _<> _a _b _t _f\s+(.*?)!macroend').Groups[1].Value.Trim()
$integerEqual=[regex]::Match($logic,'(?s)!macro _= _a _b _t _f\s+(.*?)!macroend').Groups[1].Value.Trim()
if ($stringUnequal -cne '!insertmacro _== `${_a}` `${_b}` `${_f}` `${_t}`' -or
    $stringEqual -cne 'StrCmp `${_a}` `${_b}` `${_t}` `${_f}`' -or
    $integerUnequal -cne '!insertmacro _= `${_a}` `${_b}` `${_f}` `${_t}`' -or
    $integerEqual -cne 'IntCmp `${_a}` `${_b}` `${_t}` `${_f}` `${_f}`') {throw 'Cached LogicLib comparison contract changed'}
if ($pinDirectory -notmatch 'IntOp \$0 \$0 & 0x410\s+(?:;[^\r\n]*\r?\n\s*)*\$\{If\} \$0 <> 0x10') {throw 'PinDirectory must use the numeric comparison <> after masking attributes'}
$scriptComparison=[pscustomobject]@{Test='nsis-source-comparison-dispatch';Status='PASS';GuardOperator='<>';GuardLiteral='0x10';NumericDispatch='_<> -> _= -> IntCmp';LegacyDispatch='_!= -> _== -> StrCmp';LogicLibSha256=(Get-FileHash -LiteralPath $logicPath).Hash;Scope='exact cached macro bodies and actual guard source; installer execution not claimed'}
$dll=Join-Path $projectRoot '.cache/tauri-target/.tauri/NSIS/Plugins/x86-unicode/System.dll'
if ((Get-FileHash -LiteralPath $dll).Hash -cne '8B4C47C4CF5E76EC57DD5A050D5ACD832A0D532EE875D7B44F6CDAF68F90D37C') {throw 'Trusted cached plugin differs; refusing to load'}
Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.IO;
using System.Runtime.InteropServices;
public static class NsisDirectoryProbe {
    const int N=1024;
    [DllImport("kernel32.dll",CharSet=CharSet.Unicode)] static extern IntPtr LoadLibraryExW(string p,IntPtr f,uint flags);
    [DllImport("kernel32.dll",CharSet=CharSet.Ansi)] static extern IntPtr GetProcAddress(IntPtr m,string n);
    [DllImport("kernel32.dll")] static extern IntPtr GlobalAlloc(uint flags,UIntPtr bytes);
    [DllImport("kernel32.dll")] static extern IntPtr GlobalFree(IntPtr p);
    [DllImport("kernel32.dll")] static extern void SetLastError(uint e);
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)] delegate void Plugin(IntPtr h,int n,IntPtr v,ref IntPtr s,IntPtr x);
    [UnmanagedFunctionPointer(CallingConvention.StdCall)] delegate int Register(IntPtr m,IntPtr callback);
    [UnmanagedFunctionPointer(CallingConvention.StdCall)] delegate uint MockAttributes();
    [UnmanagedFunctionPointer(CallingConvention.StdCall)] delegate int MockNext(IntPtr h,IntPtr data);
    [UnmanagedFunctionPointer(CallingConvention.StdCall)] delegate uint MockNormalize(IntPtr path,uint count,IntPtr output,IntPtr part);
    [StructLayout(LayoutKind.Sequential)] struct Extra { public IntPtr flags,execute,validate,register; }
    static readonly Register register=(m,c)=>0;
    static readonly MockAttributes denied=()=>{SetLastError(5);return UInt32.MaxValue;};
    static readonly MockNext interrupted=(h,d)=>{SetLastError(5);return 0;};
    static readonly MockNormalize normalizationDenied=(p,n,o,f)=>{SetLastError(5);return 0;};
    static readonly MockNormalize normalizationEmpty=(p,n,o,f)=>{SetLastError(0);return 1;};
    static IntPtr module,vars,top,extra;
    static void Check(bool ok,string reason) {if(!ok) throw new Exception(reason);}
    static void Push(string text) {
        IntPtr p=GlobalAlloc(0x40,(UIntPtr)(IntPtr.Size+2*N));Check(p!=IntPtr.Zero,"stack allocation");
        Marshal.WriteIntPtr(p,top);char[] s=(text+"\0").ToCharArray();Check(s.Length<=N,"stack bound");
        Marshal.Copy(s,0,IntPtr.Add(p,IntPtr.Size),s.Length);top=p;
    }
    static string Pop() {
        Check(top!=IntPtr.Zero,"missing plugin stack result");IntPtr p=top;top=Marshal.ReadIntPtr(p);
        string text=Marshal.PtrToStringUni(IntPtr.Add(p,IntPtr.Size));GlobalFree(p);return text;
    }
    static void Set(int i,string v) {char[] c=(v+"\0").ToCharArray();Check(c.Length<=N,"variable bound");Marshal.Copy(c,0,IntPtr.Add(vars,i*2*N),c.Length);}
    static string Get(int i) {return Marshal.PtrToStringUni(IntPtr.Add(vars,i*2*N));}
    static long Number(int i) {long v;Check(Int64.TryParse(Get(i),out v),"invalid plugin return: "+Get(i));return v;}
    static void Invoke(string export,string argument) {
        Push(argument);
        // Like the NSIS engine, resolve the export before each plugin call.
        IntPtr address=GetProcAddress(module,export);Check(address!=IntPtr.Zero,"missing plugin export");
        ((Plugin)Marshal.GetDelegateForFunctionPointer(address,typeof(Plugin)))(IntPtr.Zero,N,vars,ref top,extra);
    }
    static void Call(string source,string path) {Invoke("Call",source.Replace("$INSTDIR",path).Replace("$GuardTarget",path).Replace("$GuardPath",path).Replace("$GuardHandle",Get(6)).Replace("$GuardBuffer",Get(8)).Replace("${NSIS_MAX_STRLEN}",N.ToString()).Replace("$8",Get(8)));}
    static uint Error() {uint v;Check(UInt32.TryParse(Pop(),out v),"invalid error capture");return v;}
    static string Address(Delegate f) {return "::0x"+Marshal.GetFunctionPointerForDelegate(f).ToInt32().ToString("x");}
    static bool Normalized(int capacity) {return Number(5)>0&&Number(5)<capacity&&Get(7).Length>0;}
    static object ReadPinned(string path,bool expectedDirectory,string open,string information,string descriptor) {
        Call(open,path);uint openError=Error();Check(Number(6)!=-1,"metadata open failed: "+path+" error "+openError);
        string handle=Get(6);bool allocated=false;
        try {
            Invoke("Alloc","52");Set(8,Pop());allocated=Number(8)!=0;Check(allocated,"metadata buffer allocation");
            Call(information,path);uint readError=Error();Check(Number(0)!=0,"metadata read failed: "+readError);
            Call(descriptor,path);long attributes=Number(0);long mask=attributes&0x410;
            Call("*$GuardBuffer(&v40, i .r1)",path);long links=Number(1);
            Check((mask==0x10)==expectedDirectory,"real descriptor directory policy mismatch");
            return new {test="native-pinned-descriptor",path=path,attributes=attributes,mask=mask,links=links,directoryAccepted=mask==0x10,openError=openError,metadataError=readError,metadataOnly=true};
        } finally {
            if(allocated)Invoke("Free",Get(8));Set(6,handle);Call("kernel32::CloseHandle(p r6) i .r3 ?e",path);uint closeError=Error();Check(Number(3)!=0,"metadata handle release failed: "+closeError);
        }
    }
    public static List<object> Run(string dll,string lab,string normalize,string attr,string first,string next,string close,string name,string pinOpen,string pinInformation,string pinDescriptor,string project) {
        Check(IntPtr.Size==4,"cached installer uses x86-unicode plugin");
        module=LoadLibraryExW(dll,IntPtr.Zero,0x900);Check(module!=IntPtr.Zero,"cannot load trusted cached System DLL");
        vars=Marshal.AllocHGlobal(25*2*N);for(int i=0;i<25;i++)Set(i,"");
        IntPtr flags=Marshal.AllocHGlobal(56);for(int i=0;i<14;i++)Marshal.WriteInt32(flags,i*4,0);
        Marshal.WriteInt32(flags,28,0x10000);extra=Marshal.AllocHGlobal(Marshal.SizeOf(typeof(Extra)));
        Marshal.StructureToPtr(new Extra {flags=flags,register=Marshal.GetFunctionPointerForDelegate(register)},extra,false);
        var results=new List<object>();
        const string chinese="\u4e2d\u6587 \u7a7a\u683c";
        foreach(string suffix in new[]{"missing","missing-parent\\reader",chinese+"\\reader"}) {
            string p=Path.Combine(lab,suffix);Call(attr,p);uint e=Error();long a=Number(3);
            Check(a==-1&&(e==2||e==3),"new path must preserve error 2/3");
            Call("kernel32::GetFileAttributesW(w \"$INSTDIR\") i .r3",p);
            Call("kernel32::GetLastError() i .r4",p);long old=Number(4);
            results.Add(new {test="missing-directory",suffix=suffix,attributes=a,error=e,legacySeparateError=old});
        }
        // Deterministic intervening-call failure: a separate read has no
        // association with the original failed API. This is not guest evidence.
        string absent=Path.Combine(lab,"clobbered");Call(attr,absent);uint captured=Error();
        Call("kernel32::SetLastError(i 0)",lab);Call("kernel32::GetLastError() i .r4",lab);
        Check(captured==2&&Number(4)==0,"saved error must survive later API activity");
        results.Add(new {test="intervening-call-clears-last-error",immediateError=captured,laterError=Number(4),injected=true});
        Call(attr,lab+"\\invalid<name");uint bad=Error();Check(Number(3)==-1&&bad==123,"invalid name must be rejected");
        results.Add(new {test="invalid-path",error=bad});
        Call(Address(denied)+"() i .r3 ?e",lab);uint access=Error();Check(Number(3)==-1&&access==5,"injected access denial must remain 5");
        results.Add(new {test="injected-access-denial",error=access,realAclChanged=false});
        foreach(string folder in new[]{"empty",chinese}) {
            string p=Path.Combine(lab,folder);Directory.CreateDirectory(p);Call(attr,p);Error();
            Check((Number(3)&0x10)!=0&&(Number(3)&0x400)==0,"ordinary directory attributes");
            Invoke("Alloc","592");Set(8,Pop());Check(Number(8)!=0,"find buffer allocation");
            Call(first,p);uint begin=Error();Check(Number(0)!=-1,"empty directory enumeration failed: "+begin);
            int count=0;uint end=0;
            do {Call(name,p);Check(Get(1)=="."||Get(1)=="..","unexpected filename layout at "+p+": "+Get(1)+"; call="+name+"; buffer="+Get(8));count++;Check(count<=4,"enumeration bound");
                Call(next,p);end=Error();} while(Number(3)!=0);
            Call(close,p);uint cleanup=Error();Check(Number(3)!=0,"find close failed: "+cleanup);Invoke("Free",Get(8));
            Check(end==18,"normal end must remain 18 across cleanup");results.Add(new {test="empty-directory",folder=folder,endError=end,entries=count});
        }
        string foreign=Path.Combine(lab,"foreign");Directory.CreateDirectory(foreign);string keep=Path.Combine(foreign,"keep.txt");File.WriteAllText(keep,"retain");
        Invoke("Alloc","592");Set(8,Pop());Call(first,foreign);Error();Check(Number(0)!=-1,"foreign directory find");
        bool found=false;uint terminal=0;
        for(int i=0;i<8;i++) {Call(name,foreign);if(Get(1)=="keep.txt")found=true;Call(next,foreign);terminal=Error();if(Number(3)==0)break;}
        Check(found&&terminal==18,"must see foreign file");
        Call(Address(interrupted)+"(p r0,p r8) i .r3 ?e",foreign);uint failure=Error();
        Call(close,foreign);Error();Invoke("Free",Get(8));Check(failure==5,"mid-enumeration denial preserved");
        Check(File.ReadAllText(keep)=="retain","foreign material unchanged");
        results.Add(new {test="foreign-material-and-injected-enumeration-failure",foundForeign=true,error=failure,unchanged=true});
        Call(attr,keep);Error();Check((Number(3)&0x10)==0,"file must not pass directory policy");results.Add(new {test="regular-file-not-directory"});
        // GetFullPathNameW is a lexical normalization step. It deliberately does
        // not inspect existence, traversal permissions or installer ownership.
        // The later attribute/pinning/foreign-material guards remain mandatory.
        foreach(string suffix in new[]{"new-deep\\one\\..\\two\\reader",chinese+"\\nested\\reader"}) {
            string p=Path.Combine(lab,suffix);Set(7,"");Call(normalize,p);uint e=Error();
            Check(Normalized(N),"new directory normalization failed: "+e);
            string expected=Path.GetFullPath(p);Check(String.Equals(Get(7),expected,StringComparison.OrdinalIgnoreCase),"normalized path mismatch");
            Check(!Directory.Exists(expected),"normalization must not create missing directories");
            results.Add(new {test="normalize-missing-directory",suffix=suffix,path=Get(7),returnLength=Number(5),error=e,created=false,authorizationChecked=false});
        }
        string existing=Path.Combine(lab,"normalize-existing");Directory.CreateDirectory(existing);
        string existingInput=existing+"\\child\\..";Set(7,"");Call(normalize,existingInput);uint existingError=Error();
        Check(Normalized(N)&&String.Equals(Get(7),existing,StringComparison.OrdinalIgnoreCase),"existing directory normalization changed identity");
        Check(Directory.Exists(existing)&&!Directory.Exists(Path.Combine(existing,"child")),"normalization changed directory contents");
        results.Add(new {test="normalize-existing-directory",path=Get(7),returnLength=Number(5),error=existingError,authorizationChecked=false});
        // Exercise the exact cached plugin's output handling with a short API
        // buffer as well as an input variable whose absolute result exceeds N.
        // Neither case writes a path or requires enabling long-path settings.
        string shortCall=normalize.Replace("i ${NSIS_MAX_STRLEN}","i 4");Check(shortCall!=normalize,"normalization buffer contract changed");
        Set(7,"");Call(shortCall,Path.Combine(lab,"short-buffer"));uint shortError=Error();
        Check(Number(5)>=4&&!Normalized(4),"insufficient native buffer must not be accepted");
        results.Add(new {test="normalize-small-buffer-rejected",capacity=4,requiredLength=Number(5),error=shortError,accepted=false});
        string variableCall=normalize.Replace("w \"$GuardTarget\"","w r6");Check(variableCall!=normalize,"normalization argument contract changed");
        Set(6,new String('q',N-2));Set(7,"");Call(variableCall,"");uint longError=Error();
        Check(Number(5)>=N&&!Normalized(N),"output beyond NSIS string capacity must not be accepted");
        results.Add(new {test="normalize-nsis-buffer-overflow-rejected",capacity=N,requiredLength=Number(5),error=longError,accepted=false,filesCreated=false});
        string normalizeArgs=normalize.Substring(normalize.IndexOf('('));
        Set(7,"stale result");Call(Address(normalizationDenied)+normalizeArgs,lab);uint deniedError=Error();
        Check(Number(5)==0&&deniedError==5&&!Normalized(N),"zero return with access error must not pass normalization");
        results.Add(new {test="normalize-injected-zero-access-denial",returnLength=Number(5),error=deniedError,accepted=false,injected=true,realAclChanged=false});
        Set(7,"");Call(Address(normalizationEmpty)+normalizeArgs,lab);uint emptyError=Error();
        Check(Number(5)==1&&Get(7).Length==0&&!Normalized(N),"empty output must not pass positive-return normalization");
        results.Add(new {test="normalize-injected-empty-output",returnLength=Number(5),error=emptyError,accepted=false,injected=true});
        results.Add(ReadPinned("C:\\",true,pinOpen,pinInformation,pinDescriptor));
        results.Add(ReadPinned(project,true,pinOpen,pinInformation,pinDescriptor));
        results.Add(ReadPinned(keep,false,pinOpen,pinInformation,pinDescriptor));
        // Reparse rejection tests the source descriptor read through the real
        // plugin, with synthetic metadata only; no link or ACL is created.
        Invoke("Alloc","52");Set(8,Pop());Check(Number(8)!=0,"synthetic descriptor allocation");
        try {
            Call("*$GuardBuffer(i 0x410)",lab);Call(pinDescriptor,lab);long synthetic=Number(0);long mask=synthetic&0x410;
            Check(mask==0x410&&mask!=0x10,"reparse-directory descriptor must be refused");
            results.Add(new {test="synthetic-reparse-descriptor-rejected",attributes=synthetic,mask=mask,directoryAccepted=false,simulatedMetadata=true,linkCreated=false});
        } finally {Invoke("Free",Get(8));}
        Check(top==IntPtr.Zero,"unbalanced plugin stack");GC.KeepAlive(register);GC.KeepAlive(denied);GC.KeepAlive(interrupted);GC.KeepAlive(normalizationDenied);GC.KeepAlive(normalizationEmpty);
        Marshal.FreeHGlobal(extra);Marshal.FreeHGlobal(flags);Marshal.FreeHGlobal(vars);
        return results;
    }
}
'@
$checks=[NsisDirectoryProbe]::Run($dll,$lab,$normalize,$attributes,$first,$next,$close,$name,$pinOpen,$pinInformation,$pinDescriptor,$projectRoot)
$result=[pscustomobject]@{Status='PASS';Scope='cached System DLL native calls and exact cached LogicLib/source checks only; not installer or Sandbox GUI';Checks=$checks;ScriptComparison=$scriptComparison;PluginSha256=(Get-FileHash -LiteralPath $dll).Hash;Lab=$lab;InstallerStarted=$false;RegistryWrites=0;AclChanges=0}
$result | ConvertTo-Json -Depth 6 | Tee-Object -FilePath (Join-Path $lab 'native-check.json')
