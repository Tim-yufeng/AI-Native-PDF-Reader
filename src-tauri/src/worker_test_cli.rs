//! Disposable Sandbox CLI broker. Reuses production setup and LPAC byte core.
//! The fixed path/account guard prevents accidental host use, not VM attestation.
//! Only the external official wsb session ID establishes the isolation boundary.
use std::{fs::{self,OpenOptions},io::{self,Write},os::windows::process::CommandExt,path::Path,sync::atomic::AtomicBool,time::{Duration,Instant}};
use windows_sys::Win32::{Foundation::CloseHandle,Security::{AllocateAndInitializeSid,CheckTokenMembership,EqualSid,FreeSid,GetTokenInformation,TokenGroups,TOKEN_GROUPS,TOKEN_QUERY,SID_IDENTIFIER_AUTHORITY},System::Threading::{GetCurrentProcess,OpenProcessToken}};
const LAB:&str=r"C:\Users\WriterCliUser\WriterCliLab";
const INPUT:&str=r"C:\Users\WriterCliUser\WriterCliLab\input";
const SOURCE:&str="a8a9d4ac31dbcd6d11238dc5d07918da48f826d545f24a3c4e0fdca370d334a9";
const CANDIDATE:&str="b5da68da0a0ca6d988ffcc49912bb558ce3596f90c573a7ab82f379cb0877edc";
fn refused()->io::Error{io::Error::other("guest test guard refused")}
fn guard()->io::Result<()>{
    if std::env::args().collect::<Vec<_>>()!=[std::env::args().next().ok_or_else(refused)?,"--isolated-writer-cli".into(),"run".into()]{return Err(refused());}
    if !std::env::current_exe()?.to_string_lossy().eq_ignore_ascii_case(&format!(r"{LAB}\runner.exe"))||std::env::var("USERNAME").ok().as_deref()!=Some("WriterCliUser"){return Err(refused());}
    let user=crate::storage::directories::current_sid().map_err(|_|refused())?;
    if !user.starts_with("S-1-5-21-"){return Err(refused());}
    let identity=fs::read(Path::new(LAB).join("identity.json"))?;
    if identity.len()>4096||serde_json::from_slice::<serde_json::Value>(&identity)?["userSid"]!=user{return Err(refused());}
    let mut admin=std::ptr::null_mut();let authority=SID_IDENTIFIER_AUTHORITY{Value:[0,0,0,0,0,5]};
    if unsafe{AllocateAndInitializeSid(&authority,2,32,544,0,0,0,0,0,0,&mut admin)}==0{return Err(io::Error::last_os_error());}
    let mut member=0;let ok=unsafe{CheckTokenMembership(std::ptr::null_mut(),admin,&mut member)};
    let mut token=std::ptr::null_mut();
    if ok==0||member!=0||unsafe{OpenProcessToken(GetCurrentProcess(),TOKEN_QUERY,&mut token)}==0{unsafe{FreeSid(admin);}return Err(refused());}
    let mut groups=vec![0usize;8192];let mut size=0;
    let ok=unsafe{GetTokenInformation(token,TokenGroups,groups.as_mut_ptr().cast(),(groups.len()*std::mem::size_of::<usize>())as u32,&mut size)};
    let present=if ok!=0{unsafe{let g=&*(groups.as_ptr().cast::<TOKEN_GROUPS>());std::slice::from_raw_parts(g.Groups.as_ptr(),g.GroupCount as usize).iter().any(|g|EqualSid(g.Sid,admin)!=0)}}else{true};
    unsafe{CloseHandle(token);FreeSid(admin);}if present{return Err(refused());}
    let bytes=fs::read(Path::new(INPUT).join("scope.json"))?;
    if bytes.len()>4096{return Err(refused());}
    let scope:serde_json::Value=serde_json::from_slice(&bytes)?;
    if scope["scope"]!="ai-native-pdf-reader/guest-cli/v1"||scope["authorizeGuestOnlySetup"]!=true{return Err(refused());}
    Ok(())
}
fn write_result(value:&serde_json::Value)->io::Result<()>{
    let path=Path::new(LAB).join("result.json");
    let mut file=OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;file.sync_all()
}
pub(crate) fn run()->i32{
    if guard().is_err(){return 70;}
    let mut stage=1;let mut detail=serde_json::Value::Null;
    let result=(||->io::Result<serde_json::Value>{
        let root=crate::unicode_worker::installed_runtime()?;
        crate::unicode_worker::verify_runtime(&root)?;
        let source=fs::read(Path::new(INPUT).join("matrix-source.pdf"))?;
        let candidate=fs::read(Path::new(INPUT).join("matrix-candidate.pdf"))?;
        if crate::save_core::hash(&source)!=SOURCE||crate::save_core::hash(&candidate)!=CANDIDATE{return Err(refused());}
        stage=2;
        crate::worker_sandbox::provision(&root).map_err(|e|{detail=serde_json::to_value(crate::worker_sandbox::check_error(e,true)).unwrap_or_default();refused()})?;
        stage=3;crate::worker_sandbox::verify(&root)?;
        stage=4;let cancel=AtomicBool::new(false);
        crate::unicode_worker::isolation_check(&root,&cancel).map_err(|e|{let d=crate::unicode_worker::probe_details(&e);detail=serde_json::json!({"stage":d.stage,"reason":d.reason,"windowsError":d.windows_error,"workerExit":d.worker_exit});e})?;
        stage=5;
        let output=crate::unicode_worker::prepare_runtime(&root,&source,&candidate,&cancel).map_err(|e|{let d=crate::unicode_worker::probe_details(&e);detail=serde_json::json!({"stage":d.stage,"reason":d.reason,"windowsError":d.windows_error,"workerExit":d.worker_exit});e})?;
        if !output.xref_self_entries{return Err(refused());}
        let mut file=OpenOptions::new().write(true).create_new(true).open(Path::new(LAB).join("unicode-output.pdf"))?;
        file.write_all(&output.bytes)?;file.sync_all()?;drop(file);
        if crate::save_core::hash(&fs::read(Path::new(LAB).join("unicode-output.pdf"))?)!=crate::save_core::hash(&output.bytes){return Err(refused());}
        stage=6;
        let mut child=std::process::Command::new(Path::new(INPUT).join("node.exe")).arg(Path::new(INPUT).join("verify-pdf.mjs")).creation_flags(0x08000000).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn()?;
        let start=Instant::now();loop{if let Some(status)=child.try_wait()?{if !status.success(){return Err(refused());}break;}if start.elapsed()>Duration::from_secs(30){child.kill()?;child.wait()?;return Err(refused());}std::thread::sleep(Duration::from_millis(50));}
        Ok(serde_json::json!({"status":"PASS","sourceHash":SOURCE,"candidateHash":CANDIDATE,"outputHash":crate::save_core::hash(&output.bytes),"bytes":output.bytes.len(),"xrefSelfEntries":true,"independentPdfjs":true,"broker":"standard-test-user","worker":"production-LPAC","guestOnly":true,"guiPass":false}))
    })();
    match result{
        Ok(value)=>if write_result(&value).is_ok(){0}else{71},
        Err(_)=>{let value=serde_json::json!({"status":"BLOCKED","gate":stage,"detail":detail,"pdfAccepted":false,"broker":"standard-test-user","guestOnly":true,"guiPass":false});if write_result(&value).is_err(){71}else{100+stage}}
    }
}
