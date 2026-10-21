//! Fixed, application-owned AppContainer setup. This does not launch a worker.
//! The caller must show a native confirmation before `setup_after_native_confirmation`.
//! Tests can verify descriptors/markers, but must never create a host profile.
use std::{collections::BTreeSet, ffi::OsStr, fs::{self, File, OpenOptions}, io::{self, Read},
    os::windows::{ffi::{OsStrExt, OsStringExt}, fs::{MetadataExt, OpenOptionsExt}, io::AsRawHandle},
    path::{Path, PathBuf}, sync::Mutex};
use serde::{Deserialize, Serialize};
use std::io::Write;
use windows_sys::Win32::{Foundation::{LocalFree, GENERIC_ALL, GENERIC_READ, GENERIC_WRITE, GENERIC_EXECUTE}, Security::{*, Authorization::*, Isolation::*},
    Storage::FileSystem::*};

pub(crate) const PROFILE_NAME: &str = "io.github.ainativepdfreader.writer.prototype";
pub(crate) const MARKER_NAME: &str = "sandbox-state.json";
const APP_ID: &str = "io.github.ainativepdfreader";
const SYSTEM: &str = "S-1-5-18";
const ADMIN: &str = "S-1-5-32-544";
const NOT_FOUND: i32 = 0x80070002u32 as i32;
#[cfg(not(test))]const ALREADY_EXISTS: i32 = 0x800700b7u32 as i32;
static SETUP: Mutex<()> = Mutex::new(());

// Public diagnostics are finite labels/numbers, never raw paths, SIDs, ACLs or
// arbitrary OS messages. Retain the earliest failing stage across callers.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all="camelCase")]
struct Diagnostic { sandbox_stage: &'static str, sandbox_reason: &'static str, windows_error: Option<u32>, worker_exit: Option<u32> }
impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "sandbox {}: {}", self.sandbox_stage, self.sandbox_reason) }
}
impl std::error::Error for Diagnostic {}
#[derive(Debug, Serialize)]
pub(crate) struct CheckError {
    code: &'static str, message: &'static str,
    #[serde(flatten)] diagnostic: Option<Diagnostic>,
}
impl From<crate::error::AppError> for CheckError {
    fn from(error: crate::error::AppError) -> Self { Self { code:error.code, message:error.message, diagnostic:None } }
}
fn diagnostic(stage: &'static str, error: &io::Error) -> Diagnostic {
    if let Some(value) = error.get_ref().and_then(|e| e.downcast_ref::<Diagnostic>()) { return *value; }
    let windows_error = error.raw_os_error().map(|code|code as u32);
    let reason = match windows_error {
        Some(5 | 0x80070005) => "access-denied", Some(2 | 3 | 0x80070002 | 0x80070003) => "not-found",
        Some(_) => "os-error",
        None => match error.to_string().as_str() {
            "linked or wrong-type sandbox resource" | "linked sandbox file" | "linked or unknown sandbox resource" => "linked-resource",
            "foreign sandbox owner" | "foreign runtime owner; no ACL changed" | "runtime owner missing" => "owner",
            "sandbox owner/DACL required" => "dacl-missing", "sandbox protected DACL required" => "dacl-unprotected",
            "unexpected sandbox ACL entry" => "ace-type", "invalid sandbox ACL SID" => "ace-sid",
            "unexpected sandbox access mask" => "access-mask", "sandbox runtime grants package write access" => "package-write",
            "sandbox exposed to unexpected principal" => "principal", "incomplete sandbox DACL" => "incomplete-dacl",
            "sandbox system profile mapped outside application path" => "path-mapping",
            "sandbox profile canonical path differs" | "sandbox runtime path differs" => "path-canonical",
            "existing unmarked sandbox profile refused; preserved" | "隔离配置已存在但缺少匹配的本应用标记，未认领或修改。" => "unmarked-profile",
            "sandbox ownership marker differs; preserved" | "sandbox ownership marker invalid; preserved" | "sandbox marker exceeds limit" | "sandbox marker commit missing" => "marker-identity",
            "sandbox profile appeared concurrently; not claimed" => "concurrent-profile",
            _ => "unknown",
        }
    };
    Diagnostic { sandbox_stage:stage, sandbox_reason:reason, windows_error, worker_exit:None }
}
fn at<T>(stage: &'static str, result: io::Result<T>) -> io::Result<T> {
    result.map_err(|error| io::Error::new(error.kind(), diagnostic(stage, &error)))
}
pub(crate) fn check_error(error: io::Error, creating: bool) -> CheckError {
    let diagnostic = diagnostic("setup", &error);
    let (code,message) = if creating { ("writer_profile_create_failed","Writer 隔离配置创建或资源授权校验未完成；已有配置保留，未启用 Writer。") }
        else { ("writer_profile_rejected","Writer 专属隔离配置身份或权限异常，保留现场并拒绝认领。") };
    CheckError { code,message,diagnostic:Some(diagnostic) }
}
pub(crate) fn probe_error(error:io::Error)->CheckError{
    let info=crate::unicode_worker::probe_details(&error);
    let(code,message)=if info.stage=="worker-start"&&info.windows_error==Some(2){("writer_startup_2","受限 Writer 启动失败（Windows 2），未改用普通权限。")}
        else{("writer_probe_failed","受限 Writer 自检未通过，未启用导出。")};
    CheckError{code,message,diagnostic:Some(Diagnostic{sandbox_stage:info.stage,sandbox_reason:info.reason,windows_error:info.windows_error,worker_exit:info.worker_exit})}
}

#[link(name="shell32")]
unsafe extern "system" { fn SHGetKnownFolderPath(id: *const windows_sys::core::GUID, flags: u32, token: windows_sys::Win32::Foundation::HANDLE, path: *mut *mut u16) -> i32; }
#[link(name="ole32")]
unsafe extern "system" { fn CoTaskMemFree(pointer: *mut std::ffi::c_void); }
const LOCAL_APP_DATA: windows_sys::core::GUID = windows_sys::core::GUID { data1: 0xf1b32785, data2: 0x6fba, data3: 0x4fcf, data4: [0x9d, 0x55, 0x7b, 0x8e, 0x7f, 0x15, 0x70, 0x91] };
fn refused(reason: &str) -> io::Error { io::Error::other(reason) }
fn wide(value: &OsStr) -> Vec<u16> { value.encode_wide().chain(Some(0)).collect() }
fn ok(value: i32) -> io::Result<()> { if value == 0 { Err(io::Error::last_os_error()) } else { Ok(()) } }
fn me() -> io::Result<String> { crate::storage::directories::current_sid().map_err(|_| refused("sandbox user identity unavailable")) }
fn sid_text(sid: PSID) -> io::Result<String> { crate::storage::directories::sid_text(sid).map_err(|_| refused("sandbox SID unavailable")) }
struct Sid(PSID);
impl Drop for Sid { fn drop(&mut self) { unsafe { FreeSid(self.0); } } }
fn package_sid() -> io::Result<Sid> {
    let mut sid = std::ptr::null_mut();
    let hr = unsafe { DeriveAppContainerSidFromAppContainerName(wide(OsStr::new(PROFILE_NAME)).as_ptr(), &mut sid) };
    if hr < 0 || sid.is_null() { return Err(refused("sandbox package identity unavailable")); }
    Ok(Sid(sid))
}
fn returned_path(value: *mut u16) -> io::Result<PathBuf> {
    if value.is_null() { return Err(refused("sandbox system path missing")); }
    let mut length = 0;
    unsafe { while length < 32768 && *value.add(length) != 0 { length += 1; } }
    let result = if length == 32768 { Err(refused("sandbox system path exceeds limit")) }
        else { Ok(PathBuf::from(std::ffi::OsString::from_wide(unsafe { std::slice::from_raw_parts(value, length) }))) };
    unsafe { CoTaskMemFree(value.cast()); }
    result
}
pub(crate) fn local_app_data() -> io::Result<PathBuf> {
    let mut path = std::ptr::null_mut();
    let hr = unsafe { SHGetKnownFolderPath(&LOCAL_APP_DATA, 0, std::ptr::null_mut(), &mut path) };
    if hr < 0 { return Err(refused("sandbox LocalAppData unavailable")); }
    returned_path(path)
}
fn profile_path(sid: &str) -> io::Result<Option<PathBuf>> {
    let mut path = std::ptr::null_mut();
    let hr = unsafe { GetAppContainerFolderPath(wide(OsStr::new(sid)).as_ptr(), &mut path) };
    if hr == NOT_FOUND { return Ok(None); }
    if hr < 0 { return Err(io::Error::from_raw_os_error(hr)); }
    returned_path(path).map(Some)
}
fn syntax(path: &Path) -> io::Result<()> {
    let value = path.to_string_lossy(); let value = value.strip_prefix(r"\\?\").unwrap_or(&value);
    if !path.is_absolute() || value.len() < 3 || !value.as_bytes()[0].is_ascii_alphabetic() || value.as_bytes()[1] != b':'
        || !matches!(value.as_bytes()[2], b'\\' | b'/') || value[2..].contains(':')
        || path.components().any(|c| matches!(c, std::path::Component::ParentDir | std::path::Component::CurDir)) {
        return Err(refused("sandbox local absolute path required"));
    } Ok(())
}
fn path_key(path: &Path) -> String {
    let value = path.to_string_lossy().replace('/', "\\");
    value.strip_prefix(r"\\?\").unwrap_or(&value).trim_end_matches('\\').to_ascii_lowercase()
}
fn open_guard(path: &Path, directory: bool, write_dacl: bool) -> io::Result<File> {
    let access = READ_CONTROL | FILE_READ_ATTRIBUTES | if write_dacl { WRITE_DAC } else { 0 };
    let file = OpenOptions::new().access_mode(access).share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | if directory { FILE_FLAG_BACKUP_SEMANTICS } else { 0 }).open(path)?;
    let meta = file.metadata()?;
    if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 || meta.is_dir() != directory { return Err(refused("linked or wrong-type sandbox resource")); }
    if !directory {
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        ok(unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) })?;
        if info.nNumberOfLinks != 1 { return Err(refused("linked sandbox file")); }
    }
    Ok(file)
}
fn pin_chain(path: &Path) -> io::Result<Vec<File>> {
    syntax(path)?;
    path.ancestors().collect::<Vec<_>>().into_iter().rev().map(|p| open_guard(p, true, false)).collect()
}
fn runtime_guard(root: &Path) -> io::Result<Vec<File>> {
    syntax(root)?;
    let development = crate::save_core::project().join(".cache/python-worker");
    let release = crate::unicode_worker::installed_runtime()?;
    if path_key(root) != path_key(&release) && !(cfg!(debug_assertions) && path_key(root) == path_key(&development)) {
        return Err(refused("sandbox runtime is not the fixed application resource"));
    }
    let chain = pin_chain(root)?;
    if path_key(&root.canonicalize()?) != path_key(root) { return Err(refused("sandbox runtime path differs")); }
    Ok(chain)
}
fn manifest_hash(root: &Path) -> io::Result<String> {
    let _guard = open_guard(&root.join("manifest.json"), false, false)?;
    let mut file = OpenOptions::new().read(true).share_mode(FILE_SHARE_READ).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(root.join("manifest.json"))?;
    let mut bytes = Vec::new(); Read::by_ref(&mut file).take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 { return Err(refused("sandbox manifest exceeds limit")); }
    let hash = crate::save_core::hash(&bytes);
    let lock: serde_json::Value = serde_json::from_str(include_str!("../../worker/runtime-lock.json"))?;
    if lock["schema"] != 1 || lock["manifestSha256"] != hash { return Err(refused("sandbox runtime manifest differs")); }
    Ok(hash)
}
#[derive(Clone, Copy)] enum Kind { Profile, Runtime, Marker }
fn mapped(mask: u32) -> u32 {
    let mut result = mask & !(GENERIC_ALL | GENERIC_READ | GENERIC_WRITE | GENERIC_EXECUTE);
    if mask & GENERIC_ALL != 0 { result |= FILE_ALL_ACCESS; }
    if mask & GENERIC_READ != 0 { result |= FILE_GENERIC_READ; }
    if mask & GENERIC_WRITE != 0 { result |= FILE_GENERIC_WRITE; }
    if mask & GENERIC_EXECUTE != 0 { result |= FILE_GENERIC_EXECUTE; }
    result
}
fn descriptor(sd: PSECURITY_DESCRIPTOR, owner: PSID, acl: *mut ACL, user: &str, package: &str, kind: Kind) -> io::Result<()> {
    if owner.is_null() || acl.is_null() || unsafe { IsValidAcl(acl) } == 0 { return Err(refused("sandbox owner/DACL required")); }
    if ![user, SYSTEM, ADMIN].contains(&sid_text(owner)?.as_str()) { return Err(refused("foreign sandbox owner")); }
    let (mut control, mut revision) = (0, 0);
    ok(unsafe { GetSecurityDescriptorControl(sd, &mut control, &mut revision) })?;
    if !matches!(kind, Kind::Profile) && control & SE_DACL_PROTECTED == 0 { return Err(refused("sandbox protected DACL required")); }
    let mut full = BTreeSet::new(); let mut package_read = false;
    for index in 0..unsafe { (*acl).AceCount } as u32 {
        let mut ace = std::ptr::null_mut(); ok(unsafe { GetAce(acl, index, &mut ace) })?;
        let header = unsafe { &*ace.cast::<ACE_HEADER>() };
        if header.AceType != 0 || header.AceSize < 16 { return Err(refused("unexpected sandbox ACL entry")); }
        let entry = unsafe { &*ace.cast::<ACCESS_ALLOWED_ACE>() };
        let sid: PSID = (&entry.SidStart as *const u32).cast_mut().cast();
        let subauthorities = unsafe { *sid.cast::<u8>().add(1) } as usize;
        if subauthorities > 15 || 16 + 4 * subauthorities > header.AceSize as usize || unsafe { IsValidSid(sid) } == 0
            || unsafe { GetLengthSid(sid) } as usize != 8 + 4 * subauthorities { return Err(refused("invalid sandbox ACL SID")); }
        let principal = sid_text(sid)?; let access = mapped(entry.Mask);
        if [user, SYSTEM, ADMIN].contains(&principal.as_str()) {
            if access & !FILE_ALL_ACCESS != 0 { return Err(refused("unexpected sandbox access mask")); }
            if (header.AceFlags as u32) & INHERIT_ONLY_ACE == 0 && access == FILE_ALL_ACCESS { full.insert(principal); }
        } else if principal == package && !matches!(kind, Kind::Marker) {
            let permitted = if matches!(kind, Kind::Profile) { FILE_ALL_ACCESS } else { FILE_GENERIC_READ | FILE_GENERIC_EXECUTE };
            if access & !permitted != 0 { return Err(refused("sandbox runtime grants package write access")); }
            if (header.AceFlags as u32) & INHERIT_ONLY_ACE == 0 && access & (FILE_GENERIC_READ | FILE_GENERIC_EXECUTE) == (FILE_GENERIC_READ | FILE_GENERIC_EXECUTE) { package_read = true; }
        } else { return Err(refused("sandbox exposed to unexpected principal")); }
    }
    if ![user, SYSTEM, ADMIN].iter().all(|p| full.contains(*p)) || matches!(kind, Kind::Runtime) && !package_read {
        return Err(refused("incomplete sandbox DACL"));
    } Ok(())
}
fn check_acl(file: &File, user: &str, package: &str, kind: Kind) -> io::Result<()> {
    let (mut owner, mut acl, mut sd) = (std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut());
    let code = unsafe { GetSecurityInfo(file.as_raw_handle(), SE_FILE_OBJECT, OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
        &mut owner, std::ptr::null_mut(), &mut acl, std::ptr::null_mut(), &mut sd) };
    if code != 0 { return Err(io::Error::from_raw_os_error(code as i32)); }
    let result = descriptor(sd, owner, acl, user, package, kind); unsafe { LocalFree(sd.cast()); } result
}
#[derive(Serialize, Deserialize)] #[serde(rename_all="camelCase", deny_unknown_fields)]
struct Marker { schema: u32, app_id: String, profile: String, package_sid: String, user_sid: String, instance: String }
impl Marker {
    fn verify(&self, user: &str, package: &str) -> io::Result<()> {
        if self.schema != 1 || self.app_id != APP_ID || self.profile != PROFILE_NAME || self.package_sid != package || self.user_sid != user
            || uuid::Uuid::parse_str(&self.instance).is_err() {
            return Err(refused("sandbox ownership marker differs; preserved"));
        } Ok(())
    }
}
fn read_marker(root: &Path, user: &str, package: &str) -> io::Result<Option<Marker>> {
    let path = root.join(MARKER_NAME); if !path.try_exists()? { return Ok(None); }
    let guard = open_guard(&path, false, false)?; check_acl(&guard, user, package, Kind::Marker)?;
    let mut file = OpenOptions::new().read(true).share_mode(FILE_SHARE_READ).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(path)?;
    let mut bytes = Vec::new(); Read::by_ref(&mut file).take(4097).read_to_end(&mut bytes)?;
    if bytes.len() > 4096 { return Err(refused("sandbox marker exceeds limit")); }
    let marker: Marker = serde_json::from_slice(&bytes).map_err(|_| refused("sandbox ownership marker invalid; preserved"))?;
    marker.verify(user, package)?; Ok(Some(marker))
}
fn checked_profile(path: &Path, user: &str, package: &str) -> io::Result<Vec<File>> {
    let expected = at("profile-path",local_app_data())?.join("Packages").join(PROFILE_NAME);
    // Windows' documented LOCALAPPDATA projection is the fixed AC subfolder.
    // Accept only the application root or that exact projection, not descendants.
    if !profile_mapping(path, &expected) { return at("profile-path",Err(refused("sandbox system profile mapped outside application path"))); }
    let mut chain = at("profile-path",pin_chain(path))?;
    if path_key(&at("profile-path",path.canonicalize())?) != path_key(path) { return at("profile-path",Err(refused("sandbox profile canonical path differs"))); }
    let stage = if path_key(path)==path_key(&expected) { "profile-root" } else { "profile-ac" };
    at(stage,check_acl(chain.last().ok_or_else(|| refused("sandbox profile guard missing"))?, user, package, Kind::Profile))?;
    let root = at("profile-root",open_guard(&expected, true, false))?;
    at("profile-root",check_acl(&root, user, package, Kind::Profile))?; chain.push(root);
    Ok(chain)
}
fn profile_mapping(path: &Path, expected: &Path) -> bool { path_key(path) == path_key(expected) || path_key(path) == path_key(&expected.join("AC")) }
#[derive(Serialize)] #[serde(rename_all="camelCase")]
pub(crate) struct Status { pub profile_registered: bool, pub runtime_authorized: bool, pub state: &'static str, pub detail: &'static str }
pub(crate) fn status(root: &Path) -> io::Result<Status> {
    let _lock = SETUP.lock().map_err(|_| refused("sandbox setup lock unavailable"))?;
    let _guards = at("setup",runtime_guard(root))?;
    // Hold every checked resource and directory until all ACL/profile checks
    // finish, so an already-validated child cannot become a junction mid-walk.
    let _runtime_pins = at("setup",crate::unicode_worker::lock_runtime(root))?;
    let _manifest = at("setup",manifest_hash(root))?; let user = at("identity",me())?; let sid = at("identity",package_sid())?; let package = at("identity",sid_text(sid.0))?;
    let marker = at("marker",read_marker(root, &user, &package))?;
    let Some(path) = at("profile-query",profile_path(&package))? else { return Ok(Status { profile_registered: false, runtime_authorized: false, state: "setup-required", detail: "需要用户确认创建本应用专属 Windows 隔离配置；不自动操作。" }); };
    let _profile = checked_profile(&path, &user, &package)?;
    if marker.is_none() { return Ok(Status { profile_registered: true, runtime_authorized: false, state: "foreign-or-unmarked", detail: "隔离配置已存在但缺少匹配的本应用标记，未认领或修改。" }); }
    at("runtime-acl",verify_runtime_acl(root, &user, &package))?;
    Ok(Status { profile_registered: true, runtime_authorized: true, state: "configured-not-validated", detail: "专属配置和资源权限已核对，仍需真实受限进程验证。" })
}
pub(crate) fn verify(root: &Path) -> io::Result<()> {
    let result = status(root)?;
    if !result.runtime_authorized { return Err(refused(result.detail)); } Ok(())
}
pub(crate) fn ready(root: &Path) -> io::Result<bool> {
    let result = status(root)?;
    if result.state == "foreign-or-unmarked" { return at("marker",Err(refused(result.detail))); }
    Ok(result.runtime_authorized)
}
pub(crate) fn provision(root: &Path) -> io::Result<()> { setup_after_native_confirmation(root).map(|_| ()) }
fn visit_runtime(root: &Path, action: &mut impl FnMut(&Path, bool) -> io::Result<()>) -> io::Result<()> {
    action(root, true)?;
    fn visit(dir: &Path, count: &mut usize, action: &mut impl FnMut(&Path, bool) -> io::Result<()>) -> io::Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?; *count += 1;
            if *count > 1200 { return Err(refused("sandbox resource count exceeds limit")); }
            let metadata = fs::symlink_metadata(entry.path())?;
            if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 || !metadata.is_dir() && !metadata.is_file() { return Err(refused("linked or unknown sandbox resource")); }
            let guard = open_guard(&entry.path(), metadata.is_dir(), false)?;
            action(&entry.path(), metadata.is_dir())?;
            if metadata.is_dir() { visit(&entry.path(), count, action)?; }
            drop(guard); // Retain this exact child across recursive operations.
        } Ok(())
    }
    visit(root, &mut 0, action)
}
fn verify_runtime_acl(root: &Path, user: &str, package: &str) -> io::Result<()> {
    visit_runtime(root, &mut |path, directory| {
        let guard = open_guard(path, directory, false)?;
        check_acl(&guard, user, package, if path == root.join(MARKER_NAME) { Kind::Marker } else { Kind::Runtime })
    })
}
fn set_private_acl(file: &File, user: &str, package: Option<&str>) -> io::Result<()> {
    let sid_entry = package.map(|sid| format!("(A;OICI;GRGX;;;{sid})")).unwrap_or_default();
    let text = format!("D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;{user}){sid_entry}");
    let mut sd = std::ptr::null_mut(); ok(unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(wide(OsStr::new(&text)).as_ptr(), 1, &mut sd, std::ptr::null_mut()) })?;
    let (mut present, mut defaulted, mut acl) = (0, 0, std::ptr::null_mut());
    let result = (|| { ok(unsafe { GetSecurityDescriptorDacl(sd, &mut present, &mut acl, &mut defaulted) })?;
        let code = unsafe { SetSecurityInfo(file.as_raw_handle(), SE_FILE_OBJECT, DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(), std::ptr::null_mut(), acl, std::ptr::null_mut()) };
        if code != 0 { return Err(io::Error::from_raw_os_error(code as i32)); } Ok(()) })();
    unsafe { LocalFree(sd.cast()); } result
}
fn grant_runtime(root: &Path, user: &str, package: &str) -> io::Result<()> {
    // The caller has already pinned and validated the full runtime inventory.
    // Use handles with OPEN_REPARSE_POINT and refuse hardlinks; no parent ACL writes.
    visit_runtime(root, &mut |path, directory| {
        let file = open_guard(path, directory, true)?;
        let (mut owner, mut sd) = (std::ptr::null_mut(), std::ptr::null_mut());
        let code = unsafe { GetSecurityInfo(file.as_raw_handle(), SE_FILE_OBJECT, OWNER_SECURITY_INFORMATION,
            &mut owner, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), &mut sd) };
        if code != 0 { return Err(io::Error::from_raw_os_error(code as i32)); }
        let identity = if owner.is_null() { Err(refused("runtime owner missing")) } else { sid_text(owner) };
        unsafe { LocalFree(sd.cast()); }
        let allowed = [user, SYSTEM, ADMIN].contains(&identity?.as_str());
        if !allowed { return Err(refused("foreign runtime owner; no ACL changed")); }
        set_private_acl(&file, user, if path == root.join(MARKER_NAME) { None } else { Some(package) })
    })
}
fn create_marker(root: &Path, user: &str, package: &str) -> io::Result<()> {
    // access_mode overrides Win32 rights, but does not satisfy Rust's separate
    // create_new write-intent validation. Explicitly set both; never overwrite.
    let mut file=OpenOptions::new().write(true).access_mode(GENERIC_WRITE|READ_CONTROL|WRITE_DAC).create_new(true)
        .share_mode(FILE_SHARE_READ).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(root.join(MARKER_NAME))?;
    set_private_acl(&file,user,None)?;
    let value=Marker {schema:1,app_id:APP_ID.into(),profile:PROFILE_NAME.into(),package_sid:package.into(),user_sid:user.into(),instance:uuid::Uuid::new_v4().to_string()};
    file.write_all(&serde_json::to_vec(&value)?)?;file.sync_all()?;Ok(())
}
pub(crate) fn setup_after_native_confirmation(root: &Path) -> io::Result<Status> {
    #[cfg(test)] { let _ = root; return Err(refused("host profile creation forbidden in tests")); }
    #[cfg(not(test))] {
        let _lock = SETUP.lock().map_err(|_| refused("sandbox setup lock unavailable"))?;
        let _guards = at("setup",runtime_guard(root))?; let _manifest = at("setup",manifest_hash(root))?;
        let user = at("identity",me())?; let sid = at("identity",package_sid())?; let package = at("identity",sid_text(sid.0))?;
        let marker = at("marker",read_marker(root, &user, &package))?;
        // Retain the full immutable inventory, including child directories,
        // across profile setup and each ACL update. Validation alone is not a lock.
        let _runtime_pins = at("setup",crate::unicode_worker::lock_runtime(root))?;
        let existing = at("profile-query",profile_path(&package))?;
        if existing.is_some() && marker.is_none() { return at("marker",Err(refused("existing unmarked sandbox profile refused; preserved"))); }
        let path = if let Some(path) = existing { path } else {
            let mut created = std::ptr::null_mut();
            let hr = unsafe { CreateAppContainerProfile(wide(OsStr::new(PROFILE_NAME)).as_ptr(), wide(OsStr::new("AI Native PDF Reader Unicode Worker")).as_ptr(),
                wide(OsStr::new("本应用专属、无网络权限的 PDF Unicode 处理隔离配置")).as_ptr(), std::ptr::null(), 0, &mut created) };
            if hr == ALREADY_EXISTS { return at("profile-create",Err(refused("sandbox profile appeared concurrently; not claimed"))); }
            if hr < 0 { return at("profile-create",Err(io::Error::from_raw_os_error(hr))); }
            let created = Sid(created);
            if created.0.is_null() || unsafe { EqualSid(created.0, sid.0) } == 0 { return at("identity",Err(refused("sandbox profile returned unexpected SID; preserved"))); }
            at("profile-query",profile_path(&package).and_then(|path|path.ok_or_else(|| refused("created sandbox profile unavailable; preserved"))))?
        };
        let _profile = checked_profile(&path, &user, &package)?;
        at("runtime-acl",grant_runtime(root, &user, &package))?;
        if marker.is_none() {
            at("marker",create_marker(root,&user,&package))?;
        }
        at("marker",read_marker(root, &user, &package).and_then(|value|value.ok_or_else(|| refused("sandbox marker commit missing"))))?;
        at("runtime-acl",verify_runtime_acl(root, &user, &package))?;
        Ok(Status { profile_registered: true, runtime_authorized: true, state: "configured-not-validated", detail: "专属配置已创建并校验，仍需真实受限进程及拒绝访问验证。" })
    }
}

#[cfg(test)] mod tests {
    use super::*;
    fn probe(text: &str, kind: Kind, user: &str, package: &str) -> io::Result<()> {
        let mut sd = std::ptr::null_mut(); assert_ne!(unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(wide(OsStr::new(text)).as_ptr(), 1, &mut sd, std::ptr::null_mut()) }, 0);
        let (mut owner, mut acl, mut defaulted, mut present) = (std::ptr::null_mut(), std::ptr::null_mut(), 0, 0);
        assert_ne!(unsafe { GetSecurityDescriptorOwner(sd, &mut owner, &mut defaulted) }, 0);
        assert_ne!(unsafe { GetSecurityDescriptorDacl(sd, &mut present, &mut acl, &mut defaulted) }, 0);
        let result = descriptor(sd, owner, acl, user, package, kind); unsafe { LocalFree(sd.cast()); } result
    }
    #[test] fn descriptor_allowlist_does_not_accept_public_or_package_writable_runtime() {
        let user = me().unwrap(); let sid = package_sid().unwrap(); let package = sid_text(sid.0).unwrap();
        let trusted = format!("(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;{user})");
        let runtime = format!("O:BAD:P{trusted}(A;OICI;GRGX;;;{package})");
        assert!(probe(&runtime, Kind::Runtime, &user, &package).is_ok());
        assert!(probe(&format!("O:{user}D:{trusted}(A;OICI;FA;;;{package})"), Kind::Profile, &user, &package).is_ok());
        assert!(probe(&format!("O:SYD:P{trusted}"), Kind::Marker, &user, &package).is_ok());
        for principal in ["WD", "AU", "BU", "S-1-5-21-0-0-0-9999"] { assert!(probe(&format!("{runtime}(A;OICI;FA;;;{principal})"), Kind::Runtime, &user, &package).is_err()); }
        for text in [format!("O:BAD:P{trusted}(A;OICI;FA;;;{package})"), format!("O:BAD:{trusted}(A;OICI;GRGX;;;{package})"), "O:BAD:NO_ACCESS_CONTROL".into(), format!("O:S-1-5-21-0-0-0-9999D:P{trusted}(A;OICI;GRGX;;;{package})")] { assert!(probe(&text, Kind::Runtime, &user, &package).is_err()); }
        assert!(probe(&runtime, Kind::Marker, &user, &package).is_err());
    }
    #[test] fn sandbox_reported_profile_descriptors_are_accepted_without_policy_changes() {
        // Anonymous Windows Sandbox diagnostic v2, 2026-10-14. Reconstruct only
        // the reported masks/flags/owner classes; no host profile is inspected.
        let user=me().unwrap();let sid=package_sid().unwrap();let package=sid_text(sid.0).unwrap();
        for ac in [false,true] {
            let package_entry=if ac {format!("(A;OICI;FA;;;{package})")} else {String::new()};
            let sddl=format!("O:BAD:{package_entry}(A;OICIID;FA;;;SY)(A;OICIID;FA;;;BA)(A;OICIID;FA;;;{user})");
            let mut sd=std::ptr::null_mut();assert_ne!(unsafe {ConvertStringSecurityDescriptorToSecurityDescriptorW(wide(OsStr::new(&sddl)).as_ptr(),1,&mut sd,std::ptr::null_mut())},0);
            let(mut owner,mut acl,mut defaulted,mut present)=(std::ptr::null_mut(),std::ptr::null_mut(),0,0);
            assert_ne!(unsafe {GetSecurityDescriptorOwner(sd,&mut owner,&mut defaulted)},0);
            assert_ne!(unsafe {GetSecurityDescriptorDacl(sd,&mut present,&mut acl,&mut defaulted)},0);
            if ac {
                // Preserve the actual 0x23 package ACE flag, including Windows'
                // additional flag bit; do not invent its security semantics.
                let mut ace=std::ptr::null_mut();assert_ne!(unsafe {GetAce(acl,0,&mut ace)},0);
                unsafe {(*ace.cast::<ACE_HEADER>()).AceFlags=0x23;}
                assert_ne!(unsafe {SetSecurityDescriptorControl(sd,SE_DACL_AUTO_INHERITED,SE_DACL_AUTO_INHERITED)},0);
            }
            let result=descriptor(sd,owner,acl,&user,&package,Kind::Profile);
            unsafe {LocalFree(sd.cast());}
            assert!(result.is_ok(),"reported profile policy: {result:?}");
        }
    }
    #[test] fn marker_custom_access_alone_does_not_enable_rust_create_new_write_intent() {
        let root=crate::save_core::project().join("tests/work/writer").join(format!("marker-options-{}",uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();let path=root.join(MARKER_NAME);
        let result=OpenOptions::new().access_mode(GENERIC_WRITE|READ_CONTROL|WRITE_DAC).create_new(true).share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(&path);
        assert!(result.is_err(),"legacy creation unexpectedly succeeded");
        let error=result.unwrap_err();assert_eq!(error.kind(),io::ErrorKind::InvalidInput);
        assert!(error.raw_os_error().is_none(),"failure occurs before the Win32 file call");
        assert!(!path.exists(),"legacy failure must not create a partial marker");
        let user=me().unwrap();let sid=package_sid().unwrap();let package=sid_text(sid.0).unwrap();
        create_marker(&root,&user,&package).expect("production marker creation");
        let marker=read_marker(&root,&user,&package).unwrap().expect("committed marker");
        marker.verify(&user,&package).unwrap();let before=fs::read(&path).unwrap();
        assert!(create_marker(&root,&user,&package).is_err(),"existing marker must never be overwritten");
        assert_eq!(fs::read(&path).unwrap(),before);
        assert!(marker.verify("S-1-5-21-0-0-0-9999",&package).is_err());
    }
    #[test] fn runtime_grant_core_works_with_immutable_inventory_pins_on_disposable_files() {
        let root=crate::save_core::project().join("tests/work/writer").join(format!("grant-core-{}",uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();fs::create_dir(root.join("worker")).unwrap();
        for (name,bytes) in [("manifest.json",b"synthetic manifest".as_slice()),("worker/main.py",b"synthetic worker".as_slice())] {
            fs::write(root.join(name),bytes).unwrap();
        }
        let user=me().unwrap();let sid=package_sid().unwrap();let package=sid_text(sid.0).unwrap();
        let _guards=pin_chain(&root).unwrap();let mut inventory=Vec::new();
        for path in [root.join("manifest.json"),root.join("worker/main.py")] {
            crate::unicode_worker::pin_chain(path.parent().unwrap(),&mut inventory).unwrap();
            inventory.push(OpenOptions::new().read(true).share_mode(FILE_SHARE_READ).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(path).unwrap());
        }
        grant_runtime(&root,&user,&package).expect("production grant with pinned inventory");
        verify_runtime_acl(&root,&user,&package).expect("production verification after grant");
        create_marker(&root,&user,&package).expect("marker while holding production inventory pins");
        assert!(read_marker(&root,&user,&package).unwrap().is_some());
        verify_runtime_acl(&root,&user,&package).expect("marker stays private, runtime stays read-only to Worker");
        assert_eq!(fs::read(root.join("worker/main.py")).unwrap(),b"synthetic worker");
        assert!(fs::rename(root.join("worker"),root.join("moved")).is_err());
    }
    #[test] fn ownership_marker_is_fixed_and_bound_to_user_and_package() {
        let user = me().unwrap(); let sid = package_sid().unwrap(); let package = sid_text(sid.0).unwrap();
        let marker = Marker { schema: 1, app_id: APP_ID.into(), profile: PROFILE_NAME.into(), package_sid: package.clone(), user_sid: user.clone(), instance: uuid::Uuid::new_v4().to_string() };
        let value = serde_json::to_value(marker).unwrap(); let original: Marker = serde_json::from_value(value.clone()).unwrap(); assert!(original.verify(&user, &package).is_ok());
        for field in ["appId", "profile", "packageSid", "userSid", "instance"] { let mut changed = value.clone(); changed[field] = "foreign".into(); let marker: Marker = serde_json::from_value(changed).unwrap(); assert!(marker.verify(&user, &package).is_err()); }
        let mut changed = value; changed["extra"] = true.into(); assert!(serde_json::from_value::<Marker>(changed).is_err());
        assert!(syntax(Path::new(r"\\server\share\worker")).is_err()); assert!(syntax(Path::new(r"D:\x\..\worker")).is_err()); assert!(syntax(Path::new(r"D:\x:stream")).is_err());
        let expected = Path::new(r"C:\Users\Synthetic\AppData\Local\Packages").join(PROFILE_NAME);
        assert!(profile_mapping(&expected, &expected)); assert!(profile_mapping(&expected.join("AC"), &expected));
        assert!(!profile_mapping(&expected.join("AC/Temp"), &expected)); assert!(!profile_mapping(Path::new(r"D:\foreign\AC"), &expected));
    }
    #[test] fn host_profile_creation_is_unconditionally_disabled_in_tests() {
        assert_eq!(setup_after_native_confirmation(Path::new(r"C:\not-a-runtime")).err().unwrap().to_string(), "host profile creation forbidden in tests");
    }
    #[test] fn check_diagnostics_keep_first_stage_and_do_not_serialize_private_details() {
        let inner:io::Result<()> = at("profile-ac",Err(refused("sandbox exposed to unexpected principal")));
        let wrapped = at("setup",inner).unwrap_err();
        let value=serde_json::to_value(check_error(wrapped,false)).unwrap();
        assert_eq!(value["code"],"writer_profile_rejected");assert_eq!(value["sandboxStage"],"profile-ac");
        assert_eq!(value["sandboxReason"],"principal");assert!(value["windowsError"].is_null());
        let value=serde_json::to_value(check_error(at::<()>("runtime-acl",Err(io::Error::from_raw_os_error(5))).unwrap_err(),true)).unwrap();
        assert_eq!(value["sandboxStage"],"runtime-acl");assert_eq!(value["sandboxReason"],"access-denied");assert_eq!(value["windowsError"],5);
        let value=serde_json::to_value(check_error(at::<()>("profile-create",Err(io::Error::from_raw_os_error(0x80070005u32 as i32))).unwrap_err(),true)).unwrap();
        assert_eq!(value["sandboxReason"],"access-denied");assert_eq!(value["windowsError"],0x80070005u32);
        let private="FAKE-PRIVATE C:\\synthetic-private\\profile S-1-5-21-0-0-0-9999";
        let value=serde_json::to_value(check_error(at::<()>("marker",Err(refused(private))).unwrap_err(),false)).unwrap();
        assert_eq!(value["sandboxReason"],"unknown");assert!(!value.to_string().contains(private));assert!(!value.to_string().contains("S-1-5"));
        let cancelled=serde_json::to_value(CheckError::from(crate::error::AppError::cancelled())).unwrap();
        assert_eq!(cancelled["code"],"cancelled");assert!(cancelled.get("sandboxStage").is_none());
    }
    #[test] fn rejected_profile_reasons_remain_distinct_without_changing_acl_policy() {
        for (message,reason) in [("foreign sandbox owner","owner"),("sandbox owner/DACL required","dacl-missing"),
            ("unexpected sandbox ACL entry","ace-type"),("invalid sandbox ACL SID","ace-sid"),
            ("incomplete sandbox DACL","incomplete-dacl"),("sandbox system profile mapped outside application path","path-mapping"),
            ("existing unmarked sandbox profile refused; preserved","unmarked-profile"),
            ("sandbox runtime grants package write access","package-write"),("sandbox ownership marker differs; preserved","marker-identity")] {
            assert_eq!(diagnostic("profile-ac",&refused(message)).sandbox_reason,reason);
        }
    }
    #[test] fn checked_child_directory_stays_pinned_during_recursive_action() {
        let root = crate::save_core::project().join("tests/work/writer").join(format!("sandbox-pins-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap(); let child = root.join("child"); fs::create_dir(&child).unwrap();
        let nested = child.join("nested"); fs::create_dir(&nested).unwrap(); fs::write(nested.join("known.txt"), b"synthetic").unwrap();
        let moved = root.join("moved"); let mut attempted = false;
        visit_runtime(&root, &mut |path, _| {
            if path == nested {
                attempted = true;
                // A junction substitution requires first replacing this name.
                // The same checked parent handle must still deny that replacement.
                assert!(fs::rename(&child, &moved).is_err());
                assert_eq!(fs::read(nested.join("known.txt")).unwrap(), b"synthetic");
            } Ok(())
        }).unwrap(); assert!(attempted);
        // The test also proves that the denial came from the guard's lifetime,
        // rather than ordinary directory permissions or an unusable fixture.
        fs::rename(&child, &moved).unwrap();
        assert_eq!(fs::read(moved.join("nested/known.txt")).unwrap(), b"synthetic");
    }
}
