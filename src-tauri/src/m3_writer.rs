//! Explicitly opted-in debug test worker; excluded from release builds.
use std::{fs::{self, OpenOptions}, io::{self, Read, Write}, os::windows::{process::CommandExt, io::AsRawHandle},
    process::{Command, Stdio}, time::{Duration, Instant}, thread};
use windows_sys::Win32::{Foundation::{CloseHandle,HANDLE},System::JobObjects::*};
use crate::{files::{Files,Descriptor},save_core::{project, pin}};
const MAX:usize=1024*1024;
// New byte converter boundary. No fallback to the historical PoC and no save
// command enabled until the lowbox and final commit gates actually pass.
pub(crate) struct PreparedCopy { pub output:crate::unicode_worker::PreparedPdf, _source:crate::files::TestSource }
pub(crate) fn prepare_unicode_copy(files:&Files,owner:&str,grant:&str,epoch:u64,candidate:&[u8],cancel:&std::sync::atomic::AtomicBool)->io::Result<PreparedCopy>{
    let source=files.test_source(owner,grant).map_err(io::Error::other)?;
    let output=crate::unicode_worker::prepare(&source.bytes,candidate,cancel)?;
    files.with_live(owner,grant,epoch,||Ok(())).map_err(io::Error::other)?;
    Ok(PreparedCopy{output,_source:source})
}
static BUSY:std::sync::atomic::AtomicBool=std::sync::atomic::AtomicBool::new(false);
struct Job(HANDLE);
impl Drop for Job {fn drop(&mut self){unsafe{CloseHandle(self.0);}}}
fn bounded_read(reader:impl Read,maximum:usize)->io::Result<Vec<u8>>{
    let mut result=Vec::new();reader.take((maximum+1)as u64).read_to_end(&mut result)?;
    if result.len()>maximum{return Err(io::Error::other("test worker stream limit"));} Ok(result)
}
fn run(source:&[u8],input:&[u8])->io::Result<Vec<u8>>{
    if source.len()>MAX||input.len()>MAX{return Err(io::Error::other("test writer byte limit"));}
    let job=Job(unsafe{CreateJobObjectW(std::ptr::null(),std::ptr::null())});
    if job.0.is_null(){return Err(io::Error::last_os_error());}
    let mut limits=JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags=JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE|JOB_OBJECT_LIMIT_PROCESS_MEMORY|JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
    limits.ProcessMemoryLimit=256*1024*1024;limits.BasicLimitInformation.ActiveProcessLimit=1;
    if unsafe{SetInformationJobObject(job.0,JobObjectExtendedLimitInformation,(&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),std::mem::size_of_val(&limits)as u32)}==0{return Err(io::Error::last_os_error());}
    let root=project();
    let mut child=Command::new(worker_python())
        .args(["-I","-B","-X","utf8"]).arg(root.join("scripts/m3-copy-writer.py"))
        .current_dir(&root).env("TEMP",root.join(".cache/tmp/desktop")).env("TMP",root.join(".cache/tmp/desktop"))
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).creation_flags(0x08000000).spawn()?;
    if unsafe{AssignProcessToJobObject(job.0,child.as_raw_handle())}==0{let error=io::Error::last_os_error();let _=child.kill();let _=child.wait();return Err(error);}
    let mut stdin=child.stdin.take().unwrap();let stdout=child.stdout.take().unwrap();let stderr=child.stderr.take().unwrap();
    let mut packet=Vec::with_capacity(8+source.len()+input.len());packet.extend_from_slice(&(source.len()as u32).to_le_bytes());packet.extend_from_slice(&(input.len()as u32).to_le_bytes());packet.extend_from_slice(source);packet.extend_from_slice(input);
    let input_thread=thread::spawn(move||stdin.write_all(&packet));
    let output_thread=thread::spawn(move||bounded_read(stdout,MAX));let error_thread=thread::spawn(move||bounded_read(stderr,32768));
    let start=Instant::now();
    let status=loop{if let Some(status)=child.try_wait()?{break status;}if start.elapsed()>Duration::from_secs(30){let _=child.kill();break child.wait()?;}thread::sleep(Duration::from_millis(20));};
    let written=input_thread.join().map_err(|_|io::Error::other("worker input task"))?;
    let output=output_thread.join().map_err(|_|io::Error::other("worker output task"))??;
    let errors=error_thread.join().map_err(|_|io::Error::other("worker diagnostics task"))??;
    written?;
    if !status.success(){return Err(io::Error::other("bounded test Writer refused PDF; source unchanged"));}
    let diagnostics:serde_json::Value=serde_json::from_slice(errors.split(|c|*c==b'\n').rev().find(|line|!line.is_empty()).unwrap_or_default())?;
    if diagnostics["testCopyOnly"]!=true||diagnostics["bytes"].as_u64()!=Some(output.len()as u64)||!output.starts_with(b"%PDF-"){return Err(io::Error::other("writer completion differs"));}
    Ok(output)
}
// The legacy M3 bridge needs a Python runtime only for its own experiments. The
// path is configuration, never baked into the source: an unset variable spawns a
// name that cannot exist, so the failure is the ordinary "CreateProcess failed"
// the callers already handle instead of a path that only makes sense on one
// developer's machine.
fn worker_python()->std::path::PathBuf{
    std::env::var_os("AINATIVE_M3_PYTHON").map(std::path::PathBuf::from)
        .unwrap_or_else(||std::path::PathBuf::from("__AINATIVE_M3_PYTHON_NOT_CONFIGURED__.exe"))
}
pub(crate) fn copy(files:&Files,owner:&str,grant:&str,epoch:u64,input:&[u8])->io::Result<Descriptor>{
    copy_in(files,owner,grant,epoch,input,&project().join("tests/work/m3/writer-copies"))
}
// Internal test scope, never a renderer-selected path. Every target is pinned
// below the disposable lab; the GUI retains its original 32-copy budget.
fn copy_in(files:&Files,owner:&str,grant:&str,epoch:u64,input:&[u8],directory:&std::path::Path)->io::Result<Descriptor>{
    if std::env::var("M3_ISOLATED_WRITER").as_deref()!=Ok("1")||input.len()>MAX{return Err(io::Error::other("isolated writer disabled"));}
    if BUSY.swap(true,std::sync::atomic::Ordering::AcqRel){return Err(io::Error::other("test writer busy"));}
    struct Release;impl Drop for Release{fn drop(&mut self){BUSY.store(false,std::sync::atomic::Ordering::Release);}}
    let _release=Release;
    let source=files.test_source(owner,grant).map_err(io::Error::other)?;
    // Source retains the verified M3 ancestor locks across worker execution and
    // creation. Create only one child, before pinning that child; no recursion.
    match fs::create_dir(&directory){Ok(())=>{},Err(error)if error.kind()==io::ErrorKind::AlreadyExists=>{},Err(error)=>return Err(error)}
    let _pins=pin(&directory)?;
    if fs::read_dir(&directory)?.count()>=32{return Err(io::Error::other("test copy quota; retain materials"));}
    let output=run(&source.bytes,input)?;
    let target=directory.join(format!("m3-{}.pdf",uuid::Uuid::new_v4()));
    use std::os::windows::fs::OpenOptionsExt;
    let mut file=OpenOptions::new().read(true).write(true).share_mode(1).custom_flags(0x00200000)
        .create_new(true).open(&target)?;file.write_all(&output)?;file.sync_all()?;
    files.register_test_copy(owner,grant,&target,file,&output,epoch).map_err(io::Error::other)
}
#[cfg(test)]
mod tests{
    use super::*;
    static COPY_TEST:std::sync::Mutex<()>=std::sync::Mutex::new(());
    #[test]fn production_candidate_adapter_retains_grant_boundary_and_never_writes_source(){
        let _test=crate::unicode_worker::test_process_guard();
        let root=project().join("tests/work/m3").join(format!("worker-adapter-{}",uuid::Uuid::new_v4()));fs::create_dir_all(&root).unwrap();
        let source=fs::read(project().join("tests/work/writer/matrix-source.pdf")).unwrap();let candidate=fs::read(project().join("tests/work/writer/matrix-candidate.pdf")).unwrap();
        let path=root.join("source.pdf");fs::write(&path,&source).unwrap();let files=Files::default();let grant=files.authorize_at("main",&path,files.epoch()).unwrap();
        assert!(prepare_unicode_copy(&files,"other",&grant.grant_id,files.epoch(),&candidate,&std::sync::atomic::AtomicBool::new(false)).is_err());
        let outcome=prepare_unicode_copy(&files,"main",&grant.grant_id,files.epoch(),&candidate,&std::sync::atomic::AtomicBool::new(false));
        match outcome{Ok(prepared)=>assert!(prepared.output.bytes.starts_with(&candidate)),Err(error)=>assert_eq!(error.to_string(),"worker CreateProcess failed (Windows 2)")};
        assert_eq!(fs::read(path).unwrap(),source);assert_eq!(fs::read_dir(root).unwrap().count(),1);
    }
    #[test]#[ignore = "requires AINATIVE_M3_PYTHON: the legacy Python runtime is external"]
    fn existing_worker_completes_known_unicode_and_preserves_native_objects(){
        let source=fs::read(project().join("tests/fixtures/g1/plain.pdf")).unwrap();
        let input=fs::read(project().join("tests/work/g1/outputs/chinese-unicode-raw-2.pdf")).unwrap();
        let result=run(&source,&input);assert!(result.is_ok(),"{result:?}");
    }
    #[test]#[ignore = "requires AINATIVE_M3_PYTHON: the legacy Python runtime is external"]
    fn copy_core_uses_authorized_test_source_creates_new_pdf_and_keeps_input(){
        let _test=COPY_TEST.lock().unwrap();
        std::env::set_var("M3_ISOLATED_WRITER","1");
        let plain=fs::read(project().join("tests/fixtures/g1/plain.pdf")).unwrap();
        let root=project().join("tests/work/m3").join(format!("writer-input-{}",uuid::Uuid::new_v4()));fs::create_dir_all(&root).unwrap();
        let source_path=root.join("source.pdf");fs::write(&source_path,&plain).unwrap();
        let directory=root.join("copies");
        let files=Files::default();let grant=files.authorize_at("main",&source_path,files.epoch()).unwrap();
        assert!(files.test_source("other",&grant.grant_id).is_err());
        let mut results=vec![];
        for name in ["chinese-pure-unicode-raw-2.pdf","chinese-unicode-raw-2.pdf","math-unicode-raw-2.pdf"]{
            let input=fs::read(project().join("tests/work/g1/outputs").join(name)).unwrap();
            let out=copy_in(&files,"main",&grant.grant_id,files.epoch(),&input,&directory).unwrap();
            let task=files.begin("main",&out.grant_id).unwrap();let bytes=files.read("main",&task).unwrap();
            assert!(bytes.starts_with(&input));assert_eq!(fs::read(&source_path).unwrap(),plain);
            results.push(serde_json::json!({"input":name,"output":out.name,"bytes":bytes.len(),"sha256":crate::save_core::hash(&bytes)}));
            if name=="chinese-unicode-raw-2.pdf"{
                let again=copy_in(&files,"main",&out.grant_id,files.epoch(),&bytes,&directory).unwrap();
                let task=files.begin("main",&again.grant_id).unwrap();let repeated=files.read("main",&task).unwrap();
                assert!(repeated.starts_with(&bytes));files.release("main",&again.grant_id).unwrap();
            }
            files.release("main",&out.grant_id).unwrap();
        }
        assert!(copy_in(&files,"main",&grant.grant_id,files.epoch(),b"invalid",&directory).is_err());
        let outside=project().join("tests/fixtures/g1/plain.pdf");let denied=files.authorize_at("main",&outside,files.epoch()).unwrap();
        assert!(files.test_source("main",&denied.grant_id).is_err());
        files.release("main",&grant.grant_id).unwrap();assert!(files.test_source("main",&grant.grant_id).is_err());
        fs::write(project().join("tests/work/m3/writer-copy-evidence.json"),serde_json::to_vec_pretty(&results).unwrap()).unwrap();
    }
    #[test]fn saturated_copy_budget_refuses_before_worker_and_preserves_all_materials(){let _test=COPY_TEST.lock().unwrap();std::env::set_var("M3_ISOLATED_WRITER","1");let root=project().join("tests/work/m3").join(format!("writer-quota-{}",uuid::Uuid::new_v4()));fs::create_dir_all(&root).unwrap();let directory=root.join("copies");fs::create_dir(&directory).unwrap();let plain=fs::read(project().join("tests/fixtures/g1/plain.pdf")).unwrap();let source=root.join("source.pdf");fs::write(&source,&plain).unwrap();for n in 0..32{fs::write(directory.join(format!("retained-{n}.pdf")),&plain).unwrap();}let files=Files::default();let grant=files.authorize_at("main",&source,files.epoch()).unwrap();let result=copy_in(&files,"main",&grant.grant_id,files.epoch(),&plain,&directory);assert_eq!(result.err().unwrap().to_string(),"test copy quota; retain materials");assert_eq!(fs::read_dir(&directory).unwrap().count(),32);for n in 0..32{assert_eq!(fs::read(directory.join(format!("retained-{n}.pdf"))).unwrap(),plain);}assert_eq!(fs::read(source).unwrap(),plain);}
    #[test] fn source_and_parent_are_pinned_until_worker_guard_is_dropped(){
        let root=project().join("tests/work/m3").join(format!("writer-lock-{}",uuid::Uuid::new_v4()));fs::create_dir(&root).unwrap();
        let source=root.join("source.pdf");fs::copy(project().join("tests/fixtures/g1/plain.pdf"),&source).unwrap();
        let files=Files::default();let grant=files.authorize_at("main",&source,files.epoch()).unwrap();
        let guard=files.test_source("main",&grant.grant_id).unwrap();
        assert!(fs::rename(&source,root.join("moved.pdf")).is_err());assert!(fs::rename(&root,root.with_extension("moved")).is_err());
        drop(guard);files.release("main",&grant.grant_id).unwrap();
        fs::rename(&source,root.join("moved.pdf")).unwrap();fs::rename(&root,root.with_extension("moved")).unwrap();
    }
}
