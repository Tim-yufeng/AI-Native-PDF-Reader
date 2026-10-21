//! Bounded replacement candidate. No PDF write IPC. Only disposable project paths.
#![allow(dead_code)]
use sha2::{Digest, Sha256};
use std::{fs::{File, OpenOptions}, io::{self, Read, Seek, SeekFrom, Write}, path::{Path, PathBuf}};
use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
use windows_sys::Win32::Storage::FileSystem::*;
pub(crate) const READ: u32 = 0x8000_0000;
pub(crate) const WRITE: u32 = 0x4000_0000;
pub(crate) const MAX: u64 = 1024 * 1024;
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Identity { volume: u32, high: u32, low: u32, pub(crate) hash: String }
pub(crate) fn bytes(file: &mut File) -> io::Result<Vec<u8>> {
    if file.metadata()?.len() > MAX { return Err(io::Error::other("lab size limit")); }
    file.seek(SeekFrom::Start(0))?; let mut result=Vec::new(); file.read_to_end(&mut result)?; Ok(result)
}
pub(crate) fn hash(data: &[u8]) -> String { format!("{:x}", Sha256::digest(data)) }
pub(crate) fn identity(file: &mut File) -> io::Result<Identity> {
    let mut info=BY_HANDLE_FILE_INFORMATION::default();
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) }==0 {return Err(io::Error::last_os_error());}
    Ok(Identity {volume:info.dwVolumeSerialNumber, high:info.nFileIndexHigh,low:info.nFileIndexLow,hash:hash(&bytes(file)?)})
}
pub(crate) fn valid_pdf(data: &[u8]) -> bool {
    data.len()>=20 && data.len()<=MAX as usize && data.starts_with(b"%PDF-") && data[data.len().saturating_sub(1024)..].windows(5).any(|p|p==b"%%EOF")
}
pub(crate) fn project() -> PathBuf { Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf() }
pub(crate) fn allowed(path:&Path)->io::Result<()> {
    let base=project().join("tests/work/m3").canonicalize()?;
    let actual=path.canonicalize()?;
    if !actual.starts_with(&base)||actual==base {return Err(io::Error::other("outside disposable lab"));} Ok(())
}
pub(crate) struct Pinned { _chain:Vec<File>, _directory:File, pub(crate) path:PathBuf }
pub(crate) fn pin(root:&Path)->io::Result<Pinned> {
    allowed(root)?; let mut chain=Vec::new();
    for dir in root.ancestors().collect::<Vec<_>>().into_iter().rev() {
        // Deny reparse mutation and parent rename for this bounded experiment.
        let file=OpenOptions::new().access_mode(FILE_READ_ATTRIBUTES).share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS|FILE_FLAG_OPEN_REPARSE_POINT).open(dir)?;
        let mut info=BY_HANDLE_FILE_INFORMATION::default();
        if unsafe {GetFileInformationByHandle(file.as_raw_handle(),&mut info)}==0{return Err(io::Error::last_os_error());}
        if info.dwFileAttributes&FILE_ATTRIBUTE_REPARSE_POINT!=0{return Err(io::Error::other("reparse rejected"));}
        chain.push(file);
    }
    let directory=chain.pop().ok_or_else(||io::Error::other("missing directory"))?;
    Ok(Pinned{_chain:chain,_directory:directory,path:root.canonicalize()?})
}
pub(crate) fn locked(path:&Path)->io::Result<File> {
    let file=OpenOptions::new().access_mode(READ|DELETE).share_mode(FILE_SHARE_READ).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(path)?;
    let mut info=BY_HANDLE_FILE_INFORMATION::default();
    if unsafe{GetFileInformationByHandle(file.as_raw_handle(),&mut info)}==0{return Err(io::Error::last_os_error());}
    if info.dwFileAttributes&(FILE_ATTRIBUTE_REPARSE_POINT|FILE_ATTRIBUTE_DIRECTORY|FILE_ATTRIBUTE_READONLY)!=0{return Err(io::Error::other("unsupported source"));} Ok(file)
}
pub(crate) fn rename_noreplace(file:&File,parent:&Pinned,name:&str)->io::Result<()> {
    if name.is_empty()||name.contains(['\\','/',':'])||name=="."||name==".." {return Err(io::Error::other("invalid leaf"));}
    // Win32 rejected relative RootDirectory on this machine (ERROR_INVALID_PARAMETER).
    // Absolute target under the held, non-reparse parent chain is the bounded fallback.
    let wide:Vec<u16>=parent.path.join(name).to_string_lossy().encode_utf16().collect();
    let offset=std::mem::offset_of!(FILE_RENAME_INFO,FileName);
    let size=offset+wide.len()*2;
    // usize-aligned, zero-initialized storage for the variable-length Win32 structure.
    let mut storage=vec![0usize;size.div_ceil(std::mem::size_of::<usize>())];
    let info=storage.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    unsafe {
        (*info).Anonymous.ReplaceIfExists=false; (*info).RootDirectory=std::ptr::null_mut();
        (*info).FileNameLength=(wide.len()*2) as u32;
        std::ptr::copy_nonoverlapping(wide.as_ptr(),(info.cast::<u8>().add(offset)).cast::<u16>(),wide.len());
        if SetFileInformationByHandle(file.as_raw_handle(),FileRenameInfo,info.cast(),size as u32)==0{return Err(io::Error::last_os_error());}
    } Ok(())
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum ReplaceMode { Win32, Rename, Posix }
pub(crate) fn replace_handle(file: &File, parent: &Pinned, posix: bool) -> io::Result<()> {
    let wide: Vec<u16> = parent.path.join("original.pdf").to_string_lossy().encode_utf16().collect();
    let offset = std::mem::offset_of!(FILE_RENAME_INFO, FileName);
    let size = offset + wide.len() * 2;
    let mut storage = vec![0usize; size.div_ceil(std::mem::size_of::<usize>())];
    let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    unsafe {
        (*info).Anonymous.Flags = if posix { 3 } else { 1 };
        (*info).RootDirectory = std::ptr::null_mut();
        (*info).FileNameLength = (wide.len() * 2) as u32;
        std::ptr::copy_nonoverlapping(wide.as_ptr(), info.cast::<u8>().add(offset).cast::<u16>(), wide.len());
        let class = if posix { FileRenameInfoEx } else { FileRenameInfo };
        if SetFileInformationByHandle(file.as_raw_handle(), class, info.cast(), size as u32) == 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}
fn replace_win32(parent: &Pinned) -> io::Result<()> {
    let wide = |name: &str| parent.path.join(name).to_string_lossy().encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let old = wide("original.pdf"); let new = wide("pending.pdf"); let backup = wide("original.backup.pdf");
    // No IGNORE_* flags; never infer success from the existence of a file.
    if unsafe { ReplaceFileW(old.as_ptr(), new.as_ptr(), backup.as_ptr(), 0, std::ptr::null(), std::ptr::null()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
fn checkpoint(point: &str, wanted: &str) -> io::Result<()> {
    if point != wanted { return Ok(()); }
    if std::env::var("M3_REPLACE_EXIT").as_deref() == Ok("1") { std::process::exit(72); }
    Err(io::Error::other(format!("injected {point}")))
}
/// Actual replacement candidate, shared by all fault and normal tests.
/// Bound to disposable project paths; not exposed to a window or ordinary PDFs.
pub(crate) fn replace_candidate(root: &Path, expected: &Identity, output: &[u8], mode: ReplaceMode,
    share_delete: bool, fault: &str, mut hook: impl FnMut(&str)) -> io::Result<()> {
    if !valid_pdf(output) { return Err(io::Error::other("invalid output")); }
    let pins = pin(root)?;
    let share = FILE_SHARE_READ | if share_delete { FILE_SHARE_DELETE } else { 0 };
    let mut source = OpenOptions::new().access_mode(READ | DELETE).share_mode(share)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(root.join("original.pdf"))?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    if unsafe { GetFileInformationByHandle(source.as_raw_handle(), &mut info) } == 0 { return Err(io::Error::last_os_error()); }
    if info.dwFileAttributes & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_READONLY) != 0 {
        return Err(io::Error::other("unsupported source"));
    }
    if &identity(&mut source)? != expected { return Err(io::Error::other("external change")); }
    let mut pending = OpenOptions::new().write(true).access_mode(READ | WRITE | DELETE).share_mode(FILE_SHARE_READ)
        .create_new(true).open(root.join("pending.pdf"))?;
    pending.write_all(&output[..output.len()/2])?;
    if fault == "disk-full" { return Err(io::Error::from_raw_os_error(112)); }
    checkpoint("partial", fault)?;
    pending.write_all(&output[output.len()/2..])?;
    if fault == "flush-error" { return Err(io::Error::from_raw_os_error(5)); }
    pending.sync_all()?; checkpoint("flushed", fault)?;
    if bytes(&mut pending)? != output { return Err(io::Error::other("output validation")); }
    let old_bytes = bytes(&mut source)?;
    let mut verified = OpenOptions::new().read(true).write(true).share_mode(FILE_SHARE_READ)
        .create_new(true).open(root.join("verified-old.pdf"))?;
    verified.write_all(&old_bytes)?; verified.sync_all()?;
    if bytes(&mut verified)? != old_bytes { return Err(io::Error::other("backup validation")); }
    let mut journal = OpenOptions::new().write(true).create_new(true).open(root.join("replacement.json"))?;
    let record = serde_json::json!({"version":1,"old":expected,"newHash":hash(output),"mode":format!("{mode:?}")});
    journal.write_all(record.to_string().as_bytes())?; journal.sync_all()?;
    checkpoint("prepared", fault)?;
    let mut current = OpenOptions::new().read(true).share_mode(FILE_SHARE_READ | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(root.join("original.pdf"))?;
    if &identity(&mut current)? != expected { return Err(io::Error::other("external change")); }
    drop(current);
    // Deliberately after the final identity check: real competing processes may run here.
    hook("checked"); checkpoint("before-replace", fault)?;
    hook("replacing");
    match mode {
        ReplaceMode::Win32 => { drop(pending); replace_win32(&pins)?; }
        ReplaceMode::Rename => { replace_handle(&pending, &pins, false)?; drop(pending); }
        ReplaceMode::Posix => { replace_handle(&pending, &pins, true)?; drop(pending); }
    }
    hook("replaced");
    checkpoint("after-replace", fault)?;
    drop(source);
    // Win32 can report success even if another object arrived at the target path.
    // Detect from its actual backup; retain everything and never try blind rollback.
    if matches!(mode, ReplaceMode::Win32) {
        let mut actual_backup = locked(&root.join("original.backup.pdf"))?;
        if &identity(&mut actual_backup)? != expected {
            return Err(io::Error::other("post-replace conflict; external bytes retained in backup"));
        }
    }
    let mut published = OpenOptions::new().read(true).share_mode(FILE_SHARE_READ | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(root.join("original.pdf"))?;
    if bytes(&mut published)? != output { return Err(io::Error::other("published output mismatch")); }
    let mut completed = OpenOptions::new().write(true).create_new(true).open(root.join("completed.json"))?;
    completed.write_all(serde_json::json!({"version":1,"newHash":hash(output)}).to_string().as_bytes())?;
    completed.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, sync::{Arc, atomic::{AtomicBool, Ordering}}, thread};
    fn control() -> Vec<u8> { fs::read(project().join("tests/fixtures/g1/freetext-control.pdf")).unwrap() }
    fn output() -> Vec<u8> { let mut data = control(); data.extend_from_slice(b"\n% bounded replacement candidate\n"); data }
    fn external() -> Vec<u8> { let mut data = control(); data.extend_from_slice(b"\n% independent external edit\n"); data }
    fn setup(label: &str) -> (PathBuf, Identity) {
        let root = project().join("tests/work/m3").join(format!("replace-{label}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap(); fs::write(root.join("original.pdf"), control()).unwrap();
        let id = identity(&mut File::open(root.join("original.pdf")).unwrap()).unwrap(); (root, id)
    }
    fn report(root: &Path, value: serde_json::Value) { fs::write(root.join("evidence.json"),value.to_string()).unwrap(); }
    #[test]
    fn actual_modes_with_strict_and_delete_shared_target_handles() {
        for mode in [ReplaceMode::Win32, ReplaceMode::Rename, ReplaceMode::Posix] {
            for shared in [false, true] {
                let (root, id) = setup(&format!("mode-{mode:?}-{shared}"));
                let result = replace_candidate(&root,&id,&output(),mode,shared,"",|_|{});
                let current = fs::read(root.join("original.pdf")).unwrap();
                assert!(current == control() || current == output());
                let status = serde_json::json!({"mode":format!("{mode:?}"),"shareDelete":shared,"ok":result.is_ok(),"error":result.as_ref().err().map(|e|e.to_string()),"osError":result.as_ref().err().and_then(|e|e.raw_os_error()),"originalNew":current==output()});
                println!("{status}"); report(&root,status);
            }
        }
    }
    #[test]
    fn win32_normal_path_observation_records_real_limitation() {
        let stop = Arc::new(AtomicBool::new(false));
        let (root, _) = setup("continuous");
        let monitor_root=root.clone(); let stopped=stop.clone();
        let monitor=thread::spawn(move || {
            let mut opens=0u64; let mut missing=0u64; let mut other=0u64;
            while !stopped.load(Ordering::Acquire) {
                match OpenOptions::new().read(true).share_mode(FILE_SHARE_READ|FILE_SHARE_WRITE|FILE_SHARE_DELETE).open(monitor_root.join("original.pdf")) {
                    Ok(mut file) => { assert!(!bytes(&mut file).unwrap().is_empty()); opens+=1; }
                    Err(error) if error.kind()==io::ErrorKind::NotFound => missing+=1,
                    Err(_) => other+=1,
                }
                thread::yield_now();
            }
            (opens,missing,other)
        });
        // Each transaction preserves its material; keep copies small and bounded.
        for round in 0..20 {
            let current=root.join("original.pdf");
            let id=identity(&mut File::open(&current).unwrap()).unwrap();
            let next=if round%2==0 {output()} else {control()};
            replace_candidate(&root,&id,&next,ReplaceMode::Win32,true,"",|_|{}).unwrap();
            for name in ["original.backup.pdf","verified-old.pdf","replacement.json","completed.json"] {
                fs::rename(root.join(name),root.join(format!("round-{round}-{name}"))).unwrap();
            }
        }
        stop.store(true,Ordering::Release); let (opens,missing,other)=monitor.join().unwrap();
        assert!(opens>0);
        // Diagnostic regression, not a product acceptance pass. Missing > 0
        // explicitly fails the uninterrupted-path requirement.
        let value=serde_json::json!({"rounds":20,"successfulOpens":opens,"notFound":missing,"otherOpenErrors":other,"continuousPathPass":missing==0});
        println!("{value}"); report(&root,value);
    }
    #[test]
    fn final_check_then_actual_external_replacement_is_not_a_safe_cas() {
        let (root,id)=setup("race"); let external=external();
        let result=replace_candidate(&root,&id,&output(),ReplaceMode::Win32,true,"",|point|if point=="checked" {
            let child=std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact","save_core::tests::external_replacement_child"])
                .env("M3_REPLACE_ROOT",&root).output().unwrap();
            assert!(child.status.success());
        });
        assert!(result.is_err()); assert!(!root.join("completed.json").exists());
        // This is an intentionally exposed defect: an error does not undo displacement.
        assert_eq!(fs::read(root.join("original.pdf")).unwrap(),output());
        assert_eq!(fs::read(root.join("original.backup.pdf")).unwrap(),external);
        assert_eq!(fs::read(root.join("verified-old.pdf")).unwrap(),control());
        report(&root,serde_json::json!({"safe":false,"reportedSuccess":false,"externalDisplacedToBackup":true,"error":result.unwrap_err().to_string()}));
    }
    #[test]
    fn external_replacement_child() {
        let Ok(root)=std::env::var("M3_REPLACE_ROOT") else{return;}; let root=PathBuf::from(root); allowed(&root).unwrap();
        fs::rename(root.join("original.pdf"),root.join("external-retained-original.pdf")).unwrap();
        fs::write(root.join("original.pdf"),external()).unwrap();
    }
    #[test]
    fn posix_shared_delete_target_also_allows_actual_competing_replacement() {
        let (root,id)=setup("posix-race");
        let result=replace_candidate(&root,&id,&output(),ReplaceMode::Posix,true,"",|point|if point=="checked" {
            let child=std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact","save_core::tests::external_replacement_child"])
                .env("M3_REPLACE_ROOT",&root).output().unwrap();
            assert!(child.status.success());
        });
        assert!(result.is_ok()); assert_eq!(fs::read(root.join("original.pdf")).unwrap(),output());
        assert_eq!(fs::read(root.join("verified-old.pdf")).unwrap(),control());
        // Exposes the symmetric race. This mode is not approved for application use.
        report(&root,serde_json::json!({"safe":false,"reportedSuccess":true,"externalTargetWasDisplaced":true,"mode":"Posix"}));
    }
    #[test]
    fn failures_and_preexisting_external_change_keep_original() {
        for point in ["disk-full","flush-error","partial","flushed","prepared","before-replace"] {
            let (root,id)=setup(point);
            assert!(replace_candidate(&root,&id,&output(),ReplaceMode::Win32,true,point,|_|{}).is_err());
            assert_eq!(fs::read(root.join("original.pdf")).unwrap(),control());
            assert!(!root.join("completed.json").exists());
        }
        let (root,id)=setup("occupied"); let lock=OpenOptions::new().write(true).share_mode(FILE_SHARE_READ).open(root.join("original.pdf")).unwrap();
        assert!(replace_candidate(&root,&id,&output(),ReplaceMode::Win32,true,"",|_|{}).is_err()); drop(lock);
        fs::write(root.join("original.pdf"),output()).unwrap();
        assert!(replace_candidate(&root,&id,&output(),ReplaceMode::Win32,true,"",|_|{}).is_err());
        assert_eq!(fs::read(root.join("original.pdf")).unwrap(),output());
    }
    #[test]
    fn real_process_exit_before_and_after_replace_preserves_known_bytes() {
        for point in ["partial","flushed","prepared","before-replace","after-replace"] {
            let (root,_)=setup(&format!("exit-{point}"));
            let child=std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact","save_core::tests::replacement_child","--nocapture"])
                .env("M3_REPLACE_ROOT",&root).env("M3_REPLACE_POINT",point).env("M3_REPLACE_EXIT","1").output().unwrap();
            assert_eq!(child.status.code(),Some(72),"{point}: {}",String::from_utf8_lossy(&child.stdout));
            let original=fs::read(root.join("original.pdf")).unwrap();
            assert_eq!(original,if point=="after-replace" {output()} else {control()});
            if point=="after-replace" {
                assert_eq!(fs::read(root.join("original.backup.pdf")).unwrap(),control());
                assert_eq!(fs::read(root.join("verified-old.pdf")).unwrap(),control());
            }
            assert!(!root.join("completed.json").exists());
            report(&root,serde_json::json!({"exit":72,"stage":point,"originalNew":point=="after-replace","successReported":false}));
        }
    }
    #[test]
    fn replacement_child() {
        let Ok(root)=std::env::var("M3_REPLACE_ROOT") else{return;}; let root=PathBuf::from(root); allowed(&root).unwrap();
        let id=identity(&mut File::open(root.join("original.pdf")).unwrap()).unwrap();
        replace_candidate(&root,&id,&output(),ReplaceMode::Win32,true,&std::env::var("M3_REPLACE_POINT").unwrap(),|_|{}).unwrap();
    }
}
