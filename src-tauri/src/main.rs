#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![cfg_attr(feature="worker-isolation-cli",allow(dead_code))]
mod error;
mod files;
mod workspace_access;
mod settings;
mod instance;
// Real replacement primitives are bounded to project test copies; no write IPC.
mod save_core;
#[cfg(debug_assertions)]
mod m3_writer;
mod unicode_worker;
mod worker_sandbox;
#[cfg(feature="worker-isolation-cli")]mod worker_test_cli;
// The isolated Release App uses exactly the production command registration.
macro_rules! reader_handler { () => { tauri::generate_handler![crate::workspace_access::pick_workspace,crate::workspace_access::list_workspace,crate::workspace_access::open_workspace_pdf,crate::workspace_access::close_workspace,crate::check_unicode_writer,crate::begin_unicode_export,crate::finish_unicode_export,crate::cancel_unicode_export,crate::save_original_pdf,crate::pending_pdf_recovery,crate::export_pdf_recovery,crate::m7_open_pending,crate::m6_commands::m6_preferences,crate::m6_commands::m6_edit_style,crate::m6_commands::m6_import_styles,crate::m6_commands::m6_select_style,crate::m6_commands::m6_delete_style_version,crate::m6_commands::m6_set_inputs,crate::m6_commands::m6_set_context,crate::m6_commands::m6_mock_explain,crate::ai_commands::m5_directory,crate::ai_commands::m5_index_member,crate::ai_commands::m5_source,crate::ai_commands::m5_config,crate::ai_commands::m5_chat_view,crate::ai_commands::m5_chat_select,crate::ai_commands::m5_chat_archive,crate::ai_commands::m5_chat_delete,crate::ai_commands::m5_set_config,crate::ai_commands::m5_store_key,crate::ai_commands::m5_delete_key,crate::ai_commands::m5_test_connection,crate::ai_commands::m5_index,crate::ai_commands::m5_prepare,crate::ai_commands::m5_images,crate::ai_commands::m5_refine,crate::ai_commands::m5_start,crate::ai_commands::m5_cancel,crate::ai_commands::m5_details,crate::pick_pdfs,crate::begin_pdf_read,crate::read_pdf,crate::cancel_pdf_read,crate::release_pdf,crate::get_settings,crate::set_reader_settings,crate::save_m3_copy,crate::m4_status,crate::m4_bind_document,crate::m4_messages,crate::m4_add_message,crate::m4_edit_prompt,crate::m4_generate_synthetic,crate::m4_delete_message,crate::m4_delete_document_history,crate::m4_position,crate::m4_release_binding,crate::m4_put_draft,crate::m4_list_drafts,crate::m4_drop_draft,crate::m4_backup,crate::m4_cache_clear,crate::m4_pick_migrate_data,crate::m4_get_settings,crate::m4_set_settings,crate::m4_list_backups,crate::m4_restore_backup] }; }
#[cfg(feature="save-chain-lab")]mod save_chain_lab;
mod pdf_export;
mod save_transport;
mod incremental;
mod original_commit;
// Disposable Windows save experiment only. No production command can write PDFs.
#[cfg(test)]
mod save_lab;
#[cfg(test)]
mod save_coord;
// Grant-bound local data; roots require permission and integrity validation.
mod storage;
mod data;
mod credentials;
mod ai_config;
mod ai_policy;
mod preferences;
mod m6_commands;
mod ai_context;
mod ai_network;
mod ai_image;
mod ai;
#[cfg(any(test,feature="save-chain-lab"))] mod ai_http_lab;
mod ai_commands;
mod gestures;
mod launch;
#[cfg(test)]mod save_append_guard;
#[cfg(test)]mod writer_font_lab;
#[cfg(test)]mod save_performance_tests;
use error::{AppError, Result};
use std::sync::{Arc, Mutex, atomic::Ordering};
#[cfg(debug_assertions)] use std::path::PathBuf;
use tauri::{Emitter, Manager, State, WebviewWindow};

struct Backend { folders:workspace_access::Workspaces, files: Arc<files::Files>, settings: Mutex<settings::Settings>, data:Arc<Mutex<data::DataService>>, ai:Arc<Mutex<ai::Service>>,pending:Arc<launch::Pending>,writer:Arc<pdf_export::Service> }
#[tauri::command]
async fn check_unicode_writer(window:WebviewWindow,state:State<'_,Backend>)->std::result::Result<&'static str,worker_sandbox::CheckError>{
    local_owner(&window)?;
    if state.writer.checking.swap(true,Ordering::AcqRel){return Err(AppError::busy().into());}
    state.writer.checking.store(false,Ordering::Release);
    state.writer.check_cancel.store(false,Ordering::Release);
    // The writer runs in the webview (MuPDF.js/WASM): there is no worker process,
    // no isolation profile and no ACL to prepare. What is checked is the writer
    // resource itself: the wasm engine and the pinned font are frontend assets,
    // so they are verified through the asset service that serves them, against
    // the lock this build compiled in, and never through a path that only exists
    // on the machine that produced the package. Original save stays behind its
    // own setting.
    let resolver=window.app_handle().asset_resolver();
    let writer=state.writer.clone();
    // Tauri development mode may read mutable dist files. Only production's
    // immutable embedded asset set is cached, for this Service lifetime.
    tauri::async_runtime::spawn_blocking(move||writer.check_assets(!tauri::is_dev(),&|path|resolver.get(path.to_string()).map(|asset|asset.bytes().to_vec())))
        .await.map_err(|_|AppError::io())?.map_err(worker_sandbox::CheckError::from)?;
    local_owner(&window)?;Ok("MuPDF.js 写入层已就绪，可以另存为批注副本；原件（Ctrl+S）按设置另行批准。")
}
#[tauri::command]
async fn begin_unicode_export(window:WebviewWindow,state:State<'_,Backend>,grant_id:String)->Result<String>{
    let owner=local_owner(&window)?;let files=state.files.clone();let writer=state.writer.clone();
    tauri::async_runtime::spawn_blocking(move||writer.reserve(&files,&owner,&grant_id)).await.map_err(|_|AppError::io())?
}
#[tauri::command]
fn cancel_unicode_export(window:WebviewWindow,state:State<'_,Backend>,task_id:String)->Result<()>{let owner=local_owner(&window)?;if task_id.is_empty(){state.writer.check_cancel.store(true,Ordering::Release);}else{state.writer.cancel(&owner,Some(&task_id));}Ok(())}
#[tauri::command]
async fn finish_unicode_export(window:WebviewWindow,state:State<'_,Backend>,request:tauri::ipc::Request<'_>)->Result<Option<files::Descriptor>>{
    let owner=local_owner(&window)?;
    let payload=save_transport::decode(request.body(),request.headers())?;
    if payload.binding_id.is_some(){return Err(AppError::denied());}
    let save_transport::Payload{task_id,candidate_len,output,..}=payload;
    let files=state.files.clone();let writer=state.writer.clone();let id=task_id.clone();let who=owner.clone();
    let prepared=tauri::async_runtime::spawn_blocking(move||writer.prepare(&files,&who,&id,&output[..candidate_len],&output)).await.map_err(|_|AppError::io())??;
    if local_owner(&window).is_err(){state.writer.abandon(&task_id);return Err(AppError::denied());}
    // The suggested name never collides on a repeated export, so the native
    // dialog cannot offer to overwrite a file this command must refuse.
    let suggested=format!("批注副本-{}.pdf",&uuid::Uuid::new_v4().to_string()[..6]);
    #[cfg(not(feature="save-chain-lab"))]
    let selected=rfd::AsyncFileDialog::new().set_parent(&window).set_title("导出新 PDF 副本（不能覆盖已有文件）").add_filter("PDF",&["pdf"]).set_file_name(&suggested).save_file().await.map(|file|file.path().to_owned());
    #[cfg(feature="save-chain-lab")]
    let selected=match save_chain_lab::choose_output(&window){Ok(path)=>path,Err(error)=>{state.writer.abandon(&task_id);return Err(error);}};
    let Some(path)=selected else{state.writer.abandon(&task_id);return Ok(None);};
    if local_owner(&window).is_err(){state.writer.abandon(&task_id);return Err(AppError::denied());}
    let files=state.files.clone();let writer=state.writer.clone();
    tauri::async_runtime::spawn_blocking(move||writer.commit_new(&files,&owner,&task_id,&path,prepared.0,prepared.1,prepared.2).map(Some)).await.map_err(|_|AppError::io())?
}
#[derive(serde::Serialize)]
#[serde(rename_all="camelCase")]
struct OriginalSaved {descriptor:Option<files::Descriptor>,hash:String,binding:Option<data::BoundDocument>,history_pending:bool,reopen_required:bool,cleanup_pending:bool,timings:OriginalTimings}
#[derive(serde::Serialize)]
#[serde(rename_all="camelCase")]
struct OriginalTimings {validation_ms:f64,backup_ms:f64,commit_ms:f64,history_ms:f64,housekeeping_ms:f64,decode_ms:f64,queue_ms:f64,command_ms:f64,commit_total_ms:f64}
#[tauri::command]
async fn save_original_pdf(window:WebviewWindow,state:State<'_,Backend>,request:tauri::ipc::Request<'_>)->std::result::Result<OriginalSaved,original_commit::SaveError>{
    let command_started=std::time::Instant::now();
    let owner=local_owner(&window)?;
    let decoding=std::time::Instant::now();let payload=save_transport::decode(request.body(),request.headers())?;let decode_ms=decoding.elapsed().as_secs_f64()*1000.0;
    let save_transport::Payload{task_id,binding_id,candidate_len,output}=payload;
    #[cfg(feature="save-chain-lab")]
    save_chain_lab::capture_original(&window,&output[..candidate_len],&output)?;
    let files=state.files.clone();let writer=state.writer.clone();let data=state.data.clone();let ai=state.ai.clone();
    let queued=std::time::Instant::now();
    tauri::async_runtime::spawn_blocking(move||{
        let queue_ms=queued.elapsed().as_secs_f64()*1000.0;
        let started=std::time::Instant::now();
        let (prepared,task,source)=writer.prepare(&files,&owner,&task_id,&output[..candidate_len],&output)?;
        if let Some(ref id)=binding_id {
            let ready=(||{
                let valid=data.lock().map_err(|_|AppError::io())?.bindings.get(id).is_some_and(|b|b.owner==owner&&b.grant==task.grant&&b.epoch==task.epoch);
                if !valid{return Err(AppError::denied());}
                ai::stop(&files,&data,&ai,&owner,Some(id),false)?;
                let members={let mut service=ai.lock().map_err(|_|AppError::io())?;service.remove_grant(&task.grant);service.revoke_directory(&owner,id)};
                for member in members{let _=files.release(&owner,&member.grant_id);}Ok(())
            })();
            if let Err(error)=ready{writer.abandon(&task_id);return Err(error.into());}
        }
        let validation_ms=started.elapsed().as_secs_f64()*1000.0;
        let committing=std::time::Instant::now();let outcome=writer.commit_guarded_original(&files,&owner,&task_id,prepared,task,source)?;let commit_total_ms=committing.elapsed().as_secs_f64()*1000.0;
        let history_started=std::time::Instant::now();
        let hash=outcome.receipt.new_hash().to_owned();
        let descriptor=outcome.grant.as_ref().ok().cloned();let reopen_required=descriptor.is_none();
        let binding=match (&descriptor,&binding_id){(Some(d),Some(id))=>data::bind_verified_commit(&files,&data,&owner,id,&d.grant_id,&outcome.receipt,None).ok().map(|v|v.bound),_=>None};
        let mut history_pending=binding_id.is_some()&&binding.is_none();
        if !history_pending&&!reopen_required&&original_commit::acknowledge(&outcome).is_err(){history_pending=true;}
        let history_ms=history_started.elapsed().as_secs_f64()*1000.0;
        let housekeeping=std::time::Instant::now();
        // A housekeeping failure never rewrites the saved PDF, discards a
        // recovery draft, or pretends that a completed commit did not happen.
        let cleanup_pending=if !history_pending&&!reopen_required{
            (||->Result<()>{
                let service=data.lock().map_err(|_|AppError::io())?;
                let document=binding.as_ref().map(|b|b.document_id.as_str()).ok_or_else(AppError::denied)?;
                service.manager.as_ref().ok_or_else(AppError::io)?.archive_pdf(&outcome,document)
            })().is_err()
        }else{false};
        let timings=OriginalTimings{validation_ms,backup_ms:outcome.timings.backup_ms,commit_ms:outcome.timings.commit_ms,history_ms,housekeeping_ms:housekeeping.elapsed().as_secs_f64()*1000.0,decode_ms,queue_ms,command_ms:command_started.elapsed().as_secs_f64()*1000.0,commit_total_ms};
        Ok(OriginalSaved{descriptor,hash,binding,history_pending,reopen_required,cleanup_pending,timings})
    }).await.map_err(|_|AppError::io())?
}
#[tauri::command]
async fn pending_pdf_recovery(window:WebviewWindow,state:State<'_,Backend>,grant_id:String)->Result<Vec<original_commit::RecoveryItem>>{
    let owner=local_owner(&window)?;let files=state.files.clone();
    tauri::async_runtime::spawn_blocking(move||original_commit::pending(&files,&owner,&grant_id)).await.map_err(|_|AppError::io())?
}
#[tauri::command]
async fn export_pdf_recovery(window:WebviewWindow,state:State<'_,Backend>,grant_id:String,event_id:String,newer:bool)->Result<Option<files::Descriptor>>{
    let owner=local_owner(&window)?;let files=state.files.clone();let grant=grant_id.clone();let who=owner.clone();let epoch=files.epoch();
    let bytes=tauri::async_runtime::spawn_blocking(move||original_commit::recovery_bytes(&files,&who,&grant,&event_id,newer)).await.map_err(|_|AppError::io())??;
    #[cfg(not(feature="save-chain-lab"))]
    let selected=rfd::AsyncFileDialog::new().set_parent(&window).set_title("恢复为新 PDF 副本（不覆盖原件）").add_filter("PDF",&["pdf"]).set_file_name(if newer{"恢复新版.pdf"}else{"恢复保存前.pdf"}).save_file().await.map(|f|f.path().to_owned());
    #[cfg(feature="save-chain-lab")]
    let selected=save_chain_lab::choose_output(&window)?;
    let Some(path)=selected else{return Ok(None);};local_owner(&window)?;
    let files=state.files.clone();
    tauri::async_runtime::spawn_blocking(move||pdf_export::recovery_copy(&files,&owner,&grant_id,epoch,&path,&bytes).map(Some)).await.map_err(|_|AppError::io())?
}
// Unwired recovery classifier is retained as test evidence, not a production
// command: it neither restores PDFs nor validates a user-authorized recovery.
#[cfg(test)]
async fn original_save_state()->Result<serde_json::Value>{
    let resolved=tauri::async_runtime::spawn_blocking(pdf_export::recover_pending).await.map_err(|_|AppError::io())?;
    Ok(serde_json::json!({"interrupted":resolved}))
}
#[tauri::command]
async fn m7_open_pending(window:WebviewWindow,state:State<'_,Backend>)->Result<OpenBatch>{let owner=local_owner(&window)?;let pending=state.pending.clone();let files=state.files.clone();let (documents,rejected)=tauri::async_runtime::spawn_blocking(move||pending.take(&files,&owner)).await.map_err(|_|AppError::io())??;if state.pending.focus_requested(){let _=window.unminimize();let _=window.set_focus();}Ok(OpenBatch{documents,rejected})}
async fn data_task<T:Send+'static>(window:WebviewWindow,state:State<'_,Backend>,operation:impl FnOnce(&files::Files,&Mutex<data::DataService>,&str)->Result<T>+Send+'static)->Result<T>{
    let owner=local_owner(&window)?;let files=state.files.clone();let data=state.data.clone();
    tauri::async_runtime::spawn_blocking(move||operation(&files,&data,&owner)).await.map_err(|_|AppError::io())?
}
#[tauri::command]
async fn m4_status(window:WebviewWindow,state:State<'_,Backend>)->Result<data::Status>{data_task(window,state,|_,data,_|Ok(data.lock().map_err(|_|AppError::io())?.status())).await}
#[tauri::command]
async fn m4_bind_document(window:WebviewWindow,state:State<'_,Backend>,grant_id:String,page_count:u32)->Result<data::BoundDocument>{
    data_task(window,state,move|files,data,owner|{let epoch=files.epoch();let observation=files.observe(owner,&grant_id)?;
        files.with_live(owner,&grant_id,epoch,||data.try_lock().map_err(|_|AppError::busy())?.bind(owner,&grant_id,epoch,observation,page_count))}).await
}
#[tauri::command]
async fn m4_messages(window:WebviewWindow,state:State<'_,Backend>,binding_id:String)->Result<Vec<storage::model::desktop::HistoryMessage>>{data_task(window,state,move|files,data,owner|data::history(files,data,owner,&binding_id)).await}
#[tauri::command]
async fn m4_add_message(window:WebviewWindow,state:State<'_,Backend>,binding_id:String,content:String,page:u32,parent_id:Option<String>)->Result<String>{
    data_task(window,state,move|files,data,owner|data::with_binding(files,data,owner,&binding_id,|s,b|s.add_local_prompt(&b.conversation,&b.revision,parent_id.as_deref(),&content,page))).await
}
#[tauri::command]
async fn m4_edit_prompt(window:WebviewWindow,state:State<'_,Backend>,binding_id:String,message_id:String,content:String)->Result<String>{
    data_task(window,state,move|files,data,owner|data::with_binding(files,data,owner,&binding_id,|s,b|s.edit_local_prompt(&b.conversation,&message_id,&content))).await
}
#[tauri::command]
async fn m4_generate_synthetic(window:WebviewWindow,state:State<'_,Backend>,binding_id:String,parent_id:String)->Result<String>{
    data_task(window,state,move|files,data,owner|data::with_binding(files,data,owner,&binding_id,|s,b|s.local_regenerate(&b.conversation,&b.revision,&parent_id))).await
}
#[tauri::command]
async fn m4_delete_message(window:WebviewWindow,state:State<'_,Backend>,binding_id:String,message_id:String)->Result<()>{
    let owner=local_owner(&window)?;ai::stop(&state.files,&state.data,&state.ai,&owner,Some(&binding_id),false)?;
    data_task(window,state,move|files,data,owner|data::with_binding(files,data,owner,&binding_id,|s,b|s.hide_local_message(&b.conversation,&message_id))).await
}
#[tauri::command]
async fn m4_delete_document_history(window:WebviewWindow,state:State<'_,Backend>,binding_id:String)->Result<bool>{
    local_owner(&window)?;let generation=state.data.lock().map_err(|_|AppError::io())?.manager.as_ref().ok_or_else(AppError::io)?.info().generation;
    // Validate the live grant before asking, then validate it again after the
    // native confirmation. No renderer document ID or path selects the target.
    let check_binding=binding_id.clone();
    let (name,document)=data_task(window.clone(),state.clone(),move|files,data,owner|data::with_generation(files,data,owner,&check_binding,generation,|_,binding|Ok((binding.display_name.clone(),binding.document.clone())))).await?;
    // The target comes from the backend's opened-file observation, never from
    // renderer text. Strip controls/bidi formatting so the UUID stays legible.
    let display_name:String=name.chars().filter(|c|!c.is_control()&&!matches!(*c,'\u{061c}'|'\u{200e}'|'\u{200f}'|'\u{2028}'|'\u{2029}'|'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}')).take(200).collect();
    let description=format!("将删除以下指定文档在活动数据库中的全部本地聊天记录与分支，包括已隐藏记录。\n\n文件：{display_name}\n文档 UUID：{document}\n\n即使切换标签，也只删除上述指定文档。其他文档、未保存批注草稿、阅读位置及 PDF 不变。旧备份和旧数据目录仍保留历史，不执行安全擦除。确认删除？");
    let accepted=rfd::AsyncMessageDialog::new().set_parent(&window).set_title("删除指定 PDF 的全部本地聊天历史")
        .set_description(description)
        .set_buttons(rfd::MessageButtons::YesNo).show().await;
    if accepted!=rfd::MessageDialogResult::Yes{return Ok(false);}
    let ai=state.ai.clone();data_task(window,state,move|files,data,owner|ai::change_document_checked(files,data,&ai,owner,&binding_id,Some(generation),true,|_,binding|{
        if binding.document!=document{return Err(storage::DataError::Refused("history document changed"));}
        Ok(())
    },|store,binding|{
        store.delete_document_history(&binding.document,&binding.conversation)?;Ok(true)
    })).await
}
#[tauri::command]
async fn m4_position(window:WebviewWindow,state:State<'_,Backend>,binding_id:String,position:storage::model::desktop::ReadingPosition)->Result<()>{
    data_task(window,state,move|files,data,owner|data::with_binding(files,data,owner,&binding_id,|s,b|s.put_desktop_position(&b.document,&b.revision,&position))).await
}
#[tauri::command]
async fn m4_release_binding(window:WebviewWindow,state:State<'_,Backend>,binding_id:String)->Result<()>{
    let owner=local_owner(&window)?;ai::stop(&state.files,&state.data,&state.ai,&owner,Some(&binding_id),false)?;
    let members=state.ai.lock().map_err(|_|AppError::io())?.revoke_directory(&owner,&binding_id);for d in members{let _=state.files.release(&owner,&d.grant_id);}
    data_task(window,state,move|_,data,owner|{let mut data=data.lock().map_err(|_|AppError::io())?;
        if !data.bindings.get(&binding_id).is_some_and(|b|b.owner==owner){return Err(AppError::denied());}data.bindings.remove(&binding_id);Ok(())}).await
}
#[tauri::command]
async fn m4_put_draft(window:WebviewWindow,state:State<'_,Backend>,binding_id:String,id:String,json:String)->Result<()>{
    data_task(window,state,move|files,data,owner|data::with_binding(files,data,owner,&binding_id,|s,b|s.put_local_draft(&b.document,&b.revision,&id,&json))).await
}
#[tauri::command]
async fn m4_list_drafts(window:WebviewWindow,state:State<'_,Backend>,binding_id:String)->Result<Vec<storage::model::desktop::DraftRecord>>{data_task(window,state,move|files,data,owner|data::drafts(files,data,owner,&binding_id)).await}
#[tauri::command]
async fn m4_drop_draft(window:WebviewWindow,state:State<'_,Backend>,binding_id:String,id:String)->Result<()>{
    data_task(window,state,move|files,data,owner|data::with_binding(files,data,owner,&binding_id,|s,b|s.drop_local_draft(&b.document,&b.revision,&id))).await
}
#[tauri::command]
async fn m4_backup(window:WebviewWindow,state:State<'_,Backend>)->Result<String>{data_task(window,state,|_,data,_|{
    let mut service=data.lock().map_err(|_|AppError::io())?;let backup=service.manager.as_mut().ok_or_else(AppError::io)?.backup().map_err(data::error)?;Ok(backup.id)}).await}
#[tauri::command]
async fn m4_cache_clear(window:WebviewWindow,state:State<'_,Backend>)->Result<()>{data_task(window,state,|_,data,_|{
    let mut service=data.lock().map_err(|_|AppError::io())?;service.manager.as_mut().ok_or_else(AppError::io)?.store_mut().clear_rebuildable_cache().map_err(data::error)}).await}
#[tauri::command]
async fn m4_get_settings(window:WebviewWindow,state:State<'_,Backend>)->Result<data::DataSettings>{data_task(window,state,|_,data,_|{
    let service=data.lock().map_err(|_|AppError::io())?;data::DataSettings::read(service.manager.as_ref().ok_or_else(AppError::io)?.store()).map_err(data::error)}).await}
#[tauri::command]
async fn m4_set_settings(window:WebviewWindow,state:State<'_,Backend>,thumbnail_open:bool,cache_bytes:u64)->Result<data::DataSettings>{data_task(window,state,move|_,data,_|{
    if cache_bytes>512*1024*1024{return Err(AppError::denied());}
    let mut service=data.lock().map_err(|_|AppError::io())?;let store=service.manager.as_mut().ok_or_else(AppError::io)?.store_mut();
    store.update_settings(thumbnail_open,cache_bytes).map_err(data::error)?;data::DataSettings::read(store).map_err(data::error)}).await}
#[tauri::command]
async fn m4_list_backups(window:WebviewWindow,state:State<'_,Backend>)->Result<Vec<storage::directories::BackupInfo>>{data_task(window,state,|_,data,_|{
    let service=data.lock().map_err(|_|AppError::io())?;service.manager.as_ref().ok_or_else(AppError::io)?.list_backups().map_err(data::error)}).await}
#[tauri::command]
async fn m4_restore_backup(window:WebviewWindow,state:State<'_,Backend>,backup_id:String)->Result<Option<data::Status>>{
    local_owner(&window)?;
    if uuid::Uuid::parse_str(&backup_id).is_err(){return Err(AppError::denied());}
    let accepted=rfd::AsyncMessageDialog::new().set_parent(&window).set_title("确认恢复备份到新数据目录")
        .set_description("将在新的独立目录恢复这份备份，校验并重开后切换。当前数据目录保留。备份之后新增的历史不会出现在恢复后的目录中；可返回当前目录取回。确认恢复？")
        .set_buttons(rfd::MessageButtons::YesNo).show().await;
    if accepted!=rfd::MessageDialogResult::Yes{return Ok(None);}
    ai::stop(&state.files,&state.data,&state.ai,"main",None,true)?;
    let resume_ai=state.ai.clone();
    data_task(window,state,move|files,data,owner|{let result=(||{let mut service=data.lock().map_err(|_|AppError::io())?;
        service.manager.as_mut().ok_or_else(AppError::io)?.restore_backup(&backup_id).map_err(data::error)?;service.revoke();Ok(Some(service.status()))})();
        let members={let mut ai=resume_ai.lock().map_err(|_|AppError::io())?;ai.paused=false;let keys=ai.directory_sources.keys().cloned().collect::<Vec<_>>();let mut members=Vec::new();for key in keys{members.extend(ai.revoke_directory(owner,&key));}members};for d in members{let _=files.release(owner,&d.grant_id);}result}).await
}
#[tauri::command]
async fn m4_pick_migrate_data(window:WebviewWindow,state:State<'_,Backend>)->Result<Option<data::Status>>{
    local_owner(&window)?;
    let Some(parent)=rfd::AsyncFileDialog::new().set_parent(&window).set_title("选择持久数据目录的父文件夹（创建新的独立子目录）").pick_folder().await else{return Ok(None);};
    let accepted=rfd::AsyncMessageDialog::new().set_parent(&window).set_title("确认迁移本应用数据")
        .set_description("将暂停数据写入，把历史、设置、草稿和重要备份复制到新的独立目录，校验并重新打开后切换。旧目录保留；不会覆盖所选文件夹的已有文件。确认迁移当前数据？")
        .set_buttons(rfd::MessageButtons::YesNo).show().await;
    if accepted!=rfd::MessageDialogResult::Yes{return Ok(None);}
    ai::stop(&state.files,&state.data,&state.ai,"main",None,true)?;
    let resume_ai=state.ai.clone();
    let parent=parent.path().to_path_buf();
    data_task(window,state,move|_,data,_|{let result=(||{let mut service=data.lock().map_err(|_|AppError::io())?;
        service.manager.as_mut().ok_or_else(AppError::io)?.migrate_to(&parent,"").map_err(data::error)?;Ok(Some(service.status()))})();resume_ai.lock().map_err(|_|AppError::io())?.paused=false;result}).await
}
fn reader_url(url: &tauri::Url) -> bool {
    let local = url.scheme() == "https" && url.host_str() == Some("tauri.localhost") && url.port().is_none();
    let development = cfg!(debug_assertions) && url.scheme() == "http" && url.host_str() == Some("127.0.0.1") && url.port() == Some(43182);
    (local || development) && url.path() == "/" && url.query().is_none() && url.fragment().is_none() && url.username().is_empty() && url.password().is_none()
}
fn local_owner(window: &WebviewWindow) -> Result<String> {
    let url = window.url().map_err(|_| AppError::denied())?;
    if window.label() != "main" || !reader_url(&url) {
        return Err(AppError::denied());
    }
    Ok(window.label().into())
}
#[derive(Clone, serde::Serialize)]
struct OpenBatch { documents: Vec<files::Descriptor>, rejected: Vec<AppError> }
#[tauri::command]
async fn save_m3_copy(window:WebviewWindow,state:State<'_,Backend>,grant_id:String,bytes:Vec<u8>)->Result<files::Descriptor>{
    let owner=local_owner(&window)?;
    #[cfg(debug_assertions)] {
        if bytes.len()>1024*1024||std::env::var("M3_ISOLATED_WRITER").as_deref()!=Ok("1"){return Err(AppError::denied());}
        let files=state.files.clone();let epoch=files.epoch();
        return tauri::async_runtime::spawn_blocking(move||m3_writer::copy(&files,&owner,&grant_id,epoch,&bytes))
            .await.map_err(|_|AppError::io())?.map_err(|_|AppError{code:"test_writer_refused",message:"验证副本未完成：仅支持项目测试目录、单页和至多一个黑色文本框；原 PDF 未修改。"});
    }
    #[cfg(not(debug_assertions))] {let _=(owner,state,grant_id,bytes);Err(AppError::denied())}
}
#[tauri::command]
async fn pick_pdfs(window: WebviewWindow, state: State<'_, Backend>) -> Result<OpenBatch> {
    let owner = local_owner(&window)?;
    let files = state.files.clone();
    let epoch = files.epoch();
    if files.picking.swap(true, Ordering::AcqRel) { return Err(AppError::busy()); }
    struct Picking(Arc<files::Files>);
    impl Drop for Picking { fn drop(&mut self) { self.0.picking.store(false, Ordering::Release); } }
    let _picking = Picking(files.clone());
    #[cfg(not(feature="save-chain-lab"))]
    let selected = rfd::AsyncFileDialog::new().set_parent(&window).add_filter("PDF", &["pdf"])
        .set_title("打开本地 PDF").pick_files().await.unwrap_or_default();
    #[cfg(not(feature="save-chain-lab"))]
    let selected:Vec<std::path::PathBuf>=selected.into_iter().map(|f|f.path().to_owned()).collect();
    #[cfg(feature="save-chain-lab")]
    let selected=save_chain_lab::choose_input(&window)?;
    if selected.len() > 32 { return Err(AppError::busy()); }
    let result = tauri::async_runtime::spawn_blocking(move || {
        let mut batch = OpenBatch { documents: vec![], rejected: vec![] };
        for selected_file in selected {
            match files.authorize_at(&owner, &selected_file, epoch) {
                Ok(document) => batch.documents.push(document), Err(error) => batch.rejected.push(error),
            }
        }
        batch
    }).await.map_err(|_| AppError::io())?;
    Ok(result)
}
#[tauri::command]
fn begin_pdf_read(window: WebviewWindow, state: State<'_, Backend>, grant_id: String) -> Result<String> {
    state.files.begin(&local_owner(&window)?, &grant_id)
}
#[tauri::command]
async fn read_pdf(window: WebviewWindow, state: State<'_, Backend>, task_id: String) -> Result<tauri::ipc::Response> {
    let owner = local_owner(&window)?;
    let files = state.files.clone();
    let bytes = tauri::async_runtime::spawn_blocking(move || files.read(&owner, &task_id)).await.map_err(|_| AppError::io())??;
    Ok(tauri::ipc::Response::new(bytes))
}
#[tauri::command]
fn cancel_pdf_read(window: WebviewWindow, state: State<'_, Backend>, task_id: String) -> Result<()> {
    state.files.cancel(&local_owner(&window)?, &task_id)
}
#[tauri::command]
fn release_pdf(window: WebviewWindow, state: State<'_, Backend>, grant_id: String) -> Result<()> {
    let owner=local_owner(&window)?;
    state.writer.cancel_grant(&owner,&grant_id);
    let mut bindings=state.data.try_lock().map(|d|d.bindings.iter().filter(|(_,b)|b.owner==owner&&b.grant==grant_id).map(|(id,_)|id.clone()).collect::<Vec<_>>()).unwrap_or_default();
    let scoped=state.ai.lock().map_err(|_|AppError::io())?.directory_sources.iter().filter(|(_,source)|*source==&grant_id).map(|(id,_)|id.clone()).collect::<Vec<_>>();for id in scoped{if !bindings.contains(&id){bindings.push(id);}}
    for binding in bindings{
        ai::stop(&state.files,&state.data,&state.ai,&owner,Some(&binding),false)?;
        let members=state.ai.lock().map_err(|_|AppError::io())?.revoke_directory(&owner,&binding);
        for d in members{let _=state.files.release(&owner,&d.grant_id);}
    }
    state.ai.lock().map_err(|_|AppError::io())?.remove_grant(&grant_id);state.files.release(&owner, &grant_id)?;
    // The live grant is already revoked. Do not stall the GUI behind a migration;
    // stale data bindings cannot pass with_live while cleanup waits in background.
    let data=state.data.clone();tauri::async_runtime::spawn_blocking(move||{if let Ok(mut service)=data.lock(){service.release_grant(&owner,&grant_id);}});Ok(())
}
#[tauri::command]
fn get_settings(window: WebviewWindow, state: State<'_, Backend>) -> Result<settings::Snapshot> {
    local_owner(&window)?;
    let mut snapshot=state.settings.lock().map_err(|_|AppError::io())?.snapshot();
    let data=state.data.try_lock().map_err(|_|AppError::busy())?;
    if let Some(manager)=&data.manager {
        if let Some(value)=manager.store().default_zoom().map_err(data::error)? {
            snapshot.reader.default_zoom=serde_json::from_value(serde_json::Value::String(value)).map_err(|_|AppError::io())?;
        }
        snapshot.writable=true;snapshot.notice="旧版设置保留；新设置存放在当前数据目录。";
    }else{snapshot.writable=false;}
    Ok(snapshot)
}
#[tauri::command]
async fn set_reader_settings(window: WebviewWindow, reader: settings::ReaderSettings) -> Result<settings::Snapshot> {
    local_owner(&window)?;
    let app = window.app_handle().clone();
    // New settings belong to the selected data directory. Legacy JSON is retained,
    // never automatically migrated, overwritten or removed.
    tauri::async_runtime::spawn_blocking(move || {
        let backend=app.state::<Backend>();let mut data=backend.data.lock().map_err(|_|AppError::io())?;
        let value=serde_json::to_value(reader.default_zoom).map_err(|_|AppError::io())?;
        data.manager.as_mut().ok_or_else(AppError::io)?.store_mut().set_default_zoom(value.as_str().ok_or_else(AppError::io)?).map_err(data::error)?;
        let mut snapshot=backend.settings.lock().map_err(|_|AppError::io())?.snapshot();snapshot.reader=reader;snapshot.writable=true;snapshot.notice="";Ok(snapshot)
    })
        .await.map_err(|_| AppError::io())?
}

#[cfg(feature="worker-isolation-cli")]
fn main(){std::process::exit(worker_test_cli::run());}
#[cfg(all(feature="save-chain-lab",not(feature="worker-isolation-cli")))]
fn main(){save_chain_lab::run();}
#[cfg(not(any(feature="worker-isolation-cli",feature="save-chain-lab")))]
fn main() {
    let pending=Arc::new(launch::Pending::default());
    let packet=match std::env::current_dir().map_err(|_|AppError::io()).and_then(|cwd|launch::Packet::arguments(std::env::args_os(),&cwd)){Ok(p)=>p,Err(_)=>{eprintln!("PDF arguments were refused; only bounded local PDF paths are accepted");std::process::exit(2);}};
    let _instance = match instance::Instance::acquire(pending.clone(),&packet) {
        Ok(Some(instance)) => instance,
        Ok(None) => return,
        Err(_) => { eprintln!("Unable to initialize the reader instance"); std::process::exit(1); }
    };
    if pending.push(packet).is_err(){eprintln!("Too many pending PDF arguments");std::process::exit(2);}
    tauri::Builder::default()
        .setup(move |app| {
            let base=app.path().app_local_data_dir()?;
            #[cfg(debug_assertions)]
            let base=if let Some(path)=std::env::var_os("M4_TEST_ROOT") {
                let lab=PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("tests/work/m4").canonicalize()?;
                let path=PathBuf::from(path).canonicalize()?;
                if path==lab||!path.starts_with(&lab){return Err("Unauthorized synthetic M4 root".into());}path
            }else{base};
            let data=data::DataService::open(&base.join("data-manager"),&base.join("data-v1"));
            let worker_root=if cfg!(debug_assertions){unicode_worker::prototype()}else{unicode_worker::installed_runtime()?};
            app.manage(Backend { folders:workspace_access::Workspaces::default(), files: Arc::new(files::Files::default()), settings: Mutex::new(settings::Settings::load(app.path().app_config_dir()?)),data:Arc::new(Mutex::new(data)),ai:Arc::new(Mutex::new(ai::Service::default())),pending:pending.clone(),writer:Arc::new(pdf_export::Service::new(worker_root)) });
            let builder=tauri::WebviewWindowBuilder::from_config(app, &app.config().app.windows[0])?;
            #[cfg(debug_assertions)]
            let builder=if std::env::var_os("M4_TEST_ROOT").is_some(){builder.data_directory(base.join("webview-test-profile"))}else{builder};
            let window=builder
                .on_navigation(reader_url)
                .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
                .on_page_load(|window, payload| {
                    if matches!(payload.event(), tauri::webview::PageLoadEvent::Started) {let backend=window.state::<Backend>();backend.writer.cancel("main",None);if let Ok(mut ai)=backend.ai.lock(){ai.cancel("main",None);ai.index=ai_context::Index::new();ai.directories.clear();ai.directory_sources.clear();}backend.folders.clear();backend.files.revoke_all();let epoch=backend.files.epoch();let data=backend.data.clone();tauri::async_runtime::spawn_blocking(move||{if let Ok(mut service)=data.lock(){service.revoke_before(epoch);}});}
                }).build()?;
            gestures::configure(&window);
            Ok(())
        })
        .invoke_handler(reader_handler!())
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let tauri::WindowEvent::DragDrop(tauri::DragDropEvent::Drop { paths, .. }) = event {
                    let Some(webview) = window.app_handle().get_webview_window("main") else { return; };
                    if local_owner(&webview).is_err() { return; }
                    let paths = paths.clone();
                    let files = window.state::<Backend>().files.clone(); let epoch = files.epoch();
                    tauri::async_runtime::spawn_blocking(move || {
                        let mut batch = OpenBatch { documents: vec![], rejected: vec![] };
                        if paths.len() > 32 { batch.rejected.push(AppError::busy()); }
                        else { for path in paths { match files.authorize_at("main", &path, epoch) {
                            Ok(document) => batch.documents.push(document), Err(error) => batch.rejected.push(error),
                        } } }
                        if local_owner(&webview).is_err() || webview.emit("opened-pdfs", &batch).is_err() {
                            for document in batch.documents { let _ = files.release("main", &document.grant_id); }
                        }
                    });
                }
            }
            if window.label() == "main" && matches!(event, tauri::WindowEvent::Destroyed) {
                window.state::<Backend>().writer.close();window.state::<Backend>().pending.close();if let Ok(mut ai)=window.state::<Backend>().ai.lock(){ai.cancel("main",None);}window.state::<Backend>().folders.clear();window.state::<Backend>().files.shutdown();
            }
        })
        .build(tauri::generate_context!())
        .expect("Unable to start PDF Reader")
        .run(|app, event| { if matches!(event, tauri::RunEvent::Exit) { app.state::<Backend>().writer.close();app.state::<Backend>().pending.close();app.state::<Backend>().folders.clear();app.state::<Backend>().files.shutdown(); } });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn navigation_and_command_origins_allow_only_the_exact_local_reader() {
        assert!(reader_url(&"https://tauri.localhost/".parse().unwrap()));
        assert_eq!(reader_url(&"http://127.0.0.1:43182/".parse().unwrap()), cfg!(debug_assertions));
        for url in ["https://example.com/", "http://tauri.localhost/", "https://tauri.localhost/other", "https://tauri.localhost/?path=a", "https://tauri.localhost/#x", "https://user@tauri.localhost/", "http://localhost:43182/", "http://127.0.0.1:43181/", "file:///D:/test.pdf"] {
            assert!(!reader_url(&url.parse().unwrap()), "{url}");
        }
    }
}
