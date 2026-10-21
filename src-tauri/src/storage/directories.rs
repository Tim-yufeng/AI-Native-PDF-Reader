//! Private app-owned data roots and immutable, recoverable directory activation.
//! The caller holds its management mutex across every operation. ACL protection
//! is against other ordinary users; it is not a sandbox against the same user,
//! administrators, kernel code, or a malicious process holding prior authority.
use super::*;
use super::maintenance::{logical_digest, read_owned, snapshot};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, ffi::OsStr, os::windows::ffi::OsStrExt};
use windows_sys::Win32::{Foundation::{CloseHandle, LocalFree}, Security::*, Security::Authorization::*, System::Threading::{GetCurrentProcess, OpenProcessToken}};

const MATERIAL_LIMIT: u64 = 256 * 1024 * 1024;
const OWNER_MARKER: &str = ".ainative-owner.json";
fn wide(v: &OsStr) -> Vec<u16> { v.encode_wide().chain(Some(0)).collect() }
fn checksum(v: &[u8]) -> String { format!("{:x}", Sha256::digest(v)) }
pub(crate) fn sid_text(sid: PSID) -> Result<String> {
    let mut text = std::ptr::null_mut();
    if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 { return Err(io::Error::last_os_error().into()); }
    let mut len = 0;
    unsafe { while *text.add(len) != 0 { len += 1; } }
    let value = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text, len) });
    unsafe { LocalFree(text.cast()); }
    Ok(value)
}
pub(crate) fn current_sid() -> Result<String> {
    let mut token = std::ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 { return Err(io::Error::last_os_error().into()); }
    let mut size = 0;
    unsafe { GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut size); }
    let mut buffer = vec![0usize; (size as usize + std::mem::size_of::<usize>() - 1) / std::mem::size_of::<usize>()];
    let ok = unsafe { GetTokenInformation(token, TokenUser, buffer.as_mut_ptr().cast(), size, &mut size) };
    if ok == 0 { let error = io::Error::last_os_error(); unsafe { CloseHandle(token); } return Err(error.into()); }
    let result = sid_text(unsafe { (*(buffer.as_ptr().cast::<TOKEN_USER>())).User.Sid });
    unsafe { CloseHandle(token); }
    result
}
#[cfg(test)]
pub(super) fn deny_test_directory_writes(path:&Path)->Result<()>{
    let me=current_sid()?;
    let descriptor=format!("D:P(D;OICI;FW;;;{me})(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FR;;;{me})");
    let mut sd=std::ptr::null_mut();
    if unsafe{ConvertStringSecurityDescriptorToSecurityDescriptorW(wide(OsStr::new(&descriptor)).as_ptr(),1,&mut sd,std::ptr::null_mut())}==0{return Err(io::Error::last_os_error().into());}
    let ok=unsafe{SetFileSecurityW(wide(path.as_os_str()).as_ptr(),DACL_SECURITY_INFORMATION|PROTECTED_DACL_SECURITY_INFORMATION,sd)};
    let error=io::Error::last_os_error();unsafe{LocalFree(sd.cast());}
    if ok==0{return Err(error.into());}Ok(())
}
pub(crate) fn create_private_dir(path: &Path) -> Result<File> {
    let descriptor = format!("D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;{})", current_sid()?);
    let mut sd = std::ptr::null_mut();
    if unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(wide(OsStr::new(&descriptor)).as_ptr(), 1, &mut sd, std::ptr::null_mut()) } == 0 { return Err(io::Error::last_os_error().into()); }
    let sa = SECURITY_ATTRIBUTES { nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32, lpSecurityDescriptor: sd, bInheritHandle: 0 };
    let ok = unsafe { CreateDirectoryW(wide(path.as_os_str()).as_ptr(), &sa) };
    let error = io::Error::last_os_error();
    unsafe { LocalFree(sd.cast()); }
    if ok == 0 { return Err(error.into()); }
    let f = directory_handle(path)?;
    check_private_file(&f, true)?;Ok(f)
}
fn directory_handle(path: &Path) -> Result<File> {
    let file = OpenOptions::new().access_mode(0x8000_0000).share_mode(FILE_SHARE_READ|FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT).open(path)?;
    let attributes = file.metadata()?.file_attributes();
    if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 || attributes & FILE_ATTRIBUTE_DIRECTORY == 0 { return deny("unsafe directory"); }
    Ok(file)
}
fn check_safe_parent(file:&File)->Result<()>{super::parent_acl::check(file)}
pub(crate) fn check_private_file(file: &File, protected: bool) -> Result<()> {
    let mut owner = std::ptr::null_mut(); let mut acl = std::ptr::null_mut(); let mut sd = std::ptr::null_mut();
    let rc = unsafe { GetSecurityInfo(file.as_raw_handle(), SE_FILE_OBJECT, OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
        &mut owner, std::ptr::null_mut(), &mut acl, std::ptr::null_mut(), &mut sd) };
    if rc != 0 { return Err(io::Error::from_raw_os_error(rc as i32).into()); }
    let result = (|| check_private_descriptor(sd, owner, acl, &current_sid()?, protected))();
    unsafe { LocalFree(sd.cast()); }
    result
}
// Windows new-object owners come from TokenOwner, which can be Administrators,
// not necessarily TokenUser. These principals already have full control in our
// threat model. Do not change any on-disk descriptor or admit a foreign user.
fn check_private_descriptor(sd:PSECURITY_DESCRIPTOR,owner:PSID,acl:*mut ACL,me:&str,protected:bool)->Result<()> {
        if owner.is_null() || acl.is_null() || unsafe{IsValidAcl(acl)}==0 { return deny("private owner/DACL required"); }
        let owner_sid=sid_text(owner)?;
        if owner_sid!=me && owner_sid!="S-1-5-18" && owner_sid!="S-1-5-32-544" {return deny("foreign private owner");}
        let mut control = 0; let mut revision = 0;
        if unsafe { GetSecurityDescriptorControl(sd, &mut control, &mut revision) } == 0 { return Err(io::Error::last_os_error().into()); }
        if protected && control & SE_DACL_PROTECTED == 0 { return deny("protected root DACL required"); }
        let mut allowed = BTreeMap::new();
        for index in 0..unsafe { (*acl).AceCount } as u32 {
            let mut ace = std::ptr::null_mut();
            if unsafe { GetAce(acl, index, &mut ace) } == 0 { return Err(io::Error::last_os_error().into()); }
            let header = unsafe { &*(ace.cast::<ACE_HEADER>()) };
            if header.AceType != 0 { return deny("unexpected private ACL entry"); }
            let entry = unsafe { &*(ace.cast::<ACCESS_ALLOWED_ACE>()) };
            let sid = sid_text((&entry.SidStart as *const u32).cast_mut().cast())?;
            if sid != me && sid != "S-1-5-18" && sid != "S-1-5-32-544" { return deny("data exposed to other principal"); }
            if entry.Mask & FILE_ALL_ACCESS != FILE_ALL_ACCESS { return deny("private principal lacks full access"); }
            if protected && header.AceFlags & 3 != 3 { return deny("private directory inheritance required"); }
            allowed.insert(sid, ());
        }
        if allowed.len() != 3 { return deny("incomplete private DACL"); }
        Ok(())
}
#[cfg(test)]mod private_owner_tests{
    use super::*;
    fn probe(sddl:&str,protected:bool)->(String,Result<()>){
        let mut sd=std::ptr::null_mut();assert_ne!(unsafe{ConvertStringSecurityDescriptorToSecurityDescriptorW(wide(OsStr::new(sddl)).as_ptr(),1,&mut sd,std::ptr::null_mut())},0);
        let(mut owner,mut acl,mut defaulted,mut present)=(std::ptr::null_mut(),std::ptr::null_mut(),0,0);
        assert_ne!(unsafe{GetSecurityDescriptorOwner(sd,&mut owner,&mut defaulted)},0);
        assert_ne!(unsafe{GetSecurityDescriptorDacl(sd,&mut present,&mut acl,&mut defaulted)},0);
        let me=current_sid().unwrap();let text=sid_text(owner).unwrap();let result=check_private_descriptor(sd,owner,acl,&me,protected);
        unsafe{LocalFree(sd.cast());}(text,result)
    }
    #[test]fn windows_administrator_owner_is_not_a_foreign_user_and_dacl_remains_private(){
        let me=current_sid().unwrap();let entries=format!("(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;{me})");
        for owner in [me.as_str(),"BA","SY"]{let text=format!("O:{owner}D:P{entries}");let(actual,result)=probe(&text,true);assert!(result.is_ok(),"{owner}: {result:?}");if owner!="SY"&&owner!="BA"{assert_eq!(actual,me);}else{assert_ne!(actual,me,"legacy TokenUser-only check would refuse");}}
        let foreign="S-1-5-21-0-0-0-9999";
        for text in [format!("O:{foreign}D:P{entries}"),format!("O:BAD:{entries}"),format!("O:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)"),format!("O:BAD:NO_ACCESS_CONTROL")]{assert!(probe(&text,true).1.is_err(),"unsafe descriptor accepted: {text}");}
        for principal in ["AU","BU","WD",foreign]{assert!(probe(&format!("O:BAD:P{entries}(A;OICI;FA;;;{principal})"),true).1.is_err(),"foreign DACL grant");}
        println!("real Windows descriptors: current/admin/SYSTEM owner accepted; foreign/null/unprotected/incomplete/public permissions refused; no filesystem ACL modified");
    }
}
fn checked_parent(path: &Path) -> Result<(PathBuf, Vec<File>)> {
    if !path.is_absolute() || path.components().any(|v| matches!(v, std::path::Component::ParentDir | std::path::Component::CurDir)) { return deny("absolute directory required"); }
    // Reject UNC/device/ADS syntax before canonicalize can contact a share.
    let input=path.to_string_lossy();let local=input.strip_prefix(r"\\?\").unwrap_or(&input);
    if local.len()<3||!local.as_bytes()[0].is_ascii_alphabetic()||local.as_bytes()[1]!=b':'||!matches!(local.as_bytes()[2],b'\\'|b'/')||local[2..].contains(':'){return deny("local directory syntax required");}
    let actual = path.canonicalize()?;
    let text = actual.to_string_lossy();
    let plain = text.strip_prefix(r"\\?\").unwrap_or(&text);
    if plain.len() < 4 || plain.as_bytes().get(1) != Some(&b':') || plain[2..].contains(':') { return deny("local drive required"); }
    let drive = format!("{}\\", &plain[..2]);
    if unsafe { GetDriveTypeW(wide(OsStr::new(&drive)).as_ptr()) } != 3 { return deny("fixed local drive required"); }
    let mut chain = Vec::new();
    for dir in path.ancestors().collect::<Vec<_>>().into_iter().rev() { chain.push(directory_handle(dir)?); }
    if actual.parent().is_none() { return deny("drive root is not data parent"); }
    Ok((actual, chain))
}
fn ensure_new_parents(path: &Path) -> Result<()> {
    if path.try_exists()? { checked_parent(path)?; return Ok(()); }
    let parent = path.parent().ok_or(DataError::Refused("directory parent missing"))?;
    ensure_new_parents(parent)?;
    let _held = checked_parent(parent)?;check_safe_parent(_held.1.last().ok_or(DataError::Refused("parent handle"))?)?;
    create_private_dir(path).map(|_|())
}
#[cfg(test)]
pub(super) fn prepare_synthetic_cross_parent(path:&Path)->Result<()>{
    let local=PathBuf::from(std::env::var_os("LOCALAPPDATA").ok_or(DataError::Refused("LOCALAPPDATA unavailable"))?).join("io.github.ainativepdfreader/m4-synthetic-tests");
    let profile=PathBuf::from(std::env::var_os("USERPROFILE").ok_or(DataError::Refused("USERPROFILE unavailable"))?).join(".ai-native-pdf-reader-m4-synthetic-tests");
    if ![local.as_path(),profile.as_path()].into_iter().any(|base|path.parent()==Some(base))||path.file_name().and_then(|v|v.to_str()).is_none_or(|v|uuid::Uuid::parse_str(v).is_err())||path.try_exists()?{return deny("unapproved cross-drive test root");}
    ensure_new_parents(path)
}
#[cfg(test)]pub(super) fn prepare_default_validation(path:&Path)->Result<()>{let base=PathBuf::from(std::env::var_os("LOCALAPPDATA").ok_or(DataError::Refused("LOCALAPPDATA unavailable"))?).join("io.github.ainativepdfreader-m4-validation");if path.parent()!=Some(base.as_path())||path.file_name().and_then(|v|v.to_str()).is_none_or(|s|uuid::Uuid::parse_str(s).is_err())||path.try_exists()?{return deny("new synthetic default namespace required");}ensure_new_parents(path)}
#[cfg(test)]
pub(super) fn grant_test_parent_delete_child(path:&Path)->Result<()>{
    let me=current_sid()?;let text=format!("D:P(A;OICI;FA;;;{me})(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;;0x00000040;;;AU)");let mut sd=std::ptr::null_mut();
    if unsafe{ConvertStringSecurityDescriptorToSecurityDescriptorW(wide(OsStr::new(&text)).as_ptr(),1,&mut sd,std::ptr::null_mut())}==0{return Err(io::Error::last_os_error().into());}
    let ok=unsafe{SetFileSecurityW(wide(path.as_os_str()).as_ptr(),DACL_SECURITY_INFORMATION|PROTECTED_DACL_SECURITY_INFORMATION,sd)};let error=io::Error::last_os_error();unsafe{LocalFree(sd.cast());}if ok==0{return Err(error.into());}Ok(())
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all="camelCase", deny_unknown_fields)]
struct Owner { schema: u32, id: String, sid: String, kind: String }
fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut f = OpenOptions::new().write(true).create_new(true).share_mode(FILE_SHARE_READ).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(path)?;
    check_private_file(&f, false)?; f.write_all(bytes)?; f.sync_all()?; Ok(())
}
fn new_owned(path: &Path, kind: &str) -> Result<()> {
    let parent = path.parent().ok_or(DataError::Refused("missing directory parent"))?;
    let _held = checked_parent(parent)?;check_safe_parent(_held.1.last().ok_or(DataError::Refused("parent handle"))?)?;
    let _root=create_private_dir(path)?;
    write_new(&path.join(OWNER_MARKER), &serde_json::to_vec(&Owner{schema:1,id:uuid(),sid:current_sid()?,kind:kind.into()}).map_err(|_|DataError::Refused("owner format"))?)
}
fn owned_root(path: &Path, kind: &str) -> Result<DataRoot> {
    let (actual, chain) = checked_parent(path)?;
    check_safe_parent(chain.get(chain.len().saturating_sub(2)).ok_or(DataError::Refused("private root parent"))?)?;
    check_private_file(chain.last().ok_or(DataError::Refused("missing root handle"))?, true)?;
    let marker = leaf_guard(&actual.join(OWNER_MARKER), false)?; check_private_file(&marker, false)?;
    let owner: Owner = serde_json::from_slice(&read_owned(&actual.join(OWNER_MARKER), 4096)?).map_err(|_|DataError::Refused("invalid owner marker"))?;
    valid_uuid(&owner.id)?;
    if owner.schema != 1 || owner.sid != current_sid()? || owner.kind != kind { return deny("foreign data directory"); }
    DataRoot::finish(actual, chain, true)
}
pub(super) fn open_owned_root(path: &Path) -> Result<DataRoot> { owned_root(path, "data") }

#[derive(Clone, Serialize)]
#[serde(rename_all="camelCase")]
pub(crate) struct DirectoryInfo {
    pub path: String, pub acl: String, pub generation: u64, pub private_acl: bool, pub source_retained: bool,
    pub warning: Option<String>, pub areas: Vec<String>, pub cache_bytes: u64,
    pub incomplete_backups: u32,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all="camelCase", deny_unknown_fields)]
struct Activation { schema: u32, generation: u64, root: PathBuf }
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all="camelCase", deny_unknown_fields)]
pub(crate) struct BackupInfo { pub id: String, pub created_ms: u128, pub bytes: u64 }
#[derive(Serialize, Deserialize)]
#[serde(rename_all="camelCase", deny_unknown_fields)]
struct Manifest { schema: u32, logical_hash: String, files: BTreeMap<String,String>, created_ms: u128 }
pub(crate) struct DataManager { locator: DataRoot, store: Store, generation: u64, reserved: u64, warning: Option<String> }
#[derive(Debug)]pub(crate) struct InitializationFailure {pub stage:&'static str,pub cause:DataError}
impl DataManager {
    pub(crate) fn open(locator_dir: &Path, default_parent: &Path, quota: u64) -> Result<Self> {
        Self::open_reported(locator_dir,default_parent,quota).map_err(|failure|failure.cause)
    }
    pub(crate) fn open_reported(locator_dir:&Path,default_parent:&Path,quota:u64)->std::result::Result<Self,InitializationFailure>{
        let mut stage="locator-parent";
        Self::open_staged(locator_dir,default_parent,quota,&mut stage).map_err(|cause|InitializationFailure{stage,cause})
    }
    fn open_staged(locator_dir:&Path,default_parent:&Path,quota:u64,stage:&mut &'static str)->Result<Self>{
        if !locator_dir.try_exists()? { ensure_new_parents(locator_dir.parent().ok_or(DataError::Refused("locator parent"))?)?; *stage="locator-create";new_owned(locator_dir,"locator")?; }
        *stage="locator-verify";
        let locator = owned_root(locator_dir,"locator")?;
        *stage="locator-read";
        let mut records = Vec::new(); let mut reserved = 0; let mut pending = false;
        for entry in fs::read_dir(&locator.path)? {
            let entry = entry?; let name = entry.file_name().to_string_lossy().into_owned();
            if matches!(name.as_str(),OWNER_MARKER|"manager.lock") { continue; }
            let Some(stem) = name.strip_prefix("generation-").and_then(|v|v.strip_suffix(".json").or_else(||v.strip_suffix(".commit"))) else { return deny("unknown locator material; preserved"); };
            if stem.len()!=20 || !stem.bytes().all(|v|v.is_ascii_digit()) { return deny("invalid locator generation"); }
            let generation: u64 = stem.parse().map_err(|_|DataError::Refused("locator generation"))?; reserved=reserved.max(generation);
            if !name.ends_with(".json") { continue; }
            let path=locator.path.join(&name); let guard=leaf_guard(&path,false)?;check_private_file(&guard,false)?;
            let data=read_owned(&path,16384)?; let commit=path.with_extension("commit");
            if !commit.try_exists()? { pending=true;continue; }
            let commit_guard=leaf_guard(&commit,false)?;check_private_file(&commit_guard,false)?;
            let mark=read_owned(&commit,128)?;
            if mark.len()!=64 {pending=true;continue;}
            if mark!=checksum(&data).as_bytes() {return deny("locator digest mismatch; materials preserved");}
            let activation:Activation=serde_json::from_slice(&data).map_err(|_|DataError::Refused("invalid committed locator"))?;
            if activation.schema!=1 || activation.generation!=generation {return deny("locator version");}
            records.push(activation);
        }
        records.sort_by_key(|v|std::cmp::Reverse(v.generation));
        let mut failures=0;let mut last_failure=None;
        *stage="active-data-verify";
        for activation in records {
            *stage="active-data-verify";
            match open_committed(&activation.root,quota) {
                Ok(store)=>{
                    let warning=if failures>0{Some("新目录无法通过校验，已回到保留的旧目录；所有材料仍在。".into())}else if pending&&reserved>activation.generation{Some("上次目录切换未提交，仍使用原目录；未完成副本保留。".into())}else{None};
                    let mut manager=Self{locator,store,generation:activation.generation,reserved,warning};
                    if failures>0 {let root=manager.store.root.path.clone();*stage="activation-commit";manager.publish(&root,"")?;}
                    return Ok(manager)
                },
                Err(error)=>{failures+=1;last_failure=Some(error);},
            }
        }
        if failures>0{return Err(last_failure.unwrap_or(DataError::Refused("all data roots unavailable; preserved")));}
        // No committed locator exists: first start or an interrupted first start.
        *stage="data-parent";
        ensure_new_parents(default_parent)?;
        *stage="data-create";
        let root=default_parent.join(uuid());new_owned(&root,"data")?;
        *stage="sqlite-open";
        let store=Store::open_owned(&root,quota)?;
        let mut manager=Self{locator,store,generation:0,reserved,warning:if pending{Some("未完成首次初始化材料已保留，建立了新的空目录。".into())}else{None}};
        *stage="activation-commit";
        manager.publish(&root,"")?;Ok(manager)
    }
    pub(crate) fn store(&self)->&Store{&self.store}
    pub(crate) fn store_mut(&mut self)->&mut Store{&mut self.store}
    // The manager pins the configured private root throughout archival. No
    // frontend-supplied path, and a concurrent data migration cannot race this.
    pub(crate) fn archive_pdf(&self,outcome:&crate::original_commit::CommitOutcome,document:&str)->crate::error::Result<()>{
        let root=self.store.root.subdir("pdf-backups").map_err(crate::data::error)?;
        crate::original_commit::retain_completed(outcome,&root,document)
    }
    pub(crate) fn info(&self)->DirectoryInfo{
        let incomplete_backups=fs::read_dir(self.store.root.path.join("backups")).map(|entries|entries.filter_map(|e|e.ok()).filter(|e|e.file_type().is_ok_and(|t|t.is_dir())&&!e.path().join("manifest.json").is_file()).count()as u32).unwrap_or(0);
        let warning=if incomplete_backups>0{Some(format!("{}未完成的备份材料已保留，不会列为可恢复备份。",self.warning.as_deref().unwrap_or("")))}else{self.warning.clone()};
        DirectoryInfo{path:self.store.root.path.to_string_lossy().into_owned(),acl:"仅当前用户、SYSTEM 与管理员；已读取实际 DACL 校验".into(),generation:self.generation,private_acl:true,source_retained:true,warning,areas:vec!["history.sqlite（聊天、身份、设置）","recovery（未保存草稿）","backups（重要备份）","pdf-backups（每份文档最近两次保存前版本；随数据目录迁移；失败材料保留）","cache（可重建缓存）","indexes（可重建索引）","tmp（临时材料）","logs（日志）"].into_iter().map(String::from).collect(),cache_bytes:self.store.quota,incomplete_backups}
    }
    fn publish(&mut self,root:&Path,fault:&str)->Result<()> {
        let next=self.reserved.checked_add(1).ok_or(DataError::Refused("locator exhausted"))?;self.reserved=next;
        let path=self.locator.path.join(format!("generation-{next:020}.json"));
        let bytes=serde_json::to_vec(&Activation{schema:1,generation:next,root:root.into()}).map_err(|_|DataError::Refused("locator format"))?;
        write_new(&path,&bytes)?;
        if fault=="exit-before-switch"{std::process::exit(74);}
        if fault=="before-switch"{return deny("injected before switch");}
        write_new(&path.with_extension("commit"),checksum(&bytes).as_bytes())?;
        self.generation=next;
        if fault=="exit-after-switch"{std::process::exit(74);}
        Ok(())
    }
    pub(crate) fn migrate_to(&mut self,parent:&Path,fault:&str)->Result<DirectoryInfo> {
        let (actual,_chain)=checked_parent(parent)?;
        check_safe_parent(_chain.last().ok_or(DataError::Refused("parent handle"))?)?;
        let target=actual.join(uuid());
        if fault=="no-space"{return Err(io::Error::from_raw_os_error(112).into());}
        self.store.check()?; space_available(&actual,estimate_materials(&self.store)?+16*1024*1024)?;
        new_owned(&target,"data")?;
        let root=open_owned_root(&target)?;
        copy_store(&self.store,&root,fault)?;
        drop(root);
        if fault=="corrupt-target"{OpenOptions::new().write(true).truncate(true).open(target.join("history.sqlite"))?.write_all(b"corrupted synthetic target")?;}
        let next=Store::open_root(open_owned_root(&target)?,self.store.quota,"live")?;
        validate_live_materials(&next)?;
        if logical_digest(&self.store.conn)?!=logical_digest(&next.conn)?{return deny("reopened target differs");}
        self.publish(&target,fault)?;
        let old=std::mem::replace(&mut self.store,next);drop(old);
        self.warning=None;
        Ok(self.info())
    }
    pub(crate) fn backup(&self)->Result<BackupInfo>{
        let id=uuid();let target=self.store.root.subdir("backups")?.join(&id);let _held=create_private_dir(&target)?;
        let _db=snapshot(&self.store.conn,&target.join("history.sqlite"))?;
        let mut files=BTreeMap::new();
        copy_area(&self.store.root.subdir("recovery")?,&target.join("recovery"),"recovery",&mut files)?;
        let created_ms=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|_|DataError::Refused("clock"))?.as_millis();
        let manifest=Manifest{schema:1,logical_hash:logical_digest(&self.store.conn)?,files,created_ms};
        write_new(&target.join("manifest.json"),&serde_json::to_vec(&manifest).map_err(|_|DataError::Refused("manifest format"))?)?;
        Ok(BackupInfo{id,created_ms,bytes:sum_files(&target)?})
    }
    pub(crate) fn list_backups(&self)->Result<Vec<BackupInfo>>{
        let mut items=Vec::new();
        for e in fs::read_dir(self.store.root.subdir("backups")?)?{
            let e=e?;let id=e.file_name().to_string_lossy().into_owned();
            if !e.file_type()?.is_dir(){continue;}valid_uuid(&id)?;
            let verified=(||->Result<BackupInfo>{
                let held=directory_handle(&e.path())?;check_private_file(&held,false)?;
                let manifest=read_manifest(&e.path())?;verify_manifest(&e.path(),&manifest)?;
                let db=leaf_guard(&e.path().join("history.sqlite"),false)?;check_private_file(&db,false)?;
                let conn=Connection::open_with_flags(e.path().join("history.sqlite"),OpenFlags::SQLITE_OPEN_READ_ONLY)?;preflight(&conn)?;
                if logical_digest(&conn)?!=manifest.logical_hash{return deny("backup database differs");}
                Ok(BackupInfo{id,created_ms:manifest.created_ms,bytes:sum_files(&e.path())?})
            })();
            // Failed/incomplete material remains on disk, but must not prevent
            // the user finding a different, independently verified backup.
            if let Ok(info)=verified{items.push(info);}
        }items.sort_by_key(|v|std::cmp::Reverse(v.created_ms));Ok(items)
    }
    pub(crate) fn restore_backup(&mut self,id:&str)->Result<DirectoryInfo>{
        valid_uuid(id)?;let source=self.store.root.subdir("backups")?.join(id);
        let held=directory_handle(&source)?;check_private_file(&held,false)?;
        let manifest=read_manifest(&source)?;verify_manifest(&source,&manifest)?;
        let source_db=leaf_guard(&source.join("history.sqlite"),false)?;check_private_file(&source_db,false)?;
        let conn=Connection::open_with_flags(source.join("history.sqlite"),OpenFlags::SQLITE_OPEN_READ_ONLY)?;preflight(&conn)?;
        if logical_digest(&conn)?!=manifest.logical_hash{return deny("backup database differs");}
        let parent=self.store.root.path.parent().ok_or(DataError::Refused("restore parent"))?;
        space_available(parent,sum_files(&source)?+estimate_materials(&self.store)?+16*1024*1024)?;
        let target=parent.join(uuid());new_owned(&target,"data")?;let root=open_owned_root(&target)?;
        let _new_db=snapshot(&conn,&target.join("history.sqlite"))?;
        let mut files=BTreeMap::new();copy_area(&source.join("recovery"),&root.subdir("recovery")?,"recovery",&mut files)?;
        copy_area(&self.store.root.subdir("backups")?,&root.subdir("backups")?,"backups",&mut files)?;
        // Restoring chat history must not roll back or discard PDF recovery.
        copy_area(&self.store.root.subdir("pdf-backups")?,&root.subdir("pdf-backups")?,"pdf-backups",&mut files)?;
        let ready=Manifest{schema:1,logical_hash:manifest.logical_hash.clone(),files,created_ms:std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|_|DataError::Refused("clock"))?.as_millis()};
        verify_manifest(&target,&ready)?;write_new(&target.join("copy-ready.json"),&serde_json::to_vec(&ready).map_err(|_|DataError::Refused("manifest format"))?)?;
        drop(_new_db);drop(root);
        let next=Store::open_root(open_owned_root(&target)?,self.store.quota,"live")?;next.check()?;
        validate_live_materials(&next)?;
        if logical_digest(&next.conn)?!=manifest.logical_hash{return deny("restored database differs");}
        self.publish(&target,"")?;let old=std::mem::replace(&mut self.store,next);drop(old);self.warning=Some("已恢复到新目录；恢复前目录和全部备份保留。".into());Ok(self.info())
    }
}
fn read_manifest(root:&Path)->Result<Manifest>{let path=root.join("manifest.json");let guard=leaf_guard(&path,false)?;check_private_file(&guard,false)?;serde_json::from_slice(&read_owned(&path,2*1024*1024)?).map_err(|_|DataError::Refused("invalid material manifest"))}
fn safe_relative(v:&str)->Result<PathBuf>{let path=PathBuf::from(v);if path.is_absolute()||path.components().any(|c|!matches!(c,std::path::Component::Normal(_)))||v.contains(':'){return deny("unsafe manifest path");}Ok(path)}
fn open_committed(path:&Path,quota:u64)->Result<Store>{
    let root=open_owned_root(path)?;if !root.path.join("history.sqlite").try_exists()?{return deny("committed database missing; never initialize empty");}
    let ready=root.path.join("copy-ready.json");if ready.try_exists()?{
        let guard=leaf_guard(&ready,false)?;check_private_file(&guard,false)?;
        let manifest:Manifest=serde_json::from_slice(&read_owned(&ready,2*1024*1024)?).map_err(|_|DataError::Refused("copy-ready manifest"))?;
        // Copied immutable material remains verifiable after later SQL commits;
        // a snapshot logical DB hash must not be compared to a live newer DB.
        // PDF backup retention is mutable after activation. It was fully
        // checked before publish; each archive has its own verified journal.
        let mut immutable=manifest;immutable.files.retain(|name,_|!name.starts_with("pdf-backups/"));
        verify_manifest(&root.path,&immutable)?;
    }
    let store=Store::open_root(root,quota,"")?;validate_live_materials(&store)?;Ok(store)
}
fn validate_live_materials(store:&Store)->Result<()> {
    let mut statement=store.conn.prepare("SELECT key,value FROM data_meta WHERE key LIKE 'draft:%'")?;
    let rows=statement.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?;
    for row in rows {
        let(key,raw)=row?;let parts=key.split(':').collect::<Vec<_>>();if parts.len()!=4{return deny("draft reference key");}
        for id in &parts[1..]{valid_uuid(id)?;}
        let pages:u32=store.conn.query_row("SELECT pages FROM revisions WHERE id=?1 AND document_id=?2",rusqlite::params![parts[2],parts[1]],|r|r.get(0))?;
        let reference:serde_json::Value=serde_json::from_str(&raw).map_err(|_|DataError::Refused("draft reference JSON"))?;
        let file=reference.get("file").and_then(|v|v.as_str()).ok_or(DataError::Refused("draft recovery UUID"))?;valid_uuid(file)?;
        let bytes=store.recovery_get(file)?;
        if bytes.len()>256*1024{return deny("draft size");}
        let value:serde_json::Value=serde_json::from_slice(&bytes).map_err(|_|DataError::Refused("draft recovery JSON"))?;
        super::model::desktop::validate_draft(&value,pages)?;
    }Ok(())
}
fn verify_manifest(root:&Path,m:&Manifest)->Result<()>{
    if m.schema!=1{return deny("manifest schema");}valid_hash(&m.logical_hash)?;
    for(name,hash)in &m.files{
        valid_hash(hash)?;let relative=safe_relative(name)?;let mut path=root.to_path_buf();let mut held=Vec::new();
        let parts=relative.components().collect::<Vec<_>>();
        for component in parts.iter().take(parts.len().saturating_sub(1)){path.push(component.as_os_str());let file=directory_handle(&path)?;check_private_file(&file,false)?;held.push(file);}
        let path=root.join(relative);let file=leaf_guard(&path,false)?;check_private_file(&file,false)?;
        if checksum(&read_owned(&path,MATERIAL_LIMIT)?)!=*hash{return deny("material digest mismatch");}
    }Ok(())
}
fn space_available(parent:&Path,needed:u64)->Result<()>{let mut available=0; if unsafe{GetDiskFreeSpaceExW(wide(parent.as_os_str()).as_ptr(),&mut available,std::ptr::null_mut(),std::ptr::null_mut())}==0{return Err(io::Error::last_os_error().into());}if available<needed{return Err(io::Error::from_raw_os_error(112).into());}Ok(())}
fn sum_files(path:&Path)->Result<u64>{let _held=directory_handle(path)?;let mut sum=0u64;for e in fs::read_dir(path)?{let e=e?;if e.file_type()?.is_dir(){sum=sum.checked_add(sum_files(&e.path())?).ok_or(DataError::Refused("material size"))?;}else{let f=leaf_guard(&e.path(),false)?;check_private_file(&f,false)?;sum=sum.checked_add(f.metadata()?.len()).ok_or(DataError::Refused("material size"))?;}}Ok(sum)}
fn estimate_materials(s:&Store)->Result<u64>{let pages:i64=s.conn.query_row("PRAGMA page_count",[],|r|r.get(0))?;let size:i64=s.conn.query_row("PRAGMA page_size",[],|r|r.get(0))?;Ok((pages*size)as u64+sum_files(&s.root.subdir("recovery")?)?+sum_files(&s.root.subdir("backups")?)?+sum_files(&s.root.subdir("pdf-backups")?)?)}
fn copy_area(source:&Path,target:&Path,prefix:&str,manifest:&mut BTreeMap<String,String>)->Result<()>{
    let held=directory_handle(source)?;check_private_file(&held,false)?;
    if !target.try_exists()?{create_private_dir(target)?;}
    let target_held=directory_handle(target)?;check_private_file(&target_held,false)?;
    for e in fs::read_dir(source)?{
        let e=e?;let name=e.file_name().to_string_lossy().into_owned();safe_relative(&name)?;
        if prefix=="pdf-backups"||prefix.starts_with("pdf-backups/"){
            let depth=prefix.split('/').count();
            if e.file_type()?.is_dir(){
                match depth{1=>valid_uuid(&name)?,2=>valid_uuid(name.strip_prefix(".ainative-backup-").ok_or(DataError::Refused("unknown PDF archive"))?)?,_=>return deny("nested PDF archive")}
                copy_area(&e.path(),&target.join(&name),&format!("{prefix}/{name}"),manifest)?;
            }else{
                if depth!=3||!matches!(name.as_str(),"verified-old.pdf"|"intent.json"|"completed.json"|"acknowledged.json"|"retention-v1.json"){return deny("unknown PDF archive member");}
                let file=leaf_guard(&e.path(),false)?;check_private_file(&file,false)?;
                let data=read_owned(&e.path(),MATERIAL_LIMIT)?;write_new(&target.join(&name),&data)?;
                if read_owned(&target.join(&name),MATERIAL_LIMIT)?!=data{return deny("copied PDF archive differs");}
                manifest.insert(format!("{prefix}/{name}"),checksum(&data));
            }continue;
        }
        if e.file_type()?.is_dir(){if prefix=="recovery"||prefix.ends_with("/recovery"){return deny("unexpected recovery directory");}if name!="recovery"||!prefix.starts_with("backups/"){valid_uuid(&name)?;}copy_area(&e.path(),&target.join(&name),&format!("{prefix}/{name}"),manifest)?;}
        else{
            if prefix=="recovery"{let id=name.strip_suffix(".json").ok_or(DataError::Refused("unknown recovery material"))?;valid_uuid(id)?;}
            else if prefix=="backups"{let id=["schema-before-","manual-","directory-"].into_iter().find_map(|label|name.strip_prefix(label)).and_then(|v|v.strip_suffix(".sqlite")).ok_or(DataError::Refused("unknown backup material"))?;valid_uuid(id)?;}
            else if !matches!(name.as_str(),"history.sqlite"|"manifest.json")&& !prefix.ends_with("/recovery"){return deny("unknown nested backup material");}
            let file=leaf_guard(&e.path(),false)?;check_private_file(&file,false)?;
            let data=read_owned(&e.path(),MATERIAL_LIMIT)?;write_new(&target.join(&name),&data)?;
            if read_owned(&target.join(&name),MATERIAL_LIMIT)?!=data{return deny("copied material differs");}
            manifest.insert(format!("{prefix}/{name}"),checksum(&data));
        }
    }Ok(())
}
fn copy_store(source:&Store,target:&DataRoot,fault:&str)->Result<()>{
    let _db=snapshot(&source.conn,&target.path.join("history.sqlite"))?;
    if fault=="exit-after-snapshot"{std::process::exit(74);}
    if fault=="after-snapshot"{return deny("injected after snapshot");}
    let mut files=BTreeMap::new();copy_area(&source.root.subdir("recovery")?,&target.subdir("recovery")?,"recovery",&mut files)?;
    copy_area(&source.root.subdir("backups")?,&target.subdir("backups")?,"backups",&mut files)?;
    copy_area(&source.root.subdir("pdf-backups")?,&target.subdir("pdf-backups")?,"pdf-backups",&mut files)?;
    let conn=Connection::open_with_flags(target.path.join("history.sqlite"),OpenFlags::SQLITE_OPEN_READ_ONLY)?;preflight(&conn)?;
    let logical_hash=logical_digest(&source.conn)?;if logical_digest(&conn)?!=logical_hash{return deny("copied database differs");}
    let manifest=Manifest{schema:1,logical_hash,files,created_ms:std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|_|DataError::Refused("clock"))?.as_millis()};
    verify_manifest(&target.path,&manifest)?;write_new(&target.path.join("copy-ready.json"),&serde_json::to_vec(&manifest).map_err(|_|DataError::Refused("manifest format"))?)?;Ok(())
}
