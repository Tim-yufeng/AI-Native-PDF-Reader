//! Explicit project-only Release IPC lab. No mock runtime, installer, visible
//! window, extra IPC command, filesystem permission or production ACL override.
use std::{borrow::Cow,collections::BTreeMap,fs::{self,OpenOptions},io::Write,path::{Path,PathBuf},sync::{Arc,Mutex,atomic::{AtomicUsize,Ordering}},time::Duration};
use std::os::windows::fs::{OpenOptionsExt,MetadataExt};
use tauri::{Assets,Manager,Runtime,WebviewWindow};
use tauri::utils::assets::{AssetKey,AssetsIter,CspHash};
use crate::error::{AppError,Result};
const SOURCE_HASH:&str="a8a9d4ac31dbcd6d11238dc5d07918da48f826d545f24a3c4e0fdca370d334a9";
// The marker is written into every lab report as `labBinaryMarker`, which is
// what run-save-chain-lab.mjs and check-release-artifact.mjs compare against.
// It is deliberately not a byte probe: release builds use `strip = true`, so a
// literal that no live code reads is legitimately removed from the binary.
static LAB_BINARY_MARKER:&str="AI_NATIVE_SAVE_CHAIN_LAB_ONLY_v1";
const CASES:[&str;7]=["freetext-only","highlight-only","ink-only","mixed","revoked","negative","original"];
fn app_source()->std::io::Result<Vec<u8>>{
    let bytes=fs::read(crate::save_core::project().join("tests/fixtures/g1/plain.pdf"))?;
    if crate::save_core::hash(&bytes)!="bf18e6eb895bf5e845576b79335c4e104370da4cbfaa0cd2005d5dd838f7ae7e"{return Err(std::io::Error::other("plain fixture differs"));}
    let text=String::from_utf8(bytes).map_err(std::io::Error::other)?;
    Ok(text.replace("%PDF-1.7","%PDF-1.4").replace("/Root 1 0 R >>","/Root 1 0 R /ID [(012345>>89abcdef)(0123456789abcdef)] >>").into_bytes())
}
pub(crate) struct Lab { root:PathBuf, exports:AtomicUsize, app:bool }
pub(crate) fn choose_input(window:&WebviewWindow)->Result<Vec<PathBuf>>{
    crate::local_owner(window)?;let lab=window.state::<Lab>();
    if !lab.app{return Err(AppError::denied());}Ok(vec![lab.root.join("plain.pdf")])
}
pub(crate) fn capture_original(window:&WebviewWindow,candidate:&[u8],output:&[u8])->Result<()>{
    let started=std::time::Instant::now();
    crate::local_owner(window)?;let lab=window.state::<Lab>();if !lab.app{return Ok(());}
    if candidate.len()>16*1024*1024||output.len()>20*1024*1024{return Err(AppError::denied());}
    let id=uuid::Uuid::new_v4();for (name,bytes) in [("candidate",candidate),("output",output)]{
        let mut f=OpenOptions::new().write(true).create_new(true).open(lab.root.join(format!("{id}-{name}.pdf"))).map_err(|_|AppError::io())?;
        f.write_all(bytes).and_then(|_|f.sync_all()).map_err(|_|AppError::io())?;
    }
    if std::env::var("SAVE_APP_EVIDENCE").as_deref()==Ok("1"){
        let evidence=serde_json::json!({"id":id.to_string(),"captureMs":started.elapsed().as_secs_f64()*1000.0,"candidateBytes":candidate.len(),"outputBytes":output.len()});
        // Lab-only evidence I/O is reported separately, never a new save guard.
        let _=fs::write(lab.root.join(format!("{id}-capture.json")),evidence.to_string());
    }Ok(())
}

/// Replaces only the native destination picker, never validation or commit.
/// This function is absent from every normal/package build.
pub(crate) fn choose_output(window:&WebviewWindow)->Result<Option<PathBuf>>{
    crate::local_owner(window)?;
    let lab=window.state::<Lab>();let index=lab.exports.fetch_add(1,Ordering::AcqRel);
    if index>=8{return Err(AppError::denied());}
    Ok(Some(lab.root.join("exports").join(format!("export-{index:02}.pdf"))))
}
struct EmptyAssets;
impl<R:Runtime> Assets<R> for EmptyAssets{
    fn get(&self,_:&AssetKey)->Option<Cow<'_,[u8]>>{None}
    fn iter(&self)->Box<AssetsIter<'_>>{Box::new(std::iter::empty())}
    fn csp_hashes(&self,_:&AssetKey)->Box<dyn Iterator<Item=CspHash<'_>>+'_>{Box::new(std::iter::empty())}
}
struct LabAssets { original:Box<dyn Assets<tauri::Wry>>, files:BTreeMap<String,Vec<u8>> }
impl Assets<tauri::Wry> for LabAssets{
    fn setup(&self,app:&tauri::App<tauri::Wry>){self.original.setup(app);}
    fn get(&self,key:&AssetKey)->Option<Cow<'_,[u8]>>{
        self.files.get(key.as_ref()).map(|v|Cow::Borrowed(v.as_slice())).or_else(||self.original.get(key))
    }
    fn iter(&self)->Box<AssetsIter<'_>>{
        Box::new(self.original.iter().filter(move|(key,_)|!self.files.contains_key(key.as_ref())).chain(self.files.iter().map(|(k,v)|(Cow::Borrowed(k.as_str()),Cow::Borrowed(v.as_slice())))))
    }
    fn csp_hashes(&self,path:&AssetKey)->Box<dyn Iterator<Item=CspHash<'_>>+'_>{self.original.csp_hashes(path)}
}
fn regular_tree(root:&Path,dir:&Path,files:&mut BTreeMap<String,Vec<u8>>)->std::io::Result<()>{
    use std::os::windows::fs::MetadataExt;
    if fs::symlink_metadata(dir)?.file_attributes()&0x400!=0{return Err(std::io::Error::other("linked lab assets"));}
    for entry in fs::read_dir(dir)?{
        let path=entry?.path();let meta=fs::symlink_metadata(&path)?;
        if meta.file_attributes()&0x400!=0{return Err(std::io::Error::other("linked lab asset"));}
        if meta.is_dir(){regular_tree(root,&path,files)?;}
        else if meta.is_file(){
            // Vite also emits the unchanged 10 MiB MuPDF WASM fallback URL.
            if meta.len()>16*1024*1024{return Err(std::io::Error::other("oversized lab asset"));}
            let key=path.strip_prefix(root).map_err(std::io::Error::other)?.to_string_lossy().replace('\\',"/");
            if !key.starts_with("__save_chain_lab/")&&key!="tests/headless/save-chain.html"&&key!="tests/headless/save-app.html"&&key!="tests/headless/save-ipc.html"{return Err(std::io::Error::other("unexpected lab asset"));}
            files.insert(format!("/{key}"),fs::read(path)?);
        }else{return Err(std::io::Error::other("irregular lab asset"));}
    }Ok(())
}
fn publish(root:&Path,value:&serde_json::Value)->std::io::Result<()>{
    let mut file=OpenOptions::new().write(true).create_new(true).open(root.join("result.json"))?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;file.sync_all()
}
fn validate_report(root:&Path,report:&mut serde_json::Value)->std::io::Result<()>{
    let rows=report.get("rows").and_then(|v|v.as_array()).ok_or_else(||std::io::Error::other("missing rows"))?;
    if report["schema"]!=1||rows.len()>32||rows.iter().any(|r|r["status"]!="PASS")||rows.len()!=18{return Err(std::io::Error::other("incomplete lab report"));}
    let source=fs::read(root.join("source-mixed.pdf"))?;
    let mut inventory=vec![];
    for case in CASES{
        if case=="original"{continue;}
        if crate::save_core::hash(&fs::read(root.join(format!("source-{case}.pdf")))?)!=SOURCE_HASH{return Err(std::io::Error::other("source changed"));}
    }
    for index in 0..8{
        let bytes=fs::read(root.join("exports").join(format!("export-{index:02}.pdf")))?;
        if !bytes.starts_with(&source)||bytes.len()<=source.len(){return Err(std::io::Error::other("invalid output"));}
        inventory.push(serde_json::json!({"file":format!("export-{index:02}.pdf"),"bytes":bytes.len(),"sha256":crate::save_core::hash(&bytes)}));
    }
    let original=fs::read(root.join("source-original.pdf"))?;
    if !original.starts_with(&source)||original.len()<=source.len(){return Err(std::io::Error::other("original not committed"));}
    report["originalBytes"]=original.len().into();report["originalSha256"]=crate::save_core::hash(&original).into();
    report["nativeOutputs"]=serde_json::Value::Array(inventory);
    report["saveAsSourcesUnchanged"]=true.into();
    report["runtime"]="real-Wry-Release-custom-protocol".into();
    report["labBinaryMarker"]=LAB_BINARY_MARKER.into();
    report["scope"]="real frontend writer, Tauri IPC/ACL and save core; fixed synthetic destination replaces native picker; two actual original commits and history handoff; no manual GUI acceptance".into();
    Ok(())
}
fn validate_app_report(root:&Path,report:&mut serde_json::Value)->std::io::Result<()>{
    let rows=report["rows"].as_array().ok_or_else(||std::io::Error::other("missing App rows"))?;
    if rows.len()<2||rows.iter().any(|r|r["status"]!="PASS"){return Err(std::io::Error::other("incomplete App loop"));}
    let original=fs::read(root.join("plain.pdf"))?;
    if report["rows"].as_array().and_then(|r|r.last()).map(|r|r["sha256"].as_str())!=Some(Some(crate::save_core::hash(&original).as_str())){return Err(std::io::Error::other("App and disk output differ"));}
    let source=app_source()?;
    if !original.starts_with(&source)||original.len()<=source.len(){return Err(std::io::Error::other("App did not commit original"));}
    let backups=fs::read_dir(root)?.filter_map(|e|e.ok()).filter(|e|e.file_name().to_string_lossy().starts_with(".ainative-backup-")).count();
    let temporary=fs::read_dir(root)?.filter_map(|e|e.ok()).filter(|e|e.file_name().to_string_lossy().starts_with(".ainative-save-")).count();
    if backups!=0||temporary!=0{return Err(std::io::Error::other("App user-folder backup pollution"));}
    let data_root=fs::read_dir(root.join("data-v1"))?.filter_map(|e|e.ok()).map(|e|e.path()).find(|p|p.join("history.sqlite").is_file()).ok_or_else(||std::io::Error::other("App data root absent"))?;
    let archive=data_root.join("pdf-backups");let mut archived=vec![];
    for group in fs::read_dir(&archive)?{for e in fs::read_dir(group?.path())?{archived.push(e?.path());}}
    if archived.len()!=2{return Err(std::io::Error::other("App archive retention failed"));}
    let commits=fs::read_dir(root)?.filter_map(|e|e.ok()).filter(|e|e.file_name().to_string_lossy().ends_with("-output.pdf")).count();
    if commits!=10{return Err(std::io::Error::other("App expected exactly ten native original-save calls"));}
    for dir in &archived{
        let intent:serde_json::Value=serde_json::from_slice(&fs::read(dir.join("retention-v1.json"))?)?;
        let old=fs::read(dir.join("verified-old.pdf"))?;
        if crate::save_core::hash(&old)!=intent["old_hash"]||!original.starts_with(&old)||dir.join("verified-new.pdf").exists(){return Err(std::io::Error::other("App retained backup invalid"));}
    }
    let copy=fs::read(root.join("exports/export-00.pdf"))?;
    if !copy.starts_with(&original)||copy.len()<=original.len(){return Err(std::io::Error::other("App Save As invalid"));}
    report["saveAsBytes"]=copy.len().into();report["saveAsSha256"]=crate::save_core::hash(&copy).into();
    report["nativeBytes"]=original.len().into();report["nativeSha256"]=crate::save_core::hash(&original).into();report["backupCount"]=archived.len().into();report["userFolderBackupCount"]=backups.into();
    if report["inlineRows"].as_array().is_none_or(|r|r.len()!=10||r.iter().any(|v|v["status"]!="PASS")){return Err(std::io::Error::other("App inline repetition incomplete"));}
    report["temporaryTransactionCount"]=temporary.into();
    report["originalCommitCalls"]=commits.into();
    report["runtime"]="production-App-real-Wry-Release-IPC-and-files".into();report["labBinaryMarker"]=LAB_BINARY_MARKER.into();Ok(())
}
fn validate_ink_report(root:&Path,report:&mut serde_json::Value)->std::io::Result<()>{
    if report["scope"]!="ink-only"||report["inkDirty"]!="stroke/undo/redo/delete PASS"||report["inkEvidence"].as_array().is_none_or(|rows|rows.len()<5){return Err(std::io::Error::other("incomplete isolated Ink evidence"));}
    if fs::read(root.join("plain.pdf"))?!=app_source()?{return Err(std::io::Error::other("isolated Ink test changed the PDF"));}
    report["runtime"]="production-App-real-Wry-Release-ink-only".into();Ok(())
}
fn validate_ipc_report(root:&Path,report:&mut serde_json::Value)->std::io::Result<()>{
    let rows=report["rows"].as_array().ok_or_else(||std::io::Error::other("missing replay rows"))?;
    if report["scope"]!="ipc-replay"||rows.len()!=6||rows.iter().any(|r|r["status"]!="PASS"){return Err(std::io::Error::other("incomplete replay"));}
    for index in 0..6{if crate::save_core::hash(&fs::read(root.join(format!("ipc-{index}.pdf")))?)!="a4c3c7fcee7f0c65cd0d4bc15821f224cd40151fff8158778d403b086268538e"{return Err(std::io::Error::other("replay disk bytes differ"));}}
    let captures=fs::read_dir(root)?.filter_map(|e|e.ok()).filter(|e|e.file_name().to_string_lossy().ends_with("-output.pdf")).count();
    if captures!=6||fs::read_dir(root)?.filter_map(|e|e.ok()).any(|e|e.file_name().to_string_lossy().starts_with(".ainative-save-")){return Err(std::io::Error::other("replay commit count or residual differs"));}
    report["runtime"]="real-Wry-Release-identical-bytes-IPC-replay".into();report["labBinaryMarker"]=LAB_BINARY_MARKER.into();Ok(())
}
pub(crate) fn run(){
    let arguments=std::env::args().skip(1).collect::<Vec<_>>();let app_mode=arguments==["--save-app-lab"];
    let ipc_mode=arguments==["--save-ipc-lab"];
    let ink_only=app_mode&&std::env::var("SAVE_APP_INK_ONLY").as_deref()==Ok("1");
    if cfg!(debug_assertions)||tauri::is_dev()||(!app_mode&&!ipc_mode&&arguments!=["--save-chain-lab"]){std::process::exit(64);}
    let project=crate::save_core::project();
    let expected=project.join(".cache/tauri-target/release/ai-native-pdf-reader.exe");
    if std::env::current_exe().ok().and_then(|p|p.canonicalize().ok())!=expected.canonicalize().ok(){std::process::exit(64);}
    // Pin the existing project ancestry before creating even disposable files.
    // Pins stay alive through the complete hidden-runtime session.
    let mut ancestry_pins=vec![];
    if crate::unicode_worker::pin_chain(&project.join("tests/work"),&mut ancestry_pins).is_err(){std::process::exit(65);}
    let base=project.join("tests/work/save-chain-release");
    if fs::create_dir_all(&base).is_err()||crate::unicode_worker::pin_chain(&base,&mut ancestry_pins).is_err(){std::process::exit(65);}
    let root=base.join(format!("run-{}",uuid::Uuid::new_v4()));
    if fs::create_dir_all(root.join("exports")).is_err(){std::process::exit(65);}
    // The new disposable run stays protected against rename/reparse replacement,
    // but its children must be renameable for the REAL backup lifecycle. Holding
    // the old read-only-sharing root handle would artificially block housekeeping.
    let root_pin=match OpenOptions::new().access_mode(0x81).share_mode(1|2).custom_flags(0x02200000).open(&root){Ok(f)if f.metadata().is_ok_and(|m|m.file_attributes()&0x400==0)=>f,_=>std::process::exit(65)};
    ancestry_pins.push(root_pin);
    let mut preparation_stage="synthetic-source";
    let prepared=(||->std::result::Result<_,Box<dyn std::error::Error>>{
        let mut source_pins=vec![];crate::unicode_worker::pin_chain(&project.join("tests/work/writer/annotations"),&mut source_pins)?;
        let source=fs::read(project.join("tests/work/writer/annotations/mixed-source.pdf"))?;
        if crate::save_core::hash(&source)!=SOURCE_HASH{return Err("fixed synthetic source hash mismatch".into());}
        preparation_stage="synthetic-grants";
        let files=Arc::new(crate::files::Files::default());
        let mut inputs=BTreeMap::<String,_>::new();
        for case in CASES{
            let path=root.join(format!("source-{case}.pdf"));
            let mut target=OpenOptions::new().write(true).create_new(true).open(&path)?;target.write_all(&source)?;target.sync_all()?;drop(target);
            inputs.insert(case.to_owned(),files.authorize_at("main",&path,files.epoch())?);
        }
        if app_mode{
            let plain=app_source()?;
            let mut target=OpenOptions::new().write(true).create_new(true).open(root.join("plain.pdf"))?;target.write_all(&plain)?;target.sync_all()?;
            drop(target);inputs.insert("app".into(),files.authorize_at("main",&root.join("plain.pdf"),files.epoch())?);
        }
        preparation_stage="lab-assets";
        let mut extras=BTreeMap::new();let web=project.join(".cache/save-chain-web");regular_tree(&web,&web,&mut extras)?;
        if ipc_mode{
            let source=extras.get("/__save_chain_lab/ipc-old.pdf").ok_or("replay source missing")?;
            if crate::save_core::hash(source)!="080e96270e2fc153fddca2ec7f4fda201f6f5d6e79a42f5d65d5630731e5047a"{return Err("replay source differs".into());}
            for index in 0..6{let path=root.join(format!("ipc-{index}.pdf"));let mut target=OpenOptions::new().write(true).create_new(true).open(&path)?;target.write_all(source)?;target.sync_all()?;drop(target);inputs.insert(format!("ipc-{index}"),files.authorize_at("main",&path,files.epoch())?);}
        }
        let html=extras.remove(if ipc_mode{"/tests/headless/save-ipc.html"}else if app_mode{"/tests/headless/save-app.html"}else{"/tests/headless/save-chain.html"}).ok_or("lab HTML missing")?;extras.insert("/index.html".into(),html);
        preparation_stage="release-acl";
        let mut context:tauri::Context<tauri::Wry>=tauri::generate_context!();
        // Use the generated Release authority unchanged, not handwritten grants.
        let authority=context.runtime_authority_mut();
        for command in ["check_unicode_writer","begin_unicode_export","finish_unicode_export","cancel_unicode_export","save_original_pdf","pending_pdf_recovery","export_pdf_recovery","begin_pdf_read","read_pdf","cancel_pdf_read","release_pdf"]{
            if authority.resolve_access(command,"main","main",&tauri::ipc::Origin::Local).is_none(){return Err("production reader ACL missing".into());}
            if authority.resolve_access(command,"other","other",&tauri::ipc::Origin::Local).is_some()||authority.resolve_access(command,"main","main",&tauri::ipc::Origin::Remote{url:"https://example.invalid/".parse()?}).is_some(){return Err("production reader ACL too broad".into());}
        }
        let original=context.set_assets(Box::new(EmptyAssets));context.set_assets(Box::new(LabAssets{original,files:extras}));
        Ok((files,serde_json::to_string(&inputs)?,context))
    })();
    let (files,inputs,context)=match prepared{Ok(v)=>v,Err(error)=>{let windows_error=error.downcast_ref::<std::io::Error>().and_then(|e|e.raw_os_error());let _=publish(&root,&serde_json::json!({"status":"FAIL","stage":preparation_stage,"windowsError":windows_error}));std::process::exit(65);}};
    let setup_root=root.clone();let result_root=root.clone();let timeout_root=root.clone();
    let builder=tauri::Builder::default().setup(move|app|{
        let data=crate::data::DataService::open(&setup_root.join("data-manager"),&setup_root.join("data-v1"));
        app.manage(crate::Backend{folders:crate::workspace_access::Workspaces::default(),files:files.clone(),settings:Mutex::new(crate::settings::Settings::load(setup_root.join("settings"))),data:Arc::new(Mutex::new(data)),ai:Arc::new(Mutex::new(crate::ai::Service::default())),pending:Arc::new(crate::launch::Pending::default()),writer:Arc::new(crate::pdf_export::Service::new(crate::unicode_worker::prototype()))});
        app.manage(Lab{root:setup_root.clone(),exports:AtomicUsize::new(0),app:app_mode||ipc_mode});
        let script=format!("Object.defineProperty(globalThis,'__SAVE_CHAIN_INPUTS__',{{value:{inputs},writable:false,configurable:false}});Object.defineProperty(globalThis,'__SAVE_APP_INK_ONLY__',{{value:{ink_only},writable:false}});");
        let report_parts=Mutex::new((0usize,String::new()));
        tauri::WebviewWindowBuilder::from_config(app,&app.config().app.windows[0])?
            .visible(false).focused(false).skip_taskbar(true).data_directory(setup_root.join("webview-profile"))
            .additional_browser_args("--disable-background-networking --disable-sync --no-first-run --no-default-browser-check --disable-extensions")
            .initialization_script(script).on_navigation(crate::reader_url)
            .on_new_window(|_,_|tauri::webview::NewWindowResponse::Deny)
            .on_document_title_changed(move|window,title|{
                if let Some(part)=title.strip_prefix("SAVE_CHAIN_PART:"){
                    if let Some((index,data))=part.split_once(':'){
                        if let Ok(index)=index.parse::<usize>(){let mut parts=report_parts.lock().unwrap();
                            if index==0{*parts=(0,String::new());}
                            if index==parts.0&&parts.1.len()+data.len()<=32768{parts.0+=1;parts.1.push_str(data);}
                        }
                    }return;
                }
                let assembled;
                let raw=if title=="SAVE_CHAIN_END"{assembled=report_parts.lock().unwrap().1.clone();assembled.as_str()}else if let Some(raw)=title.strip_prefix("SAVE_CHAIN_RESULT:"){raw}else{return;};
                let mut report=if raw.len()<=32768{serde_json::from_str::<serde_json::Value>(raw).ok()}else{None}.unwrap_or_else(||serde_json::json!({"status":"FAIL","stage":"result-protocol"}));
                if report["status"]=="PASS"{
                    let validation=if ipc_mode{validate_ipc_report(&result_root,&mut report)}else if ink_only{validate_ink_report(&result_root,&mut report)}else if app_mode{validate_app_report(&result_root,&mut report)}else{validate_report(&result_root,&mut report)};
                    if validation.is_err(){report=serde_json::json!({"status":"FAIL","stage":"native-output-validation","browserReport":report});}
                }
                let status=report["status"]=="PASS";
                let saved=publish(&result_root,&report).is_ok();
                window.state::<crate::Backend>().files.shutdown();window.app_handle().exit(if status&&saved{0}else{1});
            }).build()?;
        let handle=app.handle().clone();std::thread::spawn(move||{std::thread::sleep(Duration::from_secs(150));let _=publish(&timeout_root,&serde_json::json!({"status":"FAIL","stage":"timeout"}));handle.exit(124);});
        Ok(())
    }).invoke_handler(reader_handler!());
    match builder.build(context){Ok(app)=>app.run(|app,event|{if matches!(event,tauri::RunEvent::Exit){app.state::<crate::Backend>().writer.close();app.state::<crate::Backend>().files.shutdown();}}),Err(_)=>{let _=publish(&root,&serde_json::json!({"status":"FAIL","stage":"Wry-startup"}));std::process::exit(66);}}
}
