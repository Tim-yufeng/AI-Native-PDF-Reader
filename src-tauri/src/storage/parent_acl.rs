//! Conservative traditional-principal checks for a data root's parent.
//! Package/capability SIDs are AppContainer's second access-check pass, not
//! ordinary account membership. The traditional pass must independently deny
//! every foreign dangerous grant. Root/leaf DACL checks remain stricter.
use super::*;
use super::directories::{current_sid,sid_text};
use windows_sys::Win32::{Foundation::{LocalFree,LUID},Security::*,Security::Authorization::*};
struct Descriptor(PSECURITY_DESCRIPTOR);
impl Drop for Descriptor{fn drop(&mut self){unsafe{LocalFree(self.0.cast());}}}
struct Resource(AUTHZ_RESOURCE_MANAGER_HANDLE);
impl Drop for Resource{fn drop(&mut self){unsafe{AuthzFreeResourceManager(self.0);}}}
struct Context(AUTHZ_CLIENT_CONTEXT_HANDLE);
impl Drop for Context{fn drop(&mut self){unsafe{AuthzFreeContext(self.0);}}}
const DANGER:u32=FILE_DELETE_CHILD|WRITE_DAC|WRITE_OWNER;
const ACCESS_ALLOWED_ACE_TYPE:u8=0;const ACCESS_DENIED_ACE_TYPE:u8=1;const MAXIMUM_ALLOWED:u32=0x02000000;
fn trusted(sid:&str,me:&str)->bool{sid==me||sid=="S-1-5-18"||sid=="S-1-5-32-544"}
fn container(sid:&str)->bool{sid.starts_with("S-1-15-2-")||sid.starts_with("S-1-15-3-")}
pub(super) fn check(file:&File)->Result<()>{
 let(mut owner,mut acl,mut sd)=(std::ptr::null_mut(),std::ptr::null_mut(),std::ptr::null_mut());
 let rc=unsafe{GetSecurityInfo(file.as_raw_handle(),SE_FILE_OBJECT,OWNER_SECURITY_INFORMATION|DACL_SECURITY_INFORMATION,&mut owner,std::ptr::null_mut(),&mut acl,std::ptr::null_mut(),&mut sd)};
 if rc!=0{return Err(io::Error::from_raw_os_error(rc as i32).into());}
 let _descriptor=Descriptor(sd);
 if owner.is_null()||acl.is_null()||unsafe{IsValidAcl(acl)}==0{return deny("data parent owner/DACL required");}
 let me=current_sid()?;if !trusted(&sid_text(owner)?,&me){return deny("data parent owned by another ordinary user");}
 let map=GENERIC_MAPPING{GenericRead:FILE_GENERIC_READ,GenericWrite:FILE_GENERIC_WRITE,GenericExecute:FILE_GENERIC_EXECUTE,GenericAll:FILE_ALL_ACCESS};
 let mut candidates=std::collections::BTreeMap::new();
 for index in 0..unsafe{(*acl).AceCount} as u32{
  let mut ace=std::ptr::null_mut();if unsafe{GetAce(acl,index,&mut ace)}==0{return Err(io::Error::last_os_error().into());}
  let header=unsafe{*ace.cast::<ACE_HEADER>()};
  if header.AceType!=ACCESS_ALLOWED_ACE_TYPE as u8&&header.AceType!=ACCESS_DENIED_ACE_TYPE as u8{return deny("unsupported parent ACL entry");}
  // Both basic ACE types share Mask/SidStart layout. Only this allocated copy
  // is mapped; no filesystem security descriptor is written or reordered.
  let entry=unsafe{&mut *ace.cast::<ACCESS_ALLOWED_ACE>()};unsafe{MapGenericMask(&mut entry.Mask,&map);}
  if header.AceFlags&INHERIT_ONLY_ACE as u8!=0||header.AceType!=ACCESS_ALLOWED_ACE_TYPE as u8{continue;}
  let sid=(&entry.SidStart as *const u32).cast_mut().cast();if unsafe{IsValidSid(sid)}==0{return deny("invalid parent SID");}let text=sid_text(sid)?;
  if !trusted(&text,&me)&&!container(&text)&&entry.Mask&DANGER!=0{candidates.insert(text,sid);}
 }
 if candidates.len()>128{return deny("complex parent ACL; fail closed");}
 if candidates.is_empty(){return Ok(());}
 let mut rm=std::ptr::null_mut();if unsafe{AuthzInitializeResourceManager(AUTHZ_RM_FLAG_NO_AUDIT,None,None,None,std::ptr::null(),&mut rm)}==0{return Err(io::Error::last_os_error().into());}let _resource=Resource(rm);
 for sid in candidates.values(){
  let mut context=std::ptr::null_mut();if unsafe{AuthzInitializeContextFromSid(AUTHZ_SKIP_TOKEN_GROUPS,*sid,rm,std::ptr::null(),LUID::default(),std::ptr::null(),&mut context)}==0{return Err(io::Error::last_os_error().into());}let _context=Context(context);
  // No account/group lookup, impersonation or credential access. Omitting
  // other groups' denials can only over-reject; every dangerous allow SID is
  // independently probed, so missing group allows cannot produce a safe result.
  let request=AUTHZ_ACCESS_REQUEST{DesiredAccess:MAXIMUM_ALLOWED,..Default::default()};let(mut granted,mut error,mut sacl)=(0,0,0);
  let mut reply=AUTHZ_ACCESS_REPLY{ResultListLength:1,GrantedAccessMask:&mut granted,Error:&mut error,SaclEvaluationResults:&mut sacl};
  if unsafe{AuthzAccessCheck(0,context,&request,std::ptr::null_mut(),sd,std::ptr::null(),0,&mut reply,std::ptr::null_mut())}==0{return Err(io::Error::last_os_error().into());}
  if ![0,5].contains(&error){return deny("parent authorization check failed");}
  if granted&DANGER!=0{return deny("data parent writable by other users; choose a private folder");}
 }Ok(())
}
#[cfg(test)]mod tests{
 use super::*;use std::os::windows::ffi::OsStrExt;
 fn set(path:&Path,extra:&str){let me=current_sid().unwrap();let text=format!("D:P(A;;FA;;;{me})(A;;FA;;;SY)(A;;FA;;;BA){extra}");let wide:Vec<u16>=text.encode_utf16().chain(Some(0)).collect();let mut sd=std::ptr::null_mut();assert_ne!(unsafe{ConvertStringSecurityDescriptorToSecurityDescriptorW(wide.as_ptr(),1,&mut sd,std::ptr::null_mut())},0);let _sd=Descriptor(sd);let p:Vec<u16>=path.as_os_str().encode_wide().chain(Some(0)).collect();assert_ne!(unsafe{SetFileSecurityW(p.as_ptr(),DACL_SECURITY_INFORMATION|PROTECTED_DACL_SECURITY_INFORMATION,sd)},0);}
 #[test]fn parent_authz_distinguishes_container_permissions_and_dangerous_accounts(){let base=crate::save_core::project().join("tests/work/m4").join(format!("acl-{}",uuid()));fs::create_dir(&base).unwrap();let cases=[
  ("(A;;FA;;;S-1-15-2-1)(A;;GA;;;S-1-15-3-1)",true),
  ("(A;;FA;;;S-1-15-2-1)(A;;0x00000040;;;AU)",false),
  ("(A;;0x00040000;;;AU)",false),("(A;;0x00080000;;;WD)",false),
  ("(A;;GA;;;AU)",false),("(A;;GW;;;AU)",true),
  ("(D;;0x00000040;;;AU)(A;;0x00000040;;;AU)",true),
  ("(A;;0x00000040;;;AU)(D;;0x00000040;;;AU)",false),
  ("(A;;FA;;;S-1-5-21-0-0-0-9999)",false),
  ("(D;;0x00000040;;;S-1-5-21-0-0-0-9999)(A;;0x00000040;;;S-1-5-21-0-0-0-9999)",true),
  ("(A;;0x00000040;;;BU)",false),
  ("(A;IO;FA;;;AU)",true),
  ("(OA;;FA;;;AU)",false)];
  for(index,(extra,safe))in cases.iter().enumerate(){let path=base.join(index.to_string());fs::create_dir(&path).unwrap();set(&path,extra);let f=OpenOptions::new().read(true).custom_flags(FILE_FLAG_BACKUP_SEMANTICS).open(path).unwrap();assert_eq!(check(&f).is_ok(),*safe,"case {index}");}
 }
}
