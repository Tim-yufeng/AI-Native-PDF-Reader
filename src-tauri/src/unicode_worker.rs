//! Controlled byte-only worker. No source-path authority and no PDF commit.
//! No profile creation or ordinary-user fallback. Lowbox startup is fail-closed.
use std::{fs::{self,File,OpenOptions},io::{self,Read,Write},path::{Path,PathBuf},
    os::windows::{ffi::OsStrExt,io::{AsRawHandle,FromRawHandle},fs::{OpenOptionsExt,MetadataExt}},
    sync::{mpsc,atomic::{AtomicBool,Ordering}},thread,time::{Duration,Instant}};
use serde::{Deserialize,Serialize};
#[cfg(test)]use std::os::windows::ffi::OsStringExt;
use windows_sys::Win32::{Foundation::*,Security::{*,Isolation::*,Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW},
    System::{Threading::*,JobObjects::*,Pipes::CreatePipe,IO::CancelSynchronousIo}};
const MAX:usize=16*1024*1024;
const MAGIC:&[u8;8]=b"AUPW\x01\x00\x00\x00";
static LOST:AtomicBool=AtomicBool::new(false);
static EXEC_BUSY:AtomicBool=AtomicBool::new(false);
#[cfg(test)]static TEST_PROCESS:std::sync::Mutex<()>=std::sync::Mutex::new(());
#[cfg(test)]pub(crate) fn test_process_guard()->std::sync::MutexGuard<'static,()>{TEST_PROCESS.lock().unwrap()}
#[cfg(test)]#[link(name="ole32")]unsafe extern "system"{fn CoTaskMemFree(value:*mut std::ffi::c_void);}
fn failure(reason:&str)->io::Error{io::Error::other(reason)}
#[derive(Clone,Copy,Debug)]
pub(crate) struct ProbeDetails{pub stage:&'static str,pub reason:&'static str,pub windows_error:Option<u32>,pub worker_exit:Option<u32>}
#[derive(Debug)]struct WorkerFailure{details:ProbeDetails,original:io::Error}
impl std::fmt::Display for WorkerFailure{fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result{std::fmt::Display::fmt(&self.original,f)}}
impl std::error::Error for WorkerFailure{}
fn failure_details(stage:&'static str,error:&io::Error,worker_exit:Option<u32>)->ProbeDetails{
    let text=error.to_string();
    let startup=text.strip_prefix("worker CreateProcess failed (Windows ").and_then(|s|s.strip_suffix(')')).and_then(|s|s.parse::<u32>().ok());
    let windows_error=error.raw_os_error().map(|n|n as u32).or(startup);
    let reason=if worker_exit.is_some(){"child-exited"}else if let Some(code)=windows_error{match code{5=>"access-denied",2|3=>"not-found",203=>"environment-missing",_=>"os-error"}}else{match text.as_str(){
        "worker cancelled"=>"cancelled","worker timeout"=>"timeout","worker already running"=>"busy",
        "worker pipe failure"|"worker pipes did not close"=>"pipe-failure",
        "worker isolation probe refused"=>"denial-unproven","worker probe protocol"=>"protocol-invalid",
        "worker memory probe refused"|"worker timeout probe refused"|"worker cancellation probe refused"|"worker output-budget probe refused"=>"limit-unproven",
        "worker not AppContainer"|"worker not less-privileged AppContainer"|"worker integrity exceeds low"|"worker unexpected capability"|"worker package identity differs"=>"token-invalid",
        "worker isolation access matrix differs"|"worker access probe inconsistent"|"worker access probe used privilege"=>"isolation-mismatch",
        "worker termination unresolved"|"previous worker termination unresolved"=>"termination-unresolved",
        _=>"unknown"}};
    ProbeDetails{stage,reason,windows_error,worker_exit}
}
fn probe_error_at(stage:&'static str,mut error:io::Error)->io::Error{
    if let Some(failure)=error.get_mut().and_then(|e|e.downcast_mut::<WorkerFailure>()){
        // Exit metadata already records what happened. Associate it with the
        // actual check, without losing the native exit code or a closer API error.
        if failure.details.stage=="worker-exit"&&stage.starts_with("probe-"){failure.details.stage=stage;}
        return error;
    }
    io::Error::new(error.kind(),WorkerFailure{details:failure_details(stage,&error,None),original:error})
}
fn probe_step<T>(stage:&'static str,result:io::Result<T>)->io::Result<T>{result.map_err(|e|probe_error_at(stage,e))}
pub(crate) fn probe_details(error:&io::Error)->ProbeDetails{
    error.get_ref().and_then(|e|e.downcast_ref::<WorkerFailure>()).map(|e|e.details).unwrap_or_else(||failure_details("probe-runtime",error,None))
}
fn wide(path:&std::ffi::OsStr)->Vec<u16>{path.encode_wide().chain(Some(0)).collect()}
// A custom CreateProcess environment replaces the parent's entire block.
// Windows needs LOCALAPPDATA while constructing the AppContainer environment;
// obtain it from the OS known-folder API, never from renderer input or by
// inheriting/enumerating parent variables (which could contain credentials).
// This supplies a location, not access authority: the same LPAC/DACL/Job checks
// still decide every read, write, network operation and child-process attempt.
fn worker_environment(system:&std::ffi::OsStr,local:&Path,root:&Path)->io::Result<Vec<u16>>{
    let mut block=Vec::new();
    for(name,value)in [("LOCALAPPDATA",local.as_os_str()),("SYSTEMROOT",system),("TEMP",root.as_os_str()),("TMP",root.as_os_str())]{
        let value:Vec<_>=value.encode_wide().collect();
        if value.is_empty()||value.contains(&0){return Err(failure("worker environment invalid"));}
        block.extend(name.encode_utf16());block.push(b'=' as u16);block.extend(value);block.push(0);
    }
    block.push(0);
    if block.len()>32767{return Err(failure("worker environment exceeds limit"));}
    Ok(block)
}
struct Handle(HANDLE);
impl Drop for Handle{fn drop(&mut self){unsafe{CloseHandle(self.0);}}}
struct Sid(PSID);
impl Drop for Sid{fn drop(&mut self){unsafe{FreeSid(self.0);}}}
struct Attributes{_buffer:Vec<usize>,ptr:LPPROC_THREAD_ATTRIBUTE_LIST}
impl Drop for Attributes{fn drop(&mut self){unsafe{DeleteProcThreadAttributeList(self.ptr);}}}
struct Process{process:Handle,_thread:Handle,job:Handle}
impl Drop for Process{fn drop(&mut self){unsafe{
    if WaitForSingleObject(self.process.0,0)!=WAIT_OBJECT_0{
        TerminateJobObject(self.job.0,125);
        // Covers failure before assignment to the Job as well as its failure.
        TerminateProcess(self.process.0,125);
        if WaitForSingleObject(self.process.0,2000)!=WAIT_OBJECT_0{LOST.store(true,Ordering::Release);}
    }
}}}
fn bool_result(ok:i32)->io::Result<()>{if ok==0{Err(io::Error::last_os_error())}else{Ok(())}}
fn token_access_probe(token:HANDLE,user:&str,principal:Option<&str>)->io::Result<bool>{
    // A virtual descriptor only: no object/ACL is created or changed. Mask 1
    // has no implicit owner/privilege grant and contains no generic rights.
    let extra=principal.map(|p|format!("(A;;0x1;;;{p})")).unwrap_or_default();
    let text=wide(std::ffi::OsStr::new(&format!("O:SYG:SYD:P(A;;0x1;;;{user}){extra}")));
    let mut raw=std::ptr::null_mut();bool_result(unsafe{ConvertStringSecurityDescriptorToSecurityDescriptorW(text.as_ptr(),1,&mut raw,std::ptr::null_mut())})?;
    struct Descriptor(PSECURITY_DESCRIPTOR);impl Drop for Descriptor{fn drop(&mut self){unsafe{LocalFree(self.0.cast());}}}let descriptor=Descriptor(raw);
    let mut copy=std::ptr::null_mut();bool_result(unsafe{DuplicateToken(token,SecurityImpersonation,&mut copy)})?;let copy=Handle(copy);
    let mapping=GENERIC_MAPPING{GenericRead:1,GenericWrite:1,GenericExecute:1,GenericAll:1};
    let mut privileges=vec![0usize;128];let mut length=(privileges.len()*std::mem::size_of::<usize>())as u32;
    let(mut granted,mut allowed)=(0u32,0i32);
    bool_result(unsafe{AccessCheck(descriptor.0,copy.0,1,&mapping,privileges.as_mut_ptr().cast(),&mut length,&mut granted,&mut allowed)})?;
    if unsafe{(*(privileges.as_ptr().cast::<PRIVILEGE_SET>())).PrivilegeCount}!=0{return Err(failure("worker access probe used privilege"));}
    match(allowed,granted){(0,0)=>Ok(false),(1,1)=>Ok(true),_=>Err(failure("worker access probe inconsistent"))}
}
fn access_matrix(own:bool,all:bool,user:bool,other:bool,lpac:bool)->io::Result<()>{
    if own&&(all!=lpac)&&!user&&!other{Ok(())}else{Err(failure("worker isolation access matrix differs"))}
}
fn verify_access_identity(token:HANDLE,package:PSID,lpac:bool)->io::Result<()>{
    let mut buffer=vec![0usize;128];let mut size=0;
    bool_result(unsafe{GetTokenInformation(token,TokenUser,buffer.as_mut_ptr().cast(),(buffer.len()*std::mem::size_of::<usize>())as u32,&mut size)})?;
    let user=unsafe{(*(buffer.as_ptr().cast::<TOKEN_USER>())).User.Sid};
    if user.is_null()||unsafe{IsValidSid(user)}==0{return Err(failure("worker user identity invalid"));}
    let user=crate::storage::directories::sid_text(user).map_err(|_|failure("worker user identity invalid"))?;
    if user!=crate::storage::directories::current_sid().map_err(|_|failure("current user identity unavailable"))?{return Err(failure("worker user identity differs"));}
    let package=crate::storage::directories::sid_text(package).map_err(|_|failure("worker package identity invalid"))?;
    // Four checks on the SAME token. API errors propagate; only an explicit
    // successful access-denied result is a negative. A bad descriptor or a
    // token that cannot access anything cannot pass the own-package control.
    access_matrix(token_access_probe(token,&user,Some(&package))?,token_access_probe(token,&user,Some("S-1-15-2-1"))?,
        token_access_probe(token,&user,None)?,token_access_probe(token,&user,Some("S-1-15-2-424242"))?,lpac)
}
fn sid()->io::Result<Sid>{let mut sid=std::ptr::null_mut();let name=wide(std::ffi::OsStr::new("io.github.ainativepdfreader.writer.prototype"));let hr=unsafe{DeriveAppContainerSidFromAppContainerName(name.as_ptr(),&mut sid)};if hr<0{return Err(failure("worker SID unavailable; no profile installation"));}Ok(Sid(sid))}
#[derive(Deserialize)]#[serde(deny_unknown_fields)]struct Entry{path:String,bytes:u64,sha256:String}
#[derive(Deserialize)]struct Manifest{schema:u32,files:Vec<Entry>}
/// Internal-only fixed project prototype; a release resource locator is separate.
pub(crate) fn prototype()->PathBuf{crate::save_core::project().join(".cache/python-worker")}
/// One broker-owned locator for installed resources. Tauri's resource_dir is
/// canonicalized (verbatim prefix on Windows), while std::current_exe is not.
/// Mixing their lexical representations rejects our own installed runtime.
/// Do not normalize renderer input or relax the later path/inventory guards.
pub(crate) fn installed_runtime()->io::Result<PathBuf>{
    Ok(std::env::current_exe()?.parent().ok_or_else(||failure("worker application path"))?.join("unicode-worker"))
}
pub(crate) fn pin_chain(path:&Path,pinned:&mut Vec<File>)->io::Result<()>{
    for dir in path.ancestors().collect::<Vec<_>>().into_iter().rev(){
        let file=OpenOptions::new().access_mode(0x81).share_mode(1).custom_flags(0x02200000).open(dir)?;
        if file.metadata()?.file_attributes()&0x400!=0{return Err(failure("linked runtime ancestor"));}pinned.push(file);
    }Ok(())
}
fn pin_runtime(root:&Path)->io::Result<Vec<File>>{
    let installed=installed_runtime()?;
    if root!=installed&&!(cfg!(debug_assertions)&&root==prototype()){return Err(failure("worker runtime not approved"));}
    let mut pinned=Vec::new();pin_chain(root,&mut pinned)?;
    let mut manifest_file=OpenOptions::new().read(true).share_mode(1).custom_flags(0x00200000).open(root.join("manifest.json"))?;
    let mut manifest_info=windows_sys::Win32::Storage::FileSystem::BY_HANDLE_FILE_INFORMATION::default();
    bool_result(unsafe{windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandle(manifest_file.as_raw_handle(),&mut manifest_info)})?;
    if manifest_info.nNumberOfLinks!=1||manifest_info.dwFileAttributes&(0x400|0x10)!=0||manifest_file.metadata()?.len()>1024*1024{return Err(failure("worker manifest shape"));}
    let mut data=vec![];Read::by_ref(&mut manifest_file).take(1024*1024+1).read_to_end(&mut data)?;pinned.push(manifest_file);
    let expected=include_str!("../../worker/runtime-lock.json");let lock:serde_json::Value=serde_json::from_str(expected)?;
    if crate::save_core::hash(&data)!=lock["manifestSha256"]{return Err(failure("worker manifest differs"));}
    let manifest:Manifest=serde_json::from_slice(&data)?;if manifest.schema!=1||manifest.files.len()>1000{return Err(failure("worker manifest budget"));}
    fn inventory(root:&Path,dir:&Path,result:&mut std::collections::BTreeSet<String>)->io::Result<()>{for child in fs::read_dir(dir)?{let child=child?;let metadata=fs::symlink_metadata(child.path())?;if metadata.file_attributes()&0x400!=0{return Err(failure("linked runtime resource"));}let relative=child.path().strip_prefix(root).map_err(|_|failure("runtime scope"))?.to_string_lossy().replace('\\',"/");if metadata.is_dir(){result.insert(format!("{relative}/"));inventory(root,&child.path(),result)?;}else if metadata.is_file(){result.insert(relative);}else{return Err(failure("unknown runtime resource"));}}Ok(())}
    let mut actual=std::collections::BTreeSet::new();inventory(root,root,&mut actual)?;
    // Broker-only non-importable identity marker. Its ACL/content are separately
    // verified by worker_sandbox before any production launch; never arbitrary
    // extra .py/.pth files or modules.
    actual.remove("sandbox-state.json");
    let mut expected:std::collections::BTreeSet<_>=manifest.files.iter().map(|e|e.path.clone()).collect();expected.insert("manifest.json".into());
    for entry in &manifest.files{for (index,_)in entry.path.match_indices('/'){expected.insert(format!("{}/",&entry.path[..index]));}}
    if actual!=expected{return Err(failure("unknown runtime module or data"));}
    for entry in manifest.files{
        let relative=Path::new(&entry.path);
        if relative.is_absolute()||relative.components().any(|p|!matches!(p,std::path::Component::Normal(_))){return Err(failure("worker manifest path"));}
        let path=root.join(relative);pin_chain(path.parent().unwrap(),&mut pinned)?;let metadata=fs::symlink_metadata(&path)?;
        if !metadata.is_file()||metadata.file_attributes()&0x400!=0||metadata.len()!=entry.bytes||entry.bytes>64*1024*1024{return Err(failure("worker resource shape"));}
        // Share-read only pins code/font versions throughout execution.
        let mut file=OpenOptions::new().read(true).share_mode(1).custom_flags(0x00200000).open(path)?;
        let mut info=windows_sys::Win32::Storage::FileSystem::BY_HANDLE_FILE_INFORMATION::default();
        bool_result(unsafe{windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandle(file.as_raw_handle(),&mut info)})?;
        if info.nNumberOfLinks!=1||info.dwFileAttributes&0x400!=0{return Err(failure("linked runtime file"));}
        let mut bytes=Vec::new();file.read_to_end(&mut bytes)?;
        if crate::save_core::hash(&bytes)!=entry.sha256{return Err(failure("worker resource identity"));}
        pinned.push(file);
    }Ok(pinned)
}
pub(crate) fn verify_runtime(root:&Path)->io::Result<()>{pin_runtime(root).map(drop)}
pub(crate) fn lock_runtime(root:&Path)->io::Result<Vec<File>>{pin_runtime(root)}
#[cfg(test)]
fn grant_prototype(root:&Path)->io::Result<()>{
    // Only our newly assembled disposable runtime. No existing user ACL changes.
    if root!=prototype(){return Err(failure("runtime ACL scope"));}
    let mut _pins=vec![];pin_chain(root,&mut _pins)?;let sid=sid()?;let text=crate::storage::directories::sid_text(sid.0).map_err(|_|failure("package SID query"))?;
    let me=crate::storage::directories::current_sid().map_err(|_|failure("current user SID query"))?;
    let sddl=wide(std::ffi::OsStr::new(&format!("D:P(A;OICI;FA;;;{me})(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;GRGX;;;{text})")));
    let mut sd=std::ptr::null_mut();bool_result(unsafe{ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(),1,&mut sd,std::ptr::null_mut())})?;
    fn apply(path:&Path,sd:PSECURITY_DESCRIPTOR)->io::Result<()>{let meta=fs::symlink_metadata(path)?;if meta.file_attributes()&0x400!=0{return Err(failure("linked runtime"));}bool_result(unsafe{SetFileSecurityW(wide(path.as_os_str()).as_ptr(),DACL_SECURITY_INFORMATION|PROTECTED_DACL_SECURITY_INFORMATION,sd)})?;if meta.is_dir(){for child in fs::read_dir(path)?{apply(&child?.path(),sd)?;}}Ok(())}
    let result=apply(root,sd);unsafe{LocalFree(sd.cast());}result
}
fn pipe(child_reads:bool)->io::Result<(File,Handle)>{
    let(mut read,mut write)=(std::ptr::null_mut(),std::ptr::null_mut());let sa=SECURITY_ATTRIBUTES{nLength:std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,lpSecurityDescriptor:std::ptr::null_mut(),bInheritHandle:1};
    bool_result(unsafe{CreatePipe(&mut read,&mut write,&sa,0)})?;
    let(parent,child)=if child_reads{(write,read)}else{(read,write)};
    if let Err(e)=bool_result(unsafe{SetHandleInformation(parent,HANDLE_FLAG_INHERIT,0)}){unsafe{CloseHandle(parent);CloseHandle(child);}return Err(e);}
    Ok((unsafe{File::from_raw_handle(parent)},Handle(child)))
}
fn launch(root:&Path,script:&str)->io::Result<(Process,File,File,File)>{
    launch_options(root,script,true,true,true)
}
fn fixed_task_path(root:&Path,script:&str)->io::Result<PathBuf>{
    // Build the fixed task component by component. PathBuf preserves the '/'
    // in a joined relative string on Windows, but the pinned native entry
    // compares argv text with its own backslash spelling, not Path equality.
    let leaf=match script{
        "worker/main.py"=>"main.py",
        "worker/isolation.py"=>"isolation.py",
        _=>return Err(failure("worker task not approved")),
    };
    Ok(root.join("worker").join(leaf))
}
// Startup diagnostics vary only handles/CWD while retaining the same LPAC.
// Never a fallback token, profile creation, or production launch alternative.
fn launch_options(root:&Path,script:&str,inherit:bool,cwd_enabled:bool,resume:bool)->io::Result<(Process,File,File,File)>{
    launch_policy(root,script,inherit,cwd_enabled,resume,true)
}
fn launch_policy(root:&Path,script:&str,inherit:bool,cwd_enabled:bool,resume:bool,lpac:bool)->io::Result<(Process,File,File,File)>{
    // A regular AppContainer is ONLY a suspended negative control. It cannot
    // run Python or receive PDF bytes, even if a caller accidentally asks.
    if !lpac&&(resume||inherit){return Err(failure("ordinary AppContainer execution forbidden"));}
    if LOST.load(Ordering::Acquire){return Err(failure("previous worker termination unresolved"));}
    let sid=sid()?;let capabilities=SECURITY_CAPABILITIES{AppContainerSid:sid.0,Capabilities:std::ptr::null_mut(),CapabilityCount:0,Reserved:0};
    let(input,output,errors)=(probe_step("worker-pipes",pipe(true))?,probe_step("worker-pipes",pipe(false))?,probe_step("worker-pipes",pipe(false))?);
    let mut handles=[input.1.0,output.1.0,errors.1.0];let mut policy=u32::from(lpac); // Ordinary control never resumes.
    let mut length=0;unsafe{InitializeProcThreadAttributeList(std::ptr::null_mut(),3,0,&mut length);}
    let mut buffer=vec![0usize;(length+std::mem::size_of::<usize>()-1)/std::mem::size_of::<usize>()];let ptr=buffer.as_mut_ptr().cast();
    probe_step("worker-policy",bool_result(unsafe{InitializeProcThreadAttributeList(ptr,3,0,&mut length)}))?;let attributes=Attributes{_buffer:buffer,ptr};
    probe_step("worker-policy",bool_result(unsafe{UpdateProcThreadAttribute(ptr,0,PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES as usize,(&capabilities as *const SECURITY_CAPABILITIES).cast(),std::mem::size_of_val(&capabilities),std::ptr::null_mut(),std::ptr::null())}))?;
    if inherit{probe_step("worker-policy",bool_result(unsafe{UpdateProcThreadAttribute(ptr,0,PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,handles.as_mut_ptr().cast(),std::mem::size_of_val(&handles),std::ptr::null_mut(),std::ptr::null())}))?;}
    probe_step("worker-policy",bool_result(unsafe{UpdateProcThreadAttribute(ptr,0,PROC_THREAD_ATTRIBUTE_ALL_APPLICATION_PACKAGES_POLICY as usize,(&mut policy as *mut u32).cast(),std::mem::size_of_val(&policy),std::ptr::null_mut(),std::ptr::null())}))?;
    let job=Handle(unsafe{CreateJobObjectW(std::ptr::null(),std::ptr::null())});if job.0.is_null(){return Err(probe_error_at("worker-job-limits",io::Error::last_os_error()));}
    let mut limits=JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();limits.BasicLimitInformation.LimitFlags=JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE|JOB_OBJECT_LIMIT_PROCESS_MEMORY|JOB_OBJECT_LIMIT_ACTIVE_PROCESS|JOB_OBJECT_LIMIT_PROCESS_TIME;
    limits.ProcessMemoryLimit=384*1024*1024;limits.BasicLimitInformation.ActiveProcessLimit=1;limits.BasicLimitInformation.PerProcessUserTimeLimit=300_000_000;
    probe_step("worker-job-limits",bool_result(unsafe{SetInformationJobObject(job.0,JobObjectExtendedLimitInformation,(&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),std::mem::size_of_val(&limits)as u32)}))?;
    let executable=root.join("python.exe");let app=wide(executable.as_os_str());let script=fixed_task_path(root,script)?;
    if executable.to_string_lossy().contains('"')||script.to_string_lossy().contains('"'){return Err(failure("runtime quote"));}
    let mut command=wide(std::ffi::OsStr::new(&if resume{format!("\"{}\" -I -B -X utf8 \"{}\"",executable.display(),script.display())}else{format!("\"{}\" -I -V",executable.display())}));
    let system=std::env::var_os("SystemRoot").unwrap_or("C:\\Windows".into());
    let local=probe_step("worker-policy",crate::worker_sandbox::local_app_data())?;
    let env=probe_step("worker-policy",worker_environment(&system,&local,root))?;
    let cwd=wide(root.as_os_str());let mut info=STARTUPINFOEXW::default();info.StartupInfo.cb=std::mem::size_of_val(&info) as u32;if inherit{info.StartupInfo.dwFlags=STARTF_USESTDHANDLES;info.StartupInfo.hStdInput=input.1.0;info.StartupInfo.hStdOutput=output.1.0;info.StartupInfo.hStdError=errors.1.0;}info.lpAttributeList=attributes.ptr;
    let mut process=PROCESS_INFORMATION::default();
    if unsafe{CreateProcessW(app.as_ptr(),command.as_mut_ptr(),std::ptr::null(),std::ptr::null(),inherit as i32,CREATE_SUSPENDED|CREATE_NO_WINDOW|CREATE_UNICODE_ENVIRONMENT|EXTENDED_STARTUPINFO_PRESENT,env.as_ptr().cast(),if cwd_enabled{cwd.as_ptr()}else{std::ptr::null()},&info.StartupInfo,&mut process)}==0{
        return Err(probe_error_at("worker-start",failure(&format!("worker CreateProcess failed (Windows {})",io::Error::last_os_error().raw_os_error().unwrap_or(0)))));
    }
    let child=Process{process:Handle(process.hProcess),_thread:Handle(process.hThread),job};
    probe_step("worker-job-assignment",bool_result(unsafe{AssignProcessToJobObject(child.job.0,child.process.0)}))?;
    let mut token=std::ptr::null_mut();probe_step("worker-token-open",bool_result(unsafe{OpenProcessToken(child.process.0,TOKEN_QUERY|TOKEN_DUPLICATE,&mut token)}))?;let token=Handle(token);let(mut flag,mut size)=(0u32,0u32);
    probe_step("worker-token-app",bool_result(unsafe{GetTokenInformation(token.0,TokenIsAppContainer,(&mut flag as *mut u32).cast(),4,&mut size)}))?;
    if flag!=1{return Err(probe_error_at("worker-token",failure("worker not AppContainer")));}
    let mut label=vec![0usize;64];probe_step("worker-token-integrity",bool_result(unsafe{GetTokenInformation(token.0,TokenIntegrityLevel,label.as_mut_ptr().cast(),(label.len()*std::mem::size_of::<usize>())as u32,&mut size)}))?;
    let integrity=unsafe{let sid=(*(label.as_ptr().cast::<TOKEN_MANDATORY_LABEL>())).Label.Sid;*GetSidSubAuthority(sid,(*GetSidSubAuthorityCount(sid)-1)as u32)};
    if integrity>4096{return Err(probe_error_at("worker-token",failure("worker integrity exceeds low")));}
    let mut group_buffer=vec![0usize;1024];probe_step("worker-token-capabilities",bool_result(unsafe{GetTokenInformation(token.0,TokenCapabilities,group_buffer.as_mut_ptr().cast(),(group_buffer.len()*std::mem::size_of::<usize>())as u32,&mut size)}))?;
    if unsafe{(*(group_buffer.as_ptr().cast::<TOKEN_GROUPS>())).GroupCount}!=0{return Err(probe_error_at("worker-token",failure("worker unexpected capability")));}
    let mut bytes=vec![0usize;64];probe_step("worker-token-package",bool_result(unsafe{GetTokenInformation(token.0,TokenAppContainerSid,bytes.as_mut_ptr().cast(),(bytes.len()*std::mem::size_of::<usize>())as u32,&mut size)}))?;
    let actual=unsafe{(*(bytes.as_ptr().cast::<TOKEN_APPCONTAINER_INFORMATION>())).TokenAppContainer};
    if unsafe{EqualSid(actual,sid.0)}==0{return Err(probe_error_at("worker-token",failure("worker package identity differs")));}
    // This is required on EVERY real Writer launch, not cached after a probe.
    probe_step("worker-token-access",verify_access_identity(token.0,actual,lpac))?;
    if resume&&unsafe{ResumeThread(child._thread.0)}==u32::MAX{return Err(probe_error_at("worker-resume",io::Error::last_os_error()));}
    drop(attributes); // child pipe ends close after launch.
    Ok((child,input.0,output.0,errors.0))
}
fn bounded(file:File,max:usize)->io::Result<Vec<u8>>{let mut out=vec![];file.take((max+1) as u64).read_to_end(&mut out)?;if out.len()>max{return Err(failure("worker stream budget"));}Ok(out)}
fn execution_outcome(status:u32,aborted:Option<&str>,pipe_failed:bool)->io::Result<()>{
    if let Some(reason)=aborted{return Err(failure(reason));}
    // A child can exit before its reader reports an excessive output stream.
    // Classify the fully collected results, not just the last polling instant.
    // Both outcomes remain failures for PDF conversion; no fallback is enabled.
    if pipe_failed{return Err(failure("worker pipe failure"));}
    if status!=0{let original=failure("worker refused or exited");return Err(io::Error::other(WorkerFailure{details:failure_details("worker-exit",&original,Some(status)),original}));}
    Ok(())
}
fn with_stderr_category(mut error:io::Error,stderr:&[u8])->io::Error{
    if let Some(failure)=error.get_mut().and_then(|e|e.downcast_mut::<WorkerFailure>()){
        if failure.details.worker_exit.is_some(){
            // Error bytes are untrusted. Match only a finite exception family;
            // never emit module names, paths, messages or the original stream.
            let mut entry_seen=false;
            for line in stderr.split(|b|*b==b'\n').rev(){
                // The entry may fail before Python can produce a traceback.
                // Read ONLY fixed stages and bounded decimal codes, never
                // arbitrary library paths/messages. Progress is not success.
                if let Some((stage,code))=entry_failure_line(line){
                    if entry_seen{continue;}entry_seen=true;
                    failure.details.stage=stage;
                    if stage=="entry-python-return"||code==0{failure.details.reason="entry-initialization";}
                    else{
                        let detail=failure_details(stage,&io::Error::from_raw_os_error(code as i32),None);
                        failure.details.reason=detail.reason;failure.details.windows_error=detail.windows_error;
                    }
                    // Py_Main can return after a traceback. Retain its final
                    // stage while still allowing a fixed exception family.
                    if stage=="entry-python-return"{continue;}
                    break;
                }
                let reason=if line.starts_with(b"ModuleNotFoundError:"){Some("python-module")}
                    else if line.starts_with(b"ImportError:"){Some("python-import")}
                    else if line.starts_with(b"PermissionError:"){Some("python-access")}
                    else if line.starts_with(b"FileNotFoundError:"){Some("python-file")}
                    else if line.starts_with(b"MemoryError"){Some("python-memory")}
                    else{None};
                if let Some(reason)=reason{failure.details.reason=reason;break;}
            }
        }
    }
    error
}
fn entry_failure_line(line:&[u8])->Option<(&'static str,u32)>{
    let text=std::str::from_utf8(line.strip_suffix(b"\r").unwrap_or(line)).ok()?;
    let(stage,number)=text.strip_prefix("AUPW_ENTRY_V1 ")?.split_once(' ')?;
    let stage=match stage{
        "entry-start"=>"entry-start","entry-path"=>"entry-path","entry-args"=>"entry-args",
        "entry-crt-primary"=>"entry-crt-primary","entry-crt-secondary"=>"entry-crt-secondary",
        "entry-python-probe-read"=>"entry-python-probe-read","entry-python-probe-map"=>"entry-python-probe-map",
        "entry-python-load"=>"entry-python-load","entry-python-symbol"=>"entry-python-symbol",
        "entry-python-main"=>"entry-python-main","entry-python-return"=>"entry-python-return",_=>return None};
    if number.is_empty()||number.len()>10||!number.bytes().all(|c|c.is_ascii_digit()){return None;}
    Some((stage,number.parse().ok()?))
}
fn execute(root:&Path,script:&str,packet:Vec<u8>,cancel:&AtomicBool,timeout:Duration)->io::Result<Vec<u8>>{
    if EXEC_BUSY.swap(true,Ordering::AcqRel){return Err(failure("worker already running"));}
    struct Release;impl Drop for Release{fn drop(&mut self){EXEC_BUSY.store(false,Ordering::Release);}}let _release=Release;
    let(child,mut input,output,errors)=launch(root,script)?;let(tx,rx)=mpsc::channel();
    let mut threads=vec![];let writer=tx.clone();threads.push(thread::spawn(move||{let result=input.write_all(&packet).map(|_|Vec::new());drop(input);let _=writer.send((0,result));}));
    for(i,file,max)in [(1,output,MAX+16400),(2,errors,8192)]{let sender=tx.clone();threads.push(thread::spawn(move||{let _=sender.send((i,bounded(file,max)));}));}drop(tx);
    let deadline=Instant::now()+timeout;let mut status=0;let mut aborted=None;
    let mut results:Vec<Option<io::Result<Vec<u8>>>>=(0..3).map(|_|None).collect();
    loop{
        while let Ok((i,result))=rx.try_recv(){if result.is_err(){aborted=Some("worker pipe failure");}results[i]=Some(result);}
        if cancel.load(Ordering::Acquire){aborted=Some("worker cancelled");}
        if Instant::now()>=deadline{aborted=Some("worker timeout");}
        if aborted.is_some(){unsafe{TerminateJobObject(child.job.0,125);}break;}
        let wait=unsafe{WaitForSingleObject(child.process.0,10)};
        if wait==WAIT_OBJECT_0{bool_result(unsafe{GetExitCodeProcess(child.process.0,&mut status)})?;break;}
        if wait==WAIT_FAILED{aborted=Some("worker wait failed");break;}
    }
    if unsafe{WaitForSingleObject(child.process.0,2000)}!=WAIT_OBJECT_0{
        LOST.store(true,Ordering::Release);for thread in &threads{unsafe{CancelSynchronousIo(thread.as_raw_handle());}}return Err(failure("worker termination unresolved"));
    }
    let end=Instant::now()+Duration::from_secs(2);
    while results.iter().any(Option::is_none){match rx.recv_timeout(end.saturating_duration_since(Instant::now())){Ok((i,result))=>results[i]=Some(result),Err(_)=>{LOST.store(true,Ordering::Release);for thread in &threads{unsafe{CancelSynchronousIo(thread.as_raw_handle());}}return Err(failure("worker pipes did not close"));}}}
    if let Err(error)=execution_outcome(status,aborted,results.iter().any(|result|matches!(result,Some(Err(_))))){
        let stderr=results[2].as_ref().and_then(|result|result.as_ref().ok()).map(Vec::as_slice).unwrap_or(&[]);
        return Err(with_stderr_category(error,stderr));
    }
    // Diagnostics are bounded but untrusted; never expose/log their raw bytes.
    results[0].take().unwrap()?;results[2].take().unwrap()?;results[1].take().unwrap()
}
#[derive(Serialize)]#[serde(rename_all="camelCase")]struct Request{schema:u32,task_id:String,source_hash:String,candidate_hash:String}
pub(crate) struct PreparedPdf{pub bytes:Vec<u8>,pub source_hash:String,pub candidate_hash:String,pub xref_self_entries:bool}
#[cfg(debug_assertions)]pub(crate) fn prepare(source:&[u8],candidate:&[u8],cancel:&AtomicBool)->io::Result<PreparedPdf>{
    prepare_runtime(&prototype(),source,candidate,cancel)
}
pub(crate) fn prepare_runtime(root:&Path,source:&[u8],candidate:&[u8],cancel:&AtomicBool)->io::Result<PreparedPdf>{
    if cancel.load(Ordering::Acquire){return Err(failure("worker cancelled"));}
    if source.len()<8||source.len()>MAX||candidate.len()<8||candidate.len()>MAX||!candidate.starts_with(source){return Err(failure("writer source/candidate bytes"));}
    let _pins=pin_runtime(root).map_err(|_|failure("worker runtime verification failed"))?;
    let request=Request{schema:1,task_id:uuid::Uuid::new_v4().to_string(),source_hash:crate::save_core::hash(source),candidate_hash:crate::save_core::hash(candidate)};let header=serde_json::to_vec(&request)?;
    let mut packet=MAGIC.to_vec();for size in [header.len(),source.len(),candidate.len()]{packet.extend_from_slice(&(size as u32).to_le_bytes());}packet.extend_from_slice(&header);packet.extend_from_slice(source);packet.extend_from_slice(candidate);
    let result=execute(root,"worker/main.py",packet,cancel,Duration::from_secs(30))?;
    if result.len()<16||&result[..8]!=MAGIC{return Err(failure("worker result protocol"));}
    let h=u32::from_le_bytes(result[8..12].try_into().unwrap())as usize;let n=u32::from_le_bytes(result[12..16].try_into().unwrap())as usize;
    if h>16384||n>MAX||result.len()!=16+h+n{return Err(failure("worker result lengths"));}
    let response:serde_json::Value=serde_json::from_slice(&result[16..16+h])?;let bytes=result[16+h..].to_vec();
    let end=bytes.iter().rposition(|c|!c.is_ascii_whitespace()).map(|i|i+1).unwrap_or(0);
    if response["ok"]!=true||response["schema"]!=1||response["taskId"]!=request.task_id||response["sourceHash"]!=request.source_hash||response["candidateHash"]!=request.candidate_hash||response["outputHash"]!=crate::save_core::hash(&bytes)||response["bytes"].as_u64()!=Some(n as u64)||!bytes.starts_with(candidate)||!bytes[..end].ends_with(b"%%EOF"){return Err(failure("worker output identity"));}
    Ok(PreparedPdf{bytes,source_hash:request.source_hash,candidate_hash:request.candidate_hash,xref_self_entries:response["standards"]["xrefSelfEntries"]==true})
}
// The private sentinel is cleaned through its original object, never by a
// second path lookup. Acquire DELETE only after the permission probe: holding
// DELETE while Python opens it without SHARE_DELETE would falsely appear to
// prove an ACL denial because of sharing incompatibility instead.
struct PrivateProbe{file:File,disposed:bool}
impl PrivateProbe{
    fn create(path:&Path)->io::Result<Self>{
        let me=crate::storage::directories::current_sid().map_err(|_|failure("probe identity"))?;
        let sddl=wide(std::ffi::OsStr::new(&format!("D:P(A;;FA;;;{me})(A;;FA;;;SY)(A;;FA;;;BA)")));let mut sd=std::ptr::null_mut();
        bool_result(unsafe{ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(),1,&mut sd,std::ptr::null_mut())})?;
        let sa=SECURITY_ATTRIBUTES{nLength:std::mem::size_of::<SECURITY_ATTRIBUTES>()as u32,lpSecurityDescriptor:sd,bInheritHandle:0};
        let raw=unsafe{windows_sys::Win32::Storage::FileSystem::CreateFileW(wide(path.as_os_str()).as_ptr(),0xC0000000,1|4,&sa,1,0x00200000,std::ptr::null_mut())};
        let error=if raw==INVALID_HANDLE_VALUE{Some(io::Error::last_os_error())}else{None};unsafe{LocalFree(sd.cast());}
        if let Some(error)=error{return Err(error);}Ok(Self{file:unsafe{File::from_raw_handle(raw)},disposed:false})
    }
    fn cleanup(&mut self)->io::Result<()>{
        if self.disposed{return Ok(());}
        use windows_sys::Win32::Storage::FileSystem::{ReOpenFile,SetFileInformationByHandle,FileDispositionInfo,FILE_DISPOSITION_INFO};
        let raw=unsafe{ReOpenFile(self.file.as_raw_handle(),0x00010000,1|2|4,0x00200000)};
        if raw==INVALID_HANDLE_VALUE{return Err(io::Error::last_os_error());}
        let delete=Handle(raw);let info=FILE_DISPOSITION_INFO{DeleteFile:true};
        bool_result(unsafe{SetFileInformationByHandle(delete.0,FileDispositionInfo,(&info as *const FILE_DISPOSITION_INFO).cast(),std::mem::size_of_val(&info)as u32)})?;
        self.disposed=true;Ok(())
    }
}
impl Drop for PrivateProbe{fn drop(&mut self){if !self.disposed{let _=self.cleanup();}}}
fn permissions_response(response:&serde_json::Value)->io::Result<()>{
    let checks=[("privateReadDenied","probe-private-read"),("runtimeWriteDenied","probe-runtime-write"),("networkDenied","probe-network"),("childProcessDenied","probe-child")];
    if response.as_object().is_none_or(|v|v.len()!=5)||response["schema"]!=1||checks.iter().any(|(key,_)|!response[*key].is_boolean()){
        return Err(probe_error_at("probe-permissions",failure("worker probe protocol")));
    }
    for (key,stage) in checks{if response[key]!=true{return Err(probe_error_at(stage,failure("worker isolation probe refused")));}}
    Ok(())
}
fn expected_probe_failure(stage:&'static str,result:io::Result<Vec<u8>>,expected:&str,missing:&str)->io::Result<()>{
    match result{Err(error)if error.to_string()==expected=>Ok(()),Err(error)=>Err(probe_error_at(stage,error)),Ok(_)=>Err(probe_error_at(stage,failure(missing)))}
}
/// Actual LPAC-only permission probe. A failed/missing profile never falls back.
/// No URL/target comes from the renderer, and the network probe is loopback only.
pub(crate) fn isolation_check(root:&Path,cancel:&AtomicBool)->io::Result<()>{
    if cancel.load(Ordering::Acquire){return Err(failure("worker cancelled"));}
    let _pins=probe_step("probe-resources",pin_runtime(root))?;
    // Full control matrix is mandatory before each export (pdf_export::prepare
    // calls isolation_check), not just in the optional diagnostics menu. Both
    // children use the same checked profile/SID/runtime and neither resumes.
    for lpac in [false,true]{
        if cancel.load(Ordering::Acquire){return Err(failure("worker cancelled"));}
        let stage=if lpac{"probe-lpac-control"}else{"probe-appcontainer-control"};
        let control=probe_step(stage,launch_policy(root,"worker/isolation.py",false,true,false,lpac))?;
        drop(control);
        if LOST.load(Ordering::Acquire){return Err(probe_error_at(stage,failure("worker termination unresolved")));}
    }
    let name=format!("worker-probe-{}.txt",uuid::Uuid::new_v4());let target=root.join(&name);
    let mut proof=probe_step("probe-sentinel",PrivateProbe::create(&target))?;
    probe_step("probe-sentinel",proof.file.write_all(b"synthetic-private-sentinel"))?;probe_step("probe-sentinel",proof.file.sync_all())?;
    let packet=serde_json::to_vec(&serde_json::json!({"schema":1,"privatePath":target,"operation":"permissions"}))?;
    let result=probe_step("probe-permissions",execute(root,"worker/isolation.py",packet,cancel,Duration::from_secs(10)));
    // A renamed sentinel is still the same object; a replacement at its former
    // name is never touched. Cleanup failure remains a failed isolation check.
    probe_step("probe-cleanup",proof.cleanup())?;drop(proof);let data=result?;
    let response:serde_json::Value=probe_step("probe-permissions",serde_json::from_slice(&data).map_err(|_|failure("worker probe protocol")))?;
    permissions_response(&response)?;
    let probe=|operation:&str|serde_json::to_vec(&serde_json::json!({"schema":1,"privatePath":target,"operation":operation})).map_err(io::Error::other);
    let memory:serde_json::Value=probe_step("probe-memory",serde_json::from_slice(&probe_step("probe-memory",execute(root,"worker/isolation.py",probe("memory")?,cancel,Duration::from_secs(10)))?).map_err(|_|failure("worker probe protocol")))?;
    if memory["schema"]!=1||memory["memoryLimited"]!=true||memory["allocatedBytes"].as_u64().is_none_or(|n|!(64*1024*1024..384*1024*1024).contains(&n)){return Err(probe_error_at("probe-memory",failure("worker memory probe refused")));}
    let timeout=execute(root,"worker/isolation.py",probe("spin")?,cancel,Duration::from_millis(150));
    expected_probe_failure("probe-timeout",timeout,"worker timeout","worker timeout probe refused")?;
    if cancel.load(Ordering::Acquire){return Err(failure("worker cancelled"));}
    let own_cancel=std::sync::Arc::new(AtomicBool::new(false));let signal=own_cancel.clone();
    let timer=thread::spawn(move||{thread::sleep(Duration::from_millis(150));signal.store(true,Ordering::Release);});
    let stopped=execute(root,"worker/isolation.py",probe("spin")?,&own_cancel,Duration::from_secs(5));let _=timer.join();
    expected_probe_failure("probe-cancel",stopped,"worker cancelled","worker cancellation probe refused")?;
    let excessive=execute(root,"worker/isolation.py",probe("output-budget")?,cancel,Duration::from_secs(10));
    expected_probe_failure("probe-output",excessive,"worker pipe failure","worker output-budget probe refused")?;
    if cancel.load(Ordering::Acquire){return Err(failure("worker cancelled"));}
    Ok(())
}
#[cfg(test)]mod tests{
    use super::*;
    #[test]fn fixed_task_arguments_match_the_native_entry_exactly(){
        let root=Path::new(r"D:\Synthetic 中文 space\unicode-worker");
        for (task,leaf) in [("worker/main.py","main.py"),("worker/isolation.py","isolation.py")]{
            let actual=fixed_task_path(root,task).unwrap();
            let expected=format!(r"{}\worker\{}",root.display(),leaf);
            // Compare text, just as CompareStringOrdinal does in the native
            // entry; Path equality would hide the mixed-separator defect.
            assert_eq!(actual.as_os_str(),std::ffi::OsStr::new(&expected));
        }
    }
    #[test]fn fixed_task_arguments_reject_aliases_and_arbitrary_scripts(){
        let root=Path::new(r"D:\Synthetic 中文 space\unicode-worker");
        for task in ["main.py",r"worker\main.py","worker/../main.py","worker/main.py/extra","worker/main.py\0","worker/main.py -c x",r"C:\foreign\main.py","worker/MAIN.py"]{
            assert_eq!(fixed_task_path(root,task).unwrap_err().to_string(),"worker task not approved");
        }
    }
    #[test]
    #[ignore = "Explicit CLI integration only; requires an already authorized LPAC environment. Never provisions profiles or changes ACLs."]
    fn restricted_worker_cli_fixed_pdf(){
        let _guard=test_process_guard();
        let project=crate::save_core::project();
        let fixtures=project.join("tests/work/writer/xref-fixed");
        let source=fs::read(fixtures.join("matrix-source.pdf")).unwrap();
        let candidate=fs::read(fixtures.join("matrix-candidate.pdf")).unwrap();
        assert_eq!(crate::save_core::hash(&source),"a8a9d4ac31dbcd6d11238dc5d07918da48f826d545f24a3c4e0fdca370d334a9");
        assert_eq!(crate::save_core::hash(&candidate),"b5da68da0a0ca6d988ffcc49912bb558ce3596f90c573a7ab82f379cb0877edc");
        let work=project.join("tests/work/writer").join(format!("restricted-cli-{}",uuid::Uuid::new_v4()));
        fs::create_dir(&work).unwrap();
        let root=prototype();let cancel=AtomicBool::new(false);
        // Run the genuine launch/token/permission gates before submitting PDF
        // bytes. No grant_prototype, provision, ordinary-token Worker or mocks.
        // An absent OS profile is recorded as failure, never a passing skip.
        let mut stage="existing-profile";
        let mut configuration=serde_json::Value::Null;
        let result=(||->io::Result<PreparedPdf>{
            let state=crate::worker_sandbox::status(&root)?;
            configuration=serde_json::json!({"profileRegistered":state.profile_registered,"runtimeAuthorized":state.runtime_authorized,"state":state.state});
            crate::worker_sandbox::verify(&root)?;
            stage="isolation";
            isolation_check(&root,&cancel)?;
            stage="conversion";
            let output=prepare_runtime(&root,&source,&candidate,&cancel)?;
            if !output.xref_self_entries{return Err(failure("worker xref gate failed"));}
            Ok(output)
        })();
        let evidence=match result{
            Ok(output)=>{
                let path=work.join("unicode-output.pdf");
                let mut file=OpenOptions::new().write(true).create_new(true).open(&path).unwrap();
                file.write_all(&output.bytes).unwrap();file.sync_all().unwrap();drop(file);
                assert_eq!(fs::read(path).unwrap(),output.bytes);
                serde_json::json!({"status":"PASS","sourceHash":output.source_hash,"candidateHash":output.candidate_hash,"outputHash":crate::save_core::hash(&output.bytes),"bytes":output.bytes.len(),"xrefSelfEntries":true,"scope":"real LPAC and production byte protocol; independent render and GUI still required"})
            },
            Err(error)=>{
                let detail=probe_details(&error);
                serde_json::json!({"status":"BLOCKED","gate":stage,"stage":detail.stage,"reason":detail.reason,"windowsError":detail.windows_error,"workerExit":detail.worker_exit,"pdfGenerated":false})
            }
        };
        let mut evidence=evidence;
        evidence["configuration"]=configuration;
        evidence["profileCreated"]=false.into();evidence["aclChanged"]=false.into();evidence["ordinaryWorkerFallback"]=false.into();
        fs::write(work.join("result.json"),serde_json::to_vec_pretty(&evidence).unwrap()).unwrap();
        println!("{evidence}");
        assert_eq!(fs::read(fixtures.join("matrix-source.pdf")).unwrap(),source);
        assert_eq!(fs::read(fixtures.join("matrix-candidate.pdf")).unwrap(),candidate);
        assert_eq!(evidence["status"],"PASS","Real restricted Worker gate failed; output was not accepted");
    }
    #[test]fn probe_native_entry_diagnostics_keep_only_fixed_stages_and_separate_error_kinds(){
        let error=with_stderr_category(execution_outcome(125,None,false).unwrap_err(),b"AUPW_ENTRY_V1 entry-start 0\nAUPW_ENTRY_V1 entry-crt-primary 5\r\n");
        let detail=probe_details(&error);assert_eq!(detail.stage,"entry-crt-primary");assert_eq!(detail.windows_error,Some(5));assert_eq!(detail.worker_exit,Some(125));assert_eq!(detail.reason,"access-denied");
        let error=with_stderr_category(execution_outcome(0xc0000022,None,false).unwrap_err(),b"AUPW_ENTRY_V1 entry-python-load 0\n");
        let detail=probe_details(&error);assert_eq!(detail.stage,"entry-python-load");assert!(detail.windows_error.is_none());assert_eq!(detail.worker_exit,Some(0xc0000022));
        let error=with_stderr_category(execution_outcome(125,None,false).unwrap_err(),b"AUPW_ENTRY_V1 entry-python-probe-read 5\r\n");
        let detail=probe_details(&error);assert_eq!(detail.stage,"entry-python-probe-read");assert_eq!(detail.windows_error,Some(5));assert_eq!(detail.worker_exit,Some(125));assert_eq!(detail.reason,"access-denied");
        let error=with_stderr_category(execution_outcome(125,None,false).unwrap_err(),b"AUPW_ENTRY_V1 entry-python-probe-read 0\nAUPW_ENTRY_V1 entry-python-probe-map 5\n");
        let detail=probe_details(&error);assert_eq!(detail.stage,"entry-python-probe-map");assert_eq!(detail.windows_error,Some(5));assert_eq!(detail.worker_exit,Some(125));assert_eq!(detail.reason,"access-denied");
        let error=with_stderr_category(execution_outcome(125,None,false).unwrap_err(),b"AUPW_ENTRY_V1 entry-python-probe-read 0\nAUPW_ENTRY_V1 entry-python-probe-map 0\nAUPW_ENTRY_V1 entry-python-load 5\n");
        let detail=probe_details(&error);assert_eq!(detail.stage,"entry-python-load");assert_eq!(detail.windows_error,Some(5));assert_eq!(detail.worker_exit,Some(125));assert_eq!(detail.reason,"access-denied");
        let error=with_stderr_category(execution_outcome(2,None,false).unwrap_err(),b"AUPW_ENTRY_V1 entry-python-return 2\n");
        assert!(probe_details(&error).windows_error.is_none());
        for line in [b"AUPW_ENTRY_V1 private-path 5".as_slice(),b"AUPW_ENTRY_V1 entry-python-load 5 PRIVATE",b"AUPW_ENTRY_V1 entry-python-probe-map 5 PRIVATE",b"AUPW_ENTRY_V1 entry-python-load -1",b"AUPW_ENTRY_V1 entry-python-load +5",b"AUPW_ENTRY_V1 entry-python-load 4294967296",b"AUPW_ENTRY_V1 entry-python-load 5\0"]{assert!(entry_failure_line(line).is_none());}
        let timeout=with_stderr_category(execution_outcome(125,Some("worker timeout"),false).unwrap_err(),b"AUPW_ENTRY_V1 entry-python-load 5");
        assert_eq!(timeout.to_string(),"worker timeout");assert_eq!(probe_details(&timeout).reason,"timeout");
        let error=with_stderr_category(execution_outcome(1,None,false).unwrap_err(),b"AUPW_ENTRY_V1 entry-python-main 0\nImportError: PRIVATE_PATH\nAUPW_ENTRY_V1 entry-python-return 1\n");
        assert_eq!(probe_details(&error).reason,"python-import");
        assert_eq!(probe_details(&error).stage,"entry-python-return");
    }
    // Ephemeral suspended test executables only: no profile API, file grants,
    // registry operation, Worker execution or resumed user code.
    fn identity_control(lpac:bool,package:PSID)->io::Result<Process>{
        let mut size=0;unsafe{InitializeProcThreadAttributeList(std::ptr::null_mut(),2,0,&mut size);}
        let mut buffer=vec![0usize;size.div_ceil(std::mem::size_of::<usize>())];let ptr=buffer.as_mut_ptr().cast();
        bool_result(unsafe{InitializeProcThreadAttributeList(ptr,2,0,&mut size)})?;let attributes=Attributes{_buffer:buffer,ptr};
        let capabilities=SECURITY_CAPABILITIES{AppContainerSid:package,Capabilities:std::ptr::null_mut(),CapabilityCount:0,Reserved:0};let mut policy=u32::from(lpac);
        bool_result(unsafe{UpdateProcThreadAttribute(ptr,0,PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES as usize,(&capabilities as *const SECURITY_CAPABILITIES).cast(),std::mem::size_of_val(&capabilities),std::ptr::null_mut(),std::ptr::null())})?;
        bool_result(unsafe{UpdateProcThreadAttribute(ptr,0,PROC_THREAD_ATTRIBUTE_ALL_APPLICATION_PACKAGES_POLICY as usize,(&mut policy as *mut u32).cast(),4,std::ptr::null_mut(),std::ptr::null())})?;
        let exe=std::env::current_exe()?;let app=wide(exe.as_os_str());let mut command=wide(std::ffi::OsStr::new(&format!("\"{}\" --list",exe.display())));
        let system=std::env::var_os("SystemRoot").ok_or_else(||failure("test system root missing"))?;
        let env=worker_environment(&system,&crate::worker_sandbox::local_app_data()?,exe.parent().unwrap())?;
        let mut info=STARTUPINFOEXW::default();info.StartupInfo.cb=std::mem::size_of_val(&info)as u32;info.lpAttributeList=attributes.ptr;
        let job=Handle(unsafe{CreateJobObjectW(std::ptr::null(),std::ptr::null())});if job.0.is_null(){return Err(io::Error::last_os_error());}
        let mut pi=PROCESS_INFORMATION::default();
        bool_result(unsafe{CreateProcessW(app.as_ptr(),command.as_mut_ptr(),std::ptr::null(),std::ptr::null(),0,CREATE_SUSPENDED|CREATE_NO_WINDOW|CREATE_UNICODE_ENVIRONMENT|EXTENDED_STARTUPINFO_PRESENT,env.as_ptr().cast(),std::ptr::null(),&info.StartupInfo,&mut pi)})?;
        let process=Process{process:Handle(pi.hProcess),_thread:Handle(pi.hThread),job};
        bool_result(unsafe{AssignProcessToJobObject(process.job.0,process.process.0)})?;
        Ok(process)
    }
    #[test]
    #[ignore = "Blocked: unregistered test SID cannot launch on this host (Windows 2); no host profile creation permitted, see ACCEPTANCE4.48"]
    fn lpac_access_controls_distinguish_actual_tokens_without_creating_profiles(){
        let _guard=test_process_guard();
        let name=wide(std::ffi::OsStr::new(&format!("ainativepdf-test.{}",uuid::Uuid::new_v4())));let mut raw=std::ptr::null_mut();
        assert!(unsafe{DeriveAppContainerSidFromAppContainerName(name.as_ptr(),&mut raw)}>=0);let package=Sid(raw);
        let own=crate::storage::directories::sid_text(package.0).unwrap();let user=crate::storage::directories::current_sid().unwrap();
        let mut parent=std::ptr::null_mut();bool_result(unsafe{OpenProcessToken(GetCurrentProcess(),TOKEN_QUERY|TOKEN_DUPLICATE,&mut parent)}).unwrap();let parent=Handle(parent);
        assert!(token_access_probe(parent.0,&user,Some(&own)).unwrap());
        assert!(token_access_probe(parent.0,&user,Some("S-1-15-2-1")).unwrap());
        for lpac in [false,true]{
            let process=identity_control(lpac,package.0).unwrap();
            let mut raw=std::ptr::null_mut();bool_result(unsafe{OpenProcessToken(process.process.0,TOKEN_QUERY|TOKEN_DUPLICATE,&mut raw)}).unwrap();let token=Handle(raw);
            let(mut app,mut bytes)=(0u32,0u32);bool_result(unsafe{GetTokenInformation(token.0,TokenIsAppContainer,(&mut app as *mut u32).cast(),4,&mut bytes)}).unwrap();assert_eq!(app,1);
            assert!(token_access_probe(token.0,&user,Some(&own)).unwrap());
            assert_eq!(token_access_probe(token.0,&user,Some("S-1-15-2-1")).unwrap(),!lpac);
            assert!(!token_access_probe(token.0,&user,None).unwrap());
            assert!(!token_access_probe(token.0,&user,Some("S-1-15-2-424242")).unwrap());
            println!("suspended-control lpac={lpac}: own-package=allow all-packages={} user-only=deny unrelated-package=deny; no profile created, no child resumed",if lpac{"deny"}else{"allow"});
            drop(token);drop(process);assert!(!LOST.load(Ordering::Acquire));
        }
    }
    #[test]fn access_identity_requires_the_complete_matrix_and_rejects_every_other_combination(){
        for lpac in [false,true]{for bits in 0..16{
            let(own,all,user,other)=(bits&1!=0,bits&2!=0,bits&4!=0,bits&8!=0);
            assert_eq!(access_matrix(own,all,user,other,lpac).is_ok(),own&&(all!=lpac)&&!user&&!other);
        }}
        assert!(access_matrix(true,true,false,false,true).is_err()); // ordinary AC
        assert!(access_matrix(true,true,true,true,true).is_err()); // ordinary user
        assert!(access_matrix(false,false,false,false,true).is_err()); // broken positive control
    }
    #[test]fn access_identity_real_ordinary_process_is_rejected_and_api_errors_stay_errors(){
        let mut raw=std::ptr::null_mut();bool_result(unsafe{OpenProcessToken(GetCurrentProcess(),TOKEN_QUERY|TOKEN_DUPLICATE,&mut raw)}).unwrap();let token=Handle(raw);
        let user=crate::storage::directories::current_sid().unwrap();let package=sid().unwrap();
        let own=crate::storage::directories::sid_text(package.0).unwrap();
        assert!(token_access_probe(token.0,&user,Some(&own)).unwrap());
        assert!(token_access_probe(token.0,&user,Some("S-1-15-2-1")).unwrap());
        assert!(token_access_probe(token.0,&user,None).unwrap());
        assert!(verify_access_identity(token.0,package.0,true).is_err());
        assert!(token_access_probe(INVALID_HANDLE_VALUE,&user,Some(&own)).is_err());
        assert!(token_access_probe(token.0,"NOT_A_SID",Some(&own)).is_err());
    }
    #[test]fn access_identity_ordinary_control_can_never_execute_or_inherit_input(){
        let root=Path::new(r"Z:\must-not-be-accessed");
        for(inherit,resume)in [(false,true),(true,false),(true,true)]{
            let result=launch_policy(root,"must-not-run.py",inherit,true,resume,false);
            assert_eq!(result.err().unwrap().to_string(),"ordinary AppContainer execution forbidden");
        }
    }
    #[test]fn worker_environment_has_only_four_sorted_variables_and_exact_unicode_termination(){
        let local=Path::new(r"C:\Synthetic User\本地数据");let root=Path::new(r"D:\Writer Test\工具");
        let block=worker_environment(std::ffi::OsStr::new(r"C:\Windows"),local,root).unwrap();
        let expected="LOCALAPPDATA=C:\\Synthetic User\\本地数据\0SYSTEMROOT=C:\\Windows\0TEMP=D:\\Writer Test\\工具\0TMP=D:\\Writer Test\\工具\0\0";
        assert_eq!(block,expected.encode_utf16().collect::<Vec<_>>());
        // No PATH, proxy, Python injection variables, credentials or parent
        // environment enumeration can enter this fixed block.
        assert_eq!(block.iter().filter(|c|**c==0).count(),5);
    }
    #[test]fn worker_environment_rejects_empty_injected_or_oversized_values(){
        let root=Path::new(r"D:\Synthetic");
        for invalid in [std::ffi::OsString::new(),std::ffi::OsString::from_wide(&[67,0,88])]{
            assert!(worker_environment(&invalid,root,root).is_err());
            assert!(worker_environment(std::ffi::OsStr::new(r"C:\Windows"),Path::new(&invalid),root).is_err());
        }
        let large=std::ffi::OsString::from("x".repeat(32768));
        assert!(worker_environment(&large,root,root).is_err());
    }
    #[test]fn worker_environment_uses_os_known_folder_without_printing_or_changing_it(){
        let local=crate::worker_sandbox::local_app_data().unwrap();assert!(local.is_absolute());
        let block=worker_environment(std::ffi::OsStr::new(r"C:\Windows"),&local,Path::new(r"D:\Synthetic")).unwrap();
        let mut expected="LOCALAPPDATA=".encode_utf16().collect::<Vec<_>>();expected.extend(local.as_os_str().encode_wide());expected.push(0);
        assert!(block.starts_with(&expected));
        let detail=probe_details(&probe_error_at("worker-start",io::Error::from_raw_os_error(203)));
        assert_eq!(detail.reason,"environment-missing");assert_eq!(detail.windows_error,Some(203));
    }
    #[test]fn probe_permission_failures_are_specific_and_remain_fail_closed(){
        let good=serde_json::json!({"schema":1,"privateReadDenied":true,"runtimeWriteDenied":true,"networkDenied":true,"childProcessDenied":true});
        assert!(permissions_response(&good).is_ok());
        for(key,stage)in [("privateReadDenied","probe-private-read"),("runtimeWriteDenied","probe-runtime-write"),("networkDenied","probe-network"),("childProcessDenied","probe-child")]{
            let mut bad=good.clone();bad[key]=false.into();let error=permissions_response(&bad).unwrap_err();let detail=probe_details(&error);
            assert_eq!(detail.stage,stage);assert_eq!(detail.reason,"denial-unproven");
        }
        for bad in [serde_json::json!({}),serde_json::json!({"schema":1,"privateReadDenied":"true","runtimeWriteDenied":true,"networkDenied":true,"childProcessDenied":true}),{
            let mut extra=good.clone();extra["untrusted"]="FAKE_PRIVATE".into();extra
        }]{assert_eq!(probe_details(&permissions_response(&bad).unwrap_err()).reason,"protocol-invalid");}
    }
    #[test]fn probe_details_preserve_first_native_failure_without_exposing_source_messages(){
        let error=probe_step::<()>("probe-permissions",Err(probe_error_at("worker-job-assignment",io::Error::from_raw_os_error(5)))).unwrap_err();
        let detail=probe_details(&error);assert_eq!(detail.stage,"worker-job-assignment");assert_eq!(detail.windows_error,Some(5));
        assert_eq!(detail.reason,"access-denied");assert!(detail.worker_exit.is_none());
        let exit=execution_outcome(0xc0000022,None,false).unwrap_err();let detail=probe_details(&exit);
        assert_eq!(detail.worker_exit,Some(0xc0000022));assert!(detail.windows_error.is_none());
        let exit=probe_error_at("probe-memory",execution_outcome(1,None,false).unwrap_err());
        assert_eq!(probe_details(&exit).stage,"probe-memory");assert_eq!(probe_details(&exit).worker_exit,Some(1));
        let public=serde_json::to_string(&crate::worker_sandbox::probe_error(probe_error_at("probe-memory",failure("FAKE_PRIVATE C:\\synthetic\\data")))).unwrap();
        assert!(public.contains("probe-memory"));assert!(!public.contains("FAKE_PRIVATE"));assert!(!public.contains("synthetic"));
        let startup=serde_json::to_value(crate::worker_sandbox::probe_error(probe_error_at("worker-start",failure("worker CreateProcess failed (Windows 2)")))).unwrap();
        assert_eq!(startup["code"],"writer_startup_2");assert_eq!(startup["windowsError"],2);
    }
    #[test]fn probe_stderr_can_only_supply_a_fixed_failure_family_never_private_text_or_a_pass(){
        for(line,reason)in [("ModuleNotFoundError: FAKE_PRIVATE C:\\synthetic\\module","python-module"),("ImportError: FAKE_PRIVATE","python-import"),("PermissionError: FAKE_PRIVATE","python-access"),("FileNotFoundError: FAKE_PRIVATE","python-file"),("MemoryError: FAKE_PRIVATE","python-memory"),("arbitrary FAKE_PRIVATE","child-exited")]{
            let error=with_stderr_category(execution_outcome(1,None,false).unwrap_err(),line.as_bytes());
            assert_eq!(probe_details(&error).reason,reason);assert_eq!(error.to_string(),"worker refused or exited");
            let public=serde_json::to_string(&crate::worker_sandbox::probe_error(error)).unwrap();
            assert!(!public.contains("FAKE_PRIVATE"));assert!(!public.contains("synthetic"));
        }
        let error=with_stderr_category(failure("worker timeout"),b"ModuleNotFoundError: FAKE_PRIVATE");
        assert_eq!(error.to_string(),"worker timeout");assert_eq!(probe_details(&error).reason,"timeout");
    }
    #[test]fn probe_limits_require_the_original_expected_error_not_an_arbitrary_failure(){
        for(stage,expected,missing)in [("probe-timeout","worker timeout","worker timeout probe refused"),("probe-cancel","worker cancelled","worker cancellation probe refused"),("probe-output","worker pipe failure","worker output-budget probe refused")]{
            assert!(expected_probe_failure(stage,Err(failure(expected)),expected,missing).is_ok());
            let error=expected_probe_failure(stage,Ok(vec![]),expected,missing).unwrap_err();assert_eq!(probe_details(&error).reason,"limit-unproven");
            let error=expected_probe_failure(stage,Err(failure("different failure")),expected,missing).unwrap_err();assert_eq!(probe_details(&error).stage,stage);
            let error=expected_probe_failure(stage,Err(probe_error_at("worker-token",failure("worker unexpected capability"))),expected,missing).unwrap_err();
            assert_eq!(probe_details(&error).stage,"worker-token");assert_eq!(probe_details(&error).reason,"token-invalid");
        }
    }
    #[test]fn installed_resource_locator_matches_the_native_authority_without_canonical_aliases(){
        let installed=installed_runtime().unwrap();
        let executable=std::env::current_exe().unwrap();
        assert_eq!(installed,executable.parent().unwrap().join("unicode-worker"));
        let tauri_executable=tauri::utils::platform::current_exe().unwrap();
        assert_eq!(tauri_executable,executable.canonicalize().unwrap());
        let former=tauri_executable.parent().unwrap().join("unicode-worker");
        // These are lexically different on an ordinary Windows launch. The
        // production broker now passes installed_runtime(), just like its guard.
        assert_ne!(Path::new(r"C:\Synthetic\unicode-worker"),Path::new(r"\\?\C:\Synthetic\unicode-worker"));
        if former!=installed{assert_eq!(pin_runtime(&former).err().unwrap().to_string(),"worker runtime not approved");}
        assert!(installed.ends_with("unicode-worker"));
    }
    #[test]fn late_pipe_failure_has_the_same_result_as_an_early_budget_failure(){
        assert_eq!(execution_outcome(1,None,true).unwrap_err().to_string(),"worker pipe failure");
        assert_eq!(execution_outcome(0,None,true).unwrap_err().to_string(),"worker pipe failure");
        assert_eq!(execution_outcome(1,Some("worker pipe failure"),true).unwrap_err().to_string(),"worker pipe failure");
        assert_eq!(execution_outcome(1,Some("worker cancelled"),true).unwrap_err().to_string(),"worker cancelled");
        assert_eq!(execution_outcome(1,Some("worker timeout"),true).unwrap_err().to_string(),"worker timeout");
        assert_eq!(execution_outcome(1,None,false).unwrap_err().to_string(),"worker refused or exited");
        assert!(execution_outcome(0,None,false).is_ok());
    }
    #[test]fn read_attribute_directory_guard_also_prevents_name_replacement(){
        let root=crate::save_core::project().join("tests/work/writer").join(format!("directory-pin-{}",uuid::Uuid::new_v4()));fs::create_dir(&root).unwrap();
        let directory=root.join("parent");let moved=root.join("moved");fs::create_dir(&directory).unwrap();let mut pins=vec![];pin_chain(&directory,&mut pins).unwrap();
        assert_eq!(fs::rename(&directory,&moved).err().unwrap().raw_os_error(),Some(32));drop(pins);fs::rename(&directory,&moved).unwrap();
    }
    #[test]fn private_probe_cleanup_uses_object_identity_after_name_substitution(){
        let root=crate::save_core::project().join("tests/work/writer").join(format!("probe-cleanup-{}",uuid::Uuid::new_v4()));fs::create_dir(&root).unwrap();
        let original=root.join("sentinel.txt");let moved=root.join("renamed-sentinel.txt");let mut proof=PrivateProbe::create(&original).unwrap();proof.file.write_all(b"synthetic-sentinel").unwrap();
        // Match the ordinary Python read sharing mode; owner access remains
        // possible, so denial in the real LPAC probe cannot be a DELETE-share artifact.
        let mut reader=OpenOptions::new().read(true).share_mode(1|2).open(&original).unwrap();let mut bytes=vec![];reader.read_to_end(&mut bytes).unwrap();assert_eq!(bytes,b"synthetic-sentinel");drop(reader);
        fs::rename(&original,&moved).unwrap();fs::write(&original,b"synthetic-replacement-preserved").unwrap();
        proof.cleanup().unwrap();drop(proof);assert!(!moved.exists());assert_eq!(fs::read(original).unwrap(),b"synthetic-replacement-preserved");
    }
    #[test]fn private_probe_is_cleaned_on_early_error_without_path_deletion(){
        let root=crate::save_core::project().join("tests/work/writer").join(format!("probe-error-{}",uuid::Uuid::new_v4()));fs::create_dir(&root).unwrap();
        let path=root.join("sentinel.txt");let preserved=root.join("unrelated.txt");fs::write(&preserved,b"synthetic-unrelated").unwrap();
        let failed=(||->io::Result<()>{let mut proof=PrivateProbe::create(&path)?;proof.file.write_all(b"synthetic-sentinel")?;Err(failure("injected probe error"))})();
        assert!(failed.is_err());assert!(!path.exists());assert_eq!(fs::read(preserved).unwrap(),b"synthetic-unrelated");
    }
    #[test]fn startup_context_and_minimal_lpac_are_read_only_diagnostics(){
        let _test=test_process_guard();
        let root=prototype();let _pins=pin_runtime(&root).unwrap();
        let sid=sid().unwrap();let sid_string=crate::storage::directories::sid_text(sid.0).unwrap();let mut folder=std::ptr::null_mut();
        let hr=unsafe{GetAppContainerFolderPath(wide(std::ffi::OsStr::new(&sid_string)).as_ptr(),&mut folder)};
        let folder_exists=if hr>=0&&!folder.is_null(){let mut n=0;unsafe{while *folder.add(n)!=0{n+=1;}}let p=std::ffi::OsString::from_wide(unsafe{std::slice::from_raw_parts(folder,n)});let exists=Path::new(&p).is_dir();unsafe{CoTaskMemFree(folder.cast());}Some(exists)}else{None};
        let mut token=std::ptr::null_mut();bool_result(unsafe{OpenProcessToken(GetCurrentProcess(),TOKEN_QUERY,&mut token)}).unwrap();let token=Handle(token);
        let query=|class|{let(mut value,mut n)=(0u32,0u32);bool_result(unsafe{GetTokenInformation(token.0,class,(&mut value as *mut u32).cast(),4,&mut n)}).unwrap();value};
        let mut data=vec![0usize;128];let mut n=0;bool_result(unsafe{GetTokenInformation(token.0,TokenIntegrityLevel,data.as_mut_ptr().cast(),(data.len()*std::mem::size_of::<usize>())as u32,&mut n)}).unwrap();
        let integrity=unsafe{let sid=(*(data.as_ptr().cast::<TOKEN_MANDATORY_LABEL>())).Label.Sid;*GetSidSubAuthority(sid,(*GetSidSubAuthorityCount(sid)-1)as u32)};
        let mut variants=vec![];for (handles,cwd)in [(false,true),(false,false)]{
            let result=launch_options(&root,"worker/main.py",handles,cwd,false);
            variants.push(serde_json::json!({"inheritHandles":handles,"explicitCwd":cwd,"resumed":false,"outcome":match result{Ok(child)=>{drop(child);"created-and-terminated".to_owned()},Err(e)=>e.to_string()}}));
        }
        let evidence=serde_json::json!({"profileFolderHresult":format!("0x{:08X}",hr as u32),"profileFolderExists":folder_exists,"parentAppContainer":query(TokenIsAppContainer),"parentHasRestrictions":query(TokenHasRestrictions),"parentIntegrityRid":integrity,"variants":variants,"profileCreated":false,"fallback":false,"scope":"read-only token/profile query; same LPAC suspended launch only"});
        fs::write(crate::save_core::project().join("tests/work/writer/startup-context.json"),evidence.to_string()).unwrap();println!("{evidence}");
    }
    // `prepare` only exists in a debug build, so this lab check is debug-only.
    #[cfg(debug_assertions)]#[test]fn cancelled_and_oversized_inputs_stop_before_runtime_or_process_access(){
        assert_eq!(prepare(b"not a PDF",b"not a PDF",&AtomicBool::new(true)).err().unwrap().to_string(),"worker cancelled");
        assert_eq!(prepare(b"",b"",&AtomicBool::new(false)).err().unwrap().to_string(),"writer source/candidate bytes");
        let root=crate::save_core::project().join("tests/work/writer");let p=root.join(format!("bounded-{}.bin",uuid::Uuid::new_v4()));fs::write(&p,b"0123456789").unwrap();
        assert!(bounded(File::open(&p).unwrap(),3).is_err());assert_eq!(fs::read(p).unwrap(),b"0123456789");
    }
    #[test]fn failed_job_assignment_cleanup_terminates_the_suspended_child(){
        use std::os::windows::process::CommandExt;
        let mut child=std::process::Command::new(std::env::current_exe().unwrap()).args(["--list"]).creation_flags(CREATE_NO_WINDOW|CREATE_SUSPENDED).spawn().unwrap();
        let mut duplicate=std::ptr::null_mut();assert_ne!(unsafe{DuplicateHandle(GetCurrentProcess(),child.as_raw_handle(),GetCurrentProcess(),&mut duplicate,0,0,DUPLICATE_SAME_ACCESS)},0);
        let process=Process{process:Handle(duplicate),_thread:Handle(std::ptr::null_mut()),job:Handle(unsafe{CreateJobObjectW(std::ptr::null(),std::ptr::null())})};
        drop(process);assert_eq!(unsafe{WaitForSingleObject(child.as_raw_handle(),0)},WAIT_OBJECT_0);assert_eq!(child.wait().unwrap().code(),Some(125));
    }
    #[cfg(debug_assertions)]#[test]fn unsupported_lowbox_startup_fails_closed_without_user_process_fallback(){
        let _test=test_process_guard();
        let root=prototype();grant_prototype(&root).unwrap();let source=fs::read(crate::save_core::project().join("tests/work/writer/matrix-source.pdf")).unwrap();let candidate=fs::read(crate::save_core::project().join("tests/work/writer/matrix-candidate.pdf")).unwrap();
        let result=prepare(&source,&candidate,&AtomicBool::new(false));
        let evidence=match result{
            Ok(output)=>{assert_eq!(output.source_hash,crate::save_core::hash(&source));assert_eq!(output.candidate_hash,crate::save_core::hash(&candidate));fs::write(crate::save_core::project().join("tests/work/writer/lowbox.pdf"),output.bytes).unwrap();serde_json::json!({"isolationStartup":"Pass","scope":"startup conversion only; unauthorized file/network probes still required"})},
            Err(error)=>{assert_eq!(error.to_string(),"worker CreateProcess failed (Windows 2)");serde_json::json!({"isolationStartup":"Blocked","failure":error.to_string(),"ordinaryUserFallback":false,"profileCreated":false,"productionEnabled":false})},
        };
        fs::write(crate::save_core::project().join("tests/work/writer/isolation.json"),evidence.to_string()).unwrap();println!("{evidence}");
    }
}
