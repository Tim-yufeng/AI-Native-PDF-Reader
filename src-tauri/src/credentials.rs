//! No renderer read API. Targets are bound to a validated endpoint, never arbitrary names.
use crate::error::{AppError,Result};
use windows_sys::Win32::Security::Credentials::*;
pub(crate) struct Secret(pub Vec<u8>);
impl Drop for Secret {fn drop(&mut self){for b in &mut self.0{unsafe{std::ptr::write_volatile(b,0);}}}}
fn target(endpoint:&str)->Vec<u16>{format!("AI-Native-PDF-Reader/api/{}",crate::save_core::hash(endpoint.as_bytes())).encode_utf16().chain(Some(0)).collect()}
pub(crate) fn read(endpoint:&str)->Result<Secret>{
    crate::ai_policy::require_real()?;
    let name=target(endpoint);let mut raw=std::ptr::null_mut();
    if unsafe{CredReadW(name.as_ptr(),CRED_TYPE_GENERIC,0,&mut raw)}==0{return Err(AppError{code:"key_missing",message:"尚未配置安全凭据，请在模型设置中保存 API Key。"});}
    if raw.is_null(){return Err(AppError::io());}
    if unsafe{(*raw).CredentialBlob.is_null()||(*raw).CredentialBlobSize==0||(*raw).CredentialBlobSize>2048}{unsafe{CredFree(raw.cast());}return Err(AppError::io());}
    let value=unsafe{std::slice::from_raw_parts((*raw).CredentialBlob,(*raw).CredentialBlobSize as usize).to_vec()};
    unsafe{CredFree(raw.cast());}Ok(Secret(value))
}
pub(crate) fn present(endpoint:&str)->bool{read(endpoint).is_ok()}
/// Removes this endpoint's stored secret. Returns whether one existed, so the
/// settings surface can report an honest result without reading the value.
pub(crate) fn delete(endpoint:&str)->Result<bool>{
    crate::ai_policy::require_real()?;
    let existed=present(endpoint);
    if unsafe{CredDeleteW(target(endpoint).as_ptr(),CRED_TYPE_GENERIC,0)}==0&&existed{return Err(AppError{code:"credential_unavailable",message:"Windows 安全凭据未能删除，请重试。"});}
    Ok(existed)
}
pub(crate) fn put(endpoint:&str,mut value:Secret)->Result<()>{
    crate::ai_policy::require_real()?;
    if value.0.is_empty()||value.0.len()>2048||!value.0.iter().all(|b|(33..=126).contains(b)){return Err(AppError{code:"key_invalid",message:"密钥格式不正确。"});}
    let mut name=target(endpoint);let mut record=CREDENTIALW::default();record.Type=CRED_TYPE_GENERIC;record.TargetName=name.as_mut_ptr();
    record.CredentialBlobSize=value.0.len() as u32;record.CredentialBlob=value.0.as_mut_ptr();record.Persist=CRED_PERSIST_LOCAL_MACHINE;
    if unsafe{CredWriteW(&record,0)}==0{return Err(AppError{code:"credential_unavailable",message:"Windows 安全凭据未能保存，请重试。"});}Ok(())
}
