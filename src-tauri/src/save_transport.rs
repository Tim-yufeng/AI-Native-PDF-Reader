//! Same save command and authority, raw bytes instead of duplicated JSON arrays.
use crate::error::{AppError,Result};
use tauri::{http::HeaderMap,ipc::InvokeBody};
pub(crate) struct Payload{pub task_id:String,pub binding_id:Option<String>,pub candidate_len:usize,pub output:Vec<u8>}
fn invalid()->AppError{AppError::denied()}
fn id(value:&str)->Result<String>{
    if value.len()!=36||uuid::Uuid::parse_str(value).ok().is_none_or(|v|v.to_string()!=value){return Err(invalid());}Ok(value.into())
}
fn field<'a>(headers:&'a HeaderMap,name:&str)->Result<&'a str>{headers.get(name).and_then(|v|v.to_str().ok()).ok_or_else(invalid)}
fn bounds(candidate:usize,output:usize)->Result<()>{
    if candidate==0||candidate>=output{return Err(invalid());}
    if candidate>crate::files::EXPORT_INPUT_MAX||output>crate::files::EXPORT_OUTPUT_MAX{return Err(AppError{code:"writer_pdf_budget",message:"批注副本超过当前写入体积上限；原 PDF 未修改。"});}Ok(())
}
pub(crate) fn decode(body:&InvokeBody,headers:&HeaderMap)->Result<Payload>{
    match body{
        InvokeBody::Raw(output)=>{
            if field(headers,"x-ai-save-version")?!="1"{return Err(invalid());}
            let task_id=id(field(headers,"x-ai-save-task")?)?;
            let binding_id=headers.get("x-ai-save-binding").map(|v|v.to_str().map_err(|_|invalid()).and_then(id)).transpose()?;
            let length=field(headers,"x-ai-save-candidate-bytes")?;
            if length.is_empty()||length.len()>8||!length.bytes().all(|b|b.is_ascii_digit())||length.starts_with('0'){return Err(invalid());}
            let candidate_len=length.parse().map_err(|_|invalid())?;bounds(candidate_len,output.len())?;
            Ok(Payload{task_id,binding_id,candidate_len,output:output.clone()})
        }
        // Only the isolated lab can replay the original JSON route for A/B.
        // This branch is absent from ordinary release and development builds.
        #[cfg(feature="save-chain-lab")]
        InvokeBody::Json(value)=>{
            #[derive(serde::Deserialize)]#[serde(rename_all="camelCase",deny_unknown_fields)]
            struct Legacy{task_id:String,candidate:Vec<u8>,output:Vec<u8>,binding_id:Option<String>}
            let old=Legacy::deserialize(value).map_err(|_|invalid())?;
            bounds(old.candidate.len(),old.output.len())?;
            if !old.output.starts_with(&old.candidate){return Err(invalid());}
            Ok(Payload{task_id:id(&old.task_id)?,binding_id:old.binding_id.as_deref().map(id).transpose()?,candidate_len:old.candidate.len(),output:old.output})
        }
        #[cfg(not(feature="save-chain-lab"))]
        _=>Err(invalid())
    }
}
#[cfg(feature="save-chain-lab")]use serde::Deserialize;

#[cfg(test)]mod tests{
    use super::*;
    fn headers()->HeaderMap{let mut h=HeaderMap::new();for(k,v)in [("x-ai-save-version","1"),("x-ai-save-task","00000000-0000-0000-0000-000000000001"),("x-ai-save-candidate-bytes","2")]{h.insert(k,v.parse().unwrap());}h}
    #[test]fn save_transport_raw_exact_prefix_and_optional_binding(){
        let mut h=headers();let bytes=vec![37,80,68,70];let p=decode(&InvokeBody::Raw(bytes.clone()),&h).unwrap();assert_eq!(p.output,bytes);assert_eq!(&p.output[..p.candidate_len],&bytes[..2]);assert!(p.binding_id.is_none());
        h.insert("x-ai-save-binding","00000000-0000-0000-0000-000000000002".parse().unwrap());assert!(decode(&InvokeBody::Raw(bytes),&h).unwrap().binding_id.is_some());
    }
    #[test]fn save_transport_refuses_malformed_headers_and_bounds_before_copy(){
        for(key,values)in [("x-ai-save-version",vec!["","2"]),("x-ai-save-task",vec!["","fake","../lesson.pdf"]),("x-ai-save-binding",vec!["","fake"]),("x-ai-save-candidate-bytes",vec!["","0","02","-1","2.0","999999999999999999999","4","5"])]{
            for value in values{let mut h=headers();h.insert(key,value.parse().unwrap());assert!(decode(&InvokeBody::Raw(vec![1,2,3,4]),&h).is_err(),"{key}={value}");}
        }
        for key in ["x-ai-save-version","x-ai-save-task","x-ai-save-candidate-bytes"]{let mut h=headers();h.remove(key);assert!(decode(&InvokeBody::Raw(vec![1,2,3,4]),&h).is_err());}
        assert!(bounds(crate::files::EXPORT_INPUT_MAX,crate::files::EXPORT_OUTPUT_MAX).is_ok());
        assert_eq!(bounds(crate::files::EXPORT_INPUT_MAX+1,crate::files::EXPORT_OUTPUT_MAX).unwrap_err().code,"writer_pdf_budget");
        assert_eq!(bounds(1,crate::files::EXPORT_OUTPUT_MAX+1).unwrap_err().code,"writer_pdf_budget");
        #[cfg(not(feature="save-chain-lab"))]assert!(decode(&InvokeBody::Json(serde_json::json!({})),&headers()).is_err());
    }
}
