use super::*;
use super::model::*;
fn root()->PathBuf {
    let base=Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("tests/work/m4");
    fs::create_dir_all(&base).unwrap();let root=base.join(uuid());fs::create_dir(&root).unwrap();root
}
fn store()->Store {Store::open_synthetic(&root(),16384).unwrap()}
fn hash(ch:char)->String{ch.to_string().repeat(64)}
fn meta(request:Option<String>)->MessageMetadata{MessageMetadata{provider:Some("synthetic".into()),model:Some("fixture".into()),preset_version:Some("default-v1".into()),request_id:request,sent_context_hash:None,slot:None}}
fn document(s:&mut Store,key:&str)->(String,String){s.create_document(key,&hash('a'),30,"中文 同名.pdf").unwrap()}
fn count(c:&Connection,table:&str)->i64{c.query_row(&format!("SELECT count(*) FROM {table}"),[],|r|r.get(0)).unwrap()}
fn v1(root:&Path){let conn=Connection::open(root.join("history.sqlite")).unwrap();conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();conn.execute_batch(include_str!("schema.sql")).unwrap();conn.execute("INSERT INTO documents VALUES(?1,NULL)",[uuid()]).unwrap();}
#[test]
fn sqlite_runtime_schema_security_and_exclusive_owner(){
    let s=store();let version:String=s.conn.query_row("SELECT sqlite_version()",[],|r|r.get(0)).unwrap();assert_eq!(version,"3.53.2");
    for(name,want)in[("user_version",2),("foreign_keys",1),("synchronous",2),("trusted_schema",0)]{let v:i64=s.conn.query_row(&format!("PRAGMA {name}"),[],|r|r.get(0)).unwrap();assert_eq!(v,want);}
    let mode:String=s.conn.query_row("PRAGMA journal_mode",[],|r|r.get(0)).unwrap();assert_eq!(mode,"wal");
    assert!(Store::open_synthetic(&s.root.path,16384).is_err());
    assert!(fs::rename(s.root.path.join("history.sqlite"),s.root.path.join("renamed.sqlite")).is_err());
    s.check().unwrap();println!("SQLite {version}, schema2, WAL/FULL/FK/FTS5; synthetic root {:?}",s.root.path);
}
#[test]
fn incremental_history_branches_and_reopen(){
    let mut s=store();let path=s.root.path.clone();let(doc,rev)=document(&mut s,"volume:fileA");let conv=s.conversation(&doc).unwrap();
    let prompt=s.message(&conv,&rev,None,"原问题",false,&meta(None)).unwrap();let request=uuid();
    let answer=s.message(&conv,&rev,Some(&prompt),"",true,&meta(Some(request.clone()))).unwrap();
    s.append_chunk(&answer,&request,0,"回答 ").unwrap();s.append_chunk(&answer,&request,1,"中文 alpha").unwrap();
    assert!(s.append_chunk(&answer,&request,1,"重复").is_err());assert!(s.append_chunk(&answer,&request,3,"乱序").is_err());
    assert!(s.append_chunk(&answer,&uuid(),2,"错误任务").is_err());
    s.finish(&answer,"complete").unwrap();assert!(s.append_chunk(&answer,&request,2,"迟到").is_err());
    let edit=s.edit_prompt(&prompt,"新问题").unwrap();let branch=s.message(&conv,&rev,Some(&edit),"新回答",true,&meta(Some(uuid()))).unwrap();s.finish(&branch,"cancelled").unwrap();
    assert_eq!(count(&s.conn,"message_chunks"),2);assert!(s.search("alpha").unwrap().contains(&answer));
    s.position(&doc,&rev,27,137.5,33009.0).unwrap();s.update_settings(true,16384).unwrap();let recovery=s.recovery_put(b"synthetic unsaved draft").unwrap();
    let expected=s.messages(&conv).unwrap();assert_eq!(expected[0].content,"原问题");assert_eq!(expected[2].edited_from.as_deref(),Some(prompt.as_str()));
    drop(s);let s=Store::open_synthetic(&path,16384).unwrap();assert_eq!(s.messages(&conv).unwrap(),expected);assert_eq!(s.read_position(&doc,&rev).unwrap(),Some((27,137.5,33009.0)));assert_eq!(s.recovery_get(&recovery).unwrap(),b"synthetic unsaved draft");s.check().unwrap();
}
#[test]
fn relations_reject_cross_document_and_conversation(){
    let mut s=store();let(d1,r1)=document(&mut s,"A");let(d2,r2)=document(&mut s,"B");let c1=s.conversation(&d1).unwrap();let c2=s.conversation(&d2).unwrap();let c3=s.conversation(&d1).unwrap();
    let m=s.message(&c1,&r1,None,"one",false,&meta(None)).unwrap();
    assert!(s.message(&c1,&r2,None,"bad",false,&meta(None)).is_err());assert!(s.message(&c2,&r2,Some(&m),"bad",false,&meta(None)).is_err());assert!(s.message(&c3,&r1,Some(&m),"bad",false,&meta(None)).is_err());
    assert!(s.anchor(&m,&r2,1,[0.,0.,1.,1.]).is_err());assert!(s.anchor(&m,&r1,31,[0.,0.,1.,1.]).is_err());assert!(s.position(&d1,&r2,1,100.,0.).is_err());assert!(s.position(&d1,&r1,1,f64::NAN,0.).is_err());assert_eq!(count(&s.conn,"messages"),1);
}
#[test]
fn identity_ambiguity_and_synthetic_save_lineage(){
    let mut s=store();let(doc,old)=document(&mut s,"volume:file1");
    assert_eq!(s.identify("volume:file1",&hash('a')).unwrap(),IdentityMatch::Known(doc.clone())); // rename/move: name is not input
    assert_eq!(s.identify("volume:file2",&hash('b')).unwrap(),IdentityMatch::New); // same name, other bytes
    assert!(matches!(s.identify("volume:copy",&hash('a')).unwrap(),IdentityMatch::Confirm(_)));
    assert!(matches!(s.identify("volume:file1",&hash('b')).unwrap(),IdentityMatch::Confirm(_))); // same ID, size/time irrelevant
    let conv=s.conversation(&doc).unwrap();let message=s.message(&conv,&old,None,"history",false,&meta(None)).unwrap();let anchor=s.anchor(&message,&old,5,[0.,0.,10.,10.]).unwrap();
    let event=SyntheticSaveEvent{event_id:uuid(),document:doc.clone(),old_revision:old.clone(),old_hash:hash('a'),new_hash:hash('b'),pages:30,new_file_key:"volume:changedID".into()};
    let next=s.save_event_synthetic(&event).unwrap();assert_eq!(s.save_event_synthetic(&event).unwrap(),next);
    assert_eq!(s.identify("volume:changedID",&hash('b')).unwrap(),IdentityMatch::Known(doc.clone()));assert!(!s.anchor_current(&anchor).unwrap());assert_eq!(count(&s.conn,"revisions"),2);
    let reused=SyntheticSaveEvent{new_hash:hash('c'),..event};assert!(s.save_event_synthetic(&reused).is_err());
    let stale=SyntheticSaveEvent{event_id:uuid(),..reused};assert!(s.save_event_synthetic(&stale).is_err());assert_eq!(count(&s.conn,"revisions"),2);
    s.create_document("volume:changedID",&hash('b'),30,"another.pdf").unwrap();assert!(matches!(s.identify("volume:changedID",&hash('b')).unwrap(),IdentityMatch::Confirm(v) if v.len()==2));
}
#[test]
fn settings_unknown_fields_and_future_version_are_preserved(){
    let mut s=store();s.conn.execute("INSERT INTO settings VALUES('reader',1,?1)",[r#"{"schema_version":1,"thumbnail_open":false,"cache_bytes":16384,"future_field":{"keep":true}}"#]).unwrap();
    s.update_settings(true,4096).unwrap();assert_eq!(s.settings().unwrap().unwrap().extensions["future_field"]["keep"],true);
    s.conn.execute("UPDATE settings SET schema_version=9 WHERE key='reader'",[]).unwrap();let before:String=s.conn.query_row("SELECT value FROM settings",[],|r|r.get(0)).unwrap();assert!(s.update_settings(false,0).is_err());let after:String=s.conn.query_row("SELECT value FROM settings",[],|r|r.get(0)).unwrap();assert_eq!(before,after);
}
#[test]
fn cache_quota_never_removes_history_recovery_or_backups(){
    let mut s=store();let(doc,rev)=document(&mut s,"cache-doc");let c=s.conversation(&doc).unwrap();s.message(&c,&rev,None,"permanent",false,&meta(None)).unwrap();s.update_settings(false,1024).unwrap();let recovery=s.recovery_put(b"keep draft").unwrap();let backup=s.backup("manual",None).unwrap();
    let first=s.cache_put(&vec![1;600]).unwrap();std::thread::sleep(Duration::from_millis(15));let second=s.cache_put(&vec![2;600]).unwrap();assert!(s.cache_get(&first).is_err());assert_eq!(s.cache_get(&second).unwrap(),vec![2;600]);assert_eq!(s.messages(&c).unwrap().len(),1);assert_eq!(s.recovery_get(&recovery).unwrap(),b"keep draft");assert!(backup.exists());assert!(s.cache_get("../history.sqlite").is_err());assert!(s.cache_put(&vec![0;1025]).is_err());
    fs::write(s.root.path.join("cache/unowned.file"),b"not a cache key").unwrap();assert!(s.trim_cache().is_err());assert!(s.root.path.join("cache/unowned.file").exists());
}
#[test]
fn backup_includes_uncheckpointed_wal_and_directory_copy_preserves_source(){
    let mut s=store();let(doc,rev)=document(&mut s,"backup");let c=s.conversation(&doc).unwrap();let m=s.message(&c,&rev,None,"WAL committed",false,&meta(None)).unwrap();s.edit_prompt(&m,"branch").unwrap();s.update_settings(true,1024).unwrap();let recovery=s.recovery_put(b"draft preserved").unwrap();
    let before=s.messages(&c).unwrap();let backup=s.backup("manual",None).unwrap();let read=Connection::open(&backup).unwrap();integrity(&read).unwrap();assert_eq!(count(&read,"messages"),2);assert_eq!(read.query_row::<String,_,_>("SELECT content FROM messages WHERE id=?1",[m],|r|r.get(0)).unwrap(),"WAL committed");
    let target=s.root.path.parent().unwrap().join(uuid());let failed=s.root.path.parent().unwrap().join(uuid());assert!(s.prepare_directory_copy(&failed,"after-db").is_err());assert!(!failed.join("copy-ready.json").exists());assert_eq!(s.messages(&c).unwrap(),before);
    s.prepare_directory_copy(&target,"").unwrap();assert!(s.root.path.join("history.sqlite").exists());let copy=Store::open_synthetic(&target,1024).unwrap();assert_eq!(copy.messages(&c).unwrap(),before);assert_eq!(copy.recovery_get(&recovery).unwrap(),b"draft preserved");assert_eq!(copy.settings().unwrap(),s.settings().unwrap());assert!(s.prepare_directory_copy(&target,"").is_err());
}
#[test]
fn schema_rollback_future_and_corruption_refuse_without_overwrite(){
    for fault in ["ddl","full",""]{let p=root();v1(&p);let before=Connection::open(p.join("history.sqlite")).unwrap().query_row::<String,_,_>("SELECT id FROM documents",[],|r|r.get(0)).unwrap();let result=Store::open_with_fault(&p,16384,fault);if fault!=""{assert!(result.is_err());if fault=="full"{assert!(matches!(result,Err(DataError::Sql(rusqlite::Error::SqliteFailure(e,_))) if e.code==rusqlite::ErrorCode::DiskFull));}let c=Connection::open(p.join("history.sqlite")).unwrap();assert_eq!(c.query_row::<i64,_,_>("PRAGMA user_version",[],|r|r.get(0)).unwrap(),1);assert_eq!(count(&c,"documents"),1);integrity(&c).unwrap();}else{let s=result.unwrap();assert_eq!(s.conn.query_row::<String,_,_>("SELECT id FROM documents",[],|r|r.get(0)).unwrap(),before);}}
    let p=root();v1(&p);let c=Connection::open(p.join("history.sqlite")).unwrap();c.execute_batch("PRAGMA user_version=99;").unwrap();drop(c);let bytes=fs::read(p.join("history.sqlite")).unwrap();assert!(Store::open_synthetic(&p,16384).is_err());assert_eq!(fs::read(p.join("history.sqlite")).unwrap(),bytes);
    for content in [b"not sqlite".as_slice(),b"".as_slice()]{let p=root();fs::write(p.join("history.sqlite"),content).unwrap();assert!(Store::open_synthetic(&p,16384).is_err());assert_eq!(fs::read(p.join("history.sqlite")).unwrap(),content);}
}
#[test]
fn actual_abrupt_process_exit_rolls_back_uncommitted_history_and_migration(){
    for operation in ["history","migration"]{let p=root();if operation=="migration"{v1(&p);}else{drop(Store::open_synthetic(&p,16384).unwrap());}
        let output=std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact","storage::tests::exit_actor"]).env("M4_EXIT_ROOT",&p).env("M4_EXIT_OP",operation).output().unwrap();assert_eq!(output.status.code(),Some(74),"{}",String::from_utf8_lossy(&output.stdout));
        let s=Store::open_synthetic(&p,16384).unwrap();s.check().unwrap();assert_eq!(count(&s.conn,"documents"),if operation=="migration"{1}else{0});
    }
}
#[test]
fn exit_actor() {
    let Ok(root)=std::env::var("M4_EXIT_ROOT")else{return;};let p=PathBuf::from(root);
    if std::env::var("M4_EXIT_OP").as_deref()==Ok("migration") {
        let _=Store::open_with_fault(&p,16384,"exit");panic!("migration exit missed");
    }
    let mut s=Store::open_synthetic(&p,16384).unwrap();
    if std::env::var("M4_EXIT_OP").as_deref()==Ok("directory") {
        let leaf=std::env::var("M4_EXIT_TARGET").unwrap();valid_uuid(&leaf).unwrap();
        let target=s.root.path.parent().unwrap().join(leaf);
        let _=s.prepare_directory_copy(&target,"exit-after-db");panic!("directory exit missed");
    }
    let tx=s.conn.transaction().unwrap();
    tx.execute("INSERT INTO documents VALUES(?1,NULL)",[uuid()]).unwrap();
    std::process::exit(74);
}
#[test]
fn sqlite_full_fails_without_false_commit_or_history_loss(){
    let mut s=store();let(doc,rev)=document(&mut s,"full");let c=s.conversation(&doc).unwrap();s.message(&c,&rev,None,"committed",false,&meta(None)).unwrap();s.checkpoint().unwrap();let pages:i64=s.conn.query_row("PRAGMA page_count",[],|r|r.get(0)).unwrap();s.conn.execute_batch(&format!("PRAGMA max_page_count={pages};")).unwrap();
    let result=s.message(&c,&rev,None,&"x".repeat(512*1024),false,&meta(None));assert!(matches!(result,Err(DataError::Sql(rusqlite::Error::SqliteFailure(e,_))) if e.code==rusqlite::ErrorCode::DiskFull));assert_eq!(s.messages(&c).unwrap().len(),1);s.check().unwrap();
}
#[test]
fn wal_long_reader_blocks_checkpoint_then_release_recovers(){
    let mut s=store();let(doc,rev)=document(&mut s,"reader");let c=s.conversation(&doc).unwrap();let external=Connection::open(s.root.path.join("history.sqlite")).unwrap();external.execute_batch("BEGIN; SELECT * FROM documents;").unwrap();
    for _ in 0..12{s.message(&c,&rev,None,&"a".repeat(8192),false,&meta(None)).unwrap();}
    assert!(s.checkpoint().is_err());external.execute_batch("ROLLBACK").unwrap();s.checkpoint().unwrap();s.check().unwrap();assert_eq!(count(&s.conn,"messages"),12);
}
#[test]
fn stored_quota_recovers_and_cache_handles_remain_bounded() {
    let mut s=store();let path=s.root.path.clone();s.update_settings(false,1024).unwrap();
    let id=s.cache_put(b"cache").unwrap();for _ in 0..100{s.cache_get(&id).unwrap();}
    assert_eq!(s.root._subdirs.borrow().len(),3);
    let cache=s.root.path.join("cache");assert!(fs::rename(&cache,s.root.path.join("moved-cache")).is_err());
    let _leaf=leaf_guard(&cache.join(format!("{id}.cache")),false).unwrap();
    assert!(fs::rename(cache.join(format!("{id}.cache")),cache.join("replaced.cache")).is_err());
    drop(_leaf);drop(s);
    let s=Store::open_synthetic(&path,16384).unwrap();assert_eq!(s.quota,1024);
    assert!(s.cache_put(&vec![0;1025]).is_err());assert_eq!(s.cache_get(&id).unwrap(),b"cache");
}
#[test]
fn online_backups_are_consistent_during_real_concurrent_sqlite_commits() {
    let s=store();let path=s.root.path.join("history.sqlite");
    s.conn.execute_batch("INSERT INTO data_meta VALUES('left','0'),('right','0');").unwrap();
    let(ready,started)=std::sync::mpsc::channel();
    let worker=std::thread::spawn(move||{
        let mut conn=Connection::open(path).unwrap();conn.busy_timeout(Duration::from_millis(250)).unwrap();
        ready.send(()).unwrap();
        for i in 1..=30 {let tx=conn.transaction().unwrap();tx.execute("UPDATE data_meta SET value=?1",[i.to_string()]).unwrap();tx.commit().unwrap();std::thread::sleep(Duration::from_millis(3));}
    });started.recv().unwrap();
    for _ in 0..8 {let p=s.backup("manual",None).unwrap();let backup=Connection::open(p).unwrap();integrity(&backup).unwrap();let values=backup.prepare("SELECT value FROM data_meta ORDER BY key").unwrap().query_map([],|r|r.get::<_,String>(0)).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap();assert_eq!(values.len(),2);assert_eq!(values[0],values[1]);}
    worker.join().unwrap();s.check().unwrap();
    s.conn.execute_batch("BEGIN IMMEDIATE; UPDATE data_meta SET value='uncommitted';").unwrap();
    let start=std::time::Instant::now();assert!(s.backup("manual",None).is_err());assert!(start.elapsed()<Duration::from_secs(5));s.conn.execute_batch("ROLLBACK").unwrap();s.check().unwrap();
}
#[test]
fn directory_copy_process_exit_keeps_source_and_no_ready_marker() {
    let mut s=store();let(doc,rev)=document(&mut s,"directory-exit");let c=s.conversation(&doc).unwrap();s.message(&c,&rev,None,"keep",false,&meta(None)).unwrap();let recovery=s.recovery_put(b"retain source draft").unwrap();
    let path=s.root.path.clone();let target_leaf=uuid();let target=path.parent().unwrap().join(&target_leaf);drop(s);
    let output=std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact","storage::tests::exit_actor"]).env("M4_EXIT_ROOT",&path).env("M4_EXIT_OP","directory").env("M4_EXIT_TARGET",&target_leaf).output().unwrap();assert_eq!(output.status.code(),Some(74));assert!(target.join("history.sqlite").exists());assert!(!target.join("copy-ready.json").exists());
    let source=Store::open_synthetic(&path,16384).unwrap();assert_eq!(source.messages(&c).unwrap().len(),1);assert_eq!(source.recovery_get(&recovery).unwrap(),b"retain source draft");source.check().unwrap();
}
#[test]
fn directory_data_digest_detects_same_count_wrong_content() {
    let mut s=store();let(doc,rev)=document(&mut s,"digest");let c=s.conversation(&doc).unwrap();s.message(&c,&rev,None,"original",false,&meta(None)).unwrap();
    let target=s.root.path.parent().unwrap().join(uuid());s.prepare_directory_copy(&target,"").unwrap();
    let changed=Connection::open(target.join("history.sqlite")).unwrap();let before=maintenance::logical_digest(&changed).unwrap();
    changed.execute("UPDATE messages SET content='different'",[]).unwrap();assert_eq!(count(&changed,"messages"),count(&s.conn,"messages"));assert_ne!(before,maintenance::logical_digest(&changed).unwrap());
}
#[test]
fn unknown_journal_leaf_and_outside_root_are_refused_before_sqlite_writes() {
    let p=root();v1(&p);let db=p.join("history.sqlite");let before=fs::read(&db).unwrap();
    fs::create_dir(p.join("history.sqlite-journal")).unwrap();
    assert!(Store::open_synthetic(&p,16384).is_err());assert_eq!(fs::read(db).unwrap(),before);
    let project=Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    assert!(Store::open_synthetic(project,16384).is_err());
}
#[test]
fn persistent_wal_flag_close_reopen_and_checkpoint_are_verified() {
    let mut s=store();let path=s.root.path.clone();let mut flag=-1i32;
    let rc=unsafe{rusqlite::ffi::sqlite3_file_control(s.conn.handle(),c"main".as_ptr(),
        rusqlite::ffi::SQLITE_FCNTL_PERSIST_WAL,(&mut flag as *mut i32).cast())};
    assert_eq!(rc,rusqlite::ffi::SQLITE_OK);assert_eq!(flag,1);
    document(&mut s,"persistent-wal");drop(s);
    assert!(path.join("history.sqlite-wal").exists());assert!(path.join("history.sqlite-shm").exists());
    let s=Store::open_synthetic(&path,16384).unwrap();s.checkpoint().unwrap();
    assert_eq!(path.join("history.sqlite-wal").metadata().unwrap().len(),0);s.check().unwrap();
}
#[test]
fn interrupted_message_content_survives_and_late_chunks_are_rejected() {
    let mut s=store();let path=s.root.path.clone();let(doc,rev)=document(&mut s,"unfinished");let c=s.conversation(&doc).unwrap();let request=uuid();let m=s.message(&c,&rev,None,"",true,&meta(Some(request.clone()))).unwrap();s.append_chunk(&m,&request,0,"persisted partial answer").unwrap();drop(s);
    let mut s=Store::open_synthetic(&path,16384).unwrap();let records=s.messages(&c).unwrap();assert_eq!(records[0].content,"persisted partial answer");assert_eq!(records[0].status,"failed");assert!(s.append_chunk(&m,&request,1,"late completion").is_err());assert_eq!(count(&s.conn,"message_chunks"),1);
}
