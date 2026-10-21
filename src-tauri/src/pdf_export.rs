//! Authorized Save As and the controlled original-save protocol. No renderer
//! paths are accepted: the renderer supplies bytes only, and every byte relation
//! (original prefix, appended revision, hashes) is re-checked here.
use std::{collections::HashMap,fs::{self,OpenOptions},io::Write,os::windows::fs::OpenOptionsExt,path::{Path,PathBuf},sync::{Arc,Mutex,atomic::{AtomicBool,Ordering}},time::Instant};
use crate::{files::{Files,Descriptor,ExportSource},error::{AppError,Result}};
const MAX:usize=crate::files::EXPORT_INPUT_MAX;
// One appended revision may add a subset font on top of the candidate bytes.
const MAX_OUTPUT:usize=crate::files::EXPORT_OUTPUT_MAX;
fn error(code:&'static str,message:&'static str)->AppError{AppError{code,message}}
pub(crate) fn isolation_error()->AppError{error("writer_isolation_blocked","Writer 隔离检查未通过。请保留错误并在 Sandbox 中验证，原 PDF 未修改。")}
fn standards_error()->AppError{error("writer_standards_blocked","Writer 输出未通过 PDF 标准结构检查，未导出。原 PDF 与恢复稿保留。")}
fn conversion_error()->AppError{error("writer_conversion_refused","批注副本必须基于本次授权的原文件增量生成；未另存，原 PDF 未修改。")}
fn changed_error()->AppError{error("writer_source_changed","原文件在打开后被修改、占用或被替换；未写入。请重新打开该文件后再保存。")}
fn create_output_error(cause:std::io::Error)->AppError{
    match cause.raw_os_error(){
        Some(80|183)=>error("writer_target_exists","所选文件已存在；未覆盖。请换一个文件名另存。"),
        Some(5)=>error("writer_target_access_denied","所选位置不可写；原 PDF 未修改，请选择有写入权限的文件夹。"),
        Some(112|39)=>error("writer_target_disk_full","所选磁盘空间不足；原 PDF 未修改，请保留恢复稿并选择其他位置。"),
        Some(32|33)=>error("writer_target_locked","所选位置被其他程序占用；原 PDF 未修改，请选择新文件名或稍后重试。"),
        _=>error("writer_export_failed","无法创建批注副本；原 PDF 和恢复稿保留，请选择其他位置重试。"),
    }
}
/// The renderer may only append: the authorized original bytes must be a prefix
/// of the candidate, and the candidate a prefix of the writer output, whose
/// appended region must carry a real cross-reference section and a trailer.
pub(crate) struct Prepared{pub bytes:Vec<u8>,pub source_hash:String,pub candidate_hash:String}
fn contains(haystack:&[u8],needle:&[u8])->bool{needle.len()<=haystack.len()&&haystack.windows(needle.len()).any(|window|window==needle)}
fn appended_revision(prefix:&[u8],output:&[u8])->bool{
    if output.len()<=prefix.len()||!output.starts_with(prefix){return false;}
    let tail=&output[prefix.len()..];
    let trailer=tail.ends_with(b"%%EOF")||tail.ends_with(b"%%EOF\n")||tail.ends_with(b"%%EOF\r\n");
    trailer&&contains(tail,b"startxref")&&(contains(tail,b"\nxref")||contains(tail,b"/Type/XRef")||contains(tail,b"/Type /XRef"))
}
pub(crate) struct Task{owner:String,pub(crate) grant:String,pub(crate) epoch:u64,created:Instant,cancel:AtomicBool,running:AtomicBool,source:Mutex<Option<ExportSource>>}
pub(crate) struct Service{pub root:PathBuf,tasks:Mutex<HashMap<String,Arc<Task>>>,closed:AtomicBool,pub checking:AtomicBool,pub check_cancel:AtomicBool,verified_assets:Mutex<bool>}
impl Service{
    pub fn new(root:PathBuf)->Self{Self{root,tasks:Mutex::new(HashMap::new()),closed:AtomicBool::new(false),checking:AtomicBool::new(false),check_cancel:AtomicBool::new(false),verified_assets:Mutex::new(false)}}
    /// Only immutable embedded assets may reuse a successful check. The state
    /// belongs to this app instance, not a file path or a persistent cache.
    /// A failed lookup/check is retried; concurrent first calls share the lock.
    pub(crate) fn check_assets(&self,immutable:bool,fetch:&dyn Fn(&str)->Option<Vec<u8>>)->Result<()>{
        if !immutable{return verify_writer_assets(fetch);}
        let mut verified=self.verified_assets.lock().map_err(|_|AppError::io())?;
        if !*verified{verify_writer_assets(fetch)?;*verified=true;}
        Ok(())
    }
    pub fn reserve(&self,files:&Files,owner:&str,grant:&str)->Result<String>{
        if self.closed.load(Ordering::Acquire){return Err(AppError::denied());}
        let epoch=files.epoch();let source=files.export_source(owner,grant)?;
        files.with_live(owner,grant,epoch,||Ok(()))?;
        let mut tasks=self.tasks.lock().map_err(|_|AppError::io())?;
        tasks.retain(|_,t|t.running.load(Ordering::Acquire)||t.created.elapsed().as_secs()<30);
        if !tasks.is_empty(){return Err(AppError::busy());}
        let id=uuid::Uuid::new_v4().to_string();tasks.insert(id.clone(),Arc::new(Task{owner:owner.into(),grant:grant.into(),epoch,created:Instant::now(),cancel:AtomicBool::new(false),running:AtomicBool::new(false),source:Mutex::new(Some(source))}));Ok(id)
    }
    pub fn cancel(&self,owner:&str,id:Option<&str>){if let Ok(mut tasks)=self.tasks.lock(){tasks.retain(|key,t|{if t.owner==owner&&id.is_none_or(|wanted|wanted==key){t.cancel.store(true,Ordering::Release);t.running.load(Ordering::Acquire)}else{true}});}}
    pub fn cancel_grant(&self,owner:&str,grant:&str){if let Ok(mut tasks)=self.tasks.lock(){tasks.retain(|_,t|{if t.owner==owner&&t.grant==grant{t.cancel.store(true,Ordering::Release);t.running.load(Ordering::Acquire)}else{true}});}}
    pub fn close(&self){self.closed.store(true,Ordering::Release);self.check_cancel.store(true,Ordering::Release);self.cancel("main",None);}
    pub fn prepare(&self,files:&Files,owner:&str,id:&str,candidate:&[u8],output:&[u8])->Result<(Prepared,Arc<Task>,ExportSource)>{
        let task=self.tasks.lock().map_err(|_|AppError::io())?.get(id).filter(|t|t.owner==owner).cloned().ok_or_else(AppError::denied)?;
        if task.running.swap(true,Ordering::AcqRel){return Err(AppError::busy());}
        if task.cancel.load(Ordering::Acquire){self.finish(id);return Err(AppError::cancelled());}
        if candidate.len()>MAX||output.len()>MAX_OUTPUT{self.finish(id);return Err(error("writer_pdf_budget","批注副本超过当前写入体积上限；原 PDF 未修改。"));}
        let result=(||{
            files.with_live(owner,&task.grant,task.epoch,||Ok(()))?;
            let source=task.source.lock().map_err(|_|AppError::io())?.take().ok_or_else(AppError::denied)?;
            // The renderer serialization must be an incremental revision of the
            // authorized original, and the writer output an appended revision of
            // that candidate. Both relations are byte-exact, so the original
            // bytes cannot be altered or dropped by the renderer or the writer.
            if !candidate.starts_with(&source.bytes){return Err(conversion_error());}
            if !appended_revision(candidate,output){return Err(standards_error());}
            if task.cancel.load(Ordering::Acquire){return Err(AppError::cancelled());}
            files.with_live(owner,&task.grant,task.epoch,||Ok(()))?;
            Ok((Prepared{bytes:output.to_vec(),source_hash:crate::save_core::hash(&source.bytes),candidate_hash:crate::save_core::hash(candidate)},task.clone(),source))
        })();
        if result.is_err(){self.finish(id);}result
    }
    fn finish(&self,id:&str){if let Ok(mut tasks)=self.tasks.lock(){tasks.remove(id);}}
    pub fn commit_new(&self,files:&Files,owner:&str,id:&str,path:&Path,prepared:Prepared,task:Arc<Task>,_source:ExportSource)->Result<Descriptor>{
        let result=(||{
            if task.owner!=owner||task.cancel.load(Ordering::Acquire)||self.closed.load(Ordering::Acquire){return Err(AppError::cancelled());}
            files.with_live(owner,&task.grant,task.epoch,||Ok(()))?;
            if prepared.bytes.len()>MAX_OUTPUT{return Err(error("writer_pdf_budget","批注副本超过当前写入体积上限；原 PDF 未修改。"));}
            if crate::save_core::hash(&_source.bytes)!=prepared.source_hash{return Err(standards_error());}
            let supplied=output_path(path)?;let mut pins=vec![];let parent=supplied.parent().ok_or_else(AppError::denied)?;
            // Reject every original ancestor before any canonicalization can
            // erase a junction or redirect the native-dialog choice elsewhere.
            crate::unicode_worker::pin_chain(parent,&mut pins).map_err(|_|AppError::denied())?;
            let target=fs::canonicalize(parent).map_err(|_|AppError::denied())?.join(supplied.file_name().ok_or_else(AppError::denied)?);
            // Never overwrite: selecting the source, any existing file, a
            // link or an external racing creation is refused by CREATE_NEW.
            let mut file=OpenOptions::new().read(true).write(true).create_new(true).share_mode(1).custom_flags(0x00200000).open(&target).map_err(create_output_error)?;
            file.write_all(&prepared.bytes).and_then(|_|file.sync_all()).map_err(|_|error("writer_export_failed","副本写入未完成，新文件可能不完整；原 PDF 和恢复稿保留，请使用新的文件名重试。"))?;
            if task.cancel.load(Ordering::Acquire){return Err(error("writer_export_cancelled","导出已取消；可能已产生独立副本，原 PDF 未修改。"));}
            let descriptor=files.register_export(owner,&task.grant,&target,file,&prepared.bytes,task.epoch)?;
            Ok(descriptor)
        })();self.finish(id);result
    }
    pub(crate) fn commit_guarded_original(&self,files:&Files,owner:&str,id:&str,prepared:Prepared,task:Arc<Task>,source:ExportSource)->std::result::Result<crate::original_commit::CommitOutcome,crate::original_commit::SaveError>{
        let result=(||{
            if task.owner!=owner||task.cancel.load(Ordering::Acquire)||self.closed.load(Ordering::Acquire){return Err(AppError::cancelled().into());}
            let approved=crate::original_commit::ApprovedOutput::checked(&source.bytes,prepared)?;
            if !crate::original_commit::pending(files,owner,&task.grant)?.is_empty(){return Err(error("writer_recovery_required","这份 PDF 有未完成保存材料；请先检查恢复副本。未覆盖原件。").into());}
            let path=files.original_path(owner,&task.grant)?;
            let parent=path.parent().ok_or_else(AppError::denied)?.to_owned();
            // Release the captured export handles only after validation. The
            // grant-to-strict-writer handoff itself keeps a non-delete anchor.
            drop(source);
            crate::original_commit::commit_approved(files,owner,&task.grant,task.epoch,approved,&parent).map_err(crate::original_commit::SaveError::from)
        })();self.finish(id);result
    }
    /// Legacy replacement remains disabled; only the guarded append is wired.
    /// replacement protocol below is not a production safety guarantee.
    pub fn commit_original(&self,files:&Files,owner:&str,id:&str,prepared:Prepared,task:Arc<Task>,_source:ExportSource)->Result<&'static str>{
        // The legacy hash-then-ReplaceFileW experiment has a demonstrated race
        // once its denying handles are closed. A confirmation dialog cannot make
        // that protocol safe. Keep it available only to disposable unit tests.
        let _=(files,owner,prepared,task,_source);
        self.finish(id);
        Err(error("writer_original_disabled","原件保存尚未通过并发修改保护，已拒绝写入；批注和恢复稿保留，请使用另存为批注副本。"))
    }
    #[cfg(test)]
    pub(crate) fn commit_original_in(&self,files:&Files,owner:&str,id:&str,prepared:Prepared,task:Arc<Task>,mut _source:ExportSource,journal:&Path)->Result<&'static str>{
        let result=(||{
            if task.owner!=owner||task.cancel.load(Ordering::Acquire)||self.closed.load(Ordering::Acquire){return Err(AppError::cancelled());}
            files.with_live(owner,&task.grant,task.epoch,||Ok(()))?;
            let new_hash=crate::save_core::hash(&prepared.bytes);
            if crate::save_core::hash(&_source.bytes)!=prepared.source_hash||new_hash==prepared.source_hash{return Err(standards_error());}
            let path=files.original_path(owner,&task.grant)?;
            let parent=path.parent().ok_or_else(AppError::denied)?.to_owned();
            let mut pins=vec![];crate::unicode_worker::pin_chain(&parent,&mut pins).map_err(|_|AppError::denied())?;
            // External modification and replacement are both rejected by byte hash.
            let current=fs::read(&path).map_err(|_|changed_error())?;
            if current.len()!=_source.bytes.len()||crate::save_core::hash(&current)!=prepared.source_hash{return Err(changed_error());}
            let stem=path.file_stem().map(|value|value.to_string_lossy().into_owned()).ok_or_else(AppError::denied)?;
            if stem.is_empty(){return Err(AppError::denied());}
            let pending=parent.join(format!(".{stem}.pending-{}.pdf",uuid::Uuid::new_v4()));
            let backup=unique_backup(&parent,&parent.join(format!("{stem}.backup.pdf")))?;
            let mut handle=OpenOptions::new().write(true).create_new(true).share_mode(0).custom_flags(0x00200000).open(&pending)
                .map_err(|_|error("writer_export_failed","无法创建写入用临时文件；原 PDF 未修改，请重试。"))?;
            handle.write_all(&prepared.bytes).and_then(|_|handle.sync_all()).map_err(|_|error("writer_export_failed","临时文件写入未完成；原 PDF 未修改。"))?;
            drop(handle);
            let record=write_journal(journal,serde_json::json!({"schema":1,"state":"prepared","target":path.to_string_lossy(),"pending":pending.to_string_lossy(),"backup":backup.to_string_lossy(),"sourceHash":prepared.source_hash,"newHash":new_hash}))?;
            if task.cancel.load(Ordering::Acquire){let _=fs::remove_file(&pending);return Err(AppError::cancelled());}
            // Both handles on the original (the captured source and the reader
            // grant) must go before the atomic replace; that is why the caller
            // must reopen the document afterwards. The directory pins stay held.
            _source.close_all();
            pins.clear();
            let _=files.release(owner,&task.grant);
            replace_file(&path,&pending,&backup,&prepared.source_hash).map_err(|cause|match cause.code{
                "writer_source_changed"=>cause,
                _=>error("writer_export_failed","替换原文件失败；原 PDF 与临时文件均保留，请用备份与临时文件恢复。"),
            })?;
            let written=fs::read(&path).map_err(|_|error("writer_export_failed","写入后无法校验原文件；请用备份恢复。"))?;
            if written.len()!=prepared.bytes.len()||crate::save_core::hash(&written)!=new_hash{
                return Err(error("writer_export_failed","写入后的原文件与预期不一致；请保留文件与备份用于恢复。"));
            }
            let _=complete_journal(&record,&new_hash);
            Ok("原件已安全写入：已生成备份并用临时文件原子替换。请重新打开该文件以刷新授权。")
        })();
        self.finish(id);result
    }
    pub fn abandon(&self,id:&str){self.finish(id);}
}
fn journal_dir()->Result<PathBuf>{
    let dir=crate::worker_sandbox::local_app_data().map_err(|_|AppError::io())?.join("writer-journal");
    fs::create_dir_all(&dir).map_err(|_|AppError::io())?;Ok(dir)
}
#[cfg(test)]fn write_journal(dir:&Path,value:serde_json::Value)->Result<PathBuf>{
    fs::create_dir_all(dir).map_err(|_|AppError::io())?;
    let path=dir.join(format!("{}.json",uuid::Uuid::new_v4()));
    let mut file=OpenOptions::new().write(true).create_new(true).custom_flags(0x00200000).open(&path).map_err(|_|AppError::io())?;
    file.write_all(&serde_json::to_vec(&value).map_err(|_|AppError::io())?).and_then(|_|file.sync_all()).map_err(|_|AppError::io())?;Ok(path)
}
#[cfg(test)]fn complete_journal(path:&Path,new_hash:&str)->Result<()>{
    let mut value:serde_json::Value=serde_json::from_slice(&fs::read(path).map_err(|_|AppError::io())?).map_err(|_|AppError::io())?;
    value["state"]=serde_json::json!("completed");value["verifiedHash"]=serde_json::json!(new_hash);
    fs::write(path,serde_json::to_vec(&value).map_err(|_|AppError::io())?).map_err(|_|AppError::io())
}
/// A fresh backup name next to the target; an existing backup is never reused.
#[cfg(test)]fn unique_backup(parent:&Path,preferred:&Path)->Result<PathBuf>{
    if !preferred.exists(){return Ok(preferred.to_owned());}
    let stem=preferred.file_stem().map(|value|value.to_string_lossy().into_owned()).unwrap_or_default();
    for index in 1..100{let candidate=parent.join(format!("{stem}-{index}.pdf"));if !candidate.exists(){return Ok(candidate);}}
    Err(AppError::denied())
}
#[cfg(test)]fn wide(path:&Path)->Vec<u16>{use std::os::windows::ffi::OsStrExt;path.as_os_str().encode_wide().chain(Some(0)).collect()}
/// One atomic swap: the verified original moves to a fresh backup name and the
/// pending bytes become the target. Every handle on the original, including the
/// captured source and the parent pins, must be released first, otherwise
/// ReplaceFileW fails with 1175 (unable to remove the replaced file).
#[cfg(test)]fn replace_file(target:&Path,pending:&Path,backup:&Path,expected_hash:&str)->Result<()>{
    replace_file_after_check(target,pending,backup,expected_hash,||{})
}
#[cfg(test)]fn replace_file_after_check(target:&Path,pending:&Path,backup:&Path,expected_hash:&str,after_check:impl FnOnce())->Result<()>{
    // Fail closed on any earlier external change before the swap.
    if crate::save_core::hash(&fs::read(target).map_err(|_|changed_error())?)!=expected_hash{return Err(changed_error());}
    after_check();
    // One atomic operation: the verified original moves to the fresh backup name
    // and the pending bytes become the file. The caller re-opens the document.
    let(wide_target,wide_pending,wide_backup)=(wide(target),wide(pending),wide(backup));
    if unsafe{windows_sys::Win32::Storage::FileSystem::ReplaceFileW(wide_target.as_ptr(),wide_pending.as_ptr(),wide_backup.as_ptr(),0,std::ptr::null(),std::ptr::null())}==0{return Err(AppError::io());}
    // The backup must hold exactly the authorized revision, never mixed content.
    if crate::save_core::hash(&fs::read(backup).map_err(|_|AppError::io())?)!=expected_hash{return Err(AppError::io());}
    Ok(())
}
/// Startup recovery: report every interrupted original save and mark its state
/// from the actual bytes on disk. Never writes to the user's PDF.
pub(crate) fn recover_pending()->Vec<serde_json::Value>{
    match journal_dir(){Ok(dir)=>recover_pending_in(&dir),Err(_)=>Vec::new()}
}
pub(crate) fn recover_pending_in(dir:&Path)->Vec<serde_json::Value>{
    let mut resolved=Vec::new();let Ok(entries)=fs::read_dir(dir)else{return resolved;};
    for entry in entries.flatten(){
        let path=entry.path();if path.extension().is_none_or(|value|!value.eq_ignore_ascii_case("json")){continue;}
        let Ok(bytes)=fs::read(&path)else{continue;};if bytes.len()>8192{continue;}
        let Ok(mut value)=serde_json::from_slice::<serde_json::Value>(&bytes)else{continue;};
        // Only an unresolved record is inspected; a resolved one keeps its verdict.
        if value["state"]!="prepared"{continue;}
        let target=value["target"].as_str().unwrap_or_default();let source=value["sourceHash"].as_str().unwrap_or_default();let new_hash=value["newHash"].as_str().unwrap_or_default();
        let state=match fs::read(target){Ok(current)=>{let hash=crate::save_core::hash(&current);if hash==new_hash{"complete-new"}else if hash==source{"complete-old"}else{"incomplete-preserved"}},Err(_)=>"incomplete-preserved"};
        value["state"]=serde_json::json!(state);let _=fs::write(&path,serde_json::to_vec(&value).unwrap_or_default());
        resolved.push(serde_json::json!({"state":state,"journal":path.to_string_lossy()}));
    }
    resolved
}
pub(crate) fn recovery_copy(files:&Files,owner:&str,grant:&str,epoch:u64,path:&Path,bytes:&[u8])->Result<Descriptor>{
    files.with_live(owner,grant,epoch,||Ok(()))?;
    if bytes.len()>MAX_OUTPUT{return Err(standards_error());}
    let supplied=output_path(path)?;let parent=supplied.parent().ok_or_else(AppError::denied)?;let mut pins=vec![];
    crate::unicode_worker::pin_chain(parent,&mut pins).map_err(|_|AppError::denied())?;
    let target=fs::canonicalize(parent).map_err(|_|AppError::denied())?.join(supplied.file_name().ok_or_else(AppError::denied)?);
    let mut file=OpenOptions::new().read(true).write(true).create_new(true).share_mode(1).custom_flags(0x00200000).open(&target).map_err(create_output_error)?;
    file.write_all(bytes).and_then(|_|file.sync_all()).map_err(|_|error("writer_export_failed","恢复副本未完成；原件和全部恢复材料保留。"))?;
    files.register_export(owner,grant,&target,file,bytes,epoch)
}
fn output_path(path:&Path)->Result<PathBuf>{
    let text=path.to_string_lossy();let text=text.strip_prefix(r"\\?\").unwrap_or(&text);
    if !path.is_absolute()||text.len()<3||!text.as_bytes()[0].is_ascii_alphabetic()||text.as_bytes()[1]!=b':'||text[2..].contains(':')||text.contains(['\0','"'])||path.components().any(|c|matches!(c,std::path::Component::ParentDir|std::path::Component::CurDir))||path.extension().is_none_or(|e|!e.eq_ignore_ascii_case("pdf")){return Err(AppError::denied());}
    use std::os::windows::ffi::OsStrExt;
    let drive:Vec<u16>=std::ffi::OsStr::new(&format!("{}:\\",text.as_bytes()[0]as char)).encode_wide().chain(Some(0)).collect();
    let kind=unsafe{windows_sys::Win32::Storage::FileSystem::GetDriveTypeW(drive.as_ptr())};if ![3,2].contains(&kind){return Err(AppError::denied());}
    let name=path.file_name().ok_or_else(AppError::denied)?;let name_text=name.to_string_lossy();let stem=name_text.split('.').next().unwrap_or("").to_ascii_uppercase();
    if name_text.ends_with(['.',' '])||matches!(stem.as_str(),"CON"|"PRN"|"AUX"|"NUL")||(stem.len()==4&&(stem.starts_with("COM")||stem.starts_with("LPT"))&&stem.as_bytes()[3].is_ascii_digit()){return Err(AppError::denied());}
    path.parent().ok_or_else(AppError::denied)?;Ok(path.to_owned())
}
/// The writer runs in the webview, so the renderer fetches the wasm engine and
/// the pinned Unicode font as ordinary frontend assets. Those bytes are pinned
/// in a lock this file compiles into the binary, so a packaged application can
/// only accept the assets that were locked when it was built and never consults
/// a path on the build machine. Verification is deliberately a byte comparison
/// of what this build actually serves; never relax it, and never derive the
/// expected values from the files being checked.
#[derive(serde::Deserialize)]#[serde(deny_unknown_fields)]
struct WriterAsset{path:String,request:String,bytes:u64,sha256:String,source:String,license:String}
#[derive(serde::Deserialize)]#[serde(deny_unknown_fields)]
struct WriterLock{schema:u32,scope:String,assets:Vec<WriterAsset>}
fn writer_lock()->&'static WriterLock{
    static LOCK:std::sync::OnceLock<WriterLock>=std::sync::OnceLock::new();
    LOCK.get_or_init(||serde_json::from_str(include_str!("../writer-assets.lock.json")).expect("the writer asset lock must parse"))
}
fn asset_error()->AppError{error("writer_resource_mismatch","Writer 资源清单或文件校验失败，未启动。")}
/// Check every locked writer asset against the bytes this build serves. The
/// caller supplies the lookup, so the same code verifies the embedded frontend
/// of an installed package and the built frontend of a development build.
pub(crate) fn verify_writer_assets(fetch:&dyn Fn(&str)->Option<Vec<u8>>)->Result<()>{
    let lock=writer_lock();
    if lock.schema!=1||lock.assets.is_empty()||lock.scope.is_empty(){return Err(asset_error());}
    for asset in &lock.assets{
        if asset.path.is_empty()||asset.request!=(format!("/{}",asset.path))||asset.source.is_empty()||asset.license.is_empty(){return Err(asset_error());}
        let bytes=fetch(&asset.path).ok_or_else(asset_error)?;
        if bytes.len()as u64!=asset.bytes||crate::save_core::hash(&bytes)!=asset.sha256{return Err(asset_error());}
    }
    Ok(())
}
#[cfg(test)]mod tests{
 use super::*;
 /// Verified MuPDF.js writer evidence: original -> renderer candidate -> writer output.
 fn writer_evidence()->(Vec<u8>,Vec<u8>,Vec<u8>){
  let root=crate::save_core::project();let fixture=root.join("tests/work/writer/mupdf");
  let source=fs::read(root.join("tests/work/writer/matrix-source.pdf")).unwrap();
  let candidate=fs::read(fixture.join("matrix-candidate.pdf")).unwrap();
  let output=fs::read(fixture.join("matrix-first.pdf")).unwrap();
  assert!(candidate.starts_with(&source)&&output.starts_with(&candidate)&&appended_revision(&candidate,&output));
  (source,candidate,output)
 }
 fn scratch(name:&str)->PathBuf{let root=crate::save_core::project().join("tests/work/m3").join(format!("{name}-{}",uuid::Uuid::new_v4()));fs::create_dir(&root).unwrap();root}
 #[test]fn fixed_mupdf_writer_output_is_saved_by_the_real_copy_core_without_source_changes(){
  let(source,candidate,output)=writer_evidence();
  let root=scratch("fixed-saveas");let original=root.join("合成 原件.pdf");fs::write(&original,&source).unwrap();
  let files=Files::default();let grant=files.authorize_at("main",&original,files.epoch()).unwrap();let service=Service::new(crate::unicode_worker::prototype());let id=service.reserve(&files,"main",&grant.grant_id).unwrap();
  let prepared=service.prepare(&files,"main",&id,&candidate,&output).unwrap();
  let path=root.join("中文 数学批注.pdf");let copy=service.commit_new(&files,"main",&id,&path,prepared.0,prepared.1,prepared.2).unwrap();
  assert_eq!(files.export_source("main",&copy.grant_id).unwrap().bytes,output);assert_eq!(fs::read(original).unwrap(),source);
  // Re-selecting the exported file through the normal native-open route must
  // work while the export grant remains live. Both grants must deny new writes.
  let reopened=files.authorize_at("main",&path,files.epoch()).unwrap();
  let task=files.begin("main",&reopened.grant_id).unwrap();assert_eq!(files.read("main",&task).unwrap(),output);
  assert!(OpenOptions::new().write(true).open(&path).is_err());
  files.release("main",&copy.grant_id).unwrap();
  assert!(OpenOptions::new().write(true).open(&path).is_err());
  files.release("main",&reopened.grant_id).unwrap();
  assert!(OpenOptions::new().write(true).open(&path).is_ok());
 }
 #[test]fn production_original_save_is_disabled_without_revoking_the_live_source(){
  let(source,candidate,output)=writer_evidence();let root=scratch("original-disabled");
  let original=root.join("保留 原件.pdf");fs::write(&original,&source).unwrap();
  let files=Files::default();let grant=files.authorize_at("main",&original,files.epoch()).unwrap();
  let service=Service::new(crate::unicode_worker::prototype());let id=service.reserve(&files,"main",&grant.grant_id).unwrap();
  let prepared=service.prepare(&files,"main",&id,&candidate,&output).unwrap();
  assert_eq!(service.commit_original(&files,"main",&id,prepared.0,prepared.1,prepared.2).unwrap_err().code,"writer_original_disabled");
  assert!(service.tasks.lock().unwrap().is_empty());assert_eq!(files.export_source("main",&grant.grant_id).unwrap().bytes,source);
  assert_eq!(fs::read(&original).unwrap(),source);assert_eq!(fs::read_dir(root).unwrap().count(),1);
 }
 #[test]fn the_legacy_check_then_replace_race_is_a_real_counterexample_not_a_safe_save(){
  let root=scratch("legacy-race");let original=root.join("原件.pdf");let pending=root.join("pending.pdf");let backup=root.join("backup.pdf");
  let(source,_,output)=writer_evidence();let mut external=source.clone();external.extend_from_slice(b"\n% external update after the final hash check\n");
  fs::write(&original,&source).unwrap();fs::write(&pending,&output).unwrap();
  let worker_path=original.clone();let external_bytes=external.clone();
  let(go,wait)=std::sync::mpsc::channel();let(done,finished)=std::sync::mpsc::channel();
  let worker=std::thread::spawn(move||{wait.recv().unwrap();fs::write(worker_path,external_bytes).unwrap();done.send(()).unwrap();});
  let failed=replace_file_after_check(&original,&pending,&backup,&crate::save_core::hash(&source),||{go.send(()).unwrap();finished.recv().unwrap();});
  worker.join().unwrap();
  assert!(failed.is_err(),"post-swap backup verification notices the conflict only after overwriting it");
  assert_eq!(fs::read(&original).unwrap(),output,"the external update has already been displaced: unsafe for production");
  assert_eq!(fs::read(&backup).unwrap(),external,"conflicting external bytes survive only in the backup");
 }
 #[test]fn output_creation_errors_distinguish_existing_files_permissions_locks_and_disk_space(){
  for(code,expected)in[(80,"writer_target_exists"),(183,"writer_target_exists"),(5,"writer_target_access_denied"),(32,"writer_target_locked"),(33,"writer_target_locked"),(112,"writer_target_disk_full"),(39,"writer_target_disk_full"),(3,"writer_export_failed")]{
   assert_eq!(create_output_error(std::io::Error::from_raw_os_error(code)).code,expected);
  }
 }
 #[test]fn output_budget_failure_cleans_the_task_and_creates_no_target(){
  let(source,candidate,output)=writer_evidence();let root=scratch("output-budget");let original=root.join("原件.pdf");fs::write(&original,&source).unwrap();
  let files=Files::default();let grant=files.authorize_at("main",&original,files.epoch()).unwrap();let service=Service::new(crate::unicode_worker::prototype());
  let id=service.reserve(&files,"main",&grant.grant_id).unwrap();let prepared=service.prepare(&files,"main",&id,&candidate,&output).unwrap();
  let oversized=Prepared{bytes:vec![0;MAX_OUTPUT+1],..prepared.0};
  assert_eq!(service.commit_new(&files,"main",&id,&root.join("拒绝.pdf"),oversized,prepared.1,prepared.2).err().unwrap().code,"writer_pdf_budget");
  assert!(service.tasks.lock().unwrap().is_empty());assert_eq!(fs::read_dir(&root).unwrap().count(),1);assert_eq!(fs::read(original).unwrap(),source);
 }
 #[test]fn renderer_candidate_and_writer_output_must_be_exact_incremental_revisions(){
  let(source,candidate,output)=writer_evidence();
  let root=scratch("revision-gate");let original=root.join("原件.pdf");fs::write(&original,&source).unwrap();
  let files=Files::default();let grant=files.authorize_at("main",&original,files.epoch()).unwrap();let service=Service::new(crate::unicode_worker::prototype());
  // A candidate that is not an incremental revision of the authorized file.
  let mut foreign=candidate.clone();foreign[0]=b'X';
  let id=service.reserve(&files,"main",&grant.grant_id).unwrap();
  let error=service.prepare(&files,"main",&id,&foreign,&output).err().unwrap();assert_eq!(error.code,"writer_conversion_refused");
  assert!(service.tasks.lock().unwrap().is_empty());
  // An output that does not append a revision to the candidate.
  let id=service.reserve(&files,"main",&grant.grant_id).unwrap();
  let error=service.prepare(&files,"main",&id,&candidate,&candidate).err().unwrap();assert_eq!(error.code,"writer_standards_blocked");
  // A truncated tail without cross-reference or trailer.
  let mut broken=output.clone();broken.truncate(candidate.len()+32);
  let id=service.reserve(&files,"main",&grant.grant_id).unwrap();
  assert_eq!(service.prepare(&files,"main",&id,&candidate,&broken).err().unwrap().code,"writer_standards_blocked");
  assert_eq!(fs::read(&original).unwrap(),source);assert_eq!(fs::read_dir(&root).unwrap().count(),1);
 }
 #[test]fn renderer_cannot_supply_network_device_or_stream_export_paths(){for p in ["relative.pdf",r"\\server\share\output.pdf",r"C:\test\x.pdf:stream",r"C:\test\..\x.pdf",r"C:\test\CON.pdf",r"C:\test\LPT1.pdf"]{assert!(output_path(Path::new(p)).is_err());}}
 #[test]fn cancelled_exports_never_create_a_task_or_modify_source(){let root=scratch("candidate-export");let source=root.join("中文 空格.pdf");let bytes=fs::read(crate::save_core::project().join("tests/fixtures/g1/plain.pdf")).unwrap();fs::write(&source,&bytes).unwrap();let files=Files::default();let grant=files.authorize_at("main",&source,files.epoch()).unwrap();let service=Service::new(crate::unicode_worker::prototype());assert!(service.reserve(&files,"other",&grant.grant_id).is_err());
 let id=service.reserve(&files,"main",&grant.grant_id).unwrap();service.cancel("other",Some(&id));assert_eq!(service.tasks.lock().unwrap().len(),1);service.cancel("main",Some(&id));assert!(service.tasks.lock().unwrap().is_empty());assert!(service.prepare(&files,"main",&id,&bytes,&bytes).is_err());
 assert_eq!(fs::read(&source).unwrap(),bytes);assert_eq!(fs::read_dir(root).unwrap().count(),1);}
 #[test]fn independently_prevalidated_synthetic_copy_reopens_and_never_overwrites_existing_files(){
  let root=scratch("export-file-core");let path=root.join("源 文件.pdf");let bytes=fs::read(crate::save_core::project().join("tests/fixtures/g1/plain.pdf")).unwrap();fs::write(&path,&bytes).unwrap();let files=Files::default();let grant=files.authorize_at("main",&path,files.epoch()).unwrap();let service=Service::new(crate::unicode_worker::prototype());
  let parts=|id:&str|{let task=service.tasks.lock().unwrap().get(id).unwrap().clone();let source=task.source.lock().unwrap().take().unwrap();let prepared=Prepared{bytes:bytes.clone(),source_hash:crate::save_core::hash(&bytes),candidate_hash:crate::save_core::hash(&bytes)};(task,source,prepared)};
  let id=service.reserve(&files,"main",&grant.grant_id).unwrap();let(t,s,p)=parts(&id);let target=root.join("新 批注副本.pdf");let out=service.commit_new(&files,"main",&id,&target,p,t,s).unwrap();
  assert_eq!(files.export_source("main",&out.grant_id).unwrap().bytes,bytes);assert!(OpenOptions::new().write(true).open(&target).is_err());
  let id=service.reserve(&files,"main",&out.grant_id).unwrap();let(t,s,p)=parts(&id);let error=service.commit_new(&files,"main",&id,&target,p,t,s).err().unwrap();assert_eq!(error.code,"writer_target_exists");
  assert_eq!(fs::read(&path).unwrap(),bytes);assert_eq!(fs::read(target).unwrap(),bytes);assert_eq!(fs::read_dir(root).unwrap().count(),2);assert!(service.tasks.lock().unwrap().is_empty());
 }
 #[test]fn original_save_replaces_atomically_with_a_backup_and_a_completed_journal(){
  let(source,candidate,output)=writer_evidence();
  let root=scratch("original-save");let journal=root.join("journal");let original=root.join("can-discard 样例.pdf");fs::write(&original,&source).unwrap();
  let files=Files::default();let grant=files.authorize_at("main",&original,files.epoch()).unwrap();let service=Service::new(crate::unicode_worker::prototype());let id=service.reserve(&files,"main",&grant.grant_id).unwrap();
  let prepared=service.prepare(&files,"main",&id,&candidate,&output).unwrap();
  let message=service.commit_original_in(&files,"main",&id,prepared.0,prepared.1,prepared.2,&journal).unwrap();
  assert!(message.contains("备份"));
  let written=fs::read(&original).unwrap();assert_eq!(written,output);assert!(written.starts_with(&source));
  assert_eq!(fs::read(root.join("can-discard 样例.backup.pdf")).unwrap(),source);
  let records:Vec<serde_json::Value>=fs::read_dir(&journal).unwrap().map(|entry|serde_json::from_slice(&fs::read(entry.unwrap().path()).unwrap()).unwrap()).collect();
  assert_eq!(records.len(),1);assert_eq!(records[0]["state"],"completed");assert_eq!(records[0]["newHash"].as_str().unwrap(),crate::save_core::hash(&output));
  assert!(service.tasks.lock().unwrap().is_empty());
  let pending=fs::read_dir(&root).unwrap().filter(|entry|entry.as_ref().unwrap().file_name().to_string_lossy().contains("pending")).count();
  assert_eq!(pending,0);
 }
 #[test]fn an_external_replacement_of_the_authorized_file_is_refused_while_it_is_open(){
  let(source,candidate,output)=writer_evidence();
  let root=scratch("original-protected");let journal=root.join("journal");let original=root.join("样例.pdf");fs::write(&original,&source).unwrap();
  let files=Files::default();let grant=files.authorize_at("main",&original,files.epoch()).unwrap();let service=Service::new(crate::unicode_worker::prototype());let id=service.reserve(&files,"main",&grant.grant_id).unwrap();
  let prepared=service.prepare(&files,"main",&id,&candidate,&output).unwrap();
  // The reader grant and the captured source deny every external replacement
  // while the document is open; nothing is written on that path.
  let mut external=source.clone();external.extend_from_slice(b"\n% external writer\n");
  let replacement=root.join("external.pdf");fs::write(&replacement,&external).unwrap();
  assert!(fs::rename(&replacement,&original).is_err());
  assert_eq!(fs::read(&original).unwrap(),source);
  // The controlled save then publishes exactly source + one revision.
  let message=service.commit_original_in(&files,"main",&id,prepared.0,prepared.1,prepared.2,&journal).unwrap();
  assert!(message.contains("备份"));
  assert_eq!(fs::read(&original).unwrap(),output);
  assert_eq!(fs::read(root.join("样例.backup.pdf")).unwrap(),source);
  assert!(replacement.exists(),"the refused external file is preserved");
  assert!(service.tasks.lock().unwrap().is_empty());
 }
 #[test]fn interrupted_original_save_is_classified_from_the_bytes_on_disk(){
  let root=scratch("recover");let journal=root.join("journal");let original=root.join("样例.pdf");
  let(source,_,output)=writer_evidence();
  fs::write(&original,&source).unwrap();
  let record=|target:&Path,state:&str,new_hash:&str|write_journal(&journal,serde_json::json!({"schema":1,"state":state,"target":target.to_string_lossy(),"sourceHash":crate::save_core::hash(&source),"newHash":new_hash})).unwrap();
  // Never replaced: the file still holds the source bytes.
  let old=record(&original,"prepared",&crate::save_core::hash(&output));
  let resolved=recover_pending_in(&journal);assert_eq!(resolved.len(),1);assert_eq!(resolved[0]["state"],"complete-old");
  assert_eq!(serde_json::from_slice::<serde_json::Value>(&fs::read(&old).unwrap()).unwrap()["state"],"complete-old");
  // Replaced but the journal was never completed.
  fs::write(&original,&output).unwrap();
  record(&original,"prepared",&crate::save_core::hash(&output));
  let resolved=recover_pending_in(&journal);assert_eq!(resolved.len(),1);assert_eq!(resolved[0]["state"],"complete-new");
  // Unknown bytes are preserved and reported, never overwritten.
  fs::write(&original,b"% unrelated bytes\n").unwrap();
  record(&original,"prepared",&crate::save_core::hash(&output));
  let resolved=recover_pending_in(&journal);assert_eq!(resolved[0]["state"],"incomplete-preserved");
  assert_eq!(fs::read(&original).unwrap(),b"% unrelated bytes\n");
 }
 #[test]fn the_replacement_step_refuses_bytes_that_are_not_the_authorized_revision(){
  let root=scratch("replace-verify");let target=root.join("原件.pdf");let pending=root.join(".pending.pdf");let backup=root.join("原件.backup.pdf");
  let(source,_,output)=writer_evidence();
  let unexpected=b"% unexpected external bytes\n".to_vec();
  fs::write(&target,&unexpected).unwrap();fs::write(&pending,&output).unwrap();
  // The step only replaces the exact bytes the caller verified and authorized.
  let error=replace_file(&target,&pending,&backup,&crate::save_core::hash(&source)).err().unwrap();
  assert_eq!(error.code,"writer_source_changed");
  assert_eq!(fs::read(&target).unwrap(),unexpected);
  assert!(!backup.exists()&&pending.exists());
  // With the authorized bytes the same step backs up and replaces them.
  fs::write(&target,&source).unwrap();
  replace_file(&target,&pending,&backup,&crate::save_core::hash(&source)).unwrap();
  assert_eq!(fs::read(&target).unwrap(),output);
  assert_eq!(fs::read(&backup).unwrap(),source);
 }
 /// Every supported writer case must pass the production Save As core. Also
 /// exercise the retained TEST-ONLY legacy replacement implementation to preserve
 /// its byte/backup evidence; this is not original-save safety acceptance (the
 /// separate concurrent-writer counterexample proves that implementation unsafe).
 /// Missing generated evidence is a failing prerequisite, never a passing skip.
 #[test]fn every_supported_annotation_type_passes_both_real_save_cores(){
  let root=crate::save_core::project().join("tests/work/writer/annotations");
  let manifest=root.join("annotation-types.json");
  let evidence=fs::read(&manifest).expect("annotation save evidence is absent: run node spikes/mupdf-writer/annotations.mjs first");
  let parsed:serde_json::Value=serde_json::from_slice(&evidence).expect("annotation save evidence must be valid JSON");
  let rows=parsed["rows"].as_array().expect("annotation save evidence must contain all five writer cases");
  assert_eq!(rows.len(),5,"all five generated writer cases must be present");
  let mut checked:Vec<String>=vec![];
  for row in rows{
   let name=row["case"].as_str().unwrap();
    assert!(row["failure"].is_null(),"{name}: the writer reported a failure; do not skip it");
   let source=fs::read(root.join(format!("{name}-source.pdf"))).unwrap();
   let candidate=fs::read(root.join(format!("{name}-candidate.pdf"))).unwrap();
   let output=fs::read(root.join(format!("{name}-output.pdf"))).unwrap();
   assert!(candidate.starts_with(&source)&&output.starts_with(&candidate)&&appended_revision(&candidate,&output),"{name}: the published bytes are not a verified revision chain");
   // Save As hashes the source, books the copy and never touches the original.
   let save_as=scratch(&format!("annotation-{name}-saveas"));
   let original=save_as.join("合成 原件.pdf");fs::write(&original,&source).unwrap();
   let files=Files::default();let grant=files.authorize_at("main",&original,files.epoch()).unwrap();
   let service=Service::new(crate::unicode_worker::prototype());
   let id=service.reserve(&files,"main",&grant.grant_id).unwrap();
   let prepared=service.prepare(&files,"main",&id,&candidate,&output).unwrap();
   let saved=service.commit_new(&files,"main",&id,&save_as.join(format!("{name} 批注副本.pdf")),prepared.0,prepared.1,prepared.2).unwrap();
   assert_eq!(files.export_source("main",&saved.grant_id).unwrap().bytes,output,"{name}: the saved copy differs from the writer output");
   assert_eq!(fs::read(&original).unwrap(),source,"{name}: Save As must not modify the original");
   assert!(service.tasks.lock().unwrap().is_empty());
   // TEST-ONLY legacy original replacement: this checks bytes and backup only,
   // not concurrent-modification safety. Production original saving stays off.
   let discardable=scratch(&format!("annotation-{name}-original"));let journal=discardable.join("journal");
   let target=discardable.join("可丢弃 样例.pdf");fs::write(&target,&source).unwrap();
   let files=Files::default();let grant=files.authorize_at("main",&target,files.epoch()).unwrap();
   let service=Service::new(crate::unicode_worker::prototype());
   let id=service.reserve(&files,"main",&grant.grant_id).unwrap();
   let prepared=service.prepare(&files,"main",&id,&candidate,&output).unwrap();
   service.commit_original_in(&files,"main",&id,prepared.0,prepared.1,prepared.2,&journal).unwrap();
   assert_eq!(fs::read(&target).unwrap(),output,"{name}: the original save must publish exactly the writer output");
   assert_eq!(fs::read(discardable.join("可丢弃 样例.backup.pdf")).unwrap(),source,"{name}: the backup must hold the authorized original");
   let records:Vec<serde_json::Value>=fs::read_dir(&journal).unwrap().map(|entry|serde_json::from_slice(&fs::read(entry.unwrap().path()).unwrap()).unwrap()).collect();
   assert_eq!(records.len(),1);assert_eq!(records[0]["state"],"completed");
   assert!(service.tasks.lock().unwrap().is_empty());
   checked.push(name.to_string());
  }
  assert_eq!(checked,vec!["freetext-only","highlight-only","ink-only","mixed","no-annotation"],"FreeText, Highlight, Ink, mixed and final-annotation removal/no-annotation must all reach the save core");
 }
}
/// The packaged application resolves the writer assets the same way the renderer
/// requests them: an installed build reads the frontend assets embedded in the
/// executable, a development build reads the built frontend tree Tauri falls
/// back to. A release test therefore verifies the bytes inside the produced
/// executable, which is what the installer ships.
#[cfg(test)]mod writer_asset_tests{
 use super::*;
 fn dev_dist(path:&str)->Option<Vec<u8>>{
  if cfg!(debug_assertions){fs::read(crate::save_core::project().join("dist").join(path)).ok()}else{None}
 }
 #[test]fn the_build_serves_exactly_the_locked_writer_assets(){
  let context:tauri::Context<tauri::Wry>=tauri::generate_context!();
  let assets=context.assets();
  // A packaged build embeds the frontend (`tauri build`, or the
  // `tauri/custom-protocol` feature) and must resolve the locked assets inside
  // its own executable; a development build serves the built frontend instead.
  assert!(assets.iter().next().is_some()||tauri::is_dev(),"a production build must embed the frontend assets; run it through the Tauri CLI or with --features tauri/custom-protocol");
  let fetch=|path:&str|->Option<Vec<u8>>{assets.get(&tauri::utils::assets::AssetKey::from(path)).map(|bytes|bytes.to_vec()).or_else(||dev_dist(path))};
  verify_writer_assets(&fetch).unwrap();
 }
 /// Every file the frontend build shipped must be reachable through the asset
 /// service the packaged application uses. A code-split chunk that the embedded
 /// asset set cannot serve is not reported as a missing file at runtime: the
 /// resolver falls back to index.html, so a lazy `import()` of that chunk fails
 /// as a module syntax error instead. This gate catches that whole class.
 #[test]fn the_packaged_build_serves_every_shipped_frontend_file(){
  let context:tauri::Context<tauri::Wry>=tauri::generate_context!();
  let assets=context.assets();
  if assets.iter().next().is_none(){assert!(tauri::is_dev(),"a production build must embed the frontend assets; run it through the Tauri CLI or with --features tauri/custom-protocol");return;}
  fn walk(dir:&Path,prefix:&str,out:&mut Vec<String>){
   for entry in fs::read_dir(dir).unwrap().flatten(){
    let path=entry.path();
    if path.is_dir(){walk(&path,&format!("{prefix}{}/",entry.file_name().to_string_lossy()),out);}
    else{out.push(format!("{prefix}{}",entry.file_name().to_string_lossy()));}
   }
  }
  let dist=crate::save_core::project().join("dist");
  let mut shipped=vec![];walk(&dist,"",&mut shipped);
  assert!(shipped.len()>=10,"the frontend build looks empty: {} files",shipped.len());
  let missing:Vec<String>=shipped.iter().filter(|name|assets.get(&tauri::utils::assets::AssetKey::from(name.as_str())).is_none()).cloned().collect();
  assert!(missing.is_empty(),"the packaged frontend cannot serve: {missing:?}");
 }
 #[test]fn a_changed_or_missing_asset_is_rejected(){
  let context:tauri::Context<tauri::Wry>=tauri::generate_context!();
  let assets=context.assets();
  let fetch=|path:&str|->Option<Vec<u8>>{assets.get(&tauri::utils::assets::AssetKey::from(path)).map(|bytes|bytes.to_vec()).or_else(||dev_dist(path))};
  let flipped=|path:&str|fetch(path).map(|mut bytes|{if !bytes.is_empty(){bytes[0]^=0xff;}bytes});
  // A build that serves different bytes is refused just as hard as one that
  // cannot serve a locked asset; neither may start the writer.
  assert_eq!(verify_writer_assets(&flipped).err().unwrap().code,"writer_resource_mismatch");
  assert_eq!(verify_writer_assets(&|_|None).err().unwrap().code,"writer_resource_mismatch");
  assert_eq!(verify_writer_assets(&|path|fetch(path).map(|bytes|bytes[..bytes.len()/2].to_vec())).err().unwrap().code,"writer_resource_mismatch");
 }
}
