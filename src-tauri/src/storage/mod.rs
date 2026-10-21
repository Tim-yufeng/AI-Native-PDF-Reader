//! Independent M4 core. Typed Rust calls only; no renderer SQL, path API or PDF writer.
//! Production roots are private app-owned directories; synthetic roots remain isolated.
#![allow(dead_code)]
pub(crate) mod model;
mod ai;
pub(crate) mod chat_view;
mod preferences;
mod maintenance;
mod parent_acl;
pub(crate) mod directories;
#[cfg(test)] mod directories_tests;
#[cfg(test)] mod history_delete_tests;
#[cfg(test)] mod tests;
use rusqlite::{Connection,OpenFlags,OptionalExtension};
use std::{fs::{self,File,OpenOptions},path::{Path,PathBuf},io::{self,Write},time::Duration};
use std::os::windows::{fs::{OpenOptionsExt,MetadataExt},io::AsRawHandle};
use windows_sys::Win32::Storage::FileSystem::*;
pub(crate) type Result<T> = std::result::Result<T,DataError>;
const APP_ID:i64=1095659602;
const SCHEMA:i64=2;
#[derive(Debug)] pub(crate) enum DataError { Io(io::Error), Sql(rusqlite::Error), Refused(&'static str) }
impl From<io::Error> for DataError {fn from(e:io::Error)->Self{Self::Io(e)}}
impl From<rusqlite::Error> for DataError {fn from(e:rusqlite::Error)->Self{Self::Sql(e)}}
fn deny<T>(message:&'static str)->Result<T>{Err(DataError::Refused(message))}
fn uuid()->String{uuid::Uuid::new_v4().to_string()}
fn valid_uuid(v:&str)->Result<()> {if uuid::Uuid::parse_str(v).is_err(){return deny("invalid UUID");}Ok(())}
fn valid_hash(v:&str)->Result<()> {if v.len()!=64||!v.bytes().all(|b|b.is_ascii_hexdigit()&&!b.is_ascii_uppercase()){return deny("invalid hash");}Ok(())}
fn leaf_guard(path:&Path,create:bool)->Result<File>{
    // Attribute-only handles do not provide the tested sharing protection.
    // Actual read access is compatible with SQLite readers/writers but denies rename.
    let file=if create {let f=OpenOptions::new().read(true).write(true).create_new(true)
        .share_mode(FILE_SHARE_READ|FILE_SHARE_WRITE).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(path)?;
        f.sync_all()?;f
    }else{OpenOptions::new().access_mode(0x8000_0000).share_mode(FILE_SHARE_READ|FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(path)?};
    let mut info=BY_HANDLE_FILE_INFORMATION::default();
    if unsafe{GetFileInformationByHandle(file.as_raw_handle(),&mut info)}==0{return Err(io::Error::last_os_error().into());}
    if info.dwFileAttributes&(FILE_ATTRIBUTE_REPARSE_POINT|FILE_ATTRIBUTE_DIRECTORY)!=0||info.nNumberOfLinks!=1{return deny("reparse/dir/hardlink leaf");}
    Ok(file)
}
struct DataRoot {path:PathBuf,_chain:Vec<File>,_lock:File,_subdirs:std::cell::RefCell<std::collections::BTreeMap<String,File>>,owned:bool}
impl DataRoot {
    fn open(path:&Path)->Result<Self>{
        let base=Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("tests/work/m4").canonicalize()?;
        let actual=path.canonicalize()?;
        if actual==base||!actual.starts_with(base){return deny("outside authorized synthetic data");}
        let mut chain=Vec::new();
        for dir in path.ancestors().collect::<Vec<_>>().into_iter().rev(){
            let f=OpenOptions::new().access_mode(0x8000_0000).share_mode(FILE_SHARE_READ|FILE_SHARE_WRITE)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS|FILE_FLAG_OPEN_REPARSE_POINT).open(dir)?;
            if f.metadata()?.file_attributes()&FILE_ATTRIBUTE_REPARSE_POINT!=0{return deny("reparse parent");}
            chain.push(f);
        }
        Self::finish(actual,chain,false)
    }
    fn finish(actual:PathBuf,chain:Vec<File>,owned:bool)->Result<Self>{
        let lock=OpenOptions::new().read(true).write(true).create(true).truncate(false).share_mode(0)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(actual.join("manager.lock"))?;
        let mut info=BY_HANDLE_FILE_INFORMATION::default();
        if unsafe{GetFileInformationByHandle(lock.as_raw_handle(),&mut info)}==0{return Err(io::Error::last_os_error().into());}
        if info.dwFileAttributes&(FILE_ATTRIBUTE_REPARSE_POINT|FILE_ATTRIBUTE_DIRECTORY)!=0||info.nNumberOfLinks!=1{return deny("reparse/dir/hardlink lock");}
        if owned{directories::check_private_file(&lock,false)?;}
        Ok(Self{path:actual,_chain:chain,_lock:lock,_subdirs:Default::default(),owned})
    }
    fn subdir(&self,name:&str)->Result<PathBuf>{
        if !matches!(name,"cache"|"recovery"|"backups"|"pdf-backups"|"indexes"|"tmp"|"logs"){return deny("unknown data area");}
        let path=self.path.join(name);
        if self._subdirs.borrow().contains_key(name){return Ok(path);}
        if !path.exists(){if self.owned{directories::create_private_dir(&path)?;}else{fs::create_dir(&path)?;}}
        let f=OpenOptions::new().access_mode(0x8000_0000).share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS|FILE_FLAG_OPEN_REPARSE_POINT).open(&path)?;
        if f.metadata()?.file_attributes()&FILE_ATTRIBUTE_REPARSE_POINT!=0{return deny("reparse subdir");}
        if self.owned{directories::check_private_file(&f,false)?;}
        self._subdirs.borrow_mut().insert(name.into(),f);
        Ok(path)
    }
}
pub(crate) struct Store {
    // Drop SQLite before name guards. No task is allowed to outlive this owner.
    conn:Connection,_leaves:Vec<File>,root:DataRoot,quota:u64,
}
fn integrity(conn:&Connection)->Result<()> {
    let ok:String=conn.query_row("PRAGMA integrity_check",[],|r|r.get(0))?;
    if ok!="ok"{return deny("database integrity");}
    let bad=conn.prepare("PRAGMA foreign_key_check")?.exists([])?;
    if bad{return deny("foreign key integrity");}Ok(())
}
fn preflight(conn:&Connection)->Result<i64> {
    let app:i64=conn.query_row("PRAGMA application_id",[],|r|r.get(0))?;
    let version:i64=conn.query_row("PRAGMA user_version",[],|r|r.get(0))?;
    if app!=APP_ID||!(1..=SCHEMA).contains(&version){return deny("foreign/future database");}
    integrity(conn)?;Ok(version)
}
fn persist_wal(conn:&Connection)->Result<()> {
    // Supported SQLite VFS control: keep the guarded WAL/SHM names instead of
    // attempting to delete them on close. Checkpoint still reclaims WAL bytes.
    let mut enabled=1i32;
    let rc=unsafe{rusqlite::ffi::sqlite3_file_control(conn.handle(),c"main".as_ptr(),
        rusqlite::ffi::SQLITE_FCNTL_PERSIST_WAL,(&mut enabled as *mut i32).cast())};
    if rc!=rusqlite::ffi::SQLITE_OK{return deny("persistent WAL unavailable");}Ok(())
}
impl Store {
    pub(crate) fn open_synthetic(path:&Path,quota:u64)->Result<Self>{Self::open_with_fault(path,quota,"")}
    fn open_with_fault(path:&Path,quota:u64,fault:&str)->Result<Self>{
        Self::open_root(DataRoot::open(path)?,quota,fault)
    }
    pub(crate) fn open_owned(path:&Path,quota:u64)->Result<Self>{
        Self::open_root(directories::open_owned_root(path)?,quota,"")
    }
    fn open_root(root:DataRoot,quota:u64,fault:&str)->Result<Self>{
        let db=root.path.join("history.sqlite");
        // An existing empty file is not presumed ours. Do not mutate unknown,
        // damaged or future DBs, including WAL mode changes, before preflight.
        let exists=db.try_exists()?;
        // Reject static hostile sidecars before SQLite sees them. Full production
        // sidecar race protection is a separate permission acceptance gate.
        let journal=root.path.join("history.sqlite-journal");
        let journal_guard=if journal.try_exists()?{
            // SQLite must be able to retire a rollback journal after recovery.
            // A private, pinned root excludes other-user name replacement; the
            // main DB/WAL/SHM retain their stricter no-delete object guards.
            let file=OpenOptions::new().read(true).share_mode(FILE_SHARE_READ|FILE_SHARE_WRITE|FILE_SHARE_DELETE)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(&journal)?;
            let mut info=BY_HANDLE_FILE_INFORMATION::default();
            if unsafe{GetFileInformationByHandle(file.as_raw_handle(),&mut info)}==0{return Err(io::Error::last_os_error().into());}
            if info.dwFileAttributes&(FILE_ATTRIBUTE_REPARSE_POINT|FILE_ATTRIBUTE_DIRECTORY)!=0||info.nNumberOfLinks!=1{return deny("unsafe rollback journal");}
            Some(file)
        }else{None};
        let main=leaf_guard(&db,!exists)?;
        let mut leaves=vec![main];
        if let Some(guard)=journal_guard{leaves.push(guard);}
        let old_version=if exists{
            for suffix in ["-wal","-shm"]{let p=root.path.join(format!("history.sqlite{suffix}"));if p.try_exists()?{leaves.push(leaf_guard(&p,false)?);}}
            if root.owned{for file in &leaves{directories::check_private_file(file,false)?;}}
            let read=Connection::open_with_flags(&db,OpenFlags::SQLITE_OPEN_READ_ONLY|OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
            preflight(&read)?
        }else{0};
        for suffix in ["-wal","-shm"]{let p=root.path.join(format!("history.sqlite{suffix}"));leaves.push(leaf_guard(&p,!p.try_exists()?)?);}
        if root.owned{for file in &leaves{directories::check_private_file(file,false)?;}}
        let mut conn=Connection::open_with_flags(&db,OpenFlags::SQLITE_OPEN_READ_WRITE|OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
        conn.busy_timeout(Duration::from_millis(250))?;
        conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA trusted_schema=OFF; PRAGMA synchronous=FULL;")?;
        // This known bundled patch is explicitly fixed for WAL-reset; runtime
        // assertions ensure a system SQLite was not silently substituted.
        let version:String=conn.query_row("SELECT sqlite_version()",[],|r|r.get(0))?;
        if version!="3.53.2"{return deny("unexpected SQLite runtime");}
        if old_version==0{
            let tx=conn.transaction()?;tx.execute_batch(include_str!("schema.sql"))?;tx.commit()?;
        }
        let mut store=Self{conn,_leaves:leaves,root,quota};
        if old_version<SCHEMA {store.upgrade(fault)?;}
        store.conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA wal_autocheckpoint=128; PRAGMA journal_size_limit=1048576;")?;
        persist_wal(&store.conn)?;
        let areas=if store.root.owned{vec!["cache","recovery","backups","indexes","tmp","logs"]}else{vec!["cache","recovery","backups"]};
        for name in areas{store.root.subdir(name)?;}
        if let Some(settings)=store.settings()?{store.quota=settings.cache_bytes;}
        store.trim_cache()?;
        integrity(&store.conn)?;
        // No live provider exists after restart. Keep partial content/chunks and
        // mark unfinished requests failed instead of pretending a reply completed.
        if fault!="live"{store.conn.execute("UPDATE messages SET status='failed' WHERE status='streaming'",[])?;}
        Ok(store)
    }
    fn upgrade(&mut self,fault:&str)->Result<()> {
        self.backup("schema-before",None)?;
        if fault=="full" {
            let pages:i64=self.conn.query_row("PRAGMA page_count",[],|r|r.get(0))?;
            self.conn.execute_batch(&format!("PRAGMA max_page_count={pages};"))?;
        }
        let tx=self.conn.transaction()?;
        tx.execute_batch("CREATE TABLE data_meta(key TEXT PRIMARY KEY,value TEXT NOT NULL); PRAGMA user_version=2;")?;
        if fault=="exit"{std::process::exit(74);}
        if fault=="ddl"{tx.execute_batch("INSERT INTO missing_table VALUES(1)")?;}
        integrity(&tx)?;tx.commit()?;Ok(())
    }
    pub(crate) fn checkpoint(&self)->Result<()> {
        let (busy,total,done):(i64,i64,i64)=self.conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
        if busy!=0||total!=done{return deny("WAL reader backlog");}Ok(())
    }
    fn writable(&self)->Result<()> {
        let p=self.root.path.join("history.sqlite-wal");
        if p.metadata()?.len()>8*1024*1024{return deny("WAL backlog; release readers/checkpoint");}Ok(())
    }
    pub(crate) fn check(&self)->Result<()> {integrity(&self.conn)}
}
