//! Windows-only, test-only bounded save experiment. Never linked into the app.
//! No IPC, no user files, no ReplaceFileW last-check race and no destructive cleanup.
use std::{fs::{self, File, OpenOptions}, io::{self, Write}, path::{Path, PathBuf}};
use std::os::windows::fs::OpenOptionsExt;
use windows_sys::Win32::Storage::FileSystem::*;

use crate::save_core::*;
#[derive(serde::Serialize, serde::Deserialize)]
struct Journal { version:u8, old:Identity, new_hash:String }
fn record(file:&mut File,value:&serde_json::Value)->io::Result<()> { writeln!(file,"{value}")?; file.sync_all() }
fn stage(point:&str,wanted:&str)->io::Result<()> {
    if point!=wanted{return Ok(());}
    if std::env::var("M3_LAB_EXIT").as_deref()==Ok("1"){std::process::exit(71);}
    Err(io::Error::other(format!("injected {point}")))
}
fn save(root:&Path,expected:&Identity,output:&[u8],fault:&str,mut hook:impl FnMut(&str))->io::Result<()> {
    if !valid_pdf(output){return Err(io::Error::other("invalid writer bytes"));}
    let pins=pin(root)?; let mut source=locked(&root.join("original.pdf"))?;
    if &identity(&mut source)?!=expected {return Err(io::Error::other("external change"));}
    hook("locked"); // A competing write/rename here must be refused by Windows.
    let mut temporary=OpenOptions::new().write(true).access_mode(READ|WRITE|DELETE).share_mode(FILE_SHARE_READ).create_new(true).open(root.join("pending.pdf"))?;
    temporary.write_all(&output[..output.len()/2])?;
    if fault=="disk-full"{return Err(io::Error::from_raw_os_error(112));}
    stage("partial",fault)?; temporary.write_all(&output[output.len()/2..])?; stage("write",fault)?;
    if fault=="flush-error"{return Err(io::Error::from_raw_os_error(5));} // Stubbed call failure, no OS permission change.
    temporary.sync_all()?; stage("flush",fault)?;
    if bytes(&mut temporary)?!=output{return Err(io::Error::other("output validation"));}
    let journal=Journal{version:1,old:expected.clone(),new_hash:hash(output)};
    let mut log=OpenOptions::new().write(true).create_new(true).open(root.join("transaction.jsonl"))?;
    record(&mut log,&serde_json::to_value(journal)?)?; stage("prepared",fault)?; stage("cancel",fault)?;
    rename_noreplace(&source,&pins,"original.backup.pdf")?;
    // Never truncate the old PDF. It remains intact under its controlled backup name.
    stage("backup",fault)?; hook("backup");
    record(&mut log,&serde_json::json!({"stage":"backup"}))?;
    rename_noreplace(&temporary,&pins,"original.pdf")?; stage("publish",fault)?;
    record(&mut log,&serde_json::json!({"stage":"published"}))?; stage("journal",fault)?;
    // Backup/journal retained. Directory durability, ACL/ADS/EFS and power loss are NOT proved.
    Ok(())
}
#[derive(Debug,PartialEq)]
enum Recovery { Old, New, Restored, Conflict }
fn recover(root:&Path)->io::Result<Recovery> {
    let pins=pin(root)?;
    let journal_path=root.join("transaction.jsonl");
    if !journal_path.exists(){return Ok(Recovery::Old);}
    let mut journal_file=locked(&journal_path)?; // Reject a reparse leaf before reading journal bytes.
    let data=bytes(&mut journal_file)?;
    let first=data.split(|c|*c==b'\n').next().unwrap_or_default();
    let journal:Journal=serde_json::from_slice(first)?;
    if journal.version!=1{return Err(io::Error::other("unknown journal"));}
    let original=root.join("original.pdf");
    if original.exists(){
        let mut file=locked(&original)?;let actual=identity(&mut file)?;
        return Ok(if actual==journal.old{Recovery::Old}else if actual.hash==journal.new_hash&&valid_pdf(&bytes(&mut file)?){Recovery::New}else{Recovery::Conflict});
    }
    let mut backup=locked(&root.join("original.backup.pdf"))?;
    if identity(&mut backup)?!=journal.old{return Err(io::Error::other("backup changed"));}
    // Explicit lab recovery; refuses a concurrently-created replacement at original path.
    rename_noreplace(&backup,&pins,"original.pdf")?;
    Ok(Recovery::Restored)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture()->Vec<u8>{fs::read(project().join("tests/fixtures/g1/freetext-control.pdf")).unwrap()}
    fn new_output()->Vec<u8>{let mut v=fixture();v.extend_from_slice(b"\n% M3 controlled valid trailing comment\n");v}
    fn setup(label:&str)->(PathBuf,Identity){
        let root=project().join("tests/work/m3").join(format!("save-{label}-{}",uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();fs::write(root.join("original.pdf"),fixture()).unwrap();
        let expected=identity(&mut File::open(root.join("original.pdf")).unwrap()).unwrap();(root,expected)
    }
    #[test] fn successful_publish_retains_exact_old_and_new_bytes(){
        let(root,old)=setup("success");let output=new_output();save(&root,&old,&output,"", |_|{}).unwrap();
        assert_eq!(fs::read(root.join("original.pdf")).unwrap(),output);assert_eq!(fs::read(root.join("original.backup.pdf")).unwrap(),fixture());
        assert_eq!(recover(&root).unwrap(),Recovery::New);
    }
    #[test] fn invalid_output_external_change_and_same_bytes_replacement_are_refused(){
        let(root,old)=setup("conflict");assert!(save(&root,&old,b"not a PDF","", |_|{}).is_err());
        let output=new_output();fs::write(root.join("original.pdf"),&output).unwrap();
        assert!(save(&root,&old,&output,"", |_|{}).is_err());assert_eq!(fs::read(root.join("original.pdf")).unwrap(),output);
        fs::rename(root.join("original.pdf"),root.join("external.pdf")).unwrap();fs::write(root.join("original.pdf"),fixture()).unwrap();
        assert!(save(&root,&old,&output,"", |_|{}).is_err());assert!(!root.join("pending.pdf").exists());
    }
    #[test] fn occupied_readonly_and_postcheck_races_do_not_overwrite(){
        let(root,old)=setup("lock");let lock=OpenOptions::new().read(true).share_mode(FILE_SHARE_READ).open(root.join("original.pdf")).unwrap();
        assert!(save(&root,&old,&new_output(),"", |_|{}).is_err());drop(lock);
        let mut permissions=fs::metadata(root.join("original.pdf")).unwrap().permissions();permissions.set_readonly(true);fs::set_permissions(root.join("original.pdf"),permissions).unwrap();
        assert!(save(&root,&old,&new_output(),"", |_|{}).is_err());
        let mut permissions=fs::metadata(root.join("original.pdf")).unwrap().permissions();permissions.set_readonly(false);fs::set_permissions(root.join("original.pdf"),permissions).unwrap();
        save(&root,&old,&new_output(),"",|point|if point=="locked"{
            assert!(OpenOptions::new().write(true).open(root.join("original.pdf")).is_err());
            assert!(fs::rename(root.join("original.pdf"),root.join("competitor.pdf")).is_err());
            assert!(fs::rename(&root,root.with_file_name("must-not-move")).is_err());
        }).unwrap();
    }
    #[test] fn original_gap_conflict_and_recovery_never_clobber_external_data(){
        let(root,old)=setup("gap");
        assert!(save(&root,&old,&new_output(),"",|point|if point=="backup"{fs::write(root.join("original.pdf"),b"external data").unwrap();}).is_err());
        assert_eq!(recover(&root).unwrap(),Recovery::Conflict);assert_eq!(fs::read(root.join("original.pdf")).unwrap(),b"external data");
        assert_eq!(fs::read(root.join("original.backup.pdf")).unwrap(),fixture());assert_eq!(fs::read(root.join("pending.pdf")).unwrap(),new_output());
    }
    #[test] fn injected_space_and_flush_errors_preserve_original(){
        for point in ["disk-full","flush-error","cancel","partial","write","flush","prepared","backup","publish","journal"]{
            let(root,old)=setup(point);let failure=save(&root,&old,&new_output(),point, |_|{}).unwrap_err();
            if point=="disk-full"{assert_eq!(failure.raw_os_error(),Some(112));}else if point=="flush-error"{assert_eq!(failure.raw_os_error(),Some(5));}else{assert_eq!(failure.to_string(),format!("injected {point}"));}
            let status=recover(&root).unwrap();assert!(matches!(status,Recovery::Old|Recovery::New|Recovery::Restored));
            let recovered=fs::read(root.join("original.pdf")).unwrap();assert!(recovered==fixture()||recovered==new_output());
        }
    }
    #[test] fn actual_child_exit_at_each_stage_recovers_complete_document(){
        for point in ["partial","write","flush","prepared","backup","publish","journal"]{
            let(root,_)=setup(&format!("exit-{point}"));
            let status=std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact","save_lab::tests::fault_child","--nocapture"])
                .env("M3_LAB_ROOT",&root).env("M3_LAB_POINT",point).env("M3_LAB_EXIT","1").output().unwrap();
            assert_eq!(status.status.code(),Some(71),"{point}: {}",String::from_utf8_lossy(&status.stdout));
            let result=recover(&root).unwrap();let recovered=fs::read(root.join("original.pdf")).unwrap();
            assert!(recovered==fixture()||recovered==new_output());println!("{point}: {result:?} {}",root.display());
        }
    }
    #[test] fn fault_child(){
        let Ok(root)=std::env::var("M3_LAB_ROOT") else{return;};let root=PathBuf::from(root);allowed(&root).unwrap();
        let expected=identity(&mut File::open(root.join("original.pdf")).unwrap()).unwrap();
        save(&root,&expected,&new_output(),&std::env::var("M3_LAB_POINT").unwrap(), |_|{}).unwrap();
    }
    #[test] fn broken_journal_preserves_recovery_materials(){
        let(root,old)=setup("journal-broken");assert!(save(&root,&old,&new_output(),"backup", |_|{}).is_err());
        let before=fs::read(root.join("original.backup.pdf")).unwrap();fs::write(root.join("transaction.jsonl"),b"broken").unwrap();
        assert!(recover(&root).is_err());assert_eq!(fs::read(root.join("original.backup.pdf")).unwrap(),before);assert!(!root.join("original.pdf").exists());
    }
}
