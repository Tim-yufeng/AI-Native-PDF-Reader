use super::*;
use rusqlite::backup::{Backup,StepResult};
use sha2::{Digest,Sha256};
use std::io::Read;
pub(super) fn snapshot(from:&Connection,to:&Path)->Result<File> {
    let guard=leaf_guard(to,true)?;
    for suffix in ["-journal","-wal","-shm"] {
        let sidecar=PathBuf::from(format!("{}{suffix}",to.to_string_lossy()));
        if sidecar.try_exists()?{return deny("unexpected backup sidecar; preserved");}
    }
    let mut dest=Connection::open_with_flags(to,OpenFlags::SQLITE_OPEN_READ_WRITE|OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
    dest.busy_timeout(Duration::from_millis(50))?;
    {
        let backup=Backup::new(from,&mut dest)?;
        let deadline=std::time::Instant::now()+Duration::from_secs(2);
        loop {
            match backup.step(64)? {
                StepResult::Done=>break,
                StepResult::More=>{},
                StepResult::Busy|StepResult::Locked=>std::thread::sleep(Duration::from_millis(10)),
                _=>return deny("backup state"),
            }
            if std::time::Instant::now()>deadline{return deny("backup time limit");}
        }
    }
    integrity(&dest)?;
    dest.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL;")?;
    drop(dest);
    OpenOptions::new().write(true).open(to)?.sync_all()?;
    Ok(guard)
}
pub(super) fn read_owned(path:&Path,limit:u64)->Result<Vec<u8>> {
    let _guard=leaf_guard(path,false)?;
    let mut file=OpenOptions::new().read(true).share_mode(FILE_SHARE_READ).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(path)?;
    if file.metadata()?.len()>limit{return deny("owned material too large");}
    let mut data=Vec::new();file.read_to_end(&mut data)?;Ok(data)
}
fn digest(data:&[u8])->String{format!("{:x}",Sha256::digest(data))}
impl Store {
    pub(crate) fn backup(&self,label:&str,after:Option<&dyn Fn(&Path)>)->Result<PathBuf> {
        if !matches!(label,"schema-before"|"manual"|"directory"){return deny("backup label");}
        let directory=self.root.subdir("backups")?;
        let path=directory.join(format!("{label}-{}.sqlite",uuid()));
        snapshot(&self.conn,&path)?;
        if let Some(check)=after{check(&path);}
        Ok(path)
    }
    pub(crate) fn recovery_put(&self,data:&[u8])->Result<String> {
        if data.len()>1024*1024{return deny("recovery material limit");}
        let dir=self.root.subdir("recovery")?;let id=uuid();
        let mut file=OpenOptions::new().write(true).create_new(true).open(dir.join(format!("{id}.json")))?;
        if self.root.owned{directories::check_private_file(&file,false)?;}
        file.write_all(data)?;file.sync_all()?;Ok(id)
    }
    pub(crate) fn recovery_get(&self,id:&str)->Result<Vec<u8>> {
        valid_uuid(id)?;let path=self.root.subdir("recovery")?.join(format!("{id}.json"));
        let _guard=leaf_guard(&path,false)?;if self.root.owned{directories::check_private_file(&_guard,false)?;}
        read_owned(&path,1024*1024)
    }
    /// New cache objects only; no arbitrary filenames or history deletion API.
    pub(crate) fn cache_put(&self,data:&[u8])->Result<String> {
        if data.len() as u64>self.quota{return deny("cache object exceeds quota");}
        let dir=self.root.subdir("cache")?;let id=uuid();
        let mut file=OpenOptions::new().write(true).create_new(true).open(dir.join(format!("{id}.cache")))?;
        if self.root.owned{directories::check_private_file(&file,false)?;}
        file.write_all(data)?;drop(file);self.trim_cache()?;Ok(id)
    }
    pub(crate) fn cache_get(&self,id:&str)->Result<Vec<u8>> {
        valid_uuid(id)?;let dir=self.root.subdir("cache")?;let path=dir.join(format!("{id}.cache"));
        let _guard=leaf_guard(&path,false)?;
        let mut file=OpenOptions::new().read(true).access_mode(0x8000_0000|FILE_WRITE_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(path)?;
        if self.root.owned{directories::check_private_file(&file,false)?;}
        if file.metadata()?.len()>self.quota{return deny("cache object exceeds quota");}
        let mut data=Vec::new();file.read_to_end(&mut data)?;
        // Same protected object from read through touch; no path reopen window.
        file.set_times(std::fs::FileTimes::new().set_modified(std::time::SystemTime::now()))?;
        Ok(data)
    }
    pub(crate) fn trim_cache(&self)->Result<()> {
        let dir=self.root.subdir("cache")?;let mut items=Vec::new();let mut total=0;
        for entry in fs::read_dir(&dir)? {
            let entry=entry?;let name=entry.file_name().to_string_lossy().into_owned();
            let Some(stem)=name.strip_suffix(".cache")else{return deny("unknown cache material; preserved");};valid_uuid(stem)?;
            let guard=leaf_guard(&entry.path(),false)?;let meta=guard.metadata()?;total+=meta.len();
            items.push((meta.modified()?,meta.len(),entry.path()));
        }
        items.sort_by_key(|v|v.0);
        for (_,size,path) in items {if total<=self.quota{break;}
            // DELETE access and no delete sharing bind deletion to this object.
            let file=OpenOptions::new().access_mode(DELETE|FILE_READ_ATTRIBUTES).share_mode(FILE_SHARE_READ|FILE_SHARE_WRITE).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(&path)?;
            if file.metadata()?.file_attributes()&FILE_ATTRIBUTE_REPARSE_POINT!=0{return deny("cache reparse; preserved");}
            let info=FILE_DISPOSITION_INFO{DeleteFile:true};
            if unsafe{SetFileInformationByHandle(file.as_raw_handle(),FileDispositionInfo,(&info as *const FILE_DISPOSITION_INFO).cast(),std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32)}==0{return Err(io::Error::last_os_error().into());}
            drop(file);total-=size;
        }Ok(())
    }
    pub(crate) fn clear_rebuildable_cache(&mut self)->Result<()>{
        let quota=self.quota;self.quota=0;let result=self.trim_cache();self.quota=quota;result
    }
    /// Frozen by exclusive Store ownership (no tasks/IPC). Copies validated DB
    /// and recovery to a NEW root; retains source and every failed target.
    /// Deliberately does not change a real user-data locator or remove old data.
    pub(crate) fn prepare_directory_copy(&self,target:&Path,fault:&str)->Result<PathBuf> {
        let parent=target.parent().ok_or(DataError::Refused("missing parent"))?;
        if parent!=self.root.path.parent().unwrap()||target.file_name().and_then(|v|v.to_str()).is_none_or(|v|uuid::Uuid::parse_str(v).is_err()){return deny("unapproved directory target");}
        fs::create_dir(target)?;let root=DataRoot::open(target)?;
        let _db_guard=snapshot(&self.conn,&root.path.join("history.sqlite"))?;
        if fault=="exit-after-db"{std::process::exit(74);}
        if fault=="after-db"{return deny("injected directory interruption");}
        let dest=root.subdir("recovery")?;let source=self.root.subdir("recovery")?;
        let mut manifest=std::collections::BTreeMap::new();
        for entry in fs::read_dir(source)? {
            let entry=entry?;let name=entry.file_name().to_string_lossy().into_owned();
            let Some(id)=name.strip_suffix(".json")else{return deny("unknown recovery material; preserved");};valid_uuid(id)?;
            let data=read_owned(&entry.path(),1024*1024)?;
            let mut file=OpenOptions::new().write(true).create_new(true).open(dest.join(&name))?;file.write_all(&data)?;file.sync_all()?;drop(file);
            if read_owned(&dest.join(&name),1024*1024)?!=data{return deny("recovery copy mismatch");}
            manifest.insert(name,digest(&data));
        }
        let read=Connection::open_with_flags(root.path.join("history.sqlite"),OpenFlags::SQLITE_OPEN_READ_ONLY)?;preflight(&read)?;
        if logical_digest(&self.conn)?!=logical_digest(&read)?{return deny("directory data mismatch");}
        let mut ready=OpenOptions::new().write(true).create_new(true).open(root.path.join("copy-ready.json"))?;
        ready.write_all(serde_json::json!({"schema":SCHEMA,"databaseLogicalHash":logical_digest(&read)?,"recovery":manifest,"cache":"rebuild","sourceRetained":true}).to_string().as_bytes())?;ready.sync_all()?;
        Ok(root.path.clone())
    }
}
// Fixed table allowlist, preserving value type and boundaries, not just row counts.
pub(super) fn logical_digest(conn:&Connection)->Result<String> {
    use rusqlite::types::ValueRef;
    let mut hash=Sha256::new();
    for table in ["documents","revisions","locations","conversations","messages","message_chunks","anchors","positions","settings","save_events","data_meta","message_search"] {
        hash.update(table.as_bytes());let mut stmt=conn.prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))?;
        let columns=stmt.column_count();let mut rows=stmt.query([])?;
        while let Some(row)=rows.next()? {
            hash.update(b"ROW");
            for col in 0..columns {
                match row.get_ref(col)? {
                    ValueRef::Null=>hash.update([0]),
                    ValueRef::Integer(v)=>{hash.update([1]);hash.update(v.to_le_bytes());},
                    ValueRef::Real(v)=>{hash.update([2]);hash.update(v.to_bits().to_le_bytes());},
                    ValueRef::Text(v)|ValueRef::Blob(v)=>{hash.update([if matches!(row.get_ref(col)?,ValueRef::Text(_)){3}else{4}]);hash.update((v.len() as u64).to_le_bytes());hash.update(v);},
                }
            }
        }
    }Ok(format!("{:x}",hash.finalize()))
}
