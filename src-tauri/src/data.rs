//! Grant-bound local data. No renderer path/SQL/document-ID authority.
use crate::{error::{AppError,Result},files::{Files,Observation},storage::{self,Store,model::{IdentityMatch,desktop::{HistoryMessage,ReadingPosition,DraftRecord}},directories::{DataManager,DirectoryInfo}}};
use serde::Serialize;
use std::{collections::HashMap,path::Path,sync::Mutex};
#[derive(Clone)] pub(crate) struct Binding {pub owner:String,pub grant:String,pub epoch:u64,pub generation:u64,pub document:String,pub revision:String,pub conversation:String,pub display_name:String}
#[derive(Serialize)] #[serde(rename_all="camelCase")]
pub(crate) struct BoundDocument {pub binding_id:String,pub document_id:String,pub revision_id:String,pub position:Option<ReadingPosition>,pub identity_notice:Option<String>}
#[derive(Serialize)] #[serde(rename_all="camelCase")]
pub(crate) struct Status {pub available:bool,pub path:String,pub areas:Vec<String>,pub acl:String,pub generation:u64,pub error:Option<String>}
#[derive(Serialize)] #[serde(rename_all="camelCase")]
pub(crate) struct DataSettings {pub schema_version:u32,pub thumbnail_open:bool,pub cache_bytes:u64}
impl DataSettings {
    pub(crate) fn read(store:&Store)->storage::Result<Self>{let settings=store.settings()?;Ok(Self{schema_version:1,thumbnail_open:settings.as_ref().is_some_and(|s|s.thumbnail_open),cache_bytes:settings.map(|s|s.cache_bytes).unwrap_or(64*1024*1024)})}
}
pub(crate) struct DataService {pub manager:Option<DataManager>,pub bindings:HashMap<String,Binding>,failure:Option<String>,failure_path:String}
fn initialization_message(failure:&storage::directories::InitializationFailure)->String {
    let stage=match failure.stage{"locator-parent"=>"准备默认目录","locator-create"=>"创建定位目录","locator-verify"=>"校验定位目录","locator-read"=>"读取定位记录","active-data-verify"=>"校验已有数据库","data-parent"=>"准备持久数据目录","data-create"=>"创建数据目录","sqlite-open"=>"初始化 SQLite","activation-commit"=>"提交定位记录",_=>"初始化本地数据"};
    let reason=match &failure.cause{
        storage::DataError::Refused(reason)=>{let explanation=match *reason{"foreign private owner"=>"文件所有者不是当前用户或受信系统主体","private owner/DACL required"=>"无法确认文件所有者或有效权限","data exposed to other principal"=>"数据权限授予了其他账户","protected root DACL required"=>"数据根目录权限未独立保护","data parent writable by other users; choose a private folder"=>"父目录允许其他普通用户修改或删除数据","foreign data directory"=>"目录身份标记不属于当前用户","incomplete private DACL"=>"私有目录缺少必要权限","unsupported parent ACL entry"=>"父目录含尚不能安全判定的权限项",_=>"安全或完整性检查拒绝"};format!("{explanation}（{reason}）")},
        storage::DataError::Io(error)=>match error.raw_os_error(){Some(5)=>"当前用户无访问权限（Windows 5）".into(),Some(32)=>"数据文件被占用（Windows 32）".into(),Some(code @ (2|3))=>format!("必要目录或身份标记不存在（Windows {code}）"),Some(112)=>"磁盘空间不足（Windows 112）".into(),Some(code)=>format!("文件操作失败（Windows {code}）"),None=>"文件操作未完成".into()},
        storage::DataError::Sql(rusqlite::Error::SqliteFailure(error,_))=>format!("数据库操作失败（SQLite {}）",error.extended_code),
        storage::DataError::Sql(_)=>"数据库初始化或完整性检查未完成".into(),
    };
    format!("本地数据暂不可用：{stage}失败；{reason}。聊天与原位解释已暂停，阅读仍可用；已有数据和 PDF 未被清空。诊断阶段：{}。",failure.stage)
}
pub(crate) fn error(error:storage::DataError)->AppError {
    // Filesystem paths, SQL and content never enter frontend error text/logs.
    if matches!(error,storage::DataError::Refused("chat node budget"|"chat path budget")){return AppError{code:"chat_budget",message:"会话记录超过本轮安全处理上限，操作未执行；现有记录保留，请新建会话或整理历史。"};}
    if matches!(error,storage::DataError::Refused("chat archived")){return AppError{code:"chat_archived",message:"此会话已归档，请先恢复后再继续提问。"};}
    if matches!(error,storage::DataError::Refused("stale chat plan")){return AppError{code:"chat_changed",message:"会话选择已改变，请重新准备本轮问题。"};}
    let code=match error{storage::DataError::Refused(_)=>"data_refused",storage::DataError::Sql(_)=>"database_unavailable",storage::DataError::Io(_)=>"data_unavailable"};
    AppError{code,message:"本地数据操作未完成，已有历史和草稿保持不变。请检查数据目录权限、空间或备份。"}
}
impl DataService {
    pub(crate) fn open(locator:&Path,parent:&Path)->Self {
        match DataManager::open_reported(locator,parent,64*1024*1024){Ok(manager)=>Self{manager:Some(manager),bindings:HashMap::new(),failure:None,failure_path:String::new()},Err(failure)=>Self{manager:None,bindings:HashMap::new(),failure:Some(initialization_message(&failure)),failure_path:locator.parent().unwrap_or(locator).to_string_lossy().into_owned()}}
    }
    pub(crate) fn status(&self)->Status {
        if let Some(manager)=&self.manager{let DirectoryInfo{path,areas,acl,generation,warning,..}=manager.info();Status{available:true,path,areas,acl,generation,error:warning}}
        else{Status{available:false,path:self.failure_path.clone(),areas:vec![],acl:String::new(),generation:0,error:self.failure.clone()}}
    }
    pub(crate) fn bind(&mut self,owner:&str,grant:&str,epoch:u64,observed:Observation,pages:u32)->Result<BoundDocument> {
        if pages==0||pages>2000||self.bindings.len()>=64{return Err(AppError::busy());}
        let manager=self.manager.as_mut().ok_or(AppError{code:"data_unavailable",message:"本地数据尚未初始化，聊天与原位解释不可用；请查看本地数据状态。"})?;
        let generation=manager.info().generation;let store=manager.store_mut();
        let (document,revision,notice)=match store.identify(&observed.key,&observed.hash).map_err(error)? {
            IdentityMatch::Known(id)=>{let(revision,count)=store.current_revision(&id).map_err(error)?;if count!=pages{return Err(AppError::changed());}(id,revision,None)},
            other=>{let(doc,rev)=store.create_document(&observed.key,&observed.hash,pages,&observed.name).map_err(error)?;
                (doc,rev,if matches!(other,IdentityMatch::Confirm(_)){Some("检测到副本或内容变化，已建立独立历史；旧历史保留，未自动合并。".into())}else{None})}
        };
        let conversation=store.local_conversation(&document).map_err(error)?;
        let position=store.desktop_position(&document,&revision).map_err(error)?;
        let id=uuid::Uuid::new_v4().to_string();
        self.bindings.insert(id.clone(),Binding{owner:owner.into(),grant:grant.into(),epoch,generation,document:document.clone(),revision:revision.clone(),conversation,display_name:observed.name});
        Ok(BoundDocument{binding_id:id,document_id:document,revision_id:revision,position,identity_notice:notice})
    }
    pub(crate) fn revoke(&mut self){self.bindings.clear();}
    pub(crate) fn revoke_before(&mut self,epoch:u64){self.bindings.retain(|_,b|b.epoch>=epoch);}
    pub(crate) fn release_grant(&mut self,owner:&str,grant:&str){self.bindings.retain(|_,b|b.owner!=owner||b.grant!=grant);}
}
pub(crate) fn with_binding<T>(files:&Files,data:&Mutex<DataService>,owner:&str,id:&str,operation:impl FnOnce(&mut Store,&Binding)->storage::Result<T>)->Result<T> {
    let binding=data.lock().map_err(|_|AppError::io())?.bindings.get(id).filter(|b|b.owner==owner).cloned().ok_or_else(AppError::denied)?;
    files.with_live(owner,&binding.grant,binding.epoch,||{
        let mut service=data.try_lock().map_err(|_|AppError::busy())?;
        if !service.bindings.contains_key(id){return Err(AppError::denied());}
        let store=service.manager.as_mut().ok_or_else(AppError::io)?.store_mut();operation(store,&binding).map_err(error)
    })
}
pub(crate) fn history(files:&Files,data:&Mutex<DataService>,owner:&str,id:&str)->Result<Vec<HistoryMessage>> {with_binding(files,data,owner,id,|s,b|s.local_messages(&b.conversation))}
pub(crate) fn with_generation<T>(files:&Files,data:&Mutex<DataService>,owner:&str,id:&str,generation:u64,operation:impl FnOnce(&mut Store,&Binding)->storage::Result<T>)->Result<T>{
    let binding=data.lock().map_err(|_|AppError::io())?.bindings.get(id).filter(|b|b.owner==owner).cloned().ok_or_else(AppError::denied)?;
    files.with_live(owner,&binding.grant,binding.epoch,||{
        let mut service=data.try_lock().map_err(|_|AppError::busy())?;
        if !service.bindings.contains_key(id)||service.manager.as_ref().is_none_or(|m|m.info().generation!=generation){return Err(AppError::cancelled());}
        operation(service.manager.as_mut().unwrap().store_mut(),&binding).map_err(error)
    })
}
pub(crate) fn drafts(files:&Files,data:&Mutex<DataService>,owner:&str,id:&str)->Result<Vec<DraftRecord>> {with_binding(files,data,owner,id,|s,b|s.local_drafts(&b.document,&b.revision))}
/// Internal post-commit handoff, deliberately absent from commands/IPC. The
/// receipt proves a PDF commit already occurred; a subsequent database failure
/// must never be described as an unchanged PDF or cause a second append.
pub(crate) struct CommittedBinding {pub bound:BoundDocument,pub pending_rebase:usize}
fn history_pending()->AppError {AppError{code:"writer_history_pending",message:"PDF 提交已发生，但本地会话关联未完成；历史、草稿和恢复材料保留，请核对已保存文件，不要重复提交。"}}
pub(crate) fn bind_verified_commit(files:&Files,data:&Mutex<DataService>,owner:&str,old_id:&str,new_grant:&str,receipt:&crate::original_commit::VerifiedReceipt,frozen:Option<&storage::model::desktop::saves::SaveDraftSnapshot>)->Result<CommittedBinding> {
    let old=data.try_lock().map_err(|_|history_pending())?.bindings.get(old_id).filter(|b|b.owner==owner).cloned().ok_or_else(AppError::denied)?;
    if old.grant==new_grant||files.live(owner,&old.grant).is_ok(){return Err(AppError::denied());}
    let observed=files.observe(owner,new_grant)?;
    if observed.key!=receipt.file_key()||observed.hash!=receipt.new_hash(){return Err(AppError::changed());}
    files.with_live(owner,new_grant,old.epoch,||{
        let mut service=data.try_lock().map_err(|_|history_pending())?;
        let live=service.bindings.get(old_id).filter(|b|b.owner==owner&&b.grant==old.grant&&b.epoch==old.epoch&&b.generation==old.generation&&b.document==old.document&&b.revision==old.revision&&b.conversation==old.conversation).cloned().ok_or_else(AppError::denied)?;
        let manager=service.manager.as_mut().ok_or_else(history_pending)?;
        if manager.info().generation!=live.generation{return Err(AppError::cancelled());}
        let store=manager.store_mut();let(_,pages)=store.current_revision(&live.document).map_err(|_|history_pending())?;
        let applied=store.apply_verified_save(receipt,&live.document,&live.revision,pages,frozen).map_err(|_|history_pending())?;
        if !applied.is_current{return Err(history_pending());}
        let position=store.desktop_position(&live.document,&applied.revision).map_err(|_|history_pending())?;
        let id=uuid::Uuid::new_v4().to_string();
        let bound=BoundDocument{binding_id:id.clone(),document_id:live.document.clone(),revision_id:applied.revision.clone(),position,identity_notice:Some("PDF 已提交，历史已关联新版本；旧恢复稿保留，未自动重放。".into())};
        // A fresh opaque ID fences delayed AI/data operations. Never reuse the
        // old ID with a different revision, or rebase old full-draft JSON here.
        service.release_grant(owner,&live.grant);
        service.bindings.insert(id,Binding{owner:owner.into(),grant:new_grant.into(),epoch:live.epoch,generation:live.generation,document:live.document,revision:applied.revision,conversation:live.conversation,display_name:observed.name});
        Ok(CommittedBinding{bound,pending_rebase:applied.pending_rebase})
    })
}
#[cfg(test)] #[path="data_save_receipt_tests.rs"] mod save_receipt_tests;
#[cfg(test)] mod tests {
    use super::*;
    use crate::storage::model::desktop::Zoom;
    fn root()->std::path::PathBuf{let root=Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("tests/work/m4").join(uuid::Uuid::new_v4().to_string());std::fs::create_dir_all(&root).unwrap();root}
    #[test]fn failed_startup_preserves_materials_and_reports_a_safe_stage_instead_of_hiding_chat(){
        let base=root();let parent=base.join("not-directory");std::fs::write(&parent,b"retain synthetic material").unwrap();
        let service=DataService::open(&parent.join("locator"),&base.join("data"));let status=service.status();
        assert!(!status.available);assert!(status.error.unwrap().contains("locator-parent"));assert_eq!(status.path,parent.to_string_lossy());assert_eq!(std::fs::read(parent).unwrap(),b"retain synthetic material");assert!(!base.join("data").exists());
        let failure=storage::directories::InitializationFailure{stage:"sqlite-open",cause:storage::DataError::Sql(rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(11),Some("synthetic private SQL text".into())))};
        let message=initialization_message(&failure);assert!(message.contains("SQLite 11"));assert!(!message.contains("private SQL"));
        let denied=storage::directories::InitializationFailure{stage:"locator-create",cause:std::io::Error::from_raw_os_error(5).into()};assert!(initialization_message(&denied).contains("Windows 5"));
    }
    fn fixture(root:&Path,name:&str,content:&[u8])->std::path::PathBuf {let path=root.join(name);std::fs::write(&path,content).unwrap();path}
    fn bind(files:&Files,service:&Mutex<DataService>,grant:&str)->BoundDocument {
        let observed=files.observe("main",grant).unwrap();let epoch=files.epoch();files.with_live("main",grant,epoch,||service.lock().unwrap().bind("main",grant,epoch,observed,3)).unwrap()
    }
    #[test] fn desktop_bind_history_drafts_settings_position_restart_and_isolation(){
        let root=root();let locator=root.join("locator");let parent=root.join("data");let files=Files::default();
        let source=fixture(&root,"中文 空格.pdf",b"%PDF-1.7\nsynthetic controlled bytes");
        let descriptor=files.authorize_at("main",&source,files.epoch()).unwrap();let service=Mutex::new(DataService::open(&locator,&parent));assert!(service.lock().unwrap().status().available);
        let first=bind(&files,&service,&descriptor.grant_id);
        let prompt=with_binding(&files,&service,"main",&first.binding_id,|s,b|s.add_local_prompt(&b.conversation,&b.revision,None,"原提问",2)).unwrap();
        let branch=with_binding(&files,&service,"main",&first.binding_id,|s,b|s.edit_local_prompt(&b.conversation,&prompt,"修改的提问")).unwrap();
        with_binding(&files,&service,"main",&first.binding_id,|s,b|s.local_regenerate(&b.conversation,&b.revision,&branch)).unwrap();
        let items=history(&files,&service,"main",&first.binding_id).unwrap();assert_eq!(items.len(),3);assert_eq!(items[1].edited_from.as_deref(),Some(prompt.as_str()));assert_eq!(items[1].page,Some(2));assert_eq!(items[2].page,Some(2));assert_eq!(items[0].content,"原提问");
        let position=ReadingPosition{page:2,zoom:Zoom::Fit("page-fit".into()),top:-0.4,left:0.25};let draft=uuid::Uuid::new_v4().to_string();
        let json=r#"{"schema":1,"pdfjs":"6.4.299","entries":[{"annotationType":3,"pageIndex":1,"rect":[1,2,30,40],"rotation":0,"value":"中文 α ≤ β","fontSize":12,"color":[0,0,0]}]}"#;
        with_binding(&files,&service,"main",&first.binding_id,|s,b|{s.put_desktop_position(&b.document,&b.revision,&position)?;s.put_local_draft(&b.document,&b.revision,&draft,json)?;s.set_default_zoom("page-fit")?;s.update_settings(true,1024*1024)?;s.cache_put(b"rebuild")?;s.clear_rebuildable_cache()?;Ok(())}).unwrap();
        assert_eq!(drafts(&files,&service,"main",&first.binding_id).unwrap()[0].json,json);
        let other=fixture(&root,"other.pdf",b"%PDF-1.7\ndifferent content");let desc=files.authorize_at("main",&other,files.epoch()).unwrap();let second=bind(&files,&service,&desc.grant_id);
        assert!(history(&files,&service,"main",&second.binding_id).unwrap().is_empty());
        assert!(with_binding(&files,&service,"main",&second.binding_id,|s,b|s.edit_local_prompt(&b.conversation,&prompt,"cross-document")).is_err());
        assert!(history(&files,&service,"foreign",&first.binding_id).is_err());assert!(history(&files,&service,"main","fake").is_err());
        with_binding(&files,&service,"main",&first.binding_id,|s,b|s.hide_local_message(&b.conversation,&prompt)).unwrap();
        assert!(history(&files,&service,"main",&first.binding_id).unwrap()[0].deleted);
        assert!(with_binding(&files,&service,"main",&first.binding_id,|s,b|s.local_regenerate(&b.conversation,&b.revision,&prompt)).is_err());
        drop(service);
        let service=Mutex::new(DataService::open(&locator,&parent));let reopened=bind(&files,&service,&descriptor.grant_id);
        assert_eq!(reopened.document_id,first.document_id);assert_eq!(reopened.position,Some(position));
        assert_eq!(history(&files,&service,"main",&reopened.binding_id).unwrap().len(),3);assert_eq!(drafts(&files,&service,"main",&reopened.binding_id).unwrap().len(),1);
        let manager=service.lock().unwrap();let store=manager.manager.as_ref().unwrap().store();assert_eq!(store.default_zoom().unwrap().as_deref(),Some("page-fit"));assert!(store.settings().unwrap().unwrap().thumbnail_open);drop(manager);
        let moved=source.with_file_name("已移动.pdf");std::fs::rename(&source,&moved).unwrap();let renamed=files.authorize_at("main",&moved,files.epoch()).unwrap();assert_eq!(bind(&files,&service,&renamed.grant_id).document_id,first.document_id);
        let copy=source.with_file_name("copy.pdf");std::fs::copy(&moved,&copy).unwrap();let copied=files.authorize_at("main",&copy,files.epoch()).unwrap();let new=bind(&files,&service,&copied.grant_id);assert_ne!(new.document_id,first.document_id);assert!(new.identity_notice.is_some());
        files.revoke_all();assert!(history(&files,&service,"main",&reopened.binding_id).is_err());
    }
    #[test] fn bound_operations_are_busy_during_migration_and_stale_epochs_cannot_publish(){
        let root=root();let files=Files::default();let p=fixture(&root,"test.pdf",b"%PDF-1.7\ncontrolled");let d=files.authorize_at("main",&p,files.epoch()).unwrap();
        let service=std::sync::Arc::new(Mutex::new(DataService::open(&root.join("locator"),&root.join("data"))));let bound=bind(&files,&service,&d.grant_id);
        let binding=service.lock().unwrap().bindings[&bound.binding_id].clone();let held=service.lock().unwrap();
        let result=files.with_live("main",&binding.grant,binding.epoch,||{let _data=service.try_lock().map_err(|_|AppError::busy())?;Ok(())});assert_eq!(result.unwrap_err().code,"busy");drop(held);
        let epoch=files.epoch();let observed=files.observe("main",&d.grant_id).unwrap();files.revoke_all();
        assert!(files.with_live("main",&d.grant_id,epoch,||service.lock().unwrap().bind("main",&d.grant_id,epoch,observed,3)).is_err());
        assert!(history(&files,&service,"main",&bound.binding_id).is_err());
        let fresh=files.authorize_at("main",&p,files.epoch()).unwrap();let new_binding=bind(&files,&service,&fresh.grant_id);
        service.lock().unwrap().revoke_before(files.epoch());
        assert!(history(&files,&service,"main",&new_binding.binding_id).is_ok());
        assert!(!service.lock().unwrap().bindings.contains_key(&bound.binding_id));
    }
    #[test] fn recent_history_is_bounded_and_newest_messages_stay_visible(){
        let root=root();let mut service=DataService::open(&root.join("locator"),&root.join("data"));let store=service.manager.as_mut().unwrap().store_mut();
        let(doc,rev)=store.create_document("synthetic-key",&"a".repeat(64),3,"test.pdf").unwrap();let conv=store.local_conversation(&doc).unwrap();
        for n in 0..105{store.add_local_prompt(&conv,&rev,None,&format!("message {n}"),1).unwrap();}
        let history=store.local_messages(&conv).unwrap();assert_eq!(history.len(),100);assert_eq!(history[0].content,"message 5");assert_eq!(history[99].content,"message 104");
        assert!(store.put_desktop_position(&doc,&rev,&ReadingPosition{page:4,zoom:Zoom::Scale(1.0),top:0.0,left:0.0}).is_err());
        assert!(store.put_local_draft(&doc,&rev,&uuid::Uuid::new_v4().to_string(),r#"{"schema":99}"#).is_err());
    }
}
