//! Real guarded PDF commits plus isolated project-only M4 databases. No IPC,
//! desktop automation, Provider, personal document, or production save enablement.
use super::*;
use crate::storage::model::desktop::{Zoom,saves::SaveDraftSnapshot};
use std::{fs,path::PathBuf};
struct Context {root:PathBuf,service:Mutex<DataService>,bound:BoundDocument,draft:String,frozen:SaveDraftSnapshot,conversation:String}
fn fixture()->(Files,crate::original_commit::CommitOutcome,Context) {
    crate::original_commit::real_commit_with_before(|files,descriptor|{
        let root=Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("tests/work/m4").join(format!("save-binding-{}",uuid::Uuid::new_v4()));fs::create_dir_all(&root).unwrap();
        let service=Mutex::new(DataService::open(&root.join("locator"),&root.join("data")));assert!(service.lock().unwrap().status().available);
        let observed=files.observe("main",&descriptor.grant_id).unwrap();
        let bound=files.with_live("main",&descriptor.grant_id,files.epoch(),||service.lock().unwrap().bind("main",&descriptor.grant_id,files.epoch(),observed,4)).unwrap();
        let draft=uuid::Uuid::new_v4().to_string();
        let frozen=with_binding(files,&service,"main",&bound.binding_id,|store,binding|{
            let prompt=store.add_local_prompt(&binding.conversation,&binding.revision,None,"合成历史 原提问",3)?;
            store.edit_local_prompt(&binding.conversation,&prompt,"保留编辑分支")?;
            store.put_desktop_position(&binding.document,&binding.revision,&ReadingPosition{page:3,zoom:Zoom::Fit("page-fit".into()),top:-0.3,left:0.2})?;
            store.put_local_draft(&binding.document,&binding.revision,&draft,r#"{"schema":1,"pdfjs":"6.4.299","entries":[{"annotationType":3,"pageIndex":2,"rect":[1,2,30,40],"rotation":0,"value":"保留合成草稿 中文 α ≤ β","fontSize":12,"color":[0,0,0]}]}"#)?;
            store.capture_save_draft(&binding.document,&binding.revision,&draft)
        }).unwrap();
        let conversation=service.lock().unwrap().bindings[&bound.binding_id].conversation.clone();
        Context{root,service,bound,draft,frozen,conversation}
    })
}
fn new_grant(outcome:&crate::original_commit::CommitOutcome)->&str {&outcome.grant.as_ref().unwrap().grant_id}
fn retained_draft(context:&Context)->usize {context.service.lock().unwrap().manager.as_ref().unwrap().store().local_drafts(&context.bound.document_id,&context.bound.revision_id).unwrap().len()}

#[test]
fn real_commit_rebinds_same_document_and_history_with_new_id_and_restores_after_restart(){
    let(files,outcome,context)=fixture();let new_grant=new_grant(&outcome);
    let result=bind_verified_commit(&files,&context.service,"main",&context.bound.binding_id,new_grant,&outcome.receipt,Some(&context.frozen)).unwrap();
    assert_ne!(result.bound.binding_id,context.bound.binding_id);assert_eq!(result.bound.document_id,context.bound.document_id);assert_ne!(result.bound.revision_id,context.bound.revision_id);assert_eq!(result.pending_rebase,1);
    assert_eq!(result.bound.position,Some(ReadingPosition{page:3,zoom:Zoom::Fit("page-fit".into()),top:-0.3,left:0.2}));
    assert_eq!(history(&files,&context.service,"main",&result.bound.binding_id).unwrap().len(),2);
    assert!(history(&files,&context.service,"main",&context.bound.binding_id).is_err());
    assert!(with_binding(&files,&context.service,"main",&context.bound.binding_id,|s,b|s.add_local_prompt(&b.conversation,&b.revision,None,"迟到任务不能附加新版本",1)).is_err());
    assert!(drafts(&files,&context.service,"main",&result.bound.binding_id).unwrap().is_empty());assert_eq!(retained_draft(&context),1);
    {let service=context.service.lock().unwrap();let live=&service.bindings[&result.bound.binding_id];assert_eq!(live.conversation,context.conversation);assert!(!service.bindings.contains_key(&context.bound.binding_id));
        assert_eq!(service.manager.as_ref().unwrap().store().local_drafts(&context.bound.document_id,&context.bound.revision_id).unwrap()[0].id,context.draft);}
    // Reopening the same physical committed PDF identifies the same document,
    // not an unrelated new history selected by its display name.
    let Context{root,service,bound,..}=context;drop(service);
    let service=Mutex::new(DataService::open(&root.join("locator"),&root.join("data")));let observed=files.observe("main",new_grant).unwrap();
    let reopened=files.with_live("main",new_grant,files.epoch(),||service.lock().unwrap().bind("main",new_grant,files.epoch(),observed,4)).unwrap();
    assert_eq!(reopened.document_id,bound.document_id);assert_eq!(reopened.revision_id,result.bound.revision_id);assert_eq!(reopened.position,result.bound.position);assert_eq!(history(&files,&service,"main",&reopened.binding_id).unwrap().len(),2);
}

#[test]
fn wrong_grant_owner_receipt_and_revocation_refuse_without_rebinding_or_losing_history(){
    let(files,outcome,context)=fixture();let new=new_grant(&outcome);
    let old_grant=context.service.lock().unwrap().bindings[&context.bound.binding_id].grant.clone();
    assert!(bind_verified_commit(&files,&context.service,"main",&context.bound.binding_id,&old_grant,&outcome.receipt,None).is_err());
    assert!(bind_verified_commit(&files,&context.service,"foreign",&context.bound.binding_id,new,&outcome.receipt,None).is_err());
    let other=context.root.join("unrelated.pdf");fs::write(&other,b"%PDF-1.7\nsynthetic authorization-only negative fixture").unwrap();
    let wrong=files.authorize_at("main",&other,files.epoch()).unwrap();
    assert_eq!(bind_verified_commit(&files,&context.service,"main",&context.bound.binding_id,&wrong.grant_id,&outcome.receipt,None).err().unwrap().code,"file_changed");
    let mismatched=crate::original_commit::verified_test_receipt();
    assert_eq!(bind_verified_commit(&files,&context.service,"main",&context.bound.binding_id,new,&mismatched,None).err().unwrap().code,"file_changed");
    assert_eq!(retained_draft(&context),1);assert_eq!(context.service.lock().unwrap().bindings.len(),1);
    files.revoke_all();assert!(bind_verified_commit(&files,&context.service,"main",&context.bound.binding_id,new,&outcome.receipt,None).is_err());
    let service=context.service.lock().unwrap();let store=service.manager.as_ref().unwrap().store();assert_eq!(store.current_revision(&context.bound.document_id).unwrap().0,context.bound.revision_id);assert_eq!(store.local_messages(&context.conversation).unwrap().len(),2);
}

#[test]
fn unavailable_database_reports_post_commit_pending_and_retries_only_association(){
    let(files,outcome,context)=fixture();let new=new_grant(&outcome);
    let manager=context.service.lock().unwrap().manager.take();
    let error=bind_verified_commit(&files,&context.service,"main",&context.bound.binding_id,new,&outcome.receipt,Some(&context.frozen)).err().unwrap();
    assert_eq!(error.code,"writer_history_pending");assert!(error.message.contains("PDF 提交已发生"));assert!(context.service.lock().unwrap().bindings.contains_key(&context.bound.binding_id));assert!(files.live("main",new).is_ok());
    context.service.lock().unwrap().manager=manager;
    let rebound=bind_verified_commit(&files,&context.service,"main",&context.bound.binding_id,new,&outcome.receipt,Some(&context.frozen)).unwrap();
    assert_eq!(rebound.pending_rebase,1);assert_eq!(retained_draft(&context),1);
    assert!(bind_verified_commit(&files,&context.service,"main",&context.bound.binding_id,new,&outcome.receipt,Some(&context.frozen)).is_err());
    assert_eq!(history(&files,&context.service,"main",&rebound.bound.binding_id).unwrap().len(),2);
}

#[test]
fn actual_project_data_migration_invalidates_old_binding_generation(){
    let(files,outcome,context)=fixture();let new=new_grant(&outcome);let target=context.root.join("migrated");fs::create_dir(&target).unwrap();
    let before=context.service.lock().unwrap().bindings[&context.bound.binding_id].generation;
    let after=context.service.lock().unwrap().manager.as_mut().unwrap().migrate_to(&target,"").unwrap().generation;assert!(after>before);
    // Deliberately leave the old binding in memory to prove that generation
    // validation works even before normal migration cleanup clears bindings.
    let error=bind_verified_commit(&files,&context.service,"main",&context.bound.binding_id,new,&outcome.receipt,Some(&context.frozen)).err().unwrap();assert_eq!(error.code,"cancelled");
    assert_eq!(retained_draft(&context),1);let service=context.service.lock().unwrap();let store=service.manager.as_ref().unwrap().store();assert_eq!(store.current_revision(&context.bound.document_id).unwrap().0,context.bound.revision_id);assert_eq!(store.local_messages(&context.conversation).unwrap().len(),2);
}
