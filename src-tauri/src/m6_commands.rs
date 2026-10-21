use crate::{Backend,local_owner,data_task,data,error::Result,preferences::{EditStyle,StyleBundle,InputSettings,ContextSettings}};
use tauri::{WebviewWindow,State};
use serde::{Deserialize,Serialize};
use base64::Engine;
#[derive(Clone,Deserialize)]#[serde(rename_all="camelCase",deny_unknown_fields)]pub(crate) struct Region{pub page:u32,pub rect:[f64;4]}
#[derive(Deserialize)]#[serde(rename_all="camelCase",deny_unknown_fields)]pub(crate) struct TemporaryInput{pub binding_id:String,pub page:u32,pub rect:[f64;4],pub source:Option<Region>,pub prompt:String,pub png:Option<String>,pub max_chars:u32}
#[derive(Serialize)]#[serde(rename_all="camelCase")]pub(crate) struct TemporaryAnswer{pub text:String,pub preset_version:Option<String>}
fn valid_rect(r:[f64;4])->bool{r.iter().all(|v|v.is_finite()&&v.abs()<1e7)&&r[0]<r[2]&&r[1]<r[3]}
pub(crate) async fn temporary(files:&crate::files::Files,data:&std::sync::Mutex<crate::data::DataService>,owner:&str,input:TemporaryInput)->Result<TemporaryAnswer>{
 if input.prompt.trim().is_empty()||input.prompt.len()>6000||!valid_rect(input.rect)||!(40..=280).contains(&input.max_chars)||input.source.as_ref().is_some_and(|s|!valid_rect(s.rect)){return Err(crate::ai_network::failure("invalid_request"));}
 let generation=data.lock().map_err(|_|crate::error::AppError::io())?.manager.as_ref().ok_or_else(crate::error::AppError::io)?.info().generation;
 let style=crate::data::with_generation(files,data,owner,&input.binding_id,generation,|s,b|{if s.ai_config()?.main.provider!="mock"{return Err(crate::storage::DataError::Refused("Mock required"));}let count=s.current_revision(&b.document)?.1;if input.page==0||input.page>count||input.source.as_ref().is_some_and(|r|r.page==0||r.page>count){return Err(crate::storage::DataError::Refused("temporary pages"));}Ok(s.styles()?.current())})?;
 match(&input.source,&input.png){(Some(_),Some(png))=>{if png.len()>3*1024*1024{return Err(crate::ai_network::failure("budget"));}let bytes=base64::engine::general_purpose::STANDARD.decode(png.strip_prefix("data:image/png;base64,").ok_or_else(||crate::ai_network::failure("invalid_image"))?).map_err(|_|crate::ai_network::failure("invalid_image"))?;crate::ai_image::validate(&bytes)?;},(None,None)=>{},_=>return Err(crate::ai_network::failure("invalid_image"))}
 let text=if input.source.is_some(){"Mock：所选公式须保留条件；此处仅演示解释流程。"}else{"Mock：这是临时示例，确认后成为批注。"};
 if text.chars().count()>input.max_chars as usize{return Err(crate::error::AppError{code:"box_small",message:"回答框太小，请扩大框体；不会缩小字体或删掉必要条件。"});}
 tokio::time::sleep(std::time::Duration::from_millis(150)).await;
 crate::data::with_generation(files,data,owner,&input.binding_id,generation,|_,_|Ok(()))?;
 Ok(TemporaryAnswer{text:text.into(),preset_version:style.map(|s|s.tag())})
}
#[tauri::command]pub(crate) async fn m6_mock_explain(window:WebviewWindow,state:State<'_,Backend>,input:TemporaryInput)->Result<TemporaryAnswer>{let owner=local_owner(&window)?;temporary(&state.files,&state.data,&owner,input).await}
#[tauri::command]pub(crate) async fn m6_preferences(window:WebviewWindow,state:State<'_,Backend>)->Result<serde_json::Value>{local_owner(&window)?;data_task(window,state,|_,data,_|{let service=data.lock().map_err(|_|crate::error::AppError::io())?;let store=service.manager.as_ref().ok_or_else(crate::error::AppError::io)?.store();Ok(serde_json::json!({"styles":store.styles().map_err(data::error)?,"inputs":store.inputs().map_err(data::error)?,"context":store.context().map_err(data::error)?}))}).await}
#[tauri::command]pub(crate) async fn m6_edit_style(window:WebviewWindow,state:State<'_,Backend>,input:EditStyle)->Result<crate::preferences::Styles>{data_task(window,state,move|_,data,_|{let mut service=data.lock().map_err(|_|crate::error::AppError::io())?;service.manager.as_mut().ok_or_else(crate::error::AppError::io)?.store_mut().edit_style(input).map_err(data::error)}).await}
#[tauri::command]pub(crate) async fn m6_select_style(window:WebviewWindow,state:State<'_,Backend>,id:Option<String>)->Result<crate::preferences::Styles>{data_task(window,state,move|_,data,_|{let mut service=data.lock().map_err(|_|crate::error::AppError::io())?;service.manager.as_mut().ok_or_else(crate::error::AppError::io)?.store_mut().select_style(id).map_err(data::error)}).await}
#[tauri::command]pub(crate) async fn m6_delete_style_version(window:WebviewWindow,state:State<'_,Backend>,id:String,version:u32)->Result<crate::preferences::Styles>{data_task(window,state,move|_,data,_|{let mut service=data.lock().map_err(|_|crate::error::AppError::io())?;service.manager.as_mut().ok_or_else(crate::error::AppError::io)?.store_mut().delete_style_version(&id,version).map_err(data::error)}).await}
fn import_error()->crate::error::AppError{crate::error::AppError{code:"style_import_refused",message:"未能导入风格：请选择不超过 1 MB 的本地 JSON，内容需符合风格格式且不能是链接。已有风格和历史未改变。"}}
fn read_style_bundle(path:&std::path::Path)->Result<StyleBundle>{
 use std::{fs::OpenOptions,io::Read,os::windows::{fs::OpenOptionsExt,io::AsRawHandle},path::{Component,Prefix}};
 let local=matches!(path.components().next(),Some(Component::Prefix(prefix)) if matches!(prefix.kind(),Prefix::Disk(_)|Prefix::VerbatimDisk(_)));
 if !local||path.components().any(|part|matches!(part,Component::ParentDir)||matches!(part,Component::Normal(name) if name.to_string_lossy().contains(':')))||path.to_string_lossy().chars().any(|c|c=='\0')||!path.extension().is_some_and(|ext|ext.eq_ignore_ascii_case("json")){return Err(import_error());}
 let mut pins=vec![];crate::unicode_worker::pin_chain(path.parent().ok_or_else(import_error)?,&mut pins).map_err(|_|import_error())?;
 let mut file=OpenOptions::new().read(true).share_mode(1).custom_flags(0x00200000).open(path).map_err(|_|import_error())?;
 let mut info=windows_sys::Win32::Storage::FileSystem::BY_HANDLE_FILE_INFORMATION::default();
 if unsafe{windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandle(file.as_raw_handle(),&mut info)}==0||info.dwFileAttributes&(0x400|0x10)!=0||info.nNumberOfLinks!=1||file.metadata().map_err(|_|import_error())?.len()>1024*1024{return Err(import_error());}
 let mut bytes=Vec::new();file.by_ref().take(1024*1024+1).read_to_end(&mut bytes).map_err(|_|import_error())?;
 if bytes.len()>1024*1024{return Err(import_error());}let bundle:StyleBundle=serde_json::from_slice(&bytes).map_err(|_|import_error())?;bundle.validate().map_err(|_|import_error())?;Ok(bundle)
}
#[tauri::command]pub(crate) async fn m6_import_styles(window:WebviewWindow,state:State<'_,Backend>)->Result<Option<crate::preferences::Styles>>{
 local_owner(&window)?;let generation=state.data.lock().map_err(|_|crate::error::AppError::io())?.manager.as_ref().ok_or_else(crate::error::AppError::io)?.info().generation;
 // Only a native, user-initiated picker grants this one JSON read. The renderer
 // cannot send an arbitrary path, source SKILL or executable import instruction.
 let Some(file)=rfd::AsyncFileDialog::new().set_parent(&window).set_title("导入私有表达风格").add_filter("表达风格 JSON",&["json"]).pick_file().await else{return Ok(None);};
 let path=file.path().to_owned();let bundle=tauri::async_runtime::spawn_blocking(move||read_style_bundle(&path)).await.map_err(|_|import_error())??;
 data_task(window,state,move|_,data,_|{let mut service=data.lock().map_err(|_|crate::error::AppError::io())?;let manager=service.manager.as_mut().ok_or_else(crate::error::AppError::io)?;if manager.info().generation!=generation{return Err(crate::error::AppError::cancelled());}manager.store_mut().import_styles(bundle).map(Some).map_err(data::error)}).await
}
#[tauri::command]pub(crate) async fn m6_set_inputs(window:WebviewWindow,state:State<'_,Backend>,inputs:InputSettings)->Result<()>{data_task(window,state,move|_,data,_|{let mut service=data.lock().map_err(|_|crate::error::AppError::io())?;service.manager.as_mut().ok_or_else(crate::error::AppError::io)?.store_mut().set_inputs(&inputs).map_err(data::error)}).await}
#[tauri::command]pub(crate) async fn m6_set_context(window:WebviewWindow,state:State<'_,Backend>,context:ContextSettings)->Result<()>{data_task(window,state,move|_,data,_|{let mut service=data.lock().map_err(|_|crate::error::AppError::io())?;service.manager.as_mut().ok_or_else(crate::error::AppError::io)?.store_mut().set_context(&context).map_err(data::error)}).await}

#[cfg(test)]mod style_import_tests{
 use super::*;
 #[test]fn m6_style_import_reader_rejects_links_oversize_and_nondata(){
  let root=crate::save_core::project().join("tests/work/m4").join(uuid::Uuid::new_v4().to_string());std::fs::create_dir_all(&root).unwrap();
  let path=root.join("合成 风格.json");let id=uuid::Uuid::new_v4().to_string();let raw=serde_json::json!({"schemaVersion":1,"styles":[{"id":id,"version":1,"name":"合成表达","aliases":["合成别称"],"guidelines":"简明准确","avoid":"不添故事","enabled":true}]});
  std::fs::write(&path,raw.to_string()).unwrap();assert_eq!(read_style_bundle(&path).unwrap().styles.len(),1);
  let linked=root.join("hardlink.json");std::fs::hard_link(&path,&linked).unwrap();assert!(read_style_bundle(&linked).is_err());assert!(read_style_bundle(&path).is_err());std::fs::remove_file(&linked).unwrap();
  std::fs::write(&path,vec![b' ';1024*1024+1]).unwrap();assert!(read_style_bundle(&path).is_err());
  std::fs::write(&path,r#"{"schemaVersion":1,"styles":[],"command":"ignored executable instruction"}"#).unwrap();assert!(read_style_bundle(&path).is_err());
  for invalid in [r"\\server\share\style.json",r"\\?\GLOBALROOT\Device\style.json",r"D:\unsafe\..\style.json",r"D:\style.json:stream",r"D:\source:stream.json"]{assert!(read_style_bundle(std::path::Path::new(invalid)).is_err());}
 }
 // Personal content is never a tracked fixture. This explicit opt-in test
 // reads only a user-authorized, ignored project-local import bundle and opens
 // a new synthetic database; it cannot reach or update the real application DB.
 #[test]#[ignore]fn m6_authorized_private_bundle_synthetic_landing(){
  let path=std::path::PathBuf::from(std::env::var_os("M6_PRIVATE_TEST_BUNDLE").expect("explicit ignored bundle required"));let base=crate::save_core::project().join(".cache/private-presets").canonicalize().unwrap();let actual=path.canonicalize().unwrap();assert!(actual.starts_with(&base));
  let bundle=read_style_bundle(&path).unwrap();let count=bundle.styles.len();assert!(count>=2);let root=crate::save_core::project().join("tests/work/m4").join(uuid::Uuid::new_v4().to_string());std::fs::create_dir_all(&root).unwrap();
  let mut store=crate::storage::Store::open_synthetic(&root,16384).unwrap();let styles=store.import_styles(bundle).unwrap();assert_eq!(styles.revisions.len(),count);assert!(styles.current().is_none());
  let identity=styles.revisions[0].id.clone();assert!(styles.revisions.iter().any(|style|style.aliases.len()>1));store.select_style(Some(identity.clone())).unwrap();drop(store);
  let mut store=crate::storage::Store::open_synthetic(&root,16384).unwrap();assert_eq!(store.styles().unwrap().current().unwrap().id,identity);assert_eq!(store.styles().unwrap().recent[0],identity);assert_eq!(store.styles().unwrap().revisions.len(),count);
  store.select_style(None).unwrap();assert_eq!(store.import_styles(read_style_bundle(&path).unwrap()).unwrap().revisions.len(),count);assert!(store.styles().unwrap().current().is_none());
  println!("authorized private bundle imported, restarted and duplicate import retained: {count} entries; default none");
 }
}
