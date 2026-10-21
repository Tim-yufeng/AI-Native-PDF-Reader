use super::*;
use crate::storage::model::{IdentityMatch,MessageMetadata};
use super::super::Zoom;
const PAGES:u32=4; // Same real matrix-source/first fixture as the core receipt.

fn fixture()->(Store,VerifiedReceipt,String,String) {
    // This helper performs the guarded append on a real project PDF and only
    // then returns the receipt. No synthetic-save event stands in for it.
    let receipt=crate::original_commit::verified_test_receipt();
    let base=Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("tests/work/m4");
    fs::create_dir_all(&base).unwrap();let root=base.join(uuid());fs::create_dir(&root).unwrap();
    let mut store=Store::open_synthetic(&root,16384).unwrap();
    let(doc,revision)=store.create_document(receipt.file_key(),receipt.old_hash(),PAGES,"synthetic-save.pdf").unwrap();
    (store,receipt,doc,revision)
}
// Drafts below are synthetic SQL lifecycle inputs; they are not asserted to
// describe the annotations in the fixed PDF fixture. These tests establish
// preservation and pending association, not production snapshot inclusion.
fn json(text:&str)->String {
    serde_json::json!({"schema":1,"pdfjs":"6.4.299","entries":[{"annotationType":3,"pageIndex":0,"rect":[1,2,30,40],"rotation":0,"value":text,"fontSize":12,"color":[0,0,0]}]}).to_string()
}
fn count(store:&Store,table:&str)->i64 {store.conn.query_row(&format!("SELECT count(*) FROM {table}"),[],|r|r.get(0)).unwrap()}
fn position()->ReadingPosition {ReadingPosition{page:1,zoom:Zoom::Fit("page-fit".into()),top:-0.4,left:0.25}}
fn history(store:&mut Store,doc:&str,revision:&str)->(String,String) {
    let conv=store.conversation(doc).unwrap();
    let meta=MessageMetadata{provider:Some("mock".into()),model:Some("synthetic".into()),preset_version:None,request_id:None,sent_context_hash:None,slot:None};
    let prompt=store.message(&conv,revision,None,"保留旧问题",false,&meta).unwrap();
    store.edit_prompt(&prompt,"保留旧分支").unwrap();
    let anchor=store.anchor(&prompt,revision,1,[0.,0.,10.,10.]).unwrap();(conv,anchor)
}

#[test]
fn verified_receipt_preserves_even_frozen_drafts_history_positions_and_reopens_idempotently(){
    let(mut store,receipt,doc,old)=fixture();let root=store.root.path.clone();let(conv,anchor)=history(&mut store,&doc,&old);
    store.put_desktop_position(&doc,&old,&position()).unwrap();
    let id=uuid();let text=json("已保存 中文 α ≤ β");store.put_local_draft(&doc,&old,&id,&text).unwrap();
    let frozen=store.capture_save_draft(&doc,&old,&id).unwrap();
    let raw:String=store.conn.query_row("SELECT value FROM data_meta WHERE key=?1",[draft_key(&doc,&old,&id)],|r|r.get(0)).unwrap();
    let reference:DraftRef=serde_json::from_str(&raw).unwrap();
    let applied=store.apply_verified_save(&receipt,&doc,&old,PAGES,Some(&frozen)).unwrap();
    assert!(applied.is_current&&!applied.draft_acknowledged);assert_eq!(applied.pending_rebase,1);
    assert_eq!(store.current_revision(&doc).unwrap().0,applied.revision);assert_eq!(store.messages(&conv).unwrap().len(),2);
    assert!(!store.anchor_current(&anchor).unwrap());assert_eq!(store.desktop_position(&doc,&applied.revision).unwrap(),Some(position()));
    assert_eq!(store.desktop_position(&doc,&old).unwrap(),Some(position()));
    assert_eq!(store.read_position(&doc,&applied.revision).unwrap(),Some((1,100.0,1.6)));
    assert_eq!(store.local_drafts(&doc,&old).unwrap()[0].json,text);assert!(store.local_drafts(&doc,&applied.revision).unwrap().is_empty());
    assert_eq!(store.recovery_get(&reference.file).unwrap(),text.as_bytes());
    assert_eq!(store.identify(receipt.file_key(),receipt.new_hash()).unwrap(),IdentityMatch::Known(doc.clone()));
    assert_eq!(store.apply_verified_save(&receipt,&doc,&old,PAGES,Some(&frozen)).unwrap(),applied);
    assert!(store.apply_verified_save(&receipt,&doc,&old,PAGES,None).is_err());assert_eq!(count(&store,"revisions"),2);assert_eq!(count(&store,"save_events"),1);
    store.conn.execute("UPDATE revisions SET hash=?1 WHERE id=?2",params!["e".repeat(64),applied.revision]).unwrap();
    assert!(store.apply_verified_save(&receipt,&doc,&old,PAGES,Some(&frozen)).is_err());
    store.conn.execute("UPDATE revisions SET hash=?1,parent_id=NULL WHERE id=?2",params![receipt.new_hash(),applied.revision]).unwrap();
    assert!(store.apply_verified_save(&receipt,&doc,&old,PAGES,Some(&frozen)).is_err());
    store.conn.execute("UPDATE revisions SET parent_id=?1 WHERE id=?2",params![old,applied.revision]).unwrap();
    drop(store);let mut store=Store::open_synthetic(&root,16384).unwrap();
    assert_eq!(store.apply_verified_save(&receipt,&doc,&old,PAGES,Some(&frozen)).unwrap(),applied);assert_eq!(store.messages(&conv).unwrap().len(),2);store.check().unwrap();
    // An unchanged frozen draft is still live: no PDF-inclusion proof exists.
    // A late old-binding draft is not silently deleted by receipt replay. The
    // result is a historical transaction summary, not permission to clear a
    // newer live editor's dirty state (production binding fencing is pending).
    let late=json("提交后的迟到编辑");store.put_local_draft(&doc,&old,&id,&late).unwrap();
    assert_eq!(store.apply_verified_save(&receipt,&doc,&old,PAGES,Some(&frozen)).unwrap(),applied);
    assert_eq!(store.local_drafts(&doc,&old).unwrap()[0].json,late);
    // A delayed old receipt must not roll back a later document head. This is
    // a local SQL future-revision fixture, not another asserted PDF save.
    let later=uuid();let tx=store.conn.transaction().unwrap();
    tx.execute("INSERT INTO revisions VALUES(?1,?2,?3,?4,?5,'future-revision-fixture')",params![later,doc,"c".repeat(64),PAGES,applied.revision]).unwrap();
    tx.execute("UPDATE documents SET current_revision=?1 WHERE id=?2",params![later,doc]).unwrap();tx.commit().unwrap();
    let replay=store.apply_verified_save(&receipt,&doc,&old,PAGES,Some(&frozen)).unwrap();
    assert!(!replay.is_current);assert_eq!(replay.revision,applied.revision);assert_eq!(store.current_revision(&doc).unwrap().0,later);
}

#[test]
fn newer_and_other_drafts_are_retained_for_rebase_never_reapplied_or_acknowledged(){
    let(mut store,receipt,doc,old)=fixture();let id=uuid();
    store.put_local_draft(&doc,&old,&id,&json("冻结旧稿")).unwrap();let frozen=store.capture_save_draft(&doc,&old,&id).unwrap();
    let newer=json("保存期间继续输入 新内容");store.put_local_draft(&doc,&old,&id,&newer).unwrap();
    let other=uuid();let unrelated=json("另一份未发送的草稿");store.put_local_draft(&doc,&old,&other,&unrelated).unwrap();
    let before=store.local_drafts(&doc,&old).unwrap();let recovery_count=fs::read_dir(store.root.path.join("recovery")).unwrap().count();
    let applied=store.apply_verified_save(&receipt,&doc,&old,PAGES,Some(&frozen)).unwrap();
    assert!(!applied.draft_acknowledged);assert_eq!(applied.pending_rebase,2);
    let retained=store.local_drafts(&doc,&old).unwrap();assert_eq!(retained.len(),2);
    for draft in &before{assert!(retained.iter().any(|d|d.id==draft.id&&d.json==draft.json&&d.updated==draft.updated));}
    // Copying the full old-base JSON would duplicate already-saved annotations.
    assert!(store.local_drafts(&doc,&applied.revision).unwrap().is_empty());
    let raw:String=store.conn.query_row("SELECT value FROM data_meta WHERE key=?1",[association_key(receipt.event_id())],|r|r.get(0)).unwrap();
    let record:Association=serde_json::from_str(&raw).unwrap();assert_eq!(record.pending_rebase.len(),2);
    for pending in &record.pending_rebase{let bytes=store.recovery_get(&pending.file).unwrap();assert_eq!(digest(&bytes),pending.digest);}
    assert_eq!(fs::read_dir(store.root.path.join("recovery")).unwrap().count(),recovery_count);
    assert_eq!(store.apply_verified_save(&receipt,&doc,&old,PAGES,Some(&frozen)).unwrap(),applied);store.check().unwrap();
}

#[test]
fn failed_association_rolls_back_and_verified_receipt_can_retry_without_pdf_rewrite(){
    let(mut store,receipt,doc,old)=fixture();let(conv,_)=history(&mut store,&doc,&old);let id=uuid();let text=json("失败时保留稿");
    store.put_local_draft(&doc,&old,&id,&text).unwrap();store.put_desktop_position(&doc,&old,&position()).unwrap();
    let frozen=store.capture_save_draft(&doc,&old,&id).unwrap();
    assert!(store.apply_verified_save_inner(&receipt,&doc,&old,PAGES,Some(&frozen),AssociationFault::Sql).is_err());
    assert_eq!(store.current_revision(&doc).unwrap().0,old);assert_eq!(count(&store,"revisions"),1);assert_eq!(count(&store,"save_events"),0);
    assert_eq!(store.desktop_position(&doc,&old).unwrap(),Some(position()));assert_eq!(store.messages(&conv).unwrap().len(),2);
    assert_eq!(store.local_drafts(&doc,&old).unwrap()[0].json,text);
    assert!(!store.conn.prepare("SELECT 1 FROM data_meta WHERE key=?1").unwrap().exists([association_key(receipt.event_id())]).unwrap());
    // The real core already committed before receipt minting. Retry ONLY the
    // SQLite association; the core append function is not called a second time.
    let result=store.apply_verified_save(&receipt,&doc,&old,PAGES,Some(&frozen)).unwrap();assert!(!result.draft_acknowledged);assert_eq!(result.pending_rebase,1);assert_eq!(store.local_drafts(&doc,&old).unwrap()[0].json,text);assert_eq!(count(&store,"save_events"),1);store.check().unwrap();
}

#[test]
fn second_connection_new_draft_between_snapshot_and_transaction_refuses_without_losing_either(){
    let(mut store,receipt,doc,old)=fixture();let id=uuid();let text=json("锁定前有新稿");
    store.put_local_draft(&doc,&old,&id,&text).unwrap();let frozen=store.capture_save_draft(&doc,&old,&id).unwrap();
    let failed=store.apply_verified_save_inner(&receipt,&doc,&old,PAGES,Some(&frozen),AssociationFault::NewDraft);
    assert!(matches!(failed,Err(DataError::Refused("save draft set changed during association"))));
    assert_eq!(count(&store,"revisions"),1);assert_eq!(count(&store,"save_events"),0);assert_eq!(store.current_revision(&doc).unwrap().0,old);
    let drafts=store.local_drafts(&doc,&old).unwrap();assert_eq!(drafts.len(),2);assert!(drafts.iter().all(|d|d.json==text));
    let retry=store.apply_verified_save(&receipt,&doc,&old,PAGES,Some(&frozen)).unwrap();assert!(!retry.draft_acknowledged);assert_eq!(retry.pending_rebase,2);
    assert_eq!(store.local_drafts(&doc,&old).unwrap().len(),2);assert!(store.local_drafts(&doc,&retry.revision).unwrap().is_empty());store.check().unwrap();
}

#[test]
fn receipt_rejects_wrong_document_hash_page_count_and_snapshot_binding(){
    let(mut store,receipt,doc,old)=fixture();
    assert!(store.apply_verified_save(&receipt,&doc,&old,PAGES+1,None).is_err());
    let(other,other_rev)=store.create_document("another-physical-file",receipt.old_hash(),PAGES,"same-name.pdf").unwrap();
    assert!(store.apply_verified_save(&receipt,&other,&other_rev,PAGES,None).is_err());
    let id=uuid();store.put_local_draft(&other,&other_rev,&id,&json("别的文档")).unwrap();
    let other_frozen=store.capture_save_draft(&other,&other_rev,&id).unwrap();assert!(store.apply_verified_save(&receipt,&doc,&old,PAGES,Some(&other_frozen)).is_err());
    store.conn.execute("UPDATE revisions SET hash=?1 WHERE id=?2",params!["b".repeat(64),old]).unwrap();
    assert!(store.apply_verified_save(&receipt,&doc,&old,PAGES,None).is_err());
    assert_eq!(count(&store,"revisions"),2);assert_eq!(count(&store,"save_events"),0);assert_eq!(store.local_drafts(&other,&other_rev).unwrap().len(),1);store.check().unwrap();
}
