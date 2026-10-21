//! Final two bounded coordination routes. Test-only, no production write entry.
use crate::save_core::*;
use std::{fs::{self,File,OpenOptions},io::{self,Write},path::{Path,PathBuf},process::Command};
use std::os::windows::fs::OpenOptionsExt;
use windows_sys::Win32::Storage::FileSystem::*;
#[derive(Clone,Copy)] enum Route { StrictReplace, OwnedRename }
fn mark(point:&str,fault:&str)->io::Result<()> {
    if point==fault { if std::env::var("COORD_EXIT").as_deref()==Ok("1"){std::process::exit(73);} return Err(io::Error::other(point.to_owned())); } Ok(())
}
fn save(root:&Path,expected:&Identity,data:&[u8],route:Route,fault:&str,mut hook:impl FnMut(&str))->io::Result<()> {
    if !valid_pdf(data){return Err(io::Error::other("invalid lab PDF"));}
    let pins=pin(root)?;let mut source=locked(&root.join("original.pdf"))?;
    if &identity(&mut source)?!=expected{return Err(io::Error::other("external change"));}
    let mut pending=OpenOptions::new().write(true).access_mode(READ|WRITE|DELETE).share_mode(FILE_SHARE_READ).create_new(true).open(root.join("pending.pdf"))?;
    pending.write_all(&data[..data.len()/2])?; mark("partial",fault)?;
    pending.write_all(&data[data.len()/2..])?;pending.sync_all()?;
    if bytes(&mut pending)?!=data{return Err(io::Error::other("output mismatch"));}
    let old=bytes(&mut source)?;
    let mut backup=OpenOptions::new().read(true).write(true).share_mode(FILE_SHARE_READ).create_new(true).open(root.join("verified-old.pdf"))?;
    backup.write_all(&old)?;backup.sync_all()?;if bytes(&mut backup)?!=old{return Err(io::Error::other("backup mismatch"));}
    let mut journal=OpenOptions::new().write(true).create_new(true).open(root.join("coord.json"))?;
    journal.write_all(serde_json::json!({"old":expected,"newHash":hash(data)}).to_string().as_bytes())?;journal.sync_all()?;
    mark("prepared",fault)?;
    if &identity(&mut source)?!=expected{return Err(io::Error::other("source changed"));}
    hook("checked");
    match route {
        Route::StrictReplace=>replace_handle(&pending,&pins,true)?,
        Route::OwnedRename=>{
            rename_noreplace(&source,&pins,"owned-old.pdf")?;
            hook("gap");mark("gap",fault)?;
            // Never replace an object arriving at the old name. The strict source
            // handle remains held, now at owned-old.pdf, throughout publication.
            rename_noreplace(&pending,&pins,"original.pdf")?;
        }
    }
    hook("published");mark("published",fault)?;
    if bytes(&mut pending)?!=data{return Err(io::Error::other("published mismatch"));}
    let mut done=OpenOptions::new().write(true).create_new(true).open(root.join("done.json"))?;
    done.write_all(hash(data).as_bytes())?;done.sync_all()?;Ok(())
}
fn recover(root:&Path,expected:&Identity)->io::Result<()> {
    let pins=pin(root)?;let mut old=locked(&root.join("owned-old.pdf"))?;
    if &identity(&mut old)?!=expected{return Err(io::Error::other("recovery mismatch"));}
    rename_noreplace(&old,&pins,"original.pdf")
}
fn control()->Vec<u8>{fs::read(project().join("tests/fixtures/g1/freetext-control.pdf")).unwrap()}
fn next()->Vec<u8>{let mut v=control();v.extend_from_slice(b"\n% final bounded save\n");v}
fn external()->Vec<u8>{let mut v=control();v.extend_from_slice(b"\n% external writer\n");v}
fn setup(label:&str)->(PathBuf,Identity){let root=project().join("tests/work/m3").join(format!("coord-{label}-{}",uuid::Uuid::new_v4()));fs::create_dir_all(&root).unwrap();fs::write(root.join("original.pdf"),control()).unwrap();let id=identity(&mut File::open(root.join("original.pdf")).unwrap()).unwrap();(root,id)}
fn child(root:&Path,operation:&str)->std::process::Output{Command::new(std::env::current_exe().unwrap()).args(["--exact","save_coord::actor","--nocapture"]).env("COORD_ROOT",root).env("COORD_OP",operation).output().unwrap()}
#[test] fn actor(){let Ok(r)=std::env::var("COORD_ROOT")else{return;};let root=PathBuf::from(r);allowed(&root).unwrap();match std::env::var("COORD_OP").unwrap().as_str(){
    "locked"=>{assert!(OpenOptions::new().write(true).open(root.join("original.pdf")).is_err());assert!(fs::rename(root.join("original.pdf"),root.join("external-old.pdf")).is_err());},
    "create"=>{let mut f=OpenOptions::new().write(true).create_new(true).open(root.join("original.pdf")).unwrap();f.write_all(&external()).unwrap();f.sync_all().unwrap();},
    "exit"=>{let id=identity(&mut File::open(root.join("original.pdf")).unwrap()).unwrap();let _=save(&root,&id,&next(),Route::OwnedRename,&std::env::var("COORD_POINT").unwrap(),|_|{});panic!("exit point missed");},_=>panic!("unknown operation")}}
#[test] fn strict_replacement_fails_closed_and_external_process_cannot_write(){let(root,id)=setup("strict");let result=save(&root,&id,&next(),Route::StrictReplace,"",|p|if p=="checked"{assert!(child(&root,"locked").status.success());});assert!(result.is_err());assert_eq!(result.as_ref().unwrap_err().raw_os_error(),Some(32));assert_eq!(fs::read(root.join("original.pdf")).unwrap(),control());assert!(!root.join("done.json").exists());fs::write(root.join("evidence.json"),serde_json::json!({"route":"strict-posix","osError":32,"externalWriteDenied":true,"published":false,"ctrlSPass":false}).to_string()).unwrap();}
#[test] fn owned_rename_refuses_actual_competing_target_and_recovery_does_not_overwrite(){let(root,id)=setup("race");let result=save(&root,&id,&next(),Route::OwnedRename,"",|p|{if p=="checked"{assert!(child(&root,"locked").status.success());}if p=="gap"{assert!(child(&root,"create").status.success());}});assert!(result.is_err());assert_eq!(fs::read(root.join("original.pdf")).unwrap(),external());assert_eq!(fs::read(root.join("owned-old.pdf")).unwrap(),control());assert_eq!(fs::read(root.join("pending.pdf")).unwrap(),next());assert!(recover(&root,&id).is_err());assert_eq!(fs::read(root.join("original.pdf")).unwrap(),external());assert!(!root.join("done.json").exists());fs::write(root.join("evidence.json"),serde_json::json!({"route":"owned-noreplace","publishError":result.unwrap_err().raw_os_error(),"externalPreserved":true,"recoveryRefusedOccupiedTarget":true,"continuousPathPass":false}).to_string()).unwrap();}
#[test]
fn actual_process_exit_and_explicit_recovery() {
    for point in ["partial", "prepared", "gap", "published"] {
        let (root, id) = setup(point);
        let result = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "save_coord::actor"])
            .env("COORD_ROOT", &root).env("COORD_OP", "exit")
            .env("COORD_EXIT", "1").env("COORD_POINT", point).output().unwrap();
        assert_eq!(result.status.code(), Some(73));
        assert!(!root.join("done.json").exists());
        if point != "partial" {
            assert_eq!(fs::read(root.join("verified-old.pdf")).unwrap(), control());
        }
        if matches!(point, "gap" | "published") {
            assert_eq!(fs::read(root.join("owned-old.pdf")).unwrap(), control());
        }
        if point == "gap" {
            assert!(!root.join("original.pdf").exists());
            recover(&root, &id).unwrap();
            assert_eq!(fs::read(root.join("original.pdf")).unwrap(), control());
        } else {
            assert_eq!(fs::read(root.join("original.pdf")).unwrap(),
                if point == "published" { next() } else { control() });
        }
        fs::write(root.join("evidence.json"), serde_json::json!({
            "exit": point, "exitCode": 73, "oldMaterialVerified": true,
            "recoveredMissingPath": point == "gap"
        }).to_string()).unwrap();
    }
}
#[test] fn raw_path_errors_are_timed_and_phase_classified(){use std::sync::{Arc,Mutex,atomic::{AtomicBool,AtomicU8,Ordering}};use std::time::Instant;
    for owned in [false,true]{let(root,_)=setup(if owned{"continuous-owned"}else{"continuous-win32"});let stage=Arc::new(AtomicU8::new(0));let stop=Arc::new(AtomicBool::new(false));let errors=Arc::new(Mutex::new(std::collections::BTreeMap::<String,u64>::new()));let s=stage.clone();let t=stop.clone();let e=errors.clone();let p=root.clone();let start=Instant::now();
        let monitor=std::thread::spawn(move||{let mut ok=0;let mut first=None;let mut last=None;while !t.load(Ordering::Acquire){match OpenOptions::new().read(true).share_mode(FILE_SHARE_READ|FILE_SHARE_WRITE|FILE_SHARE_DELETE).open(p.join("original.pdf")){Ok(_)=>ok+=1,Err(error)=>{let time=start.elapsed().as_micros();first.get_or_insert(time);last=Some(time);*e.lock().unwrap().entry(format!("phase{}-code{}",s.load(Ordering::Acquire),error.raw_os_error().unwrap_or(-1))).or_default()+=1;}}std::thread::yield_now();}(ok,first,last)});
        for round in 0..20{let id=identity(&mut File::open(root.join("original.pdf")).unwrap()).unwrap();let update=if round%2==0{next()}else{control()};stage.store(0,Ordering::Release);if owned{save(&root,&id,&update,Route::OwnedRename,"",|p|{stage.store(match p{"gap"=>2,"published"=>3,_=>1},Ordering::Release);}).unwrap();}else{replace_candidate(&root,&id,&update,ReplaceMode::Win32,true,"",|p|stage.store(match p{"replacing"=>2,"replaced"=>3,_=>1},Ordering::Release)).unwrap();}stage.store(4,Ordering::Release);for name in if owned{vec!["owned-old.pdf","verified-old.pdf","coord.json","done.json"]}else{vec!["original.backup.pdf","verified-old.pdf","replacement.json","completed.json"]}{fs::rename(root.join(name),root.join(format!("round-{round}-{name}"))).unwrap();}}
        stop.store(true,Ordering::Release);let(ok,first,last)=monitor.join().unwrap();assert!(ok>0);let histogram=errors.lock().unwrap();let value=serde_json::json!({"route":if owned{"owned-noreplace"}else{"win32-baseline"},"rounds":20,"successfulOpens":ok,"rawErrorHistogram":*histogram,"firstErrorMicroseconds":first,"lastErrorMicroseconds":last,"elapsedMicroseconds":start.elapsed().as_micros(),"phaseLegend":{"0":"prepare","1":"checked","2":"API/gap","3":"published","4":"material archive"},"note":"stage sampled after open; boundary observations may straddle phase"});println!("{value}");fs::write(root.join("evidence.json"),value.to_string()).unwrap();}
}
