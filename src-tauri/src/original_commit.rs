//! Grant-bound append commit. Complete, verified old/new materials precede writes.
//! Recovery only classifies or exports a new copy; never truncates an original.
#![allow(dead_code)]
use crate::{error::AppError,files::{Descriptor,Files}};
use serde::{Deserialize,Serialize};
use std::{fs::{self,File,OpenOptions},io::{Read,Seek,SeekFrom,Write},os::windows::{fs::OpenOptionsExt,io::AsRawHandle},path::{Path,PathBuf}};
#[path="original_materials.rs"] mod materials;
pub(crate) use materials::retain_completed;

pub(crate) struct ApprovedOutput {source_hash:String,bytes:Vec<u8>}
impl ApprovedOutput {
    pub(crate) fn checked(source:&[u8],prepared:crate::pdf_export::Prepared)->std::result::Result<Self,AppError>{
        if crate::save_core::hash(source)!=prepared.source_hash{return Err(AppError{code:"writer_source_changed",message:"保存前的源文件版本不一致，未写入。"});}
        crate::incremental::incremental_preflight(source,&prepared.bytes).map_err(|error|AppError{code:"writer_original_structure_invalid",message:crate::incremental::diagnostic_rule(&error)})?;
        Ok(Self{source_hash:prepared.source_hash,bytes:prepared.bytes})
    }
}
/// Disk evidence, created only after append + flush + full byte verification.
/// Renderer data and the synthetic M4 event cannot construct this receipt.
pub(crate) struct VerifiedReceipt {event_id:String,file_key:String,old_hash:String,new_hash:String}
impl VerifiedReceipt {
    pub(crate) fn event_id(&self)->&str {&self.event_id}
    pub(crate) fn file_key(&self)->&str {&self.file_key}
    pub(crate) fn old_hash(&self)->&str {&self.old_hash}
    pub(crate) fn new_hash(&self)->&str {&self.new_hash}
}
pub(crate) struct CommitOutcome {
    pub(crate) receipt:VerifiedReceipt,
    pub(crate) grant:std::result::Result<Descriptor,AppError>,
    pub(crate) materials:PathBuf,
    pub(crate) timings:CommitTimings,
}
#[derive(Default,serde::Serialize)]
#[serde(rename_all="camelCase")]
pub(crate) struct CommitTimings {pub(crate) backup_ms:f64,pub(crate) commit_ms:f64}
#[derive(Debug)]
pub(crate) struct CommitFailure {
    pub(crate) error:AppError,
    pub(crate) materials:Option<PathBuf>,
    pub(crate) original_may_be_partial:bool,
    pub(crate) recovery_grant:Option<Descriptor>,
}
#[derive(Serialize)]
#[serde(rename_all="camelCase")]
pub(crate) struct SaveError {
    #[serde(flatten)] error:AppError,
    recovery_descriptor:Option<Descriptor>,
}
impl From<AppError> for SaveError {fn from(error:AppError)->Self{Self{error,recovery_descriptor:None}}}
impl From<CommitFailure> for SaveError {fn from(failure:CommitFailure)->Self{
    let error=if failure.original_may_be_partial{AppError{code:"writer_original_interrupted",message:"保存中断或最终校验未完成，请从恢复材料另存副本。"}}else{failure.error};
    Self{error,recovery_descriptor:failure.recovery_grant}
}}
#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct Intent {schema:u32,event_id:String,file_key:String,old_hash:String,new_hash:String,old_len:usize,new_len:usize}
#[derive(Debug,PartialEq,Serialize)]
#[serde(rename_all="camelCase")]
pub(crate) enum RecoveryState {CompleteOld,CompleteNew,IncompletePreserved,ExternalChangePreserved}
#[derive(Clone,Copy,PartialEq)]
enum Fault {None,#[cfg(test)]AfterPrepared,#[cfg(test)]PartialWrite,#[cfg(test)]FinalFlush}
fn io_error()->AppError {AppError{code:"writer_original_io_failed",message:"原件提交未完成；请保留备份、新版材料和草稿，并重新打开文件检查恢复。"}}
fn refusal()->AppError {AppError{code:"writer_original_materials_invalid",message:"保存或恢复材料的身份、内容或版本无法确认；原件未被自动修复，所有材料保留。"}}
fn failed(error:AppError,materials:Option<PathBuf>,partial:bool)->CommitFailure {CommitFailure{error,materials,original_may_be_partial:partial,recovery_grant:None}}
fn read_limited(file:&mut File,limit:usize)->std::result::Result<Vec<u8>,AppError> {
    if file.metadata().map_err(|_|io_error())?.len()>limit as u64{return Err(refusal());}
    file.seek(SeekFrom::Start(0)).map_err(|_|io_error())?;let mut bytes=Vec::new();
    file.take(limit as u64+1).read_to_end(&mut bytes).map_err(|_|io_error())?;
    if bytes.len()>limit{return Err(refusal());}Ok(bytes)
}
fn safe_material(file:&File)->std::result::Result<(),AppError> {
    let mut info=windows_sys::Win32::Storage::FileSystem::BY_HANDLE_FILE_INFORMATION::default();
    if unsafe{windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandle(file.as_raw_handle(),&mut info)}==0||info.nNumberOfLinks!=1||info.dwFileAttributes&(0x400|0x10)!=0{return Err(refusal());}
    crate::storage::directories::check_private_file(file,false).map_err(|_|refusal())
}
fn write_material(dir:&Path,name:&str,bytes:&[u8])->std::result::Result<File,AppError> {
    let mut file=OpenOptions::new().read(true).write(true).create_new(true).share_mode(1).custom_flags(0x00200000).open(dir.join(name)).map_err(|_|io_error())?;
    file.write_all(bytes).and_then(|_|file.sync_all()).map_err(|_|io_error())?;
    safe_material(&file)?;if read_limited(&mut file,bytes.len())?!=bytes{return Err(refusal());}Ok(file)
}
fn material_bytes(dir:&Path,name:&str,limit:usize)->std::result::Result<Vec<u8>,AppError> {
    let mut file=OpenOptions::new().read(true).share_mode(1).custom_flags(0x00200000).open(dir.join(name)).map_err(|_|refusal())?;
    safe_material(&file)?;read_limited(&mut file,limit)
}
pub(crate) fn commit_approved(files:&Files,owner:&str,grant:&str,epoch:u64,output:ApprovedOutput,material_parent:&Path)->std::result::Result<CommitOutcome,CommitFailure> {
    commit_inner(files,owner,grant,epoch,output,material_parent,Fault::None,|_|{})
}
fn commit_inner(files:&Files,owner:&str,grant:&str,epoch:u64,output:ApprovedOutput,material_parent:&Path,fault:Fault,mut hook:impl FnMut(&str))->std::result::Result<CommitOutcome,CommitFailure> {
    if output.bytes.len()>crate::files::EXPORT_OUTPUT_MAX{return Err(failed(refusal(),None,false));}
    let mut pins=Vec::new();
    crate::unicode_worker::pin_chain(material_parent,&mut pins).map_err(|_|failed(refusal(),None,false))?;
    let mut recovery=None;
    let mut lease=files.take_original_write_recovering(owner,grant,epoch,&output.source_hash,||{},&mut recovery).map_err(|e|{
        let mut failure=failed(e,None,false);failure.recovery_grant=recovery;failure
    })?;
    let result=(||{
    let backup_started=std::time::Instant::now();
    let old=lease.bytes().to_owned();
    if output.bytes.len()<=old.len()||!output.bytes.starts_with(&old){return Err(failed(refusal(),None,false));}
    let event=uuid::Uuid::new_v4().to_string();
    let dir=material_parent.join(format!(".ainative-save-{event}"));
    // Only this new directory receives a private ACL. Never alter the source,
    // its parent, inherited user data, or any existing recovery directory.
    let _private=crate::storage::directories::create_private_dir(&dir).map_err(|_|failed(io_error(),None,false))?;
    let materials=Some(dir.clone());let new_hash=crate::save_core::hash(&output.bytes);
    // Pin the newly created leaf as well. Pinning only its parent does not
    // exclude replacement of this intermediate component with a junction.
    crate::unicode_worker::pin_chain(&dir,&mut pins).map_err(|_|failed(refusal(),materials.clone(),false))?;
    let intent=Intent{schema:1,event_id:event.clone(),file_key:lease.file_key().into(),old_hash:output.source_hash.clone(),new_hash:new_hash.clone(),old_len:old.len(),new_len:output.bytes.len()};
    // All complete materials precede the FIRST byte written to the original.
    let serialized=serde_json::to_vec(&intent).map_err(|_|failed(refusal(),materials.clone(),false))?;
    let _old=write_material(&dir,"verified-old.pdf",&old).map_err(|e|failed(e,materials.clone(),false))?;
    let _new=write_material(&dir,"verified-new.pdf",&output.bytes).map_err(|e|failed(e,materials.clone(),false))?;
    let _intent=write_material(&dir,"intent.json",&serialized).map_err(|e|failed(e,materials.clone(),false))?;
    let backup_ms=backup_started.elapsed().as_secs_f64()*1000.0;
    hook("prepared");
    #[cfg(test)]if fault==Fault::AfterPrepared{return Err(failed(io_error(),materials,false));}
    let commit_started=std::time::Instant::now();
    files.original_lease_live(&lease).map_err(|e|failed(e,materials.clone(),false))?;
    let file=lease.file_mut();
    if read_limited(file,crate::files::EXPORT_INPUT_MAX).map_err(|e|failed(e,materials.clone(),false))?!=old{return Err(failed(refusal(),materials,false));}
    if file.seek(SeekFrom::End(0)).map_err(|_|failed(io_error(),materials.clone(),false))?!=old.len()as u64{return Err(failed(refusal(),materials,false));}
    let suffix=&output.bytes[old.len()..];let middle=(suffix.len()/2).max(1);
    // Strict WRITE-denying sharing stays held throughout append and verification.
    file.write_all(&suffix[..middle]).map_err(|_|failed(io_error(),materials.clone(),true))?;
    hook("partial");
    #[cfg(test)]if fault==Fault::PartialWrite{return Err(failed(io_error(),materials,true));}
    file.write_all(&suffix[middle..]).map_err(|_|failed(io_error(),materials.clone(),true))?;
    #[cfg(test)]if fault==Fault::FinalFlush{return Err(failed(io_error(),materials,true));}
    file.sync_all().map_err(|_|failed(io_error(),materials.clone(),true))?;
    if read_limited(file,crate::files::EXPORT_OUTPUT_MAX).map_err(|e|failed(e,materials.clone(),true))?!=output.bytes{return Err(failed(refusal(),materials,true));}
    hook("verified");
    let _complete=write_material(&dir,"completed.json",&serialized).map_err(|e|failed(e,materials.clone(),true))?;
    let receipt=VerifiedReceipt{event_id:event,file_key:intent.file_key,old_hash:output.source_hash.clone(),new_hash};
    Ok((receipt,dir,CommitTimings{backup_ms,commit_ms:commit_started.elapsed().as_secs_f64()*1000.0}))
    })();
    match result {
        Ok((receipt,dir,timings))=>{let grant=files.register_original_commit(lease,&output.bytes);Ok(CommitOutcome{receipt,grant,materials:dir,timings})},
        Err(mut failure)=>{
            // Keep a fresh read-only authority even after a failed append. It
            // authorizes recovery inspection only; the live editor is retained.
            if let Ok(current)=read_limited(lease.file_mut(),crate::files::EXPORT_OUTPUT_MAX){failure.recovery_grant=files.register_original_commit(lease,&current).ok();}
            Err(failure)
        }
    }
}
/// Explicit, authorized inspection only. No journal target path is followed and
/// no automatic write/truncate/rename/cleanup occurs, even for a matching prefix.
pub(crate) fn classify_recovery(files:&Files,owner:&str,grant:&str,epoch:u64,dir:&Path)->std::result::Result<RecoveryState,AppError> {
    let mut pins=Vec::new();crate::unicode_worker::pin_chain(dir,&mut pins).map_err(|_|refusal())?;
    let intent:Intent=serde_json::from_slice(&material_bytes(dir,"intent.json",4096)?).map_err(|_|refusal())?;
    if intent.schema!=1||uuid::Uuid::parse_str(&intent.event_id).is_err()||intent.old_len>crate::files::EXPORT_INPUT_MAX||intent.new_len>crate::files::EXPORT_OUTPUT_MAX{return Err(refusal());}
    let old=material_bytes(dir,"verified-old.pdf",crate::files::EXPORT_INPUT_MAX)?;let new=material_bytes(dir,"verified-new.pdf",crate::files::EXPORT_OUTPUT_MAX)?;
    if old.len()!=intent.old_len||new.len()!=intent.new_len||new.len()<=old.len()||!new.starts_with(&old)||crate::save_core::hash(&old)!=intent.old_hash||crate::save_core::hash(&new)!=intent.new_hash{return Err(refusal());}
    let(key,current)=files.original_recovery_snapshot(owner,grant,epoch)?;
    if key!=intent.file_key{return Ok(RecoveryState::ExternalChangePreserved);}
    Ok(if current==new{RecoveryState::CompleteNew}else if current==old{RecoveryState::CompleteOld}else if current.starts_with(&old)&&new.starts_with(&current){RecoveryState::IncompletePreserved}else{RecoveryState::ExternalChangePreserved})
}

#[derive(Serialize)]
#[serde(rename_all="camelCase")]
pub(crate) struct RecoveryItem {event_id:String,state:RecoveryState}
fn recovery_dir(files:&Files,owner:&str,grant:&str,event:&str)->std::result::Result<PathBuf,AppError>{
    if uuid::Uuid::parse_str(event).is_err()||event.len()!=36{return Err(refusal());}
    let path=files.original_path(owner,grant)?;
    Ok(path.parent().ok_or_else(refusal)?.join(format!(".ainative-save-{event}")))
}
pub(crate) fn pending(files:&Files,owner:&str,grant:&str)->std::result::Result<Vec<RecoveryItem>,AppError>{
    let epoch=files.epoch();let(key,_)=files.original_recovery_snapshot(owner,grant,epoch)?;
    let path=files.original_path(owner,grant)?;let parent=path.parent().ok_or_else(refusal)?;
    let mut pins=vec![];crate::unicode_worker::pin_chain(parent,&mut pins).map_err(|_|refusal())?;
    let mut result=Vec::new();
    for entry in fs::read_dir(parent).map_err(|_|refusal())?.take(4096){
        let entry=entry.map_err(|_|refusal())?;let name=entry.file_name();let Some(name)=name.to_str()else{continue;};
        let Some(event)=name.strip_prefix(".ainative-save-")else{continue;};
        if uuid::Uuid::parse_str(event).is_err()||event.len()!=36{continue;}
        let dir=recovery_dir(files,owner,grant,event)?;
        crate::unicode_worker::pin_chain(&dir,&mut pins).map_err(|_|refusal())?;
        let intent:Intent=serde_json::from_slice(&material_bytes(&dir,"intent.json",4096)?).map_err(|_|refusal())?;
        if intent.file_key!=key{continue;}
        if let Ok(completed)=material_bytes(&dir,"acknowledged.json",4096){
            if completed==material_bytes(&dir,"intent.json",4096)?{continue;}
        }
        result.push(RecoveryItem{event_id:event.into(),state:classify_recovery(files,owner,grant,epoch,&dir)?});
        if result.len()>=16{break;}
    }Ok(result)
}
pub(crate) fn acknowledge(outcome:&CommitOutcome)->std::result::Result<(),AppError>{
    let mut pins=vec![];crate::unicode_worker::pin_chain(&outcome.materials,&mut pins).map_err(|_|refusal())?;
    let intent=material_bytes(&outcome.materials,"intent.json",4096)?;
    if material_bytes(&outcome.materials,"completed.json",4096)?!=intent{return Err(refusal());}
    write_material(&outcome.materials,"acknowledged.json",&intent)?;Ok(())
}
pub(crate) fn recovery_bytes(files:&Files,owner:&str,grant:&str,event:&str,newer:bool)->std::result::Result<Vec<u8>,AppError>{
    let dir=recovery_dir(files,owner,grant,event)?;
    let mut pins=vec![];crate::unicode_worker::pin_chain(&dir,&mut pins).map_err(|_|refusal())?;
    // A copied journal is never authority to read another document's materials.
    let intent:Intent=serde_json::from_slice(&material_bytes(&dir,"intent.json",4096)?).map_err(|_|refusal())?;
    let(key,_)=files.original_recovery_snapshot(owner,grant,files.epoch())?;
    if key!=intent.file_key||intent.event_id!=event{return Err(refusal());}
    classify_recovery(files,owner,grant,files.epoch(),&dir)?;
    material_bytes(&dir,if newer{"verified-new.pdf"}else{"verified-old.pdf"},crate::files::EXPORT_OUTPUT_MAX)
}

#[cfg(test)]
fn fixed_output()->(Vec<u8>,ApprovedOutput) {
    let project=crate::save_core::project();let source=fs::read(project.join("tests/work/writer/matrix-source.pdf")).unwrap();
    let bytes=fs::read(project.join("tests/work/writer/mupdf/matrix-first.pdf")).unwrap();
    // Admission is limited to the EXACT independently verified MuPDF regression
    // fixture. These hashes are not a general PDF validation implementation.
    let source_hash=crate::save_core::hash(&source);
    assert_eq!(source_hash,"a8a9d4ac31dbcd6d11238dc5d07918da48f826d545f24a3c4e0fdca370d334a9");
    assert_eq!(crate::save_core::hash(&bytes),"5208cc8103b67fc42b4929290f6e9ecc39a9e480ee713a32447ef9b40ea4423f");
    (source,ApprovedOutput{source_hash,bytes})
}
#[cfg(test)]
fn test_setup()->(PathBuf,Files,Descriptor,ApprovedOutput) {
    let root=crate::save_core::project().join("tests/work/m3").join(format!("original-core-{}",uuid::Uuid::new_v4()));fs::create_dir(&root).unwrap();fs::create_dir(root.join("materials")).unwrap();
    let(source,output)=fixed_output();let path=root.join("合成 原件.pdf");fs::write(&path,source).unwrap();
    let files=Files::default();let descriptor=files.authorize_at("main",&path,files.epoch()).unwrap();(root,files,descriptor,output)
}
/// Tests of M4 receipts use a real controlled append, never a forged constructor.
#[cfg(test)]
pub(crate) fn verified_test_receipt()->VerifiedReceipt {
    let(root,files,grant,output)=test_setup();
    let outcome=commit_approved(&files,"main",&grant.grant_id,files.epoch(),output,&root.join("materials")).unwrap();
    assert!(outcome.grant.is_ok());outcome.receipt
}
/// Bind synthetic desktop data to the live OLD grant before the real controlled
/// append consumes it. Test callers receive the actual refreshed grant/receipt.
#[cfg(test)]
pub(crate) fn real_commit_with_before<T>(before:impl FnOnce(&Files,&Descriptor)->T)->(Files,CommitOutcome,T) {
    let(root,files,grant,output)=test_setup();let context=before(&files,&grant);
    let outcome=commit_approved(&files,"main",&grant.grant_id,files.epoch(),output,&root.join("materials")).unwrap();
    (files,outcome,context)
}
#[cfg(test)]mod tests {
    use super::*;
    use std::{process::{Command,Stdio},time::{Duration,Instant}};
    fn path(root:&Path)->PathBuf {root.join("合成 原件.pdf")}
    fn materials(root:&Path)->PathBuf {fs::read_dir(root.join("materials")).unwrap().next().unwrap().unwrap().path()}
    fn reopen(files:&Files,root:&Path)->Descriptor {files.authorize_at("main",&path(root),files.epoch()).unwrap()}
    // The production admission gate is `ApprovedOutput::checked`. A real
    // `%PDF-1.4` document with a verifiable appended revision must pass it; this
    // is the exact call that reported `writer_original_structure_invalid` for
    // ordinary lower-version PDFs.
    #[test]fn production_admission_accepts_a_real_lower_version_document(){
        for header in ["%PDF-1.3","%PDF-1.4"]{
            let source=crate::incremental::tests::third_party_pdf(header,6,1,5);
            let output=crate::incremental::tests::appended_revision(&source,6,1);
            let prepared=crate::pdf_export::Prepared{bytes:output.clone(),source_hash:crate::save_core::hash(&source),candidate_hash:String::new()};
            let approved=ApprovedOutput::checked(&source,prepared).unwrap_or_else(|e|panic!("{header} refused by production admission: {}",e.code));
            assert_eq!(approved.bytes,output);
            // A revision whose /Prev does not lead back to the held source is
            // still refused: this is the case the structural walk exists for.
            let text=String::from_utf8_lossy(&output).to_string();
            let at=text.rfind("/Prev ").expect("the appended trailer carries /Prev");
            let value=text[at+6..].split(|c:char|!c.is_ascii_digit()).next().unwrap().to_string();
            let shifted=format!("/Prev {}",value.parse::<usize>().unwrap()+1);
            let tampered=text.replacen(&format!("/Prev {value}"),&shifted,1).into_bytes();
            let prepared=crate::pdf_export::Prepared{bytes:tampered,source_hash:crate::save_core::hash(&source),candidate_hash:String::new()};
            assert_eq!(ApprovedOutput::checked(&source,prepared).err().unwrap().code,"writer_original_structure_invalid");
        }
    }    #[test]fn production_admission_and_user_recovery_export_keep_all_materials(){
        let(root,files,grant,output)=test_setup();let old=fs::read(path(&root)).unwrap();let new=output.bytes.clone();
        let approved=ApprovedOutput::checked(&old,crate::pdf_export::Prepared{bytes:new.clone(),source_hash:crate::save_core::hash(&old),candidate_hash:String::new()}).unwrap();
        let failure=commit_inner(&files,"main",&grant.grant_id,files.epoch(),approved,&root,Fault::PartialWrite,|_|{}).err().unwrap();
        let partial=fs::read(path(&root)).unwrap();let opened=reopen(&files,&root);
        let records=pending(&files,"main",&opened.grant_id).unwrap();assert_eq!(records.len(),1);assert_eq!(records[0].state,RecoveryState::IncompletePreserved);
        for newer in [false,true]{
            let bytes=recovery_bytes(&files,"main",&opened.grant_id,&records[0].event_id,newer).unwrap();
            assert_eq!(bytes,if newer{new.clone()}else{old.clone()});
            let target=root.join(if newer{"恢复新版.pdf"}else{"恢复保存前.pdf"});
            let copy=crate::pdf_export::recovery_copy(&files,"main",&opened.grant_id,files.epoch(),&target,&bytes).unwrap();
            assert_eq!(fs::read(target).unwrap(),bytes);files.release("main",&copy.grant_id).unwrap();
        }
        assert_eq!(fs::read(path(&root)).unwrap(),partial);assert!(failure.materials.unwrap().exists());
        assert!(recovery_bytes(&files,"main",&opened.grant_id,"../outside",true).is_err());
        let mut corrupt=new;corrupt.truncate(corrupt.len()-6);assert!(ApprovedOutput::checked(&old,crate::pdf_export::Prepared{bytes:corrupt,source_hash:crate::save_core::hash(&old),candidate_hash:String::new()}).is_err());
    }
    #[test]fn real_grant_handoff_appends_and_republishes_a_readonly_grant() {
        let(root,files,grant,output)=test_setup();let expected=output.bytes.clone();let old=fs::read(path(&root)).unwrap();
        let outcome=commit_inner(&files,"main",&grant.grant_id,files.epoch(),output,&root,Fault::None,|point|if point=="prepared"{let result=Command::new(std::env::current_exe().unwrap()).args(["--exact","original_commit::tests::external_actor","--ignored"]).env("M3_ORIGINAL_CORE_ROOT",&root).env("M3_ORIGINAL_CORE_OP","locked").stdout(Stdio::null()).status().unwrap();assert!(result.success());}).unwrap();
        assert!(files.live("main",&grant.grant_id).is_err());let new=outcome.grant.as_ref().unwrap();assert_ne!(new.grant_id,grant.grant_id);
        let task=files.begin("main",&new.grant_id).unwrap();assert_eq!(files.read("main",&task).unwrap(),expected);
        assert_eq!(fs::read(outcome.materials.join("verified-old.pdf")).unwrap(),old);
        assert_eq!(classify_recovery(&files,"main",&new.grant_id,files.epoch(),&outcome.materials).unwrap(),RecoveryState::CompleteNew);
        assert_eq!(pending(&files,"main",&new.grant_id).unwrap().len(),1);acknowledge(&outcome).unwrap();assert!(pending(&files,"main",&new.grant_id).unwrap().is_empty());
        assert!(OpenOptions::new().write(true).open(path(&root)).is_err());
    }
    #[test]fn active_tasks_aliases_foreign_owner_and_epoch_refuse_without_losing_the_grant() {
        for variant in ["task","alias","owner","epoch"] {
            let(root,files,grant,output)=test_setup();let before=fs::read(path(&root)).unwrap();let mut owner="main";let mut epoch=files.epoch();
            match variant {"task"=>{files.begin("main",&grant.grant_id).unwrap();},"alias"=>{reopen(&files,&root);},"owner"=>owner="foreign",_=>epoch+=1}
            assert!(commit_approved(&files,owner,&grant.grant_id,epoch,output,&root.join("materials")).is_err(),"{variant}");
            assert_eq!(fs::read(path(&root)).unwrap(),before);assert!(files.live("main",&grant.grant_id).is_ok());
        }
    }
    #[test]fn occupied_original_is_not_written_and_returns_a_readonly_authority(){
        let(root,files,grant,output)=test_setup();let before=fs::read(path(&root)).unwrap();
        let _occupied=OpenOptions::new().read(true).share_mode(1|4).open(path(&root)).unwrap();
        let failure=commit_approved(&files,"main",&grant.grant_id,files.epoch(),output,&root).err().unwrap();
        assert!(!failure.original_may_be_partial);assert_eq!(fs::read(path(&root)).unwrap(),before);
        let recovered=failure.recovery_grant.unwrap();assert_ne!(recovered.grant_id,grant.grant_id);
        assert_eq!(files.export_source("main",&recovered.grant_id).unwrap().bytes,before);
    }
    #[test]fn external_replacement_and_unavailable_material_directory_refuse_before_source_writes() {
        let(root,files,grant,output)=test_setup();let old=fs::read(path(&root)).unwrap();
        fs::rename(path(&root),root.join("outside-displaced.pdf")).unwrap();
        let external=[old.clone(),b"\n% external edit\n".to_vec()].concat();fs::write(path(&root),&external).unwrap();
        assert!(commit_approved(&files,"main",&grant.grant_id,files.epoch(),output,&root.join("materials")).is_err());
        assert_eq!(fs::read(path(&root)).unwrap(),external);assert_eq!(fs::read(root.join("outside-displaced.pdf")).unwrap(),old);
        let(root,files,grant,output)=test_setup();let before=fs::read(path(&root)).unwrap();
        assert!(commit_approved(&files,"main",&grant.grant_id,files.epoch(),output,&root.join("missing")).is_err());
        assert_eq!(fs::read(path(&root)).unwrap(),before);assert!(files.live("main",&grant.grant_id).is_ok());
    }
    #[test]fn partial_io_and_uncertain_flush_keep_materials_and_never_truncate_on_recovery() {
        for fault in [Fault::AfterPrepared,Fault::PartialWrite,Fault::FinalFlush] {
            let(root,files,grant,output)=test_setup();let old=fs::read(path(&root)).unwrap();let new=output.bytes.clone();
            let failure=commit_inner(&files,"main",&grant.grant_id,files.epoch(),output,&root.join("materials"),fault,|_|{}).err().unwrap();
            let before=fs::read(path(&root)).unwrap();let reopened=reopen(&files,&root);let dir=failure.materials.unwrap();
            let state=classify_recovery(&files,"main",&reopened.grant_id,files.epoch(),&dir).unwrap();
            assert_eq!(state,if fault==Fault::AfterPrepared{RecoveryState::CompleteOld}else if fault==Fault::FinalFlush{RecoveryState::CompleteNew}else{RecoveryState::IncompletePreserved});
            assert_eq!(fs::read(path(&root)).unwrap(),before);assert_eq!(fs::read(dir.join("verified-old.pdf")).unwrap(),old);assert_eq!(fs::read(dir.join("verified-new.pdf")).unwrap(),new);
            assert!(!dir.join("completed.json").exists());
            assert_eq!(failure.original_may_be_partial,fault!=Fault::AfterPrepared);
        }
    }
    #[test]fn later_matching_prefix_and_changed_identity_are_never_repaired() {
        for replaced in [false,true] {
            let(root,files,grant,output)=test_setup();let new=output.bytes.clone();
            let failure=commit_inner(&files,"main",&grant.grant_id,files.epoch(),output,&root.join("materials"),Fault::PartialWrite,|_|{}).err().unwrap();
            if let Some(ref recovered)=failure.recovery_grant{files.release("main",&recovered.grant_id).unwrap();}
            let mut current=fs::read(path(&root)).unwrap();current.extend_from_slice(&new[current.len()..current.len()+1]);
            if replaced{fs::rename(path(&root),root.join("displaced.pdf")).unwrap();}fs::write(path(&root),&current).unwrap();
            let reopened=reopen(&files,&root);let state=classify_recovery(&files,"main",&reopened.grant_id,files.epoch(),&failure.materials.unwrap()).unwrap();
            assert_eq!(state,if replaced{RecoveryState::ExternalChangePreserved}else{RecoveryState::IncompletePreserved});assert_eq!(fs::read(path(&root)).unwrap(),current);
        }
    }
    #[test]fn recovery_refuses_tampered_or_aliased_materials_and_does_not_modify_source() {
        for variant in ["old","new","intent","hardlink"] {
            let(root,files,grant,output)=test_setup();
            let failure=commit_inner(&files,"main",&grant.grant_id,files.epoch(),output,&root.join("materials"),Fault::PartialWrite,|_|{}).err().unwrap();
            let before=fs::read(path(&root)).unwrap();let dir=failure.materials.unwrap();
            match variant {
                "old"=>{let p=dir.join("verified-old.pdf");let mut bytes=fs::read(&p).unwrap();bytes[10]^=1;fs::write(p,bytes).unwrap();},
                "new"=>{let p=dir.join("verified-new.pdf");let mut bytes=fs::read(&p).unwrap();bytes[10]^=1;fs::write(p,bytes).unwrap();},
                "intent"=>fs::write(dir.join("intent.json"),b"{\"schema\":2}").unwrap(),
                _=>fs::hard_link(dir.join("verified-old.pdf"),dir.join("alias.pdf")).unwrap()
            }
            let reopened=reopen(&files,&root);assert!(classify_recovery(&files,"main",&reopened.grant_id,files.epoch(),&dir).is_err());assert_eq!(fs::read(path(&root)).unwrap(),before);
        }
    }
    #[test]#[ignore="private disposable actor"]fn external_actor() {
        let root=PathBuf::from(std::env::var_os("M3_ORIGINAL_CORE_ROOT").unwrap());crate::save_core::allowed(&root).unwrap();
        match std::env::var("M3_ORIGINAL_CORE_OP").unwrap().as_str(){
            "locked"=>{assert_eq!(OpenOptions::new().write(true).open(path(&root)).unwrap_err().raw_os_error(),Some(32));assert_eq!(fs::rename(path(&root),root.join("external.pdf")).unwrap_err().raw_os_error(),Some(32));assert!(OpenOptions::new().read(true).share_mode(7).open(path(&root)).is_ok());},
            "kill"=>{let files=Files::default();let grant=reopen(&files,&root);let(_,output)=fixed_output();let pause=std::env::var("M3_ORIGINAL_CORE_PAUSE").unwrap();let _=commit_inner(&files,"main",&grant.grant_id,files.epoch(),output,&root.join("materials"),Fault::None,|point|if point==pause{fs::write(root.join("paused.ready"),point).unwrap();std::thread::sleep(Duration::from_secs(15));});panic!("parent did not terminate actor");},
            _=>panic!("private actor operation")
        }
    }
    #[test]fn forced_process_exit_retains_complete_materials_and_authorized_recovery_is_readonly() {
        for point in ["prepared","partial","verified"] {
            let(root,files,grant,_)=test_setup();files.release("main",&grant.grant_id).unwrap();
            let mut child=Command::new(std::env::current_exe().unwrap()).args(["--exact","original_commit::tests::external_actor","--ignored"]).env("M3_ORIGINAL_CORE_ROOT",&root).env("M3_ORIGINAL_CORE_OP","kill").env("M3_ORIGINAL_CORE_PAUSE",point).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
            let start=Instant::now();while !root.join("paused.ready").exists(){assert!(start.elapsed()<Duration::from_secs(12));std::thread::sleep(Duration::from_millis(10));}
            child.kill().unwrap();assert!(!child.wait().unwrap().success());
            let before=fs::read(path(&root)).unwrap();let reopened=reopen(&files,&root);let state=classify_recovery(&files,"main",&reopened.grant_id,files.epoch(),&materials(&root)).unwrap();
            assert_eq!(state,match point{"prepared"=>RecoveryState::CompleteOld,"partial"=>RecoveryState::IncompletePreserved,_=>RecoveryState::CompleteNew});assert_eq!(fs::read(path(&root)).unwrap(),before);
        }
    }
}
