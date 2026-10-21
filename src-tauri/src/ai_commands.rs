use crate::{Backend,local_owner,data_task,data,error::{AppError,Result},ai,ai_config::Config,ai_network as net};
use tauri::{State,WebviewWindow,ipc::Channel};
#[tauri::command]pub(crate) async fn m5_chat_view(window:WebviewWindow,state:State<'_,Backend>,binding_id:String)->Result<crate::storage::chat_view::ChatView>{data_task(window,state,move|files,data,owner|data::with_binding(files,data,owner,&binding_id,|s,b|s.chat_view(&b.conversation))).await}
#[tauri::command]pub(crate) async fn m5_chat_select(window:WebviewWindow,state:State<'_,Backend>,binding_id:String,message_id:Option<String>)->Result<crate::storage::chat_view::ChatView>{let ai=state.ai.clone();data_task(window,state,move|files,data,owner|{let generation=data.lock().map_err(|_|AppError::io())?.manager.as_ref().ok_or_else(AppError::io)?.info().generation;data::with_generation(files,data,owner,&binding_id,generation,|s,b|s.validate_chat_select(&b.conversation,message_id.as_deref()))?;ai::change_document_checked(files,data,&ai,owner,&binding_id,Some(generation),false,|s,b|s.validate_chat_select(&b.conversation,message_id.as_deref()),|s,b|s.chat_select(&b.conversation,message_id.as_deref()))}).await}
#[tauri::command]pub(crate) async fn m5_chat_archive(window:WebviewWindow,state:State<'_,Backend>,binding_id:String,session_id:String,archived:bool)->Result<crate::storage::chat_view::ChatView>{let ai=state.ai.clone();data_task(window,state,move|files,data,owner|{let generation=data.lock().map_err(|_|AppError::io())?.manager.as_ref().ok_or_else(AppError::io)?.info().generation;data::with_generation(files,data,owner,&binding_id,generation,|s,b|s.validate_chat_session(&b.conversation,&session_id))?;ai::change_document_checked(files,data,&ai,owner,&binding_id,Some(generation),false,|s,b|s.validate_chat_session(&b.conversation,&session_id),|s,b|s.chat_archive(&b.conversation,&session_id,archived))}).await}
#[tauri::command]pub(crate) async fn m5_chat_delete(window:WebviewWindow,state:State<'_,Backend>,binding_id:String,session_id:String)->Result<bool>{
 local_owner(&window)?;let generation=state.data.lock().map_err(|_|AppError::io())?.manager.as_ref().ok_or_else(AppError::io)?.info().generation;
 let check=binding_id.clone();let target=session_id.clone();let(document,title)=data_task(window.clone(),state.clone(),move|files,data,owner|data::with_generation(files,data,owner,&check,generation,|s,b|{let view=s.chat_view(&b.conversation)?;let session=view.sessions.iter().find(|session|session.id==target).ok_or(crate::storage::DataError::Refused("chat session target"))?;Ok((b.document.clone(),session.title.clone()))})).await?;
 let accepted=rfd::AsyncMessageDialog::new().set_parent(&window).set_title("删除指定会话及全部版本")
  .set_description(format!("将删除这一个会话的全部问题、回复、版本、后续分支与来源记录。\n\n会话：{title}\n会话 UUID：{session_id}\n文档 UUID：{document}\n\n其他会话、文档、PDF、批注草稿与阅读位置保留。旧备份和旧数据目录仍可能含有历史，不执行安全擦除。确认删除？"))
  .set_buttons(rfd::MessageButtons::YesNo).show().await;if accepted!=rfd::MessageDialogResult::Yes{return Ok(false);}
 let ai=state.ai.clone();data_task(window,state,move|files,data,owner|ai::change_document_checked(files,data,&ai,owner,&binding_id,Some(generation),false,|s,b|{if b.document!=document{return Err(crate::storage::DataError::Refused("chat document changed"));}s.validate_chat_session(&b.conversation,&session_id)},|s,b|{s.chat_delete(&b.conversation,&session_id)?;Ok(true)})).await
}
#[tauri::command]pub(crate) async fn m5_config(window:WebviewWindow,state:State<'_,Backend>)->Result<ai::Configuration>{local_owner(&window)?;ai::config(&state.data)}
#[tauri::command]pub(crate) async fn m5_set_config(window:WebviewWindow,state:State<'_,Backend>,config:Config)->Result<()>{
 config.validate()?;local_owner(&window)?;if ai::config(&state.data)?.config.main!=config.main{state.ai.lock().map_err(|_|AppError::io())?.cancel("main",None);}
 data_task(window,state,move|_,data,_|{let mut data=data.lock().map_err(|_|AppError::io())?;data.manager.as_mut().ok_or_else(AppError::io)?.store_mut().ai_set_config(&config).map_err(data::error)}).await
}
#[tauri::command]pub(crate) async fn m5_store_key(window:WebviewWindow,state:State<'_,Backend>,slot:String,endpoint:String,key:String)->Result<()>{
 crate::ai_policy::require_real()?;
 local_owner(&window)?;let secret=crate::credentials::Secret(key.into_bytes());let config=ai::config(&state.data)?.config;
 let model=match slot.as_str(){"main"=>config.main,"auxiliary"=>config.auxiliary.ok_or_else(AppError::denied)?,_=>return Err(AppError::denied())};model.validate()?;
 if model.provider=="mock"||model.endpoint!=endpoint{return Err(AppError::denied());}tauri::async_runtime::spawn_blocking(move||crate::credentials::put(&model.endpoint,secret)).await.map_err(|_|AppError::io())?
}
#[tauri::command]pub(crate) async fn m5_delete_key(window:WebviewWindow,state:State<'_,Backend>,slot:String)->Result<bool>{
 crate::ai_policy::require_real()?;
 local_owner(&window)?;let config=ai::config(&state.data)?.config;
 let model=match slot.as_str(){"main"=>config.main,"auxiliary"=>config.auxiliary.ok_or_else(AppError::denied)?,_=>return Err(AppError::denied())};model.validate()?;
 if model.provider=="mock"{return Ok(false);}
 tauri::async_runtime::spawn_blocking(move||crate::credentials::delete(&model.endpoint)).await.map_err(|_|AppError::io())?
}
#[tauri::command]pub(crate) async fn m5_test_connection(window:WebviewWindow,state:State<'_,Backend>,slot:String)->Result<serde_json::Value>{
 local_owner(&window)?;let config=ai::config(&state.data)?.config;let model=match slot.as_str(){"main"=>config.main,"auxiliary"=>config.auxiliary.ok_or_else(AppError::denied)?,_=>return Err(AppError::denied())};
 if model.provider=="mock"{return Ok(serde_json::json!({"connected":true,"vision":true,"mock":true}));}
 let mut url=net::endpoint(&model.endpoint)?;let client=net::client(&url).await?;let secret=crate::credentials::read(&model.endpoint)?;
 let path=url.path().replace("chat/completions","models");url.set_path(&path);
 let mut response=client.get(url).bearer_auth(std::str::from_utf8(&secret.0).map_err(|_|net::failure("key_invalid"))?).send().await.map_err(|_|net::failure("network"))?;
 if !response.status().is_success(){return Err(net::status(response.status().as_u16()));}let mut bytes=Vec::new();while let Some(chunk)=response.chunk().await.map_err(|_|net::failure("network"))?{if bytes.len()+chunk.len()>2*1024*1024{return Err(net::failure("budget"));}bytes.extend_from_slice(&chunk);}
 let value:serde_json::Value=serde_json::from_slice(&bytes).map_err(|_|net::failure("protocol"))?;
 let item=value.get("data").and_then(|v|v.as_array()).and_then(|items|items.iter().find(|m|m.get("id").and_then(|v|v.as_str())==Some(&model.model))).ok_or_else(||net::failure("model_missing"))?;
 let vision=model.provider=="deepseek"&&model.model=="deepseek-flash"||item.pointer("/architecture/input_modalities").and_then(|v|v.as_array()).is_some_and(|v|v.iter().any(|v|v.as_str()==Some("image")));
 Ok(serde_json::json!({"connected":true,"vision":vision,"model":model.model,"note":"模型目录检查通过；不代表真实生成已验收。"}))
}
#[tauri::command]pub(crate) async fn m5_index(window:WebviewWindow,state:State<'_,Backend>,binding_id:String,pages:Vec<crate::ai_context::PageText>)->Result<()>{
 let owner=local_owner(&window)?;let files=state.files.clone();let data=state.data.clone();let ai=state.ai.clone();tauri::async_runtime::spawn_blocking(move||ai::index(&files,&data,&ai,&owner,&binding_id,pages)).await.map_err(|_|AppError::io())?
}
#[tauri::command]pub(crate) async fn m5_prepare(window:WebviewWindow,state:State<'_,Backend>,binding_id:String,input:ai::PrepareInput)->Result<ai::Plan>{
 let owner=local_owner(&window)?;let files=state.files.clone();let data=state.data.clone();let ai=state.ai.clone();tauri::async_runtime::spawn_blocking(move||ai::prepare(&files,&data,&ai,&owner,&binding_id,input)).await.map_err(|_|AppError::io())?
}
#[tauri::command]pub(crate) async fn m5_images(window:WebviewWindow,state:State<'_,Backend>,binding_id:String,prepared_id:String,images:Vec<ai::Image>)->Result<()>{
 let owner=local_owner(&window)?;data::with_binding(&state.files,&state.data,&owner,&binding_id,|_,_|Ok(()))?;ai::images(&state.ai,&binding_id,&prepared_id,images)
}
#[tauri::command]pub(crate) async fn m5_refine(window:WebviewWindow,state:State<'_,Backend>,binding_id:String,prepared_id:String,source_ids:Vec<String>)->Result<ai::Plan>{
 let owner=local_owner(&window)?;data::with_binding(&state.files,&state.data,&owner,&binding_id,|_,_|Ok(()))?;ai::refine(&state.ai,&binding_id,&prepared_id,&source_ids)
}
#[tauri::command]pub(crate) async fn m5_start(window:WebviewWindow,state:State<'_,Backend>,input:ai::Start,on_event:Channel<ai::Event>)->Result<String>{
 let owner=local_owner(&window)?;ai::start(state.files.clone(),state.data.clone(),state.ai.clone(),owner,input,on_event).await
}
#[tauri::command]pub(crate) async fn m5_cancel(window:WebviewWindow,state:State<'_,Backend>,request_id:String)->Result<()>{let owner=local_owner(&window)?;state.ai.lock().map_err(|_|AppError::io())?.cancelled(&owner,&request_id)}
#[tauri::command]pub(crate) async fn m5_details(window:WebviewWindow,state:State<'_,Backend>,binding_id:String,message_id:String)->Result<serde_json::Value>{
 data_task(window,state,move|files,data,owner|data::with_binding(files,data,owner,&binding_id,|s,b|s.ai_details(&b.conversation,&message_id))).await
}
#[tauri::command]pub(crate) async fn m5_directory(window:WebviewWindow,state:State<'_,Backend>,binding_id:String,enabled:bool)->Result<Vec<crate::files::Descriptor>>{
 let owner=local_owner(&window)?;let grant=data::with_binding(&state.files,&state.data,&owner,&binding_id,|_,b|Ok(b.grant.clone()))?;
 ai::stop(&state.files,&state.data,&state.ai,&owner,Some(&binding_id),false)?;
 let (released,scope_version)={let mut service=state.ai.lock().map_err(|_|AppError::io())?;let released=service.revoke_directory(&owner,&binding_id);(released,service.scope_version(&binding_id))};for d in released{let _=state.files.release(&owner,&d.grant_id);}
 if !enabled{return Ok(vec![]);}
 let accepted=rfd::AsyncMessageDialog::new().set_parent(&window).set_title("允许同目录 PDF 上下文")
 .set_description("仅读取当前 PDF 实际目录直接包含的其他 PDF，最多16份，不递归、不读取链接目标。内容先在本机索引，发送前显示实际来源。撤销后停止检索及待发请求；已经发往远程的内容无法撤回。")
 .set_buttons(rfd::MessageButtons::YesNo).show().await;if accepted!=rfd::MessageDialogResult::Yes{return Ok(vec![]);}
 let files=state.files.clone();let data=state.data.clone();let ai=state.ai.clone();tauri::async_runtime::spawn_blocking(move||{
  let _serial=ai::COORDINATOR.lock().map_err(|_|AppError::io())?;data::with_binding(&files,&data,&owner,&binding_id,|_,_|Ok(()))?;
  if ai.lock().map_err(|_|AppError::io())?.scope_version(&binding_id)!=scope_version{return Err(AppError::cancelled());}
  let members=files.directory_members(&owner,&grant)?;if let Err(e)=data::with_binding(&files,&data,&owner,&binding_id,|_,_|Ok(())){for d in members{let _=files.release(&owner,&d.grant_id);}return Err(e);}
  let mut service=ai.lock().map_err(|_|AppError::io())?;if service.scope_version(&binding_id)!=scope_version{drop(service);for d in members{let _=files.release(&owner,&d.grant_id);}return Err(AppError::cancelled());}service.directory_sources.insert(binding_id.clone(),grant);service.directories.insert(binding_id,members.clone());Ok(members)
 }).await.map_err(|_|AppError::io())?
}
#[tauri::command]pub(crate) async fn m5_index_member(window:WebviewWindow,state:State<'_,Backend>,binding_id:String,grant_id:String,count:u32,pages:Vec<crate::ai_context::PageText>)->Result<()>{
 let owner=local_owner(&window)?;if count==0||count>2000{return Err(AppError::denied());}
 let base_grant=data::with_binding(&state.files,&state.data,&owner,&binding_id,|_,b|Ok(b.grant.clone()))?;state.files.directory_live(&owner,&base_grant,&grant_id)?;
 state.files.with_live(&owner,&grant_id,state.files.epoch(),||{let mut ai=state.ai.lock().map_err(|_|AppError::io())?;
  if !ai.directories.get(&binding_id).is_some_and(|v|v.iter().any(|d|d.grant_id==grant_id)){return Err(AppError::denied());}ai.index.put(&grant_id,&pages,count)})
}
#[tauri::command]pub(crate) async fn m5_source(window:WebviewWindow,state:State<'_,Backend>,binding_id:String,message_id:String,source_id:String)->Result<(crate::files::Descriptor,u32)>{
 let owner=local_owner(&window)?;let(value,current,document)=data::with_binding(&state.files,&state.data,&owner,&binding_id,|s,b|Ok((s.ai_details(&b.conversation,&message_id)?,b.grant.clone(),b.document.clone())))?;
 let sources:Vec<crate::ai_context::Source>=serde_json::from_value(value.get("sources").cloned().ok_or_else(AppError::denied)?).map_err(|_|AppError::denied())?;
 let source=sources.iter().find(|s|s.id==source_id).ok_or_else(AppError::denied)?;
 if source.document_id.as_deref()==Some(&document){return Ok((state.files.duplicate_grant(&owner,&current)?,source.page));}
 let members=state.ai.lock().map_err(|_|AppError::io())?.directories.get(&binding_id).cloned().unwrap_or_default();
 for d in members{state.files.directory_live(&owner,&current,&d.grant_id)?;let observed=state.files.observe(&owner,&d.grant_id)?;if Some(&observed.key)==source.file_key.as_ref()&&Some(&observed.hash)==source.hash.as_ref(){
  let duplicate=state.files.duplicate_grant(&owner,&d.grant_id)?;if let Err(e)=state.files.directory_live(&owner,&current,&d.grant_id){let _=state.files.release(&owner,&duplicate.grant_id);return Err(e);}return Ok((duplicate,source.page));}}
 Err(AppError{code:"source_revoked",message:"此来源尚未重新授权或内容已变化；请重新允许同目录检索，不按文件名猜测关联。"})
}
