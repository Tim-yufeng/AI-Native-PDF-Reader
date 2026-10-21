//! One bounded append-only candidate, test-only with no save command.
//! A prefix-preserving Writer avoids name replacement. Strict sharing must
//! exclude writes and pre-existing writable views; complete materials precede
//! append and explicit recovery refuses unknown versions. Not production-safe
//! approval: reader-grant coordination, general validation and power loss remain.
//! Canonical xref/header checks are not an object-boundary parser: a matching
//! header can occur inside a stream. Known pypdf outputs also omit the xref
//! stream's own entry. These lab tests do not establish PDF conformance or
//! authorize untrusted Worker output for an original-file commit.
use crate::save_core::*;
use std::{collections::BTreeMap,fs::{self,File,OpenOptions},io::{self,Read,Write,Seek,SeekFrom},os::windows::{fs::OpenOptionsExt,io::AsRawHandle},path::{Path,PathBuf},process::Command,time::{Duration,Instant}};
use windows_sys::Win32::Foundation::{HANDLE,CloseHandle};
use windows_sys::Win32::Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION,GetFileInformationByHandle};

#[link(name="kernel32")]unsafe extern "system"{
 fn CreateFileMappingW(file:HANDLE,attributes:*const std::ffi::c_void,protect:u32,high:u32,low:u32,name:*const u16)->HANDLE;
 fn MapViewOfFile(mapping:HANDLE,access:u32,high:u32,low:u32,bytes:usize)->*mut std::ffi::c_void;
 fn FlushViewOfFile(view:*const std::ffi::c_void,bytes:usize)->i32;
 fn UnmapViewOfFile(view:*const std::ffi::c_void)->i32;
}
struct Mapping{handle:HANDLE,view:*mut std::ffi::c_void}
impl Drop for Mapping{fn drop(&mut self){unsafe{UnmapViewOfFile(self.view);CloseHandle(self.handle);}}}
fn wait(path:&std::path::Path){let start=Instant::now();while !path.exists(){assert!(start.elapsed()<Duration::from_secs(4),"private actor timed out");std::thread::sleep(Duration::from_millis(10));}}
#[test]#[ignore="private mapped-view actor, invoked only by the parent"]fn mapped_writer_actor(){let root=PathBuf::from(std::env::var_os("M3_APPEND_GUARD_ROOT").expect("private parent"));allowed(&root).unwrap();let file=OpenOptions::new().read(true).write(true).share_mode(7).open(root.join("original.pdf")).unwrap();let length=file.metadata().unwrap().len() as usize;let mapping=unsafe{CreateFileMappingW(file.as_raw_handle(),std::ptr::null(),4,0,0,std::ptr::null())};assert!(!mapping.is_null());let view=unsafe{MapViewOfFile(mapping,2|4,0,0,length)};assert!(!view.is_null());let _mapping=Mapping{handle:mapping,view};drop(file);fs::write(root.join("mapped.ready"),b"ready").unwrap();wait(&root.join("mapped.go"));unsafe{std::ptr::write_volatile(view.cast::<u8>().add(length-2),b'B');assert_ne!(FlushViewOfFile(view,length),0);}fs::write(root.join("mapped.done"),b"done").unwrap();}
#[test]fn append_guard_rejects_real_preexisting_writable_mapping(){let root=project().join("tests/work/m3").join(format!("append-guard-{}",uuid::Uuid::new_v4()));fs::create_dir_all(&root).unwrap();let mut old=control();old.extend_from_slice(b"\n% mapped A\n");fs::write(root.join("original.pdf"),&old).unwrap();let mut child=Command::new(std::env::current_exe().unwrap()).args(["--exact","save_append_guard::mapped_writer_actor","--ignored","--nocapture"]).env("M3_APPEND_GUARD_ROOT",&root).stdout(std::process::Stdio::null()).spawn().unwrap();wait(&root.join("mapped.ready"));let _pins=pin(&root).unwrap();let attempt=OpenOptions::new().read(true).write(true).share_mode(1).open(root.join("original.pdf"));let error=attempt.err().expect("active writable mapping must be excluded");assert_eq!(error.raw_os_error(),Some(32));assert_eq!(fs::read(root.join("original.pdf")).unwrap(),old);fs::write(root.join("mapped.go"),b"go").unwrap();wait(&root.join("mapped.done"));assert!(child.wait().unwrap().success());let evidence=serde_json::json!({"candidate":"strict append-only","sourceShare":"READ only","strictOpenError":32,"existingWritableMappingRejected":true,"appendPerformed":false,"ctrlSPass":false});fs::write(root.join("evidence.json"),evidence.to_string()).unwrap();println!("{evidence}");}
fn control()->Vec<u8>{fs::read(project().join("tests/work/g1/outputs/chinese-unicode-raw-2.pdf")).unwrap()}
fn update()->Vec<u8>{fs::read(project().join("tests/work/g1/outputs/chinese-unicode-ap-2.pdf")).unwrap()}
fn setup()->(PathBuf,Identity){let root=project().join("tests/work/m3").join(format!("append-save-{}",uuid::Uuid::new_v4()));fs::create_dir_all(&root).unwrap();fs::write(root.join("original.pdf"),control()).unwrap();let expected=identity(&mut File::open(root.join("original.pdf")).unwrap()).unwrap();(root,expected)}
fn safe_leaf(file:&File)->io::Result<()>{let mut info=BY_HANDLE_FILE_INFORMATION::default();if unsafe{GetFileInformationByHandle(file.as_raw_handle(),&mut info)}==0{return Err(io::Error::last_os_error());}if info.nNumberOfLinks!=1||info.dwFileAttributes&(0x400|0x10)!=0{return Err(io::Error::other("unsupported leaf/alias"));}Ok(())}
fn exclusive(root:&Path)->io::Result<File>{let file=OpenOptions::new().read(true).write(true).share_mode(1).custom_flags(0x00200000).open(root.join("original.pdf"))?;safe_leaf(&file)?;Ok(file)}
fn read_material(root:&Path,name:&str,limit:u64)->io::Result<Vec<u8>>{let mut file=OpenOptions::new().read(true).share_mode(1).custom_flags(0x00200000).open(root.join(name))?;safe_leaf(&file)?;if file.metadata()?.len()>limit{return Err(io::Error::other("material budget"));}bytes(&mut file)}
fn checkpoint(point:&str,fault:&str)->io::Result<()>{if point==fault{if std::env::var("M3_APPEND_EXIT").as_deref()==Ok("1"){std::process::exit(75);}if std::env::var("M3_APPEND_PAUSE").as_deref()==Ok("1"){let root=PathBuf::from(std::env::var_os("M3_APPEND_ROOT").ok_or_else(x_error)?);allowed(&root)?;fs::write(root.join("paused.ready"),point)?;let start=Instant::now();while start.elapsed()<Duration::from_secs(10){std::thread::sleep(Duration::from_millis(10));}return Err(io::Error::other("private parent failed to terminate"));}return Err(io::Error::other(point.to_owned()));}Ok(())}
use crate::incremental::{incremental_preflight,x_error,last_xref};
fn material(root:&Path,name:&str,value:&[u8])->io::Result<File>{let mut f=OpenOptions::new().read(true).write(true).create_new(true).share_mode(1).open(root.join(name))?;f.write_all(value)?;f.sync_all()?;Ok(f)}
fn save(root:&Path,expected:&Identity,output:&[u8],fault:&str,mut hook:impl FnMut(&str))->io::Result<()>{let _pins=pin(root)?;let mut source=exclusive(root)?;if &identity(&mut source)?!=expected{return Err(io::Error::other("external version differs"));}let old=bytes(&mut source)?;if !valid_pdf(output)||!output.starts_with(&old)||output.len()<=old.len(){return Err(io::Error::other("verified incremental prefix required"));}incremental_preflight(&old,output)?;let mut old_material=material(root,"verified-old.pdf",&old)?;if bytes(&mut old_material)?!=old{return Err(io::Error::other("backup differs"));}let mut new_material=material(root,"verified-new.pdf",output)?;if bytes(&mut new_material)?!=output{return Err(io::Error::other("candidate material differs"));}let _intent=material(root,"append-intent.json",serde_json::json!({"old":expected,"oldLength":old.len(),"newHash":hash(output),"newLength":output.len(),"schema":1,"testOnly":true}).to_string().as_bytes())?;checkpoint("prepared",fault)?;if &identity(&mut source)?!=expected{return Err(io::Error::other("external version differs"));}hook("checked");source.seek(SeekFrom::End(0))?;let suffix=&output[old.len()..];let middle=suffix.len()/2;if fault=="short-write"{source.write_all(&suffix[..13.min(middle)])?;return Err(io::Error::new(io::ErrorKind::WriteZero,"injected partial write"));}source.write_all(&suffix[..middle])?;checkpoint("flush-error",fault)?;source.sync_all()?;hook("partial");checkpoint("partial",fault)?;if fault=="full"{return Err(io::Error::from_raw_os_error(112));}if fault=="write-error"{return Err(io::Error::from_raw_os_error(29));}let trailer=suffix.len().saturating_sub(7).max(middle);source.write_all(&suffix[middle..trailer])?;source.sync_all()?;checkpoint("trailer",fault)?;source.write_all(&suffix[trailer..])?;checkpoint("final-flush-error",fault)?;source.sync_all()?;checkpoint("appended",fault)?;if bytes(&mut source)?!=output{return Err(io::Error::other("saved bytes differ"));}let _completed=material(root,"completed.json",hash(output).as_bytes())?;checkpoint("committed",fault)?;Ok(())}
// Explicit lab recovery, never automatic source truncation. The full verified
// old PDF remains in a separate material and unknown external states refuse.
fn recover(root:&Path,expected:&Identity)->io::Result<&'static str>{let _pins=pin(root)?;let mut source=exclusive(root)?;let mut id=identity(&mut source)?;id.hash=expected.hash.clone();if &id!=expected{return Err(io::Error::other("different physical source"));}let old=read_material(root,"verified-old.pdf",MAX)?;let new=read_material(root,"verified-new.pdf",MAX)?;let intent:serde_json::Value=serde_json::from_slice(&read_material(root,"append-intent.json",16384)?).map_err(io::Error::other)?;if intent["schema"]!=1||intent["testOnly"]!=true||hash(&old)!=expected.hash||intent["old"]!=serde_json::to_value(expected).map_err(io::Error::other)?||intent["oldLength"].as_u64()!=Some(old.len() as u64)||intent["newLength"].as_u64()!=Some(new.len() as u64)||intent["newHash"]!=hash(&new)||!valid_pdf(&new)||!new.starts_with(&old){return Err(io::Error::other("recovery materials differ"));}let current=bytes(&mut source)?;if current==new{return Ok("complete-new");}if current==old{return Ok("complete-old");}if !current.starts_with(&old)||!new.starts_with(&current){return Err(io::Error::other("external state preserved; no automatic repair"));}// After the process guard is lost, matching bytes do not prove writer provenance. Never truncate on recovery. Complete old/new materials remain available for explicit recovery to a new file.
Ok("incomplete-preserved-no-truncation")}
#[test]#[ignore="private append actor, invoked by the parent"]fn append_actor(){let root=PathBuf::from(std::env::var_os("M3_APPEND_ROOT").expect("parent"));allowed(&root).unwrap();match std::env::var("M3_APPEND_OP").unwrap().as_str(){"locked"=>{for name in ["original.pdf","verified-old.pdf","verified-new.pdf","append-intent.json"]{assert_eq!(OpenOptions::new().write(true).open(root.join(name)).unwrap_err().raw_os_error(),Some(32));assert_eq!(fs::rename(root.join(name),root.join(format!("external-{name}"))).unwrap_err().raw_os_error(),Some(32));}},"extend-prefix"=>{let mut source=OpenOptions::new().append(true).open(root.join("original.pdf")).unwrap();let start=source.metadata().unwrap().len() as usize;source.write_all(&update()[start..start+8]).unwrap();source.sync_all().unwrap();},"exit"=>{let id=identity(&mut File::open(root.join("original.pdf")).unwrap()).unwrap();save(&root,&id,&update(),&std::env::var("M3_APPEND_POINT").unwrap(),|_|{}).unwrap();},_=>panic!("private actor")}}

#[test]
fn recovery_must_not_truncate_a_later_external_write_matching_the_candidate_prefix() {
    let (root, id) = setup();
    assert!(save(&root, &id, &update(), "partial", |_| {}).is_err());
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "save_append_guard::append_actor", "--ignored"])
        .env("M3_APPEND_ROOT", &root).env("M3_APPEND_OP", "extend-prefix").output().unwrap();
    assert!(output.status.success());
    let external = fs::read(root.join("original.pdf")).unwrap();
    let outcome = recover(&root, &id);
    let after = fs::read(root.join("original.pdf")).unwrap();
    fs::write(root.join("recovery-collision.json"), serde_json::json!({
        "externalWriterRan": true, "beforeBytes": external.len(), "afterBytes": after.len(),
        "preserved": external == after, "outcome": format!("{outcome:?}"), "ctrlSPass": false
    }).to_string()).unwrap();
    assert!(after == external, "matching bytes cannot prove ownership after the write guard was lost: {} -> {} bytes", external.len(), after.len());
}

#[test]
fn incremental_matrix_and_io_faults_keep_original_and_verified_materials() {
    let work=project().join("tests/work/security-close/round2");
    let old=fs::read(work.join("complex-v17-source.pdf")).unwrap();
    let new=fs::read(work.join("complex-v17-update.pdf")).unwrap();
    let make=|label:&str|{
        let root=project().join("tests/work/m3").join(format!("{label}-{}",uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();fs::write(root.join("original.pdf"),&old).unwrap();
        let id=identity(&mut File::open(root.join("original.pdf")).unwrap()).unwrap();(root,id)
    };
    let (root,id)=make("append-complex-v17");
    save(&root,&id,&new,"",|_|{}).unwrap();
    assert!(fs::read(root.join("original.pdf")).unwrap()==new);
    for fault in ["short-write","write-error","flush-error","final-flush-error"] {
        let (root,id)=make("append-io");
        assert!(save(&root,&id,&new,fault,|_|{}).is_err());
        assert!(!root.join("completed.json").exists());
        let before=fs::read(root.join("original.pdf")).unwrap();
        let outcome=recover(&root,&id).unwrap();
        assert!(fs::read(root.join("original.pdf")).unwrap()==before);
        assert!(fs::read(root.join("verified-old.pdf")).unwrap()==old);
        assert!(fs::read(root.join("verified-new.pdf")).unwrap()==new);
        fs::write(root.join("io-evidence.json"),serde_json::json!({
            "fault":fault,"injection":"controlled I/O boundary, not real disk/OS failure",
            "result":outcome,"sourceUntouchedByRecovery":true,"ctrlSPass":false
        }).to_string()).unwrap();
    }
    let mut bad_prev=new.clone();let offset=last_xref(&new).unwrap();
    let prior=last_xref(&old).unwrap().to_string();
    let marker=format!("/Prev {prior}");let index=bad_prev[offset..].windows(marker.len()).position(|w|w==marker.as_bytes()).unwrap()+offset+6;
    bad_prev[index..index+prior.len()].fill(b'0');
    for damaged in [bad_prev,new[..new.len()-8].to_vec(),[old.clone(),b"\n% not an update\n".to_vec()].concat()] {
        let(root,id)=make("append-invalid");
        assert!(save(&root,&id,&damaged,"",|_|{}).is_err());
        assert!(fs::read(root.join("original.pdf")).unwrap()==old);
        assert!(!root.join("append-intent.json").exists());
    }
    // The version header is not a safety signal: an ordinary `%PDF-1.3` or
    // `%PDF-1.4` document whose revision chain is verifiable must be accepted,
    // and the structural checks still refuse a revision that cannot be walked.
    for version in [b'3',b'4',b'5',b'7'] {
        let mut lower_old=old.clone();lower_old[7]=version;let mut lower_new=new.clone();lower_new[7]=version;
        assert!(incremental_preflight(&lower_old,&lower_new).is_ok(),"PDF 1.{version} was refused although its revision is verifiable");
    }
    let mut broken=new.clone();let broken_at=last_xref(&new).unwrap();broken[broken_at..].fill(b' ');
    assert!(incremental_preflight(&old,&broken).is_err(),"a revision with no readable xref was accepted");
}
#[test]fn append_protocol_preserves_name_conflicts_locks_and_permissions(){use std::sync::{Arc,atomic::{AtomicBool,Ordering}};let(root,id)=setup();let stop=Arc::new(AtomicBool::new(false));let stopped=stop.clone();let target=root.join("original.pdf");let monitor=std::thread::spawn(move||{let(mut opens,mut failures)=(0,0);while !stopped.load(Ordering::Acquire){match OpenOptions::new().read(true).share_mode(7).open(&target){Ok(mut file)=>{if bytes(&mut file).is_ok(){opens+=1;}else{failures+=1;}},Err(_)=>failures+=1}std::thread::yield_now();}(opens,failures)});save(&root,&id,&update(),"",|point|if point=="checked"{assert!(Command::new(std::env::current_exe().unwrap()).args(["--exact","save_append_guard::append_actor","--ignored"]).env("M3_APPEND_ROOT",&root).env("M3_APPEND_OP","locked").output().unwrap().status.success());}else if point=="partial"{material(&root,"observed-partial.pdf",&fs::read(root.join("original.pdf")).unwrap()).unwrap();}).unwrap();stop.store(true,Ordering::Release);let(opens,failures)=monitor.join().unwrap();assert!(opens>0);assert_eq!(failures,0);assert_eq!(fs::read(root.join("original.pdf")).unwrap(),update());assert_eq!(fs::read(root.join("verified-old.pdf")).unwrap(),control());let evidence=serde_json::json!({"successfulOpenAndReads":opens,"openFailures":failures,"oldBackupVerified":true,"independentWriterDenied":true,"ctrlSPass":false});fs::write(root.join("evidence.json"),evidence.to_string()).unwrap();println!("{evidence}");let(root,id)=setup();let changed=[control(),b"\n% outside".to_vec()].concat();fs::write(root.join("original.pdf"),&changed).unwrap();assert!(save(&root,&id,&update(),"",|_|{}).is_err());assert_eq!(fs::read(root.join("original.pdf")).unwrap(),changed);let(root,id)=setup();let occupied=OpenOptions::new().read(true).write(true).share_mode(7).open(root.join("original.pdf")).unwrap();assert!(save(&root,&id,&update(),"",|_|{}).is_err());drop(occupied);let mut permission=fs::metadata(root.join("original.pdf")).unwrap().permissions();permission.set_readonly(true);fs::set_permissions(root.join("original.pdf"),permission).unwrap();assert_eq!(save(&root,&id,&update(),"",|_|{}).unwrap_err().raw_os_error(),Some(5));}
#[test]fn append_process_exit_retains_verified_materials_and_refuses_unknown_external_recovery(){for point in ["prepared","partial","trailer","appended","committed"]{let(root,id)=setup();let output=Command::new(std::env::current_exe().unwrap()).args(["--exact","save_append_guard::append_actor","--ignored"]).env("M3_APPEND_ROOT",&root).env("M3_APPEND_OP","exit").env("M3_APPEND_POINT",point).env("M3_APPEND_EXIT","1").output().unwrap();assert_eq!(output.status.code(),Some(75));assert!(root.join("original.pdf").exists());assert_eq!(fs::read(root.join("verified-old.pdf")).unwrap(),control());assert_eq!(fs::read(root.join("verified-new.pdf")).unwrap(),update());let outcome=recover(&root,&id).unwrap();assert_eq!(fs::read(root.join("original.pdf")).unwrap(),if matches!(point,"appended"|"committed"){update()}else if point=="prepared"{control()}else{let current=fs::read(root.join("original.pdf")).unwrap();assert!(update().starts_with(&current));assert_eq!(outcome,"incomplete-preserved-no-truncation");current});fs::write(root.join("evidence.json"),serde_json::json!({"exit":point,"code":75,"recovery":outcome,"ctrlSPass":false}).to_string()).unwrap();}let(root,id)=setup();assert_eq!(save(&root,&id,&update(),"full",|_|{}).unwrap_err().raw_os_error(),Some(112));let changed=[control(),b"\n% unrelated later change".to_vec()].concat();fs::write(root.join("original.pdf"),&changed).unwrap();assert!(recover(&root,&id).is_err());assert_eq!(fs::read(root.join("original.pdf")).unwrap(),changed);}

#[test]
fn xref_entries_are_validated_before_any_source_write() {
    let work=project().join("tests/work/security-close/round2");
    let old=fs::read(work.join("complex-v17-source.pdf")).unwrap();
    let new=fs::read(work.join("complex-v17-update.pdf")).unwrap();
    incremental_preflight(&old,&new).unwrap();
    let offset=last_xref(&new).unwrap();
    let change=|needle:&[u8],replacement:&[u8]|{
        assert_eq!(needle.len(),replacement.len());let mut data=new.clone();
        let index=data[offset..].windows(needle.len()).position(|v|v==needle).unwrap()+offset;
        data[index..index+needle.len()].copy_from_slice(replacement);data
    };
    let mut damaged=vec![
        change(b"/Root 1 0 R",b"/Root 2 0 R"),
        change(b"/Size 89",b"/Size 88"),
        change(b"/W [ 1 4 1 ]",b"/W [ 1 4 0 ]"),
        change(b"/Index [ 27 2",b"/Index [ 27 0"),
        change(b"/Length 73",b"/Length 72"),
        change(b"/FlateDecode",b"/DlateDecode"),
    ];
    let mut invalid_object=new.clone();invalid_object[old.len()]=b'9';damaged.push(invalid_object);
    for (index,data)in damaged.iter().enumerate(){
        let root=project().join("tests/work/m3").join(format!("append-xref-{}",uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();fs::write(root.join("original.pdf"),&old).unwrap();
        let id=identity(&mut File::open(root.join("original.pdf")).unwrap()).unwrap();
        assert!(save(&root,&id,data,"",|_|{}).is_err(),"variant {index}");
        assert_eq!(fs::read(root.join("original.pdf")).unwrap(),old);
        assert!(!root.join("append-intent.json").exists());
    }
}

#[test]
fn unicode_worker_multi_revision_output_reaches_held_source() {
    let work=project().join("tests/work/writer");
    let original=fs::read(work.join("matrix-source.pdf")).unwrap();
    let first=fs::read(work.join("matrix-first.pdf")).unwrap();
    let final_pdf=fs::read(work.join("matrix-final.pdf")).unwrap();
    // Real PDF.js plus Unicode Worker outputs, not a replacement fixture.
    incremental_preflight(&original,&first).unwrap();
    incremental_preflight(&first,&final_pdf).unwrap();
    let root=project().join("tests/work/m3").join(format!("append-worker-{}",uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();fs::write(root.join("original.pdf"),&original).unwrap();
    let identity=identity(&mut File::open(root.join("original.pdf")).unwrap()).unwrap();
    save(&root,&identity,&first,"",|_|{}).unwrap();
    assert_eq!(fs::read(root.join("original.pdf")).unwrap(),first);
    let mut broken=first.clone();let xref=last_xref(&first).unwrap();
    let key=b"/Prev ";let at=broken[xref..].windows(key.len()).position(|v|v==key).unwrap()+xref+key.len();
    let end=broken[at..].iter().position(|v|!v.is_ascii_digit()).unwrap()+at;
    broken[at..end].fill(b'0');assert!(incremental_preflight(&original,&broken).is_err());
    fs::write(root.join("writer-chain.json"),serde_json::json!({"realWorkerBytes":first.len(),"sourceBytes":original.len(),"multipleNewRevisionsVerified":true,"full20EditHistoryVerified":true,"brokenChainRejected":true,"ctrlSPass":false}).to_string()).unwrap();
}

#[test]
fn recovery_refuses_replaced_identity_material_tampering_and_aliases() {
    for variant in ["replacement","old-material","new-material","intent","hardlink"]{
        let(root,id)=setup();assert!(save(&root,&id,&update(),"partial",|_|{}).is_err());
        let before=fs::read(root.join("original.pdf")).unwrap();
        match variant{
            "replacement"=>{fs::rename(root.join("original.pdf"),root.join("displaced.pdf")).unwrap();fs::write(root.join("original.pdf"),&before).unwrap();},
            "old-material"=>{let mut v=control();v[10]^=1;fs::write(root.join("verified-old.pdf"),v).unwrap();},
            "new-material"=>{let mut v=update();v[10]^=1;fs::write(root.join("verified-new.pdf"),v).unwrap();},
            "intent"=>{fs::write(root.join("append-intent.json"),b"{\"schema\":2}").unwrap();},
            "hardlink"=>{fs::hard_link(root.join("verified-old.pdf"),root.join("alias-old.pdf")).unwrap();},
            _=>unreachable!()
        }
        assert!(recover(&root,&id).is_err(),"{variant}");
        assert_eq!(fs::read(root.join("original.pdf")).unwrap(),before);
        fs::write(root.join("recovery-rejection.json"),serde_json::json!({"case":variant,"rejected":true,"originalPreserved":true,"ctrlSPass":false}).to_string()).unwrap();
    }
}

#[test]
fn force_terminated_process_keeps_complete_materials_without_source_repair() {
    for point in ["prepared","partial","trailer","appended","committed"]{
        let(root,id)=setup();
        let mut child=Command::new(std::env::current_exe().unwrap())
            .args(["--exact","save_append_guard::append_actor","--ignored"])
            .env("M3_APPEND_ROOT",&root).env("M3_APPEND_OP","exit")
            .env("M3_APPEND_POINT",point).env("M3_APPEND_PAUSE","1")
            .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap();
        wait(&root.join("paused.ready"));child.kill().unwrap();let exited=child.wait().unwrap();assert!(!exited.success());
        let before=fs::read(root.join("original.pdf")).unwrap();let outcome=recover(&root,&id).unwrap();
        assert_eq!(fs::read(root.join("original.pdf")).unwrap(),before);
        assert_eq!(fs::read(root.join("verified-old.pdf")).unwrap(),control());
        assert_eq!(fs::read(root.join("verified-new.pdf")).unwrap(),update());
        fs::write(root.join("terminated.json"),serde_json::json!({"stage":point,"forcedProcessTermination":true,"recovery":outcome,"sourceBytes":before.len(),"oldMaterialValid":true,"newMaterialValid":true,"sourceUnchangedByRecovery":true,"powerLossTest":false,"ctrlSPass":false}).to_string()).unwrap();
    }
}
