use crate::error::{AppError, Result};
use serde::Serialize;
use std::{collections::HashMap, fs::{File, OpenOptions}, io::{Read, Seek, SeekFrom}, path::{Path, PathBuf},
    sync::{Arc, Mutex, atomic::{AtomicBool, AtomicU64, Ordering}}, time::{Instant, SystemTime}};
use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle, ffi::OsStringExt};
use windows_sys::Win32::Storage::FileSystem::{GetFinalPathNameByHandleW, GetDriveTypeW};
use windows_sys::Win32::System::WindowsProgramming::{DRIVE_FIXED, DRIVE_REMOVABLE};

const MAX_BYTES: u64 = 64 * 1024 * 1024; // Same provisional guard as the front end.
pub(crate) const EXPORT_INPUT_MAX: usize = 16 * 1024 * 1024;
pub(crate) const EXPORT_OUTPUT_MAX: usize = EXPORT_INPUT_MAX + 4 * 1024 * 1024;
const MAX_GRANTS: usize = 32;
const MAX_TASKS: usize = 2;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Descriptor { pub grant_id: String, pub name: String, pub size: u64 }
struct Grant {
    owner: String, file: Mutex<File>, size: u64, modified: Option<SystemTime>, revoked: AtomicBool,
    // Actual handle target is private and cannot become a directory grant.
    _actual_path: PathBuf,
}
pub(crate) struct Observation { pub key:String, pub hash:String, pub name:String }
pub(crate) struct ExportSource {pub bytes:Vec<u8>,_pins:Vec<File>,_file:Option<File>}
/// Consumes one idle read grant while preserving the exact opened source object.
/// No renderer can obtain this lease. Failure after consumption requires reopening.
pub(crate) struct OriginalWriteLease {
    file:File, _pins:Vec<File>, bytes:Vec<u8>, path:PathBuf,
    file_key:String, epoch:u64, owner:String,
}
impl OriginalWriteLease {
    pub(crate) fn file_mut(&mut self)->&mut File {&mut self.file}
    pub(crate) fn bytes(&self)->&[u8] {&self.bytes}
    pub(crate) fn file_key(&self)->&str {&self.file_key}
}
impl ExportSource{
    /// Release the extra read handle so the explicit original-save protocol can
    /// replace the file atomically.
    pub(crate) fn close_handle(&mut self){self._file=None;}
    /// Release the extra handle and the captured parent pins. Used only for the
    /// final replacement step, after identity and bytes were already re-checked.
    pub(crate) fn close_all(&mut self){self._file=None;self._pins.clear();}
}
#[cfg(debug_assertions)]
pub(crate) struct TestSource {pub(crate) bytes:Vec<u8>,_pins:crate::save_core::Pinned,_file:File}
struct Task { owner: String, grant_id: String, grant: Arc<Grant>, cancelled: AtomicBool, started: AtomicBool, created: Instant }
#[derive(Default)]
struct Inner { grants: HashMap<String, Arc<Grant>>, tasks: HashMap<String, Arc<Task>> }
#[derive(Default)]
pub struct Files { inner: Mutex<Inner>, closed: AtomicBool, epoch: AtomicU64, pub picking: AtomicBool }

pub(crate) fn final_path(file: &File) -> Result<PathBuf> {
    // Query the already-open handle, never resolve a second path for reading.
    let handle = file.as_raw_handle();
    let count = unsafe { GetFinalPathNameByHandleW(handle, std::ptr::null_mut(), 0, 0) };
    if count == 0 || count > 32768 { return Err(AppError::io()); }
    let mut wide = vec![0; count as usize + 1];
    let written = unsafe { GetFinalPathNameByHandleW(handle, wide.as_mut_ptr(), wide.len() as u32, 0) };
    if written == 0 || written as usize >= wide.len() { return Err(AppError::io()); }
    let path = PathBuf::from(std::ffi::OsString::from_wide(&wide[..written as usize]));
    let text = path.to_string_lossy();
    let bytes = text.as_bytes();
    if bytes.len() < 7 || !text.starts_with("\\\\?\\") || !bytes[4].is_ascii_alphabetic() || bytes[5] != b':' || bytes[6] != b'\\' {
        return Err(AppError::invalid());
    }
    let drive: Vec<u16> = format!("{}:\\", bytes[4] as char).encode_utf16().chain(Some(0)).collect();
    let drive_type = unsafe { GetDriveTypeW(drive.as_ptr()) };
    if ![DRIVE_FIXED, DRIVE_REMOVABLE].contains(&drive_type) { return Err(AppError::invalid()); }
    Ok(path)
}
fn pdf_extension(path: &Path) -> bool { path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("pdf")) }

fn reopen(file:&File,access:u32,share:u32)->Result<File> {
    use std::os::windows::io::FromRawHandle;
    let handle=unsafe{windows_sys::Win32::Storage::FileSystem::ReOpenFile(file.as_raw_handle(),access,share,0)};
    if handle==windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE{return Err(AppError{code:"writer_original_reopen_required",message:"无法取得原件的独占写入保护；未开始写入，请关闭并重新打开文件。"});}
    Ok(unsafe{File::from_raw_handle(handle)})
}
fn physical_key(file:&File)->Result<String> {
    use windows_sys::Win32::Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION,GetFileInformationByHandle,GetFileInformationByHandleEx,FileIdInfo,FILE_ID_INFO};
    let mut info=BY_HANDLE_FILE_INFORMATION::default();let mut id=FILE_ID_INFO::default();
    if unsafe{GetFileInformationByHandle(file.as_raw_handle(),&mut info)}==0||unsafe{GetFileInformationByHandleEx(file.as_raw_handle(),FileIdInfo,(&mut id as *mut FILE_ID_INFO).cast(),std::mem::size_of::<FILE_ID_INFO>()as u32)}==0||id.FileId.Identifier.iter().all(|v|*v==0)||info.nNumberOfLinks!=1||info.dwFileAttributes&(0x400|0x10)!=0{return Err(AppError::changed());}
    let object=id.FileId.Identifier.iter().map(|v|format!("{v:02x}")).collect::<String>();
    Ok(format!("win128:{:016x}:{object}:{:08x}{:08x}",id.VolumeSerialNumber,info.ftCreationTime.dwHighDateTime,info.ftCreationTime.dwLowDateTime))
}
fn bounded_bytes(file:&mut File,limit:usize)->Result<Vec<u8>> {
    if file.metadata().map_err(|_|AppError::io())?.len()>limit as u64{return Err(AppError::changed());}
    file.seek(SeekFrom::Start(0)).map_err(|_|AppError::io())?;let mut bytes=Vec::new();
    file.take(limit as u64+1).read_to_end(&mut bytes).map_err(|_|AppError::io())?;
    if bytes.len()>limit{return Err(AppError::changed());}Ok(bytes)
}

impl Files {
    /// No path lookup after releasing the reader: the captured no-DELETE anchor
    /// survives the handoff, and ReOpenFile addresses that same kernel object.
    /// An active task/alias/borrow refuses before consuming the original grant.
    pub(crate) fn take_original_write(&self,owner:&str,id:&str,epoch:u64,expected_hash:&str)->Result<OriginalWriteLease> {
        self.take_original_write_inner(owner,id,epoch,expected_hash,||{})
    }
    fn take_original_write_inner(&self,owner:&str,id:&str,epoch:u64,expected_hash:&str,after_reader_close:impl FnOnce())->Result<OriginalWriteLease> {
        self.take_original_write_recovering(owner,id,epoch,expected_hash,after_reader_close,&mut None)
    }
    pub(crate) fn take_original_write_recovering(&self,owner:&str,id:&str,epoch:u64,expected_hash:&str,after_reader_close:impl FnOnce(),recovery:&mut Option<Descriptor>)->Result<OriginalWriteLease> {
        let mut captured=self.export_source(owner,id)?;
        if crate::save_core::hash(&captured.bytes)!=expected_hash{return Err(AppError::changed());}
        let anchor=captured._file.take().ok_or_else(AppError::denied)?;
        let key=physical_key(&anchor)?;let path=final_path(&anchor)?;
        let grant={
            let mut inner=self.inner.lock().map_err(|_|AppError::io())?;
            if self.closed.load(Ordering::Acquire)||self.epoch()!=epoch{return Err(AppError::denied());}
            let selected=inner.grants.get(id).filter(|g|g.owner==owner&&!g.revoked.load(Ordering::Acquire)).ok_or_else(AppError::denied)?;
            if Arc::strong_count(selected)!=1||inner.tasks.values().any(|t|t.grant_id==id){return Err(AppError::busy());}
            // Never wait for file mutexes under Inner: active readers use the
            // opposite order when rechecking their live authorization.
            for (other_id,other)in &inner.grants {
                let file=other.file.try_lock().map_err(|_|AppError::busy())?;
                if other_id!=id&&physical_key(&file)?==key{return Err(AppError::busy());}
            }
            let removed=inner.grants.remove(id).ok_or_else(AppError::denied)?;
            match Arc::try_unwrap(removed){Ok(grant)=>grant,Err(grant)=>{inner.grants.insert(id.into(),grant);return Err(AppError::busy());}}
        };
        // The old token is now deliberately absent. Once its denying handle is
        // closed, any failure is a revoke/reopen result, never a permissive grant.
        drop(grant.file.into_inner().map_err(|_|AppError::io())?);
        after_reader_close();
        let strict=(||{let mut file=reopen(&anchor,0x8000_0000|0x4000_0000,1)?;
            if physical_key(&file)?!=key||final_path(&file)?!=path||bounded_bytes(&mut file,EXPORT_INPUT_MAX)?!=captured.bytes||self.closed.load(Ordering::Acquire)||self.epoch()!=epoch{return Err(AppError{code:"writer_original_reopen_required",message:"取得写入保护后检测到文件或授权变化；未写入，请重新打开文件。"});}Ok(file)})();
        match strict{
            Ok(file)=>Ok(OriginalWriteLease{file,_pins:captured._pins,bytes:captured.bytes,path,file_key:key,epoch,owner:owner.into()}),
            Err(error)=>{
                // Never retry with weaker WRITE sharing. Only hand back a new
                // READ authority for this still-pinned object if it is safe.
                if let Ok(mut file)=reopen(&anchor,0x8000_0000,1|4){if let Ok(current)=bounded_bytes(&mut file,EXPORT_OUTPUT_MAX){
                    let lease=OriginalWriteLease{file,_pins:captured._pins,bytes:current.clone(),path,file_key:key,epoch,owner:owner.into()};
                    *recovery=self.register_original_commit(lease,&current).ok();
                }}Err(error)
            }
        }
    }
    pub(crate) fn original_lease_live(&self,lease:&OriginalWriteLease)->Result<()> {
        if self.closed.load(Ordering::Acquire)||self.epoch()!=lease.epoch{return Err(AppError::denied());}Ok(())
    }
    pub(crate) fn original_recovery_snapshot(&self,owner:&str,id:&str,epoch:u64)->Result<(String,Vec<u8>)> {
        let grant=self.inner.lock().map_err(|_|AppError::io())?.grants.get(id).filter(|g|g.owner==owner&&!g.revoked.load(Ordering::Acquire)).cloned().ok_or_else(AppError::denied)?;
        let mut file=grant.file.lock().map_err(|_|AppError::io())?;
        if self.epoch()!=epoch{return Err(AppError::denied());}self.live(owner,id)?;
        let before=file.metadata().map_err(|_|AppError::io())?;
        if before.len()!=grant.size||before.modified().ok()!=grant.modified{return Err(AppError::changed());}
        let key=physical_key(&file)?;let bytes=bounded_bytes(&mut file,EXPORT_OUTPUT_MAX)?;
        let after=file.metadata().map_err(|_|AppError::io())?;
        if after.len()!=before.len()||after.modified().ok()!=before.modified().ok()||final_path(&file)?!=grant._actual_path||self.epoch()!=epoch{return Err(AppError::changed());}
        self.live(owner,id)?;Ok((key,bytes))
    }
    /// A successful disk commit is not undone if this read-only publication
    /// fails. The caller must return its verified receipt and require reopening.
    pub(crate) fn register_original_commit(&self,lease:OriginalWriteLease,expected:&[u8])->Result<Descriptor> {
        let OriginalWriteLease{file,_pins,path,file_key,epoch,owner,..}=lease;
        let anchor=reopen(&file,0x8000_0000,1|2)?;drop(file);
        let mut reader=reopen(&anchor,0x8000_0000,1|4)?;
        if physical_key(&reader)?!=file_key||final_path(&reader)?!=path||bounded_bytes(&mut reader,EXPORT_OUTPUT_MAX)?!=expected{return Err(AppError::changed());}
        let meta=reader.metadata().map_err(|_|AppError::io())?;
        reader.seek(SeekFrom::Start(0)).map_err(|_|AppError::io())?;
        let grant_id=uuid::Uuid::new_v4().to_string();let name=path.file_name().ok_or_else(AppError::invalid)?.to_string_lossy().into_owned();
        let descriptor=Descriptor{grant_id:grant_id.clone(),name,size:meta.len()};
        let mut inner=self.inner.lock().map_err(|_|AppError::io())?;
        if self.closed.load(Ordering::Acquire)||self.epoch()!=epoch||inner.grants.len()>=MAX_GRANTS{return Err(AppError::denied());}
        inner.grants.insert(grant_id,Arc::new(Grant{owner,file:Mutex::new(reader),size:meta.len(),modified:meta.modified().ok(),revoked:AtomicBool::new(false),_actual_path:path}));
        drop(anchor);drop(_pins);Ok(descriptor)
    }
    /// Original on-disk target of an authorized grant, for the explicit
    /// original-save replace protocol only. Never sent to the renderer.
    pub(crate) fn original_path(&self,owner:&str,id:&str)->Result<PathBuf>{
        let grant=self.inner.lock().map_err(|_|AppError::io())?.grants.get(id).filter(|g|g.owner==owner&&!g.revoked.load(Ordering::Acquire)).cloned().ok_or_else(AppError::denied)?;
        Ok(grant._actual_path.clone())
    }
    // Byte-only export authority: pin this exact opened object and its name.
    // No renderer-selected path or source write permission is exposed.
    pub(crate) fn export_source(&self,owner:&str,id:&str)->Result<ExportSource>{
        let grant=self.inner.lock().map_err(|_|AppError::io())?.grants.get(id).filter(|g|g.owner==owner&&!g.revoked.load(Ordering::Acquire)).cloned().ok_or_else(AppError::denied)?;
        if grant.size>EXPORT_INPUT_MAX as u64{return Err(AppError{code:"writer_pdf_budget",message:"当前另存为支持不超过 16 MB 的 PDF；原文件未修改。"});}
        let mut pins=vec![];crate::unicode_worker::pin_chain(grant._actual_path.parent().ok_or_else(AppError::denied)?,&mut pins).map_err(|_|AppError::denied())?;
        // SHARE_WRITE accommodates only our already-held private output handle.
        // The existing grant itself denies every new external writable handle.
        let mut current=OpenOptions::new().read(true).share_mode(1|2).custom_flags(0x00200000).open(&grant._actual_path).map_err(|_|AppError::changed())?;
        let original=grant.file.lock().map_err(|_|AppError::io())?;
        let mut a=windows_sys::Win32::Storage::FileSystem::BY_HANDLE_FILE_INFORMATION::default();let mut b=a;
        if unsafe{windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandle(original.as_raw_handle(),&mut a)}==0||unsafe{windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandle(current.as_raw_handle(),&mut b)}==0{return Err(AppError::io());}
        let metadata=current.metadata().map_err(|_|AppError::io())?;
        if a.dwVolumeSerialNumber!=b.dwVolumeSerialNumber||a.nFileIndexHigh!=b.nFileIndexHigh||a.nFileIndexLow!=b.nFileIndexLow||b.nNumberOfLinks!=1||b.dwFileAttributes&(0x400|0x10)!=0||metadata.len()!=grant.size||metadata.modified().ok()!=grant.modified||final_path(&current)?!=grant._actual_path{return Err(AppError::changed());}
        let mut bytes=vec![];current.by_ref().take(EXPORT_INPUT_MAX as u64+1).read_to_end(&mut bytes).map_err(|_|AppError::io())?;
        if bytes.len()!=grant.size as usize{return Err(AppError::changed());}drop(original);self.live(owner,id)?;
        Ok(ExportSource{bytes,_pins:pins,_file:Some(current)})
    }
    pub(crate) fn register_export(&self,owner:&str,source_id:&str,path:&Path,file:File,expected:&[u8],epoch:u64)->Result<Descriptor>{
        self.register_export_inner(owner,source_id,path,file,expected,epoch,||{})
    }
    fn register_export_inner(&self,owner:&str,source_id:&str,path:&Path,mut file:File,expected:&[u8],epoch:u64,after_writable_close:impl FnOnce())->Result<Descriptor>{
        let actual=final_path(&file)?;let metadata=file.metadata().map_err(|_|AppError::io())?;
        let mut info=windows_sys::Win32::Storage::FileSystem::BY_HANDLE_FILE_INFORMATION::default();
        if unsafe{windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandle(file.as_raw_handle(),&mut info)}==0{return Err(AppError::io());}
        if expected.len()>EXPORT_OUTPUT_MAX||expected.len()as u64>MAX_BYTES{return Err(AppError{code:"writer_pdf_budget",message:"批注副本超过当前写入体积上限；原 PDF 未修改。"});}
        if !pdf_extension(&actual)||actual!=path||info.nNumberOfLinks!=1||info.dwFileAttributes&(0x400|0x10)!=0||metadata.len()!=expected.len()as u64{return Err(AppError::denied());}
        file.seek(SeekFrom::Start(0)).map_err(|_|AppError::io())?;let mut data=vec![];file.by_ref().take(expected.len()as u64+1).read_to_end(&mut data).map_err(|_|AppError::io())?;
        if data!=expected{return Err(AppError::changed());}
        // A retained writable handle makes a later native READ/share_READ|DELETE
        // open fail, even though the app never writes through the retained grant.
        // Keep this exact object pinned while replacing that handle with a
        // read-only one. The intermediate handle refuses DELETE, so the name
        // cannot be replaced while the original writable handle is dropped.
        use std::os::windows::io::FromRawHandle;
        let reopen=|source:&File,share:u32|->Result<File>{
            let handle=unsafe{windows_sys::Win32::Storage::FileSystem::ReOpenFile(source.as_raw_handle(),0x8000_0000,share,0)};
            if handle==windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE{return Err(AppError::changed());}
            Ok(unsafe{File::from_raw_handle(handle)})
        };
        let pinned=reopen(&file,1|2)?;
        drop(file);
        after_writable_close();
        // Acquisition fails if any external writer won the short transition.
        // Once held, the final read grant again denies every new write handle.
        let mut file=reopen(&pinned,1|4)?;
        let mut reopened=windows_sys::Win32::Storage::FileSystem::BY_HANDLE_FILE_INFORMATION::default();
        if unsafe{windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandle(file.as_raw_handle(),&mut reopened)}==0{return Err(AppError::io());}
        if reopened.dwVolumeSerialNumber!=info.dwVolumeSerialNumber||reopened.nFileIndexHigh!=info.nFileIndexHigh||reopened.nFileIndexLow!=info.nFileIndexLow||reopened.nNumberOfLinks!=1||reopened.dwFileAttributes&(0x400|0x10)!=0||final_path(&file)?!=actual{return Err(AppError::changed());}
        let metadata=file.metadata().map_err(|_|AppError::io())?;
        if metadata.len()!=expected.len()as u64{return Err(AppError::changed());}
        file.seek(SeekFrom::Start(0)).map_err(|_|AppError::io())?;
        let mut verified=vec![];file.by_ref().take(expected.len()as u64+1).read_to_end(&mut verified).map_err(|_|AppError::io())?;
        if verified!=expected{return Err(AppError::changed());}
        file.seek(SeekFrom::Start(0)).map_err(|_|AppError::io())?;
        drop(pinned);
        let id=uuid::Uuid::new_v4().to_string();let descriptor=Descriptor{grant_id:id.clone(),name:actual.file_name().ok_or_else(AppError::invalid)?.to_string_lossy().into_owned(),size:metadata.len()};
        let mut inner=self.inner.lock().map_err(|_|AppError::io())?;
        if self.closed.load(Ordering::Acquire)||self.epoch()!=epoch||!inner.grants.get(source_id).is_some_and(|g|g.owner==owner&&!g.revoked.load(Ordering::Acquire)){return Err(AppError::denied());}
        if inner.grants.len()>=MAX_GRANTS{return Err(AppError::busy());}
        inner.grants.insert(id,Arc::new(Grant{owner:owner.into(),file:Mutex::new(file),size:metadata.len(),modified:metadata.modified().ok(),revoked:AtomicBool::new(false),_actual_path:actual}));Ok(descriptor)
    }
    pub(crate) fn directory_live(&self,owner:&str,source:&str,member:&str)->Result<()>{
        let (base,other)={let inner=self.inner.lock().map_err(|_|AppError::io())?;let get=|id:&str|inner.grants.get(id).filter(|g|g.owner==owner&&!g.revoked.load(Ordering::Acquire)).cloned().ok_or_else(AppError::denied);(get(source)?,get(member)?)};
        let actual=final_path(&*base.file.lock().map_err(|_|AppError::io())?)?;let child=final_path(&*other.file.lock().map_err(|_|AppError::io())?)?;
        if actual!=base._actual_path||child.parent()!=actual.parent(){return Err(AppError::changed());}self.live(owner,source)?;self.live(owner,member)
    }
    // Explicit directory opt-in only. Pin a non-reparse ancestry during enumeration;
    // resulting grants keep the selected file objects, not a future path lookup.
    pub(crate) fn directory_members(&self,owner:&str,source:&str)->Result<Vec<Descriptor>>{
        use std::os::windows::fs::MetadataExt;
        let grant=self.inner.lock().map_err(|_|AppError::io())?.grants.get(source).filter(|g|g.owner==owner&&!g.revoked.load(Ordering::Acquire)).cloned().ok_or_else(AppError::denied)?;
        let actual=final_path(&*grant.file.lock().map_err(|_|AppError::io())?)?;
        if actual!=grant._actual_path{return Err(AppError::changed());}
        let parent=actual.parent().ok_or_else(AppError::denied)?;let mut pins=Vec::new();
        for dir in parent.ancestors().collect::<Vec<_>>().into_iter().rev(){
            let f=OpenOptions::new().read(true).share_mode(1).custom_flags(0x0200_0000|0x0020_0000).open(dir).map_err(|_|AppError::io())?;
            if f.metadata().map_err(|_|AppError::io())?.file_attributes()&0x400!=0{return Err(AppError::denied());}pins.push(f);
        }
        let epoch=self.epoch();let mut results=Vec::new();let operation=(||{
            let entries=std::fs::read_dir(parent).map_err(|_|AppError::io())?;
            for (n,entry) in entries.enumerate(){if n>2000{return Err(AppError::busy());}let entry=entry.map_err(|_|AppError::io())?;let path=entry.path();
                if path==actual||!pdf_extension(&path){continue;}let metadata=std::fs::symlink_metadata(&path).map_err(|_|AppError::io())?;
                if !metadata.is_file()||metadata.file_attributes()&0x400!=0{continue;}if results.len()>=16{return Err(AppError::busy());}
                let d=self.authorize_member(owner,&path,parent,epoch)?;
                let child=self.inner.lock().map_err(|_|AppError::io())?.grants.get(&d.grant_id).filter(|g|!g.revoked.load(Ordering::Acquire)).cloned().ok_or_else(AppError::cancelled)?;
                let child_path=final_path(&*child.file.lock().map_err(|_|AppError::io())?)?;
                if child_path.parent()!=Some(parent){let _=self.release(owner,&d.grant_id);return Err(AppError::denied());}
                results.push(d);
            }self.live(owner,source)?;Ok(())
        })();
        if let Err(e)=operation{for d in results{let _=self.release(owner,&d.grant_id);}return Err(e);}drop(pins);Ok(results)
    }
    pub(crate) fn duplicate_grant(&self,owner:&str,id:&str)->Result<Descriptor>{
        use std::os::windows::io::FromRawHandle;
        let epoch=self.epoch();let grant=self.inner.lock().map_err(|_|AppError::io())?.grants.get(id).filter(|g|g.owner==owner&&!g.revoked.load(Ordering::Acquire)).cloned().ok_or_else(AppError::denied)?;
        let handle=unsafe{windows_sys::Win32::Storage::FileSystem::ReOpenFile(grant.file.lock().map_err(|_|AppError::io())?.as_raw_handle(),0x8000_0000,1|2|4,0)};
        if handle==windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE{return Err(AppError::io());}let file=unsafe{File::from_raw_handle(handle)};
        let actual=final_path(&file)?;let next=uuid::Uuid::new_v4().to_string();let descriptor=Descriptor{grant_id:next.clone(),name:actual.file_name().unwrap().to_string_lossy().into_owned(),size:grant.size};
        let mut inner=self.inner.lock().map_err(|_|AppError::io())?;if self.epoch()!=epoch||self.closed.load(Ordering::Acquire)||grant.revoked.load(Ordering::Acquire)||!inner.grants.contains_key(id){return Err(AppError::denied());}if inner.grants.len()>=MAX_GRANTS{return Err(AppError::busy());}
        inner.grants.insert(next,Arc::new(Grant{owner:owner.into(),file:Mutex::new(file),size:grant.size,modified:grant.modified,revoked:AtomicBool::new(false),_actual_path:actual}));Ok(descriptor)
    }
    pub(crate) fn authorize_member(&self,owner:&str,path:&Path,parent:&Path,epoch:u64)->Result<Descriptor>{
        use std::os::windows::fs::MetadataExt;
        let mut file=OpenOptions::new().read(true).share_mode(1|4).custom_flags(0x0020_0000).open(path).map_err(|_|AppError::io())?;
        let meta=file.metadata().map_err(|_|AppError::io())?;let actual=final_path(&file)?;let mut info=windows_sys::Win32::Storage::FileSystem::BY_HANDLE_FILE_INFORMATION::default();
        if unsafe{windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandle(file.as_raw_handle(),&mut info)}==0{return Err(AppError::io());}
        // Validate this exact opened object before reading even a PDF header.
        if actual.parent()!=Some(parent)||meta.file_attributes()&0x400!=0||!meta.is_file()||info.nNumberOfLinks!=1||meta.len()==0||meta.len()>MAX_BYTES{return Err(AppError::denied());}
        let mut prefix=[0;1024];let count=file.read(&mut prefix).map_err(|_|AppError::io())?;if !prefix[..count].windows(5).any(|v|v==b"%PDF-"){return Err(AppError::invalid());}
        file.seek(SeekFrom::Start(0)).map_err(|_|AppError::io())?;let id=uuid::Uuid::new_v4().to_string();
        let descriptor=Descriptor{grant_id:id.clone(),name:actual.file_name().unwrap().to_string_lossy().into_owned(),size:meta.len()};
        let grant=Arc::new(Grant{owner:owner.into(),file:Mutex::new(file),size:meta.len(),modified:meta.modified().ok(),revoked:AtomicBool::new(false),_actual_path:actual});
        let mut inner=self.inner.lock().map_err(|_|AppError::io())?;if self.closed.load(Ordering::Acquire)||self.epoch()!=epoch{return Err(AppError::denied());}if inner.grants.len()>=MAX_GRANTS{return Err(AppError::busy());}inner.grants.insert(id,grant);Ok(descriptor)
    }
    pub(crate) fn with_live<T>(&self,owner:&str,id:&str,epoch:u64,operation:impl FnOnce()->Result<T>)->Result<T> {
        let inner=self.inner.lock().map_err(|_|AppError::io())?;
        if self.closed.load(Ordering::Acquire)||self.epoch()!=epoch||!inner.grants.get(id).is_some_and(|g|g.owner==owner&&!g.revoked.load(Ordering::Acquire)){return Err(AppError::denied());}
        // Short DB transactions only. Release/reload cannot revoke midway and a
        // stale hash task cannot publish a new binding after revocation.
        operation()
    }
    pub(crate) fn live(&self,owner:&str,id:&str)->Result<()> {
        if self.closed.load(Ordering::Acquire){return Err(AppError::denied());}
        let inner=self.inner.lock().map_err(|_|AppError::io())?;
        if !inner.grants.get(id).is_some_and(|g|g.owner==owner&&!g.revoked.load(Ordering::Acquire)){return Err(AppError::denied());}Ok(())
    }
    // All identity observations come from the authorized object, not renderer paths
    // or bytes. Creation time reduces accidental reuse of a retired file ID.
    pub(crate) fn observe(&self,owner:&str,id:&str)->Result<Observation> {
        use sha2::{Digest,Sha256};
        use windows_sys::Win32::Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION,GetFileInformationByHandle,GetFileInformationByHandleEx,FileIdInfo,FILE_ID_INFO};
        let grant=self.inner.lock().map_err(|_|AppError::io())?.grants.get(id)
            .filter(|g|g.owner==owner&&!g.revoked.load(Ordering::Acquire)).cloned().ok_or_else(AppError::denied)?;
        let mut file=grant.file.lock().map_err(|_|AppError::io())?;
        let before=file.metadata().map_err(|_|AppError::io())?;
        if before.len()!=grant.size||before.modified().ok()!=grant.modified{return Err(AppError::changed());}
        let mut info=BY_HANDLE_FILE_INFORMATION::default();
        if unsafe{GetFileInformationByHandle(file.as_raw_handle(),&mut info)}==0{return Err(AppError::io());}
        let mut id_info=FILE_ID_INFO::default();
        if unsafe{GetFileInformationByHandleEx(file.as_raw_handle(),FileIdInfo,(&mut id_info as *mut FILE_ID_INFO).cast(),std::mem::size_of::<FILE_ID_INFO>() as u32)}==0||id_info.FileId.Identifier.iter().all(|b|*b==0){return Err(AppError{code:"document_identity_unavailable",message:"此位置不能提供可靠的 PDF 文件身份，聊天暂不可用。请使用本地磁盘上的 PDF 重新打开；不会按文件名合并历史。"});}
        let object_id=id_info.FileId.Identifier.iter().map(|b|format!("{b:02x}")).collect::<String>();
        let key=format!("win128:{:016x}:{object_id}:{:08x}{:08x}",id_info.VolumeSerialNumber,info.ftCreationTime.dwHighDateTime,info.ftCreationTime.dwLowDateTime);
        file.seek(SeekFrom::Start(0)).map_err(|_|AppError::io())?;
        let mut hash=Sha256::new();let mut total=0u64;let mut block=[0;64*1024];
        loop {
            self.live(owner,id)?;
            let n=file.read(&mut block).map_err(|_|AppError::io())?;
            if n==0{break;}total+=n as u64;if total>MAX_BYTES{return Err(AppError::changed());}hash.update(&block[..n]);
        }
        let after=file.metadata().map_err(|_|AppError::io())?;
        if total!=grant.size||after.len()!=before.len()||after.modified().ok()!=before.modified().ok(){return Err(AppError::changed());}
        self.live(owner,id)?;
        let actual=final_path(&file)?;
        Ok(Observation{key,hash:format!("{:x}",hash.finalize()),name:actual.file_name().ok_or_else(AppError::invalid)?.to_string_lossy().into_owned()})
    }
    // A newly-created test output is registered from the same held handle.
    // Never reopen its name through the native-dialog authorization path.
    #[cfg(debug_assertions)]
    pub(crate) fn register_test_copy(&self,owner:&str,source_id:&str,path:&Path,mut file:File,expected:&[u8],epoch:u64)->Result<Descriptor>{
        use std::os::windows::fs::MetadataExt;
        let actual=final_path(&file)?;
        let base=crate::save_core::project().join("tests/work/m3/writer-copies");
        // Unit tests must not consume the GUI's retained-material budget. This
        // alternate namespace is compiled out of both desktop build variants.
        #[cfg(test)]let base={let parent=path.parent().ok_or_else(AppError::denied)?;let run=parent.parent().ok_or_else(AppError::denied)?;let lab=crate::save_core::project().join("tests/work/m3").canonicalize().map_err(|_|AppError::io())?;let unique=run.file_name().and_then(|v|v.to_str()).and_then(|s|s.strip_prefix("writer-input-")).is_some_and(|s|uuid::Uuid::parse_str(s).is_ok());if parent.file_name().is_some_and(|s|s=="copies")&&unique&&run.parent().is_some_and(|p|p.canonicalize().ok().as_ref()==Some(&lab)){parent.to_owned()}else{base}};
        let base=base.canonicalize().map_err(|_|AppError::io())?;
        let wanted=base.join(path.file_name().ok_or_else(AppError::invalid)?);
        let meta=file.metadata().map_err(|_|AppError::io())?;
        if actual!=wanted||!pdf_extension(&actual)||!meta.is_file()||meta.file_attributes()&0x400!=0
            ||meta.len()!=expected.len()as u64||meta.len()>1024*1024{return Err(AppError::denied());}
        if crate::save_core::bytes(&mut file).map_err(|_|AppError::io())?!=expected{return Err(AppError::changed());}
        let grant_id=uuid::Uuid::new_v4().to_string();
        let descriptor=Descriptor{grant_id:grant_id.clone(),name:path.file_name().unwrap().to_string_lossy().into_owned(),size:meta.len()};
        // The private create/readback handle keeps SHARE_READ protection. Although
        // it was opened writable, no write method or handle is exposed by Files.
        let grant=Arc::new(Grant{owner:owner.into(),file:Mutex::new(file),size:meta.len(),modified:meta.modified().ok(),
            revoked:AtomicBool::new(false),_actual_path:actual});
        let mut inner=self.inner.lock().map_err(|_|AppError::io())?;
        if self.closed.load(Ordering::Acquire)||self.epoch()!=epoch{return Err(AppError::denied());}
        if !inner.grants.get(source_id).is_some_and(|g|g.owner==owner&&!g.revoked.load(Ordering::Acquire)){return Err(AppError::denied());}
        if inner.grants.len()>=MAX_GRANTS{return Err(AppError::busy());}
        inner.grants.insert(grant_id,grant);Ok(descriptor)
    }
    #[cfg(debug_assertions)]
    pub(crate) fn test_source(&self, owner:&str, id:&str)->Result<TestSource> {
        let grant=self.inner.lock().map_err(|_|AppError::io())?.grants.get(id)
            .filter(|g|g.owner==owner&&!g.revoked.load(Ordering::Acquire)).cloned().ok_or_else(AppError::denied)?;
        let base=crate::save_core::project().join("tests/work/m3").canonicalize().map_err(|_|AppError::io())?;
        if !grant._actual_path.starts_with(&base)||grant.size>1024*1024{return Err(AppError::denied());}
        let pins=crate::save_core::pin(grant._actual_path.parent().ok_or_else(AppError::denied)?).map_err(|_|AppError::denied())?;
        let mut old=grant.file.lock().map_err(|_|AppError::io())?;
        // READ-only current handle denies deletion. SHARE_WRITE only permits our
        // already-held private create/readback handle; the older grant denies
        // other writes throughout. DELETE access would conflict with that grant.
        let mut current=OpenOptions::new().read(true).share_mode(1|2).custom_flags(0x00200000)
            .open(&grant._actual_path).map_err(|_|AppError::io())?;
        use std::os::windows::fs::MetadataExt;
        if current.metadata().map_err(|_|AppError::io())?.file_attributes()&0x400!=0{return Err(AppError::denied());}
        if crate::save_core::identity(&mut old).map_err(|_|AppError::io())?!=crate::save_core::identity(&mut current).map_err(|_|AppError::io())?{return Err(AppError::changed());}
        let bytes=crate::save_core::bytes(&mut current).map_err(|_|AppError::io())?;
        Ok(TestSource{bytes,_pins:pins,_file:current})
    }
    // Private native-dialog result only. No command accepts a renderer-supplied path.
    #[cfg(test)]
    fn authorize(&self, owner: &str, path: &Path) -> Result<Descriptor> {
        self.authorize_at(owner, path, self.epoch())
    }
    pub fn epoch(&self) -> u64 { self.epoch.load(Ordering::Acquire) }
    pub fn authorize_at(&self, owner: &str, path: &Path, epoch: u64) -> Result<Descriptor> {
        if self.closed.load(Ordering::Acquire) { return Err(AppError::denied()); }
        let disk_path = matches!(path.components().next(), Some(std::path::Component::Prefix(prefix)) if matches!(prefix.kind(), std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_)));
        if !path.is_absolute() || !disk_path || !pdf_extension(path) { return Err(AppError::invalid()); }
        let mut file = OpenOptions::new().read(true).share_mode(1 | 4).open(path).map_err(|_| AppError::io())?;
        let actual = final_path(&file)?;
        let meta = file.metadata().map_err(|_| AppError::io())?;
        if !pdf_extension(&actual) || !meta.is_file() || meta.len() == 0 || meta.len() > MAX_BYTES { return Err(AppError::invalid()); }
        let mut prefix = [0u8; 1024];
        let count = file.read(&mut prefix).map_err(|_| AppError::io())?;
        if !prefix[..count].windows(5).any(|bytes| bytes == b"%PDF-") { return Err(AppError::invalid()); }
        file.seek(SeekFrom::Start(0)).map_err(|_| AppError::io())?;
        let grant_id = uuid::Uuid::new_v4().to_string();
        let name = path.file_name().ok_or_else(AppError::invalid)?.to_string_lossy().into_owned();
        let descriptor = Descriptor { grant_id: grant_id.clone(), name, size: meta.len() };
        let grant = Arc::new(Grant { owner: owner.into(), file: Mutex::new(file), size: meta.len(),
            modified: meta.modified().ok(), revoked: AtomicBool::new(false), _actual_path: actual });
        let mut inner = self.inner.lock().map_err(|_| AppError::io())?;
        if self.closed.load(Ordering::Acquire) || self.epoch() != epoch { return Err(AppError::denied()); }
        if inner.grants.len() >= MAX_GRANTS { return Err(AppError::busy()); }
        inner.grants.insert(grant_id, grant);
        Ok(descriptor)
    }
    pub fn begin(&self, owner: &str, id: &str) -> Result<String> {
        if self.closed.load(Ordering::Acquire) { return Err(AppError::denied()); }
        let mut inner = self.inner.lock().map_err(|_| AppError::io())?;
        // Expire unused reservations. Running reads remain counted and cancellable.
        inner.tasks.retain(|_, task| task.started.load(Ordering::Acquire) || task.created.elapsed().as_secs() < 30);
        let grant = inner.grants.get(id).filter(|g| g.owner == owner && !g.revoked.load(Ordering::Acquire)).cloned().ok_or_else(AppError::denied)?;
        if inner.tasks.len() >= MAX_TASKS { return Err(AppError::busy()); }
        let task_id = uuid::Uuid::new_v4().to_string();
        inner.tasks.insert(task_id.clone(), Arc::new(Task { owner: owner.into(), grant_id: id.into(), grant,
            cancelled: AtomicBool::new(false), started: AtomicBool::new(false), created: Instant::now() }));
        Ok(task_id)
    }
    pub fn cancel(&self, owner: &str, task_id: &str) -> Result<()> {
        let mut inner = self.inner.lock().map_err(|_| AppError::io())?;
        let task = inner.tasks.get(task_id).filter(|t| t.owner == owner).ok_or_else(AppError::denied)?;
        task.cancelled.store(true, Ordering::Release);
        if !task.started.load(Ordering::Acquire) { inner.tasks.remove(task_id); }
        Ok(())
    }
    pub fn release(&self, owner: &str, id: &str) -> Result<()> {
        let mut inner = self.inner.lock().map_err(|_| AppError::io())?;
        let grant = inner.grants.get(id).filter(|g| g.owner == owner).ok_or_else(AppError::denied)?;
        grant.revoked.store(true, Ordering::Release);
        inner.grants.remove(id);
        inner.tasks.retain(|_, task| {
            if task.grant_id != id { return true; }
            task.cancelled.store(true, Ordering::Release);
            task.started.load(Ordering::Acquire)
        });
        Ok(())
    }
    pub fn read(&self, owner: &str, task_id: &str) -> Result<Vec<u8>> {
        self.read_inner(owner, task_id, || {})
    }
    fn read_inner(&self, owner: &str, task_id: &str, mut after_chunk: impl FnMut()) -> Result<Vec<u8>> {
        let task = {
            let inner = self.inner.lock().map_err(|_| AppError::io())?;
            let task = inner.tasks.get(task_id).filter(|t| t.owner == owner).cloned().ok_or_else(AppError::denied)?;
            if task.started.swap(true, Ordering::AcqRel) { return Err(AppError::denied()); }
            task
        };
        let result = (|| {
            let check = || {
                if self.closed.load(Ordering::Acquire) || task.cancelled.load(Ordering::Acquire) || task.grant.revoked.load(Ordering::Acquire) {
                    Err(AppError::cancelled())
                } else { Ok(()) }
            };
            check()?;
            let mut file = task.grant.file.lock().map_err(|_| AppError::io())?;
            check()?;
            let unchanged = |file: &File| -> Result<()> {
                let meta = file.metadata().map_err(|_| AppError::io())?;
                if meta.len() != task.grant.size || meta.modified().ok() != task.grant.modified { Err(AppError::changed()) } else { Ok(()) }
            };
            unchanged(&file)?;
            file.seek(SeekFrom::Start(0)).map_err(|_| AppError::io())?;
            let mut bytes = Vec::new();
            bytes.try_reserve_exact(task.grant.size as usize).map_err(|_| AppError::busy())?;
            let mut block = [0; 64 * 1024];
            while bytes.len() < task.grant.size as usize {
                check()?;
                let remaining = (task.grant.size as usize - bytes.len()).min(block.len());
                let count = file.read(&mut block[..remaining]).map_err(|_| AppError::io())?;
                if count == 0 { return Err(AppError::changed()); }
                bytes.extend_from_slice(&block[..count]);
                after_chunk();
            }
            unchanged(&file)?; check()?;
            Ok(bytes)
        })();
        if let Ok(mut inner) = self.inner.lock() { inner.tasks.remove(task_id); }
        result
    }
    pub fn shutdown(&self) {
        self.closed.store(true, Ordering::Release);
        self.revoke_all();
    }
    pub fn revoke_all(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            self.epoch.fetch_add(1, Ordering::AcqRel);
            for task in inner.tasks.values() { task.cancelled.store(true, Ordering::Release); }
            for grant in inner.grants.values() { grant.revoked.store(true, Ordering::Release); }
            inner.tasks.clear(); inner.grants.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn wait_handoff(path:&Path) {
        let start=Instant::now();while !path.exists(){assert!(start.elapsed()<std::time::Duration::from_secs(8),"private handoff actor timeout");std::thread::sleep(std::time::Duration::from_millis(10));}
    }
    #[test]#[ignore="private disposable handoff actor"]
    fn original_handoff_actor() {
        use std::io::Write;
        let root=PathBuf::from(std::env::var_os("M3_HANDOFF_ROOT").unwrap());crate::save_core::allowed(&root).unwrap();
        let mut file=OpenOptions::new().append(true).share_mode(7).open(root.join("original.pdf")).unwrap();
        file.write_all(b"\n% gap-external\n").unwrap();file.sync_all().unwrap();
        if std::env::var("M3_HANDOFF_ACTIVE").as_deref()==Ok("1") {
            std::fs::write(root.join("writer.ready"),b"holding").unwrap();wait_handoff(&root.join("writer.release"));
        }
    }
    #[test]
    fn original_handoff_gap_rejects_finished_and_still_open_external_writes() {
        use std::process::{Command,Stdio};
        for active in [false,true] {
            let root=crate::save_core::project().join("tests/work/m3").join(format!("handoff-{}",uuid::Uuid::new_v4()));std::fs::create_dir(&root).unwrap();
            let path=root.join("original.pdf");let before=b"%PDF-1.7\ncontrolled handoff bytes\n%%EOF\n";std::fs::write(&path,before).unwrap();
            let files=Files::default();let grant=files.authorize("main",&path).unwrap();let mut held=None;
            let result=files.take_original_write_inner("main",&grant.grant_id,files.epoch(),&crate::save_core::hash(before),||{
                // This is the EXACT interval after the denying reader closes
                // and before ReOpenFile obtains the guarded writable handle.
                let mut child=Command::new(std::env::current_exe().unwrap()).args(["--exact","files::tests::original_handoff_actor","--ignored"]).env("M3_HANDOFF_ROOT",&root).env("M3_HANDOFF_ACTIVE",if active{"1"}else{"0"}).stdout(Stdio::null()).spawn().unwrap();
                if active{wait_handoff(&root.join("writer.ready"));held=Some(child);}else{assert!(child.wait().unwrap().success());}
            });
            if let Some(mut child)=held {std::fs::write(root.join("writer.release"),b"release").unwrap();assert!(child.wait().unwrap().success());}
            assert_eq!(result.err().unwrap().code,"writer_original_reopen_required");
            assert!(files.live("main",&grant.grant_id).is_err(),"a closed reader must not survive as a permissive token");
            assert_eq!(std::fs::read(&path).unwrap(),[before.as_slice(),b"\n% gap-external\n"].concat(),"external writer bytes must be preserved");
            let reopened=files.authorize("main",&path).unwrap();assert_ne!(reopened.grant_id,grant.grant_id);
        }
    }
    #[test] fn physical_identity_rename_copy_and_revocation(){
        let files=Files::default();let path=fixture();let original=files.authorize("main",&path).unwrap();
        let first=files.observe("main",&original.grant_id).unwrap();
        assert!(files.observe("other",&original.grant_id).is_err());
        let moved=path.with_file_name("重命名.pdf");std::fs::rename(&path,&moved).unwrap();
        let second=files.authorize("main",&moved).unwrap();let observed=files.observe("main",&second.grant_id).unwrap();
        assert_eq!(first.key,observed.key);assert_eq!(first.hash,observed.hash);
        let copy=moved.with_file_name("副本.pdf");std::fs::copy(&moved,&copy).unwrap();
        let third=files.authorize("main",&copy).unwrap();let duplicate=files.observe("main",&third.grant_id).unwrap();
        assert_ne!(first.key,duplicate.key);assert_eq!(first.hash,duplicate.hash);
        files.revoke_all();assert!(files.observe("main",&second.grant_id).is_err());
    }
    fn fixture() -> PathBuf {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("tests/work/m2-b/rust-tests").join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("中文 空格.pdf"); std::fs::write(&path, b"%PDF-1.7\ncontrolled bytes").unwrap(); path
    }
    #[test] fn export_readonly_transition_refuses_external_changes_and_keeps_the_name_pinned(){
        for active_writer in [false,true]{
            let path=fixture();let files=Files::default();let source=files.authorize("main",&path).unwrap();
            let target=path.with_file_name("批注副本.pdf");let expected=std::fs::read(&path).unwrap();
            let mut output=OpenOptions::new().read(true).write(true).create_new(true).share_mode(1).open(&target).unwrap();
            use std::io::Write;output.write_all(&expected).unwrap();output.sync_all().unwrap();
            let canonical=target.canonicalize().unwrap();let mut pins=vec![];crate::unicode_worker::pin_chain(canonical.parent().unwrap(),&mut pins).unwrap();
            let mut held=None;
            let result=files.register_export_inner("main",&source.grant_id,&canonical,output,&expected,files.epoch(),||{
                // The intermediate exact-object handle forbids replacement even
                // though its sharing must momentarily accommodate the old writer.
                assert!(std::fs::rename(&target,target.with_extension("moved.pdf")).is_err());
                if active_writer {held=Some(OpenOptions::new().write(true).share_mode(1|2|4).open(&target).unwrap());}
                else {let mut other=OpenOptions::new().write(true).share_mode(1|2|4).open(&target).unwrap();other.write_all(b"%PDF-1.7\nexternal edit!!!").unwrap();other.sync_all().unwrap();}
            });
            assert_eq!(result.err().unwrap(),AppError::changed());
            assert_eq!(files.inner.lock().unwrap().grants.len(),1,"no unverified export grant may be published");
            assert_eq!(std::fs::read(&path).unwrap(),expected,"the source is never modified");
            drop(held);drop(pins);
        }
    }
    #[test] fn export_registration_accepts_the_writer_output_budget_then_uses_normal_reader_limits(){
        let path=fixture();let files=Files::default();let source=files.authorize("main",&path).unwrap();
        let target=path.with_file_name("预算边界副本.pdf");
        // This tests registration size accounting only, not PDF parse validity.
        // Real writer output/parse evidence remains in pdf_export's writer cases.
        let mut expected=vec![b' ';EXPORT_INPUT_MAX+1];expected[..9].copy_from_slice(b"%PDF-1.7\n");
        use std::io::Write;let mut output=OpenOptions::new().read(true).write(true).create_new(true).share_mode(1).open(&target).unwrap();output.write_all(&expected).unwrap();output.sync_all().unwrap();
        let grant=files.register_export("main",&source.grant_id,&target.canonicalize().unwrap(),output,&expected,files.epoch()).unwrap();
        let native=files.authorize("main",&target).unwrap();let task=files.begin("main",&native.grant_id).unwrap();
        assert_eq!(files.read("main",&task).unwrap(),expected);
        assert_eq!(files.export_source("main",&grant.grant_id).err().unwrap().code,"writer_pdf_budget","a larger readable output still respects the separate next-save input budget");
        assert!(OpenOptions::new().write(true).open(&target).is_err());
    }
    #[test] fn grant_owner_tasks_and_revocation() {
        let files = Files::default(); let path = fixture();
        let d = files.authorize("main", &path).unwrap(); assert_eq!(d.name, "中文 空格.pdf");
        assert!(files.begin("main", "forged").is_err()); assert!(files.begin("foreign", &d.grant_id).is_err());
        let task = files.begin("main", &d.grant_id).unwrap(); assert!(files.read("foreign", &task).is_err());
        assert!(files.cancel("foreign", &task).is_err());
        assert_eq!(files.read("main", &task).unwrap(), std::fs::read(&path).unwrap());
        assert!(files.read("main", &task).is_err()); files.release("main", &d.grant_id).unwrap();
        assert!(files.begin("main", &d.grant_id).is_err());
        assert!(OpenOptions::new().write(true).open(path).is_ok());
    }
    #[test] fn cancel_before_read_shutdown_and_limits() {
        let files = Files::default(); let d = files.authorize("main", &fixture()).unwrap();
        let a = files.begin("main", &d.grant_id).unwrap(); let b = files.begin("main", &d.grant_id).unwrap();
        assert_eq!(files.begin("main", &d.grant_id).unwrap_err(), AppError::busy());
        files.cancel("main", &a).unwrap(); assert!(files.read("main", &a).is_err());
        files.release("main", &d.grant_id).unwrap(); assert!(files.read("main", &b).is_err());
        let d = files.authorize("main", &fixture()).unwrap(); files.shutdown();
        assert!(files.begin("main", &d.grant_id).is_err()); assert!(files.authorize("main", &fixture()).is_err());
    }
    #[test] fn actual_handle_survives_path_replacement_without_reading_new_target() {
        let files = Files::default(); let path = fixture(); let original = std::fs::read(&path).unwrap();
        let d = files.authorize("main", &path).unwrap(); assert!(OpenOptions::new().write(true).open(&path).is_err());
        std::fs::rename(&path, path.with_extension("old.pdf")).unwrap(); std::fs::write(&path, b"%PDF-1.7\nreplacement").unwrap();
        let task = files.begin("main", &d.grant_id).unwrap(); assert_eq!(files.read("main", &task).unwrap(), original);
    }
    #[test] fn rejects_empty_malformed_directory_and_oversize() {
        let files = Files::default(); let path = fixture(); std::fs::write(&path, b"").unwrap(); assert!(files.authorize("main", &path).is_err());
        std::fs::write(&path, b"untrusted").unwrap(); assert!(files.authorize("main", &path).is_err());
        let big = File::create(&path).unwrap(); big.set_len(MAX_BYTES+1).unwrap(); drop(big); assert!(files.authorize("main", &path).is_err());
        assert!(files.authorize("main", path.parent().unwrap()).is_err());
        std::fs::remove_file(path).unwrap();
    }
    #[test] fn actual_running_read_blocks_write_until_cancelled_task_handle_is_released() {
        let files=Files::default();let path=fixture();let d=files.authorize("main",&path).unwrap();
        let task=files.begin("main",&d.grant_id).unwrap();
        assert!(OpenOptions::new().read(true).write(true).share_mode(1).open(&path).is_err());
        let mut held_after_release=false;
        assert!(files.read_inner("main",&task,||{
            files.release("main",&d.grant_id).unwrap();
            held_after_release=OpenOptions::new().read(true).write(true).share_mode(1).open(&path).is_err();
        }).is_err());
        assert!(held_after_release,"running read owns its handle even after grant revocation");
        assert!(files.begin("main",&d.grant_id).is_err());
        let writer=OpenOptions::new().read(true).write(true).share_mode(1).open(&path).unwrap();drop(writer);
        let reopened=files.authorize("main",&path).unwrap();assert_ne!(reopened.grant_id,d.grant_id);
    }
    #[test] fn cancel_or_revoke_during_read_releases_running_task() {
        for revoke in [false, true] {
            let files = Files::default(); let path = fixture();
            let mut bytes = vec![b'x'; 256 * 1024]; bytes[..8].copy_from_slice(b"%PDF-1.7");
            std::fs::write(&path, bytes).unwrap();
            let d = files.authorize("main", &path).unwrap(); let task = files.begin("main", &d.grant_id).unwrap();
            let result = files.read_inner("main", &task, || {
                if revoke { files.release("main", &d.grant_id).unwrap(); }
                else { files.cancel("main", &task).unwrap(); }
            });
            assert_eq!(result.unwrap_err(), AppError::cancelled());
            assert!(files.inner.lock().unwrap().tasks.is_empty());
            if !revoke { files.release("main", &d.grant_id).unwrap(); }
            assert!(OpenOptions::new().write(true).open(&path).is_ok());
        }
    }
    #[test] fn reload_revokes_old_grants_and_late_dialog_results_without_closing_registry() {
        let files = Files::default(); let path = fixture();
        for _ in 0..40 {
            let epoch = files.epoch(); let d = files.authorize_at("main", &path, epoch).unwrap();
            let task = files.begin("main", &d.grant_id).unwrap();
            files.revoke_all();
            assert!(files.begin("main", &d.grant_id).is_err()); assert!(files.read("main", &task).is_err());
            assert!(files.authorize_at("main", &path, epoch).is_err());
            assert!(files.inner.lock().unwrap().grants.is_empty());
            assert!(OpenOptions::new().write(true).open(&path).is_ok());
        }
    }
    #[test] fn junction_target_changes_do_not_redirect_existing_grants() {
        use std::os::windows::process::CommandExt;
        let path = fixture(); let root = path.parent().unwrap();
        let first = root.join("first"); let second = root.join("second"); let junction = root.join("selected-folder");
        std::fs::create_dir(&first).unwrap(); std::fs::create_dir(&second).unwrap();
        std::fs::write(first.join("selected.pdf"), b"%PDF-1.7 first target").unwrap();
        std::fs::write(second.join("selected.pdf"), b"%PDF-1.7 other target").unwrap();
        let link = |target: &Path| {
            // Only this test's UUID directory. No arbitrary command is exposed by the app.
            let command = format!("mklink /J \"{}\" \"{}\"", junction.display().to_string().replace('/', "\\"), target.display().to_string().replace('/', "\\"));
            let output = std::process::Command::new("cmd.exe").args(["/d", "/c"]).raw_arg(command)
                .output().unwrap(); assert!(output.status.success(), "junction creation: {:?} / {:?}", output.status, output.stderr);
        };
        link(&first);
        let files = Files::default(); let d = files.authorize("main", &junction.join("selected.pdf")).unwrap();
        assert!(files.inner.lock().unwrap().grants[&d.grant_id]._actual_path.ends_with("first/selected.pdf"));
        // remove_dir removes this junction itself, without recursion into its target.
        std::fs::remove_dir(&junction).unwrap(); link(&second);
        let task = files.begin("main", &d.grant_id).unwrap(); assert_eq!(files.read("main", &task).unwrap(), b"%PDF-1.7 first target");
        files.release("main", &d.grant_id).unwrap();
        let d = files.authorize("main", &junction.join("selected.pdf")).unwrap();
        let task = files.begin("main", &d.grant_id).unwrap(); assert_eq!(files.read("main", &task).unwrap(), b"%PDF-1.7 other target");
        files.shutdown(); std::fs::remove_dir(&junction).unwrap();
        assert!(first.join("selected.pdf").is_file()); assert!(second.join("selected.pdf").is_file());
    }
    #[test] fn nonexistent_relative_network_and_device_paths_are_rejected() {
        let files = Files::default();
        let missing = fixture().with_file_name("not-present.pdf"); assert!(files.authorize("main", &missing).is_err());
        for path in ["relative.pdf", "\\\\server\\share\\a.pdf", "\\\\.\\pipe\\a.pdf"] { assert!(files.authorize("main", Path::new(path)).is_err()); }
    }
}
