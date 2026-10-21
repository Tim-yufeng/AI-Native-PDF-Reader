use super::*;
use super::model::*;
use super::model::desktop::{ReadingPosition,Zoom};
use rusqlite::params;
fn root()->PathBuf{let base=Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("tests/work/m4");fs::create_dir_all(&base).unwrap();let path=base.join(uuid());fs::create_dir(&path).unwrap();path}
fn meta(request:Option<String>)->MessageMetadata{MessageMetadata{provider:Some("synthetic".into()),model:Some("local-fixture".into()),preset_version:None,request_id:request,sent_context_hash:None,slot:None}}
fn total(conn:&Connection,table:&str)->i64{conn.query_row(&format!("SELECT count(*) FROM {table}"),[],|r|r.get(0)).unwrap()}
struct Fixture{store:Store,path:PathBuf,doc:String,revision:String,conversation:String,other_doc:String,other_revision:String,other_conversation:String,draft:String,recovery:String,position:ReadingPosition,backup:PathBuf,pdf:PathBuf}
fn seeded()->Fixture{
    let path=root();let mut store=Store::open_synthetic(&path,16384).unwrap();let(doc,revision)=store.create_document("file:target",&"a".repeat(64),3,"合成目标.pdf").unwrap();
    let conversation=store.conversation(&doc).unwrap();let second=store.conversation(&doc).unwrap();
    let prompt=store.add_local_prompt(&conversation,&revision,None,"deleteTargetEnglish 原问题",1).unwrap();
    let request=uuid();let answer=store.message(&conversation,&revision,Some(&prompt),"",true,&meta(Some(request.clone()))).unwrap();store.append_chunk(&answer,&request,0,"target ").unwrap();store.append_chunk(&answer,&request,1,"answer").unwrap();store.finish(&answer,"complete").unwrap();
    let edit=store.edit_local_prompt(&conversation,&prompt,"deleteTargetEnglish 修改分支").unwrap();store.local_regenerate(&conversation,&revision,&edit).unwrap();
    let hidden=store.add_local_prompt(&second,&revision,None,"deleteTargetEnglish 第二会话隐藏记录",2).unwrap();store.hide_local_message(&second,&hidden).unwrap();
    let(other_doc,other_revision)=store.create_document("file:other",&"b".repeat(64),4,"另一文档.pdf").unwrap();let other_conversation=store.conversation(&other_doc).unwrap();let other=store.add_local_prompt(&other_conversation,&other_revision,None,"preserveOther 另一文档",3).unwrap();let req=uuid();let reply=store.message(&other_conversation,&other_revision,Some(&other),"",true,&meta(Some(req.clone()))).unwrap();store.append_chunk(&reply,&req,0,"preserveOther untouched answer").unwrap();store.finish(&reply,"complete").unwrap();
    let draft=uuid();store.put_local_draft(&doc,&revision,&draft,r#"{"schema":1,"pdfjs":"6.4.299","entries":[]}"#).unwrap();let recovery=store.recovery_put(b"unreferenced important recovery").unwrap();let position=ReadingPosition{page:2,zoom:Zoom::Fit("page-fit".into()),top:-0.3,left:0.2};store.put_desktop_position(&doc,&revision,&position).unwrap();store.update_settings(true,16384).unwrap();store.set_default_zoom("page-fit").unwrap();
    let backup=store.backup("manual",None).unwrap();let pdf=path.join("synthetic-source.pdf");fs::write(&pdf,b"%PDF-1.7\nuntouched synthetic fixture").unwrap();
    Fixture{store,path,doc,revision,conversation,other_doc,other_revision,other_conversation,draft,recovery,position,backup,pdf}
}
#[test]
fn bound_document_deletion_removes_all_branches_and_keeps_unrelated_data(){
    let mut f=seeded();let other=serde_json::to_string(&f.store.local_messages(&f.other_conversation).unwrap()).unwrap();let other_search=f.store.search("preserveOther").unwrap();let backup=fs::read(&f.backup).unwrap();let pdf=fs::read(&f.pdf).unwrap();let draft=f.store.local_drafts(&f.doc,&f.revision).unwrap()[0].json.clone();
    assert_eq!(total(&f.store.conn,"messages"),7);assert_eq!(f.store.delete_document_history(&f.doc,&f.conversation).unwrap(),5);
    assert_eq!(total(&f.store.conn,"messages"),2);assert!(f.store.search("deleteTargetEnglish").unwrap().is_empty());assert_eq!(f.store.search("preserveOther").unwrap(),other_search);assert_eq!(total(&f.store.conn,"message_chunks"),1);assert_eq!(total(&f.store.conn,"anchors"),1);
    assert_eq!(f.store.conn.query_row("SELECT count(*) FROM data_meta WHERE key LIKE 'message-hidden:%'",[],|r|r.get::<_,i64>(0)).unwrap(),0);
    assert_eq!(total(&f.store.conn,"documents"),2);assert_eq!(total(&f.store.conn,"revisions"),2);assert_eq!(total(&f.store.conn,"locations"),2);assert_eq!(total(&f.store.conn,"conversations"),3);
    assert_eq!(serde_json::to_string(&f.store.local_messages(&f.other_conversation).unwrap()).unwrap(),other);assert_eq!(f.store.current_revision(&f.other_doc).unwrap().0,f.other_revision);assert_eq!(f.store.desktop_position(&f.doc,&f.revision).unwrap(),Some(f.position.clone()));assert_eq!(f.store.local_drafts(&f.doc,&f.revision).unwrap()[0].id,f.draft);assert_eq!(f.store.local_drafts(&f.doc,&f.revision).unwrap()[0].json,draft);assert_eq!(f.store.recovery_get(&f.recovery).unwrap(),b"unreferenced important recovery");assert!(f.store.settings().unwrap().unwrap().thumbnail_open);assert_eq!(f.store.default_zoom().unwrap().as_deref(),Some("page-fit"));assert_eq!(fs::read(&f.backup).unwrap(),backup);assert_eq!(fs::read(&f.pdf).unwrap(),pdf);f.store.check().unwrap();
    assert_eq!(f.store.delete_document_history(&f.doc,&f.conversation).unwrap(),0);let path=f.path.clone();let doc=f.doc.clone();let rev=f.revision.clone();let conv=f.conversation.clone();drop(f.store);let mut reopened=Store::open_synthetic(&path,16384).unwrap();assert!(reopened.local_messages(&conv).unwrap().is_empty());assert_eq!(reopened.desktop_position(&doc,&rev).unwrap(),Some(f.position));assert_eq!(reopened.local_drafts(&doc,&rev).unwrap().len(),1);assert_eq!(serde_json::to_string(&reopened.local_messages(&f.other_conversation).unwrap()).unwrap(),other);
    reopened.add_local_prompt(&conv,&rev,None,"删除后仍可新增",1).unwrap();assert_eq!(reopened.local_messages(&conv).unwrap().len(),1);println!("whole document logical deletion: all conversations/branches/chunks/anchors/FTS/hidden metadata removed; PDF/identity/positions/drafts/settings/backups/other doc retained, reopen passed");
}
#[test]
fn interrupted_delete_and_wrong_document_binding_roll_back(){
    let mut f=seeded();let before=maintenance::logical_digest(&f.store.conn).unwrap();
    assert!(f.store.delete_document_history(&f.doc,&f.other_conversation).is_err());assert_eq!(maintenance::logical_digest(&f.store.conn).unwrap(),before);
    assert!(f.store.delete_document_history("invalid",&f.conversation).is_err());assert_eq!(maintenance::logical_digest(&f.store.conn).unwrap(),before);
    assert!(f.store.delete_document_history_interrupted(&f.doc,&f.conversation).is_err());assert_eq!(maintenance::logical_digest(&f.store.conn).unwrap(),before);assert_eq!(total(&f.store.conn,"messages"),7);assert_eq!(total(&f.store.conn,"message_chunks"),5);assert!(!f.store.search("deleteTargetEnglish").unwrap().is_empty());f.store.check().unwrap();
    let path=f.path.clone();drop(f.store);let reopened=Store::open_synthetic(&path,16384).unwrap();assert_eq!(maintenance::logical_digest(&reopened.conn).unwrap(),before);println!("wrong document/conversation and SQL interruption refused with exact logical rollback; reopen preserved all original state");
}
#[test]
fn inconsistent_anchor_owner_is_refused_before_deletion(){let mut f=seeded();let target:String=f.store.conn.query_row("SELECT id FROM messages WHERE document_id=?1 LIMIT 1",[&f.doc],|r|r.get(0)).unwrap();f.store.conn.execute("INSERT INTO anchors VALUES(?1,?2,?3,?4,1,'[0,0,1,1]')",params![uuid(),target,f.other_doc,f.other_revision]).unwrap();let before=maintenance::logical_digest(&f.store.conn).unwrap();assert!(f.store.delete_document_history(&f.doc,&f.conversation).is_err());assert_eq!(maintenance::logical_digest(&f.store.conn).unwrap(),before);println!("inconsistent cross-document anchor rejected rather than removing another document reference");}
