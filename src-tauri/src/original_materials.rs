//! Housekeeping AFTER verified disk commit, fresh grant and history receipt.
//! Only newly enrolled backups are rotated. Legacy save/recovery dirs are never
//! swept. Cleanup failure is not a failed PDF save and preserves extra material.
use super::*;
use std::{collections::BTreeMap,os::windows::{ffi::OsStrExt,fs::MetadataExt}};
use windows_sys::Win32::Storage::FileSystem::{SetFileInformationByHandle,FileDispositionInfo,FILE_DISPOSITION_INFO,FileRenameInfo,FILE_RENAME_INFO};
const MARKER:&str="retention-v1.json";
fn pin_parent(path:&Path)->std::result::Result<Vec<File>,AppError>{
    let mut pins=vec![];
    for dir in path.ancestors().collect::<Vec<_>>().into_iter().rev(){
        // Keep ancestors unrenameable (NO delete sharing), but allow directory
        // entry updates needed by our own archive rename. No ACL/access grant
        // changes; child files still deny write/delete sharing independently.
        let file=OpenOptions::new().access_mode(0x81).share_mode(1|2).custom_flags(0x0220_0000).open(dir).map_err(|_|refusal())?;
        if file.metadata().map_err(|_|refusal())?.file_attributes()&0x400!=0{return Err(refusal());}pins.push(file);
    }Ok(pins)
}

fn directory(path:&Path)->std::result::Result<File,AppError>{
    let file=OpenOptions::new().access_mode(0x0003_0081).share_mode(1).custom_flags(0x0220_0000).open(path).map_err(|_|refusal())?;
    let attrs=file.metadata().map_err(|_|refusal())?.file_attributes();
    if attrs&0x400!=0||attrs&0x10==0{return Err(refusal());}
    crate::storage::directories::check_private_file(&file,true).map_err(|_|refusal())?;Ok(file)
}
fn deletion_file(dir:&Path,name:&str,expected:&[u8])->std::result::Result<File,AppError>{
    // A handle carrying DELETE, denying write/delete sharing, and opened without
    // following links binds content verification and removal to the same object.
    let mut file=OpenOptions::new().access_mode(0x8001_0000).share_mode(1).custom_flags(0x0020_0000).open(dir.join(name)).map_err(|_|refusal())?;
    safe_material(&file)?;
    if read_limited(&mut file,expected.len())?!=expected{return Err(refusal());}Ok(file)
}
fn rename_directory(file:&File,to:&Path)->std::result::Result<(),AppError>{
    // Rename the already pinned directory itself. Zeroed ReplaceIfExists means
    // a collision is refused; neither an existing path nor its contents change.
    let name:Vec<u16>=to.as_os_str().encode_wide().collect();
    // The UTF-16 terminator must be inside the reported buffer AND inside the
    // allocation. Passing only the name bytes let the kernel read past the end of
    // the buffer, which produced a directory name with random trailing units.
    let name_bytes=(name.len()+1)*2;
    let size=std::mem::offset_of!(FILE_RENAME_INFO,FileName)+name_bytes;
    let units=size.div_ceil(std::mem::size_of::<usize>());
    let mut buffer=vec![0usize;units];
    unsafe{
        let info=buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();
        // FileNameLength counts the name itself, without the terminator.
        (*info).FileNameLength=(name.len()*2)as u32;
        std::ptr::copy_nonoverlapping(name.as_ptr(),(*info).FileName.as_mut_ptr(),name.len());
        // The whole zeroed allocation is handed over, never a byte count that
        // stops short of the bytes that were actually reserved.
        if SetFileInformationByHandle(file.as_raw_handle(),FileRenameInfo,info.cast(),(units*std::mem::size_of::<usize>())as u32)==0{return Err(io_error());}
    }Ok(())
}
fn dispose(file:File)->std::result::Result<(),AppError>{
    let info=FILE_DISPOSITION_INFO{DeleteFile:true};
    if unsafe{SetFileInformationByHandle(file.as_raw_handle(),FileDispositionInfo,(&info as *const FILE_DISPOSITION_INFO).cast(),std::mem::size_of_val(&info)as u32)}==0{#[cfg(test)]eprintln!("archive disposition: {}",std::io::Error::last_os_error());return Err(io_error());}
    drop(file);Ok(())
}
fn verified_backup(dir:&Path)->std::result::Result<(Intent,BTreeMap<String,Vec<u8>>),AppError>{
    let bytes=material_bytes(dir,MARKER,4096)?;
    let intent:Intent=serde_json::from_slice(&bytes).map_err(|_|refusal())?;
    if intent.schema!=1||dir.file_name().and_then(|n|n.to_str())!=Some(format!(".ainative-backup-{}",intent.event_id).as_str())||uuid::Uuid::parse_str(&intent.event_id).is_err()||intent.old_len>crate::files::EXPORT_INPUT_MAX||intent.new_len>crate::files::EXPORT_OUTPUT_MAX{return Err(refusal());}
    let mut members=BTreeMap::new();
    for name in ["intent.json","completed.json","acknowledged.json",MARKER]{
        if material_bytes(dir,name,4096)?!=bytes{return Err(refusal());}members.insert(name.into(),bytes.clone());
    }
    let old=material_bytes(dir,"verified-old.pdf",crate::files::EXPORT_INPUT_MAX)?;
    if old.len()!=intent.old_len||crate::save_core::hash(&old)!=intent.old_hash{return Err(refusal());}
    members.insert("verified-old.pdf".into(),old);
    // A crash immediately after archival can leave the redundant new PDF. It
    // remains safe, and is accepted only if its complete bytes/hash also match.
    for entry in fs::read_dir(dir).map_err(|_|refusal())?{
        let name=entry.map_err(|_|refusal())?.file_name().into_string().map_err(|_|refusal())?;
        if name=="verified-new.pdf"{
            let new=material_bytes(dir,&name,crate::files::EXPORT_OUTPUT_MAX)?;
            if new.len()!=intent.new_len||crate::save_core::hash(&new)!=intent.new_hash||!new.starts_with(&members["verified-old.pdf"]){return Err(refusal());}
            members.insert(name,new);
        }else if !members.contains_key(&name){return Err(refusal());}
    }Ok((intent,members))
}
fn remove_backup(dir:&Path)->std::result::Result<(),AppError>{
    let guard=directory(dir)?;let(_,members)=verified_backup(dir)?;
    // Validate/open EVERY object before the first deletion. Unknown files,
    // hardlinks, junctions, locks or corruption cause refusal and retention.
    let held=members.iter().map(|(name,bytes)|deletion_file(dir,name,bytes)).collect::<std::result::Result<Vec<_>,_>>()?;
    for file in held{dispose(file)?;}dispose(guard)
}
pub(crate) fn retain_completed(outcome:&CommitOutcome,archive_root:&Path,document:&str)->std::result::Result<(),AppError>{
    retain_inner(outcome,archive_root,document,"")
}
fn retain_inner(outcome:&CommitOutcome,archive_root:&Path,document:&str,fault:&str)->std::result::Result<(),AppError>{

    if uuid::Uuid::parse_str(document).is_err()||document.len()!=36{return Err(refusal());}
    let parent=outcome.materials.parent().ok_or_else(refusal)?;
    let _pins=pin_parent(parent)?;
    let _archive_pins=pin_parent(archive_root)?;
    let archive_guard=OpenOptions::new().access_mode(0x0002_0081).share_mode(1|2).custom_flags(0x0220_0000).open(archive_root).map_err(|_|refusal())?;
    crate::storage::directories::check_private_file(&archive_guard,true).map_err(|_|refusal())?;
    let group=archive_root.join(document);
    if !group.try_exists().map_err(|_|refusal())?{drop(crate::storage::directories::create_private_dir(&group).map_err(|_|refusal())?);}
    let _group_pins=pin_parent(&group)?;
    if outcome.materials.file_name().and_then(|n|n.to_str())!=Some(format!(".ainative-save-{}",outcome.receipt.event_id()).as_str()){return Err(refusal());}
    let guard=directory(&outcome.materials)?;
    let bytes=material_bytes(&outcome.materials,"intent.json",4096)?;
    let intent:Intent=serde_json::from_slice(&bytes).map_err(|_|refusal())?;
    if intent.event_id!=outcome.receipt.event_id()||intent.file_key!=outcome.receipt.file_key()||intent.old_hash!=outcome.receipt.old_hash()||intent.new_hash!=outcome.receipt.new_hash(){return Err(refusal());}
    for name in ["completed.json","acknowledged.json"]{if material_bytes(&outcome.materials,name,4096)?!=bytes{return Err(refusal());}}
    // Only this live outcome is enrolled. Old user-folder backups are NEVER
    // enumerated or migrated. Copy/flush/verify works even across volumes.
    let old=material_bytes(&outcome.materials,"verified-old.pdf",crate::files::EXPORT_INPUT_MAX)?;
    let new=material_bytes(&outcome.materials,"verified-new.pdf",crate::files::EXPORT_OUTPUT_MAX)?;
    if old.len()!=intent.old_len||new.len()!=intent.new_len||crate::save_core::hash(&old)!=intent.old_hash||crate::save_core::hash(&new)!=intent.new_hash||!new.starts_with(&old){return Err(refusal());}
    let expected=[("intent.json",bytes.clone()),("completed.json",bytes.clone()),("acknowledged.json",bytes.clone()),("verified-old.pdf",old.clone()),("verified-new.pdf",new)].into_iter().collect::<BTreeMap<_,_>>();
    for e in fs::read_dir(&outcome.materials).map_err(|_|refusal())?{if !expected.contains_key(e.map_err(|_|refusal())?.file_name().to_str().ok_or_else(refusal)?){return Err(refusal());}}
    let archived=group.join(format!(".ainative-backup-{}",intent.event_id));
    let _new_archive=crate::storage::directories::create_private_dir(&archived).map_err(|_|io_error())?;
    drop(write_material(&archived,"verified-old.pdf",&old)?);
    if fault=="after-copy"{return Err(io_error());}
    for name in ["intent.json","completed.json","acknowledged.json",MARKER]{drop(write_material(&archived,name,&bytes)?);}
    verified_backup(&archived)?;
    if fault=="after-verify"{return Err(io_error());}
    drop(_new_archive);
    // Rename the transaction directory BEFORE opening any member. Windows
    // refuses to rename a directory while a handle inside it denies delete
    // sharing, so opening the members first and renaming afterwards can never
    // succeed. Renaming first is also what keeps a partially deleted transaction
    // from being read as an unfinished save: pending() only scans
    // `.ainative-save-`, and the archive above is already complete and verified.
    let cleanup=parent.join(format!(".ainative-cleanup-{}",intent.event_id));
    rename_directory(&guard,&cleanup)?;
    // Re-open each member at its new name and verify the same bytes again before
    // the first deletion, so nothing is removed that was not archived.
    let held=expected.iter().map(|(name,value)|deletion_file(&cleanup,name,value).map(|file|(*name,file))).collect::<std::result::Result<Vec<_>,_>>()?;
    for(name,file)in held{
        dispose(file)?;
        if (fault=="after-delete-ack"&&name=="acknowledged.json")||(fault=="after-delete-intent"&&name=="intent.json"){return Err(io_error());}
    }dispose(guard)?;
    let mut backups=vec![];let mut uncertain=false;
    for entry in fs::read_dir(&group).map_err(|_|refusal())?.take(4096){
        let path=entry.map_err(|_|refusal())?.path();
        if !path.file_name().and_then(|n|n.to_str()).is_some_and(|n|n.starts_with(".ainative-backup-")){continue;}
        let verified=(||{let _guard=directory(&path)?;verified_backup(&path)})();
        match verified{
            Ok((info,_))=>if info.file_key==intent.file_key{backups.push((info.old_len,path));},
            // Preserve failed/incomplete material, but do not let an old
            // interrupted copy disable retention of other verified archives.
            Err(_)=>uncertain=true,
        }
    }
    // Incremental commits strictly increase source length. Keep the two most
    // recent complete source versions for THIS file identity, not its filename.
    backups.sort_by(|a,b|b.0.cmp(&a.0));
    if backups.len()>1&&backups.windows(2).any(|p|p[0].0==p[1].0){return Err(refusal());}
    for(_,path)in backups.into_iter().skip(2){if remove_backup(&path).is_err(){uncertain=true;}}
    if uncertain{Err(refusal())}else{Ok(())}
}

#[cfg(test)]mod tests{
    use super::*;
    fn destination(root:&Path)->(PathBuf,String){let path=root.join("private-recovery");drop(crate::storage::directories::create_private_dir(&path).unwrap());(path,uuid::Uuid::new_v4().to_string())}
    fn archived(root:&Path,doc:&str,outcome:&CommitOutcome)->PathBuf{root.join(doc).join(format!(".ainative-backup-{}",outcome.receipt.event_id()))}
    #[test]fn only_current_acknowledged_transaction_is_archived_legacy_and_incomplete_are_untouched(){
        let(root,files,grant,output)=test_setup();
        let(archive,doc)=destination(&root);
        let legacy=root.join(format!(".ainative-save-{}",uuid::Uuid::new_v4()));fs::create_dir(&legacy).unwrap();fs::write(legacy.join("historical-evidence"),b"retain").unwrap();
        let outcome=commit_approved(&files,"main",&grant.grant_id,files.epoch(),output,&root).unwrap();
        assert!(retain_completed(&outcome,&archive,&doc).is_err());assert!(outcome.materials.join("verified-new.pdf").exists());
        acknowledge(&outcome).unwrap();retain_completed(&outcome,&archive,&doc).unwrap();
        assert!(!outcome.materials.exists());let dir=archived(&archive,&doc,&outcome);
        assert!(!dir.join("verified-new.pdf").exists());assert_eq!(verified_backup(&dir).unwrap().0.old_hash,outcome.receipt.old_hash());
        assert!(!root.join(format!(".ainative-backup-{}",outcome.receipt.event_id())).exists(),"no long-term user-folder backup");
        assert_eq!(verified_backup(&dir).unwrap().0.event_id,outcome.receipt.event_id());
        assert_eq!(fs::read(legacy.join("historical-evidence")).unwrap(),b"retain");
        // A cleanup attempt on the same outcome cannot enroll or consume a
        // different path, nor does it run as a startup sweep.
        assert!(retain_completed(&outcome,&archive,&doc).is_err());assert!(dir.join("verified-old.pdf").exists());
    }
    #[test]fn uncertain_or_unknown_backup_materials_refuse_cleanup_and_keep_original(){
        for variant in ["corrupt","foreign","occupied"]{
            let(root,files,grant,output)=test_setup();let expected=output.bytes.clone();
            let(archive,doc)=destination(&root);
            let outcome=commit_approved(&files,"main",&grant.grant_id,files.epoch(),output,&root).unwrap();acknowledge(&outcome).unwrap();retain_completed(&outcome,&archive,&doc).unwrap();
            let dir=archived(&archive,&doc,&outcome);
            let mut occupied=None;
            match variant{
                "corrupt"=>{let p=dir.join("verified-old.pdf");let mut b=fs::read(&p).unwrap();b[10]^=1;fs::write(p,b).unwrap();},
                "foreign"=>fs::write(dir.join("unrecognized"),b"preserve").unwrap(),
                _=>occupied=Some(OpenOptions::new().read(true).share_mode(1).open(dir.join("verified-old.pdf")).unwrap()),
            }
            assert!(remove_backup(&dir).is_err(),"{variant}");
            assert!(dir.join("verified-old.pdf").exists());assert!(dir.join("retention-v1.json").exists());
            assert_eq!(fs::read(root.join("合成 原件.pdf")).unwrap(),expected);drop(occupied);
        }
    }
    #[test]fn archive_failure_keeps_complete_old_new_and_saved_original(){
        for fault in ["missing-root","after-copy","after-verify"]{
            let(root,files,grant,output)=test_setup();let saved=output.bytes.clone();let old=fs::read(root.join("合成 原件.pdf")).unwrap();
            let(archive,doc)=destination(&root);let outcome=commit_approved(&files,"main",&grant.grant_id,files.epoch(),output,&root).unwrap();acknowledge(&outcome).unwrap();
            let target=if fault=="missing-root"{archive.join("missing")}else{archive.clone()};
            assert!(retain_inner(&outcome,&target,&doc,fault).is_err());
            assert_eq!(fs::read(outcome.materials.join("verified-old.pdf")).unwrap(),old);
            assert_eq!(fs::read(outcome.materials.join("verified-new.pdf")).unwrap(),saved);
            assert_eq!(fs::read(root.join("合成 原件.pdf")).unwrap(),saved);
            if fault=="after-verify"{assert_eq!(verified_backup(&archived(&archive,&doc,&outcome)).unwrap().0.old_hash,outcome.receipt.old_hash());}
        }
    }
    /// Writes an archive that `verified_backup` accepts, with a chosen old length
    /// so rotation order can be exercised without running three real commits.
    /// Content is irrelevant to rotation: only the recorded length and its own
    /// hash have to agree with the marker.
    fn fabricate(group:&Path,file_key:&str,old_len:usize)->PathBuf{
        let event=uuid::Uuid::new_v4().to_string();
        let dir=group.join(format!(".ainative-backup-{event}"));
        drop(crate::storage::directories::create_private_dir(&dir).unwrap());
        let old=vec![b'x';old_len];
        let bytes=serde_json::to_vec(&serde_json::json!({
            "schema":1,"event_id":event,"file_key":file_key,
            "old_hash":crate::save_core::hash(&old),"new_hash":crate::save_core::hash(b"later"),
            "old_len":old.len(),"new_len":old_len+64})).unwrap();
        for name in ["intent.json","completed.json","acknowledged.json",MARKER]{drop(write_material(&dir,name,&bytes).unwrap());}
        drop(write_material(&dir,"verified-old.pdf",&old).unwrap());
        dir
    }
    /// A "cleanup interrupted after the archive was verified" save leaves the
    /// archive intact and the transaction under `.ainative-cleanup-`. The next
    /// save must not see it as an unfinished transaction, or every later Ctrl+S
    /// is refused by a recovery prompt that can never be satisfied.
    #[test]fn interrupted_cleanup_keeps_the_archive_and_never_blocks_the_next_save(){
        for fault in ["after-delete-ack","after-delete-intent"]{
            let(root,files,grant,output)=test_setup();let saved=output.bytes.clone();
            let(archive,doc)=destination(&root);
            let outcome=commit_approved(&files,"main",&grant.grant_id,files.epoch(),output,&root).unwrap();acknowledge(&outcome).unwrap();
            let event=outcome.receipt.event_id().to_owned();
            assert!(retain_inner(&outcome,&archive,&doc,fault).is_err(),"{fault}");
            // The old version is already safely archived and still verifies.
            let kept=archived(&archive,&doc,&outcome);
            assert_eq!(verified_backup(&kept).unwrap().0.old_hash,outcome.receipt.old_hash(),"{fault}");
            assert_eq!(fs::read(root.join("合成 原件.pdf")).unwrap(),saved,"{fault}");
            // Cleanup left a private partial directory, but no `.ainative-save-`
            // that recovery would ever report as unfinished.
            assert!(root.join(format!(".ainative-cleanup-{event}")).exists(),"{fault}");
            let saves=fs::read_dir(&root).unwrap().filter_map(|e|e.ok()).filter(|e|e.file_name().to_string_lossy().starts_with(".ainative-save-")).count();
            assert_eq!(saves,0,"{fault}: an interrupted cleanup still looks like an unfinished save");
            let live=outcome.grant.as_ref().expect("commit registers the new grant").grant_id.clone();
            assert!(pending(&files,"main",&live).unwrap().is_empty(),"{fault}: recovery still reports the cleaned transaction");
        }
    }
    /// Rotation keeps the two most recent complete versions for this document
    /// identity and removes the rest.
    #[test]fn rotation_keeps_the_two_most_recent_verified_archives(){
        let(root,files,grant,output)=test_setup();
        let(archive,doc)=destination(&root);
        let outcome=commit_approved(&files,"main",&grant.grant_id,files.epoch(),output,&root).unwrap();acknowledge(&outcome).unwrap();
        let intent:Intent=serde_json::from_slice(&material_bytes(&outcome.materials,"intent.json",4096).unwrap()).unwrap();
        // The per-document group is created by retention itself; fabricate into
        // an existing group so the fixture matches what retention will read.
        let group=archive.join(&doc);
        drop(crate::storage::directories::create_private_dir(&group).unwrap());
        // Two older versions, both with the same identity and distinct lengths.
        let older=[fabricate(&group,&intent.file_key,intent.old_len-200),fabricate(&group,&intent.file_key,intent.old_len-100)];
        retain_completed(&outcome,&archive,&doc).unwrap();
        let mut remaining=fs::read_dir(archive.join(&doc)).unwrap().filter_map(|e|e.ok()).map(|e|e.path()).filter(|p|p.file_name().is_some_and(|n|n.to_string_lossy().starts_with(".ainative-backup-"))).collect::<Vec<_>>();
        remaining.sort();
        assert_eq!(remaining.len(),2,"rotation must keep exactly two versions");
        assert!(remaining.contains(&archived(&archive,&doc,&outcome)),"the newest version must be kept");
        assert!(remaining.contains(&older[1]),"the second newest version must be kept");
        assert!(!remaining.contains(&older[0]),"the oldest version must be removed");
        assert_eq!(verified_backup(&remaining[0]).unwrap().0.file_key,intent.file_key);
        assert_eq!(verified_backup(&remaining[1]).unwrap().0.file_key,intent.file_key);
    }
    /// A single archive that cannot be verified must be preserved untouched, but
    /// it must not stop the other archives from rotating. Retention is restored
    /// by refusing the pass, not by deleting anything uncertain.
    #[test]fn incomplete_archive_never_blocks_rotation_of_valid_ones(){
        let(root,files,grant,output)=test_setup();
        let(archive,doc)=destination(&root);
        let group=archive.join(&doc);
        drop(crate::storage::directories::create_private_dir(&group).unwrap());
        let outcome=commit_approved(&files,"main",&grant.grant_id,files.epoch(),output,&root).unwrap();acknowledge(&outcome).unwrap();
        let intent:Intent=serde_json::from_slice(&material_bytes(&outcome.materials,"intent.json",4096).unwrap()).unwrap();
        let older=[fabricate(&group,&intent.file_key,intent.old_len-200),fabricate(&group,&intent.file_key,intent.old_len-100)];
        // An interrupted archive: a directory that exists but cannot verify.
        let broken=group.join(format!(".ainative-backup-{}",uuid::Uuid::new_v4()));
        drop(crate::storage::directories::create_private_dir(&broken).unwrap());
        fs::write(broken.join("verified-old.pdf"),b"partial").unwrap();
        assert!(retain_completed(&outcome,&archive,&doc).is_err(),"an unverifiable archive must be reported, not silently dropped");
        assert!(broken.join("verified-old.pdf").exists(),"unverifiable material must be preserved");
        let mut remaining=fs::read_dir(&group).unwrap().filter_map(|e|e.ok()).map(|e|e.path()).filter(|p|p.file_name().is_some_and(|n|n.to_string_lossy().starts_with(".ainative-backup-"))).collect::<Vec<_>>();
        remaining.sort();
        // Three verified versions were present; the oldest has now been removed.
        assert_eq!(remaining.len(),3,"two verified versions plus the preserved unknown one");
        assert!(remaining.contains(&archived(&archive,&doc,&outcome)));
        assert!(remaining.contains(&older[1]));
        assert!(!remaining.contains(&older[0]),"rotation must still happen despite the unverifiable archive");
        assert!(remaining.contains(&broken));
    }
}
