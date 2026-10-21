//! Explicit folder browsing only. Opaque nodes never become AI directory grants.
use crate::{error::{AppError,Result},files::{Files,Descriptor,final_path}};
use serde::Serialize;
use std::{collections::HashMap,ffi::OsString,fs::{File,OpenOptions},path::{Path,PathBuf,Component},sync::Mutex};
use std::os::windows::{fs::{OpenOptionsExt,MetadataExt},ffi::OsStringExt,io::AsRawHandle};
use tauri::{State,WebviewWindow};

#[derive(Clone,Serialize)]#[serde(rename_all="camelCase")]
pub(crate) struct Folder {pub id:String,pub name:String}
#[derive(Clone,Serialize)]#[serde(rename_all="camelCase")]
pub(crate) struct Entry {pub id:String,pub name:String,pub directory:bool}
#[derive(Serialize)]#[serde(rename_all="camelCase")]
pub(crate) struct Listing {pub entries:Vec<Entry>,pub skipped_links:usize}
#[derive(Clone)]struct Node {relative:PathBuf,directory:bool}
struct Workspace {owner:String,epoch:u64,root:PathBuf,_pins:Vec<File>,nodes:HashMap<String,Node>}
#[derive(Default)]pub(crate) struct Workspaces {inner:Mutex<HashMap<String,Workspace>>}
fn budget()->AppError{AppError{code:"workspace_budget",message:"文件夹条目或层级超过浏览容量，未省略条目；请选择更小的课程文件夹。"}}
fn folder_error()->AppError{AppError{code:"workspace_boundary",message:"文件夹不可访问或包含链接，未越过所选范围；请重新选择普通本地文件夹。"}}
// Deny delete/rename while using the ancestry. OPEN_REPARSE_POINT ensures the
// inspected object is the link itself, never a followed junction/symlink.
fn pin(path:&Path)->Result<Vec<File>> {
 if !path.is_absolute()||path.components().any(|c|matches!(c,Component::ParentDir)) {return Err(folder_error());}
 let mut pins=Vec::new();
 for dir in path.ancestors().collect::<Vec<_>>().into_iter().rev(){
  let file=OpenOptions::new().read(true).share_mode(3).custom_flags(0x0200_0000|0x0020_0000).open(dir).map_err(|_|folder_error())?;
  let meta=file.metadata().map_err(|_|folder_error())?;
  if !meta.is_dir()||meta.file_attributes()&0x400!=0{return Err(folder_error());}
  final_path(&file)?;pins.push(file);
 }
 Ok(pins)
}
// Query the verified directory object, never reopen its path or stat children
// by path. A concurrent in-place reparse change must not redirect enumeration.
// FILE_ID_BOTH_DIR_INFO supplies each name and its attributes together.
fn directory_entries(directory:&File)->Result<Vec<(OsString,u32)>>{
 use windows_sys::Win32::{Foundation::{GetLastError,ERROR_NO_MORE_FILES},Storage::FileSystem::{GetFileInformationByHandleEx,FileIdBothDirectoryRestartInfo,FileIdBothDirectoryInfo,FILE_ID_BOTH_DIR_INFO}};
 // The Win32 records require 8-byte alignment, including each NextEntryOffset.
 let mut buffer=vec![0u64;8192];let bytes=buffer.len()*8;let name_offset=std::mem::offset_of!(FILE_ID_BOTH_DIR_INFO,FileName);
 let mut entries=Vec::new();let mut class=FileIdBothDirectoryRestartInfo;
 loop{
  buffer.fill(0);
  if unsafe{GetFileInformationByHandleEx(directory.as_raw_handle(),class,buffer.as_mut_ptr().cast(),bytes as u32)}==0{
   if unsafe{GetLastError()}==ERROR_NO_MORE_FILES{return Ok(entries);}return Err(folder_error());
  }
  class=FileIdBothDirectoryInfo;let mut offset=0;
  loop{
   if offset+std::mem::size_of::<FILE_ID_BOTH_DIR_INFO>()>bytes{return Err(folder_error());}
   // All bounds and alignment are checked before borrowing each variable record.
   let record=unsafe{&*buffer.as_ptr().cast::<u8>().add(offset).cast::<FILE_ID_BOTH_DIR_INFO>()};
   let next=record.NextEntryOffset as usize;let end=if next==0{bytes}else{offset.checked_add(next).filter(|end|next%8==0&&next>=name_offset&&*end<=bytes).ok_or_else(folder_error)?};
   let length=record.FileNameLength as usize;
   if length==0||length%2!=0||offset+name_offset+length>end{return Err(folder_error());}
   let wide=unsafe{std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>().add(offset+name_offset).cast::<u16>(),length/2)};
   if wide!=[46]&&wide!=[46,46]{
    let name=OsString::from_wide(wide);let mut components=Path::new(&name).components();
    if !matches!(components.next(),Some(Component::Normal(_)))||components.next().is_some(){return Err(folder_error());}
    if entries.len()>=2000{return Err(budget());}entries.push((name,record.FileAttributes));
   }
   if next==0{break;}offset=end;
  }
 }
}
impl Workspaces {
 pub(crate) fn select(&self,owner:&str,path:&Path,files:&Files,epoch:u64)->Result<Folder>{
  let pins=pin(path)?;let root=final_path(pins.last().ok_or_else(folder_error)?)?;
  let id=uuid::Uuid::new_v4().to_string();let name=root.file_name().unwrap_or(root.as_os_str()).to_string_lossy().into_owned();
  let mut inner=self.inner.lock().map_err(|_|AppError::io())?;if files.epoch()!=epoch{return Err(AppError::cancelled());}if inner.len()>=4{return Err(budget());}
  let mut nodes=HashMap::new();nodes.insert(id.clone(),Node{relative:PathBuf::new(),directory:true});
  inner.insert(id.clone(),Workspace{owner:owner.into(),epoch,root,_pins:pins,nodes});Ok(Folder{id,name})
 }
 fn live<'a>(inner:&'a mut HashMap<String,Workspace>,owner:&str,id:&str,files:&Files)->Result<&'a mut Workspace>{
  let w=inner.get_mut(id).filter(|w|w.owner==owner&&w.epoch==files.epoch()).ok_or_else(AppError::denied)?;
  if final_path(w._pins.last().ok_or_else(folder_error)?)?!=w.root{return Err(AppError::changed());}Ok(w)
 }
 pub(crate) fn list(&self,owner:&str,id:&str,node:&str,files:&Files)->Result<Listing>{
  let mut inner=self.inner.lock().map_err(|_|AppError::io())?;let w=Self::live(&mut inner,owner,id,files)?;
  let parent=w.nodes.get(node).filter(|n|n.directory).ok_or_else(AppError::denied)?.relative.clone();
  if parent.components().count()>32{return Err(budget());}let path=w.root.join(&parent);let pins=pin(&path)?;
  let directory=pins.last().ok_or_else(folder_error)?;
  // An ancestor may have become a junction while pinning the next component.
  // Check the exact enumeration handle before consuming any directory entries.
  if final_path(directory)?!=path{return Err(folder_error());}
  #[cfg(test)] tests::after_list_pin();
  let mut found=Vec::new();let mut skipped_links=0;
  for (name,attributes) in directory_entries(directory)?{
   if attributes&0x400!=0{skipped_links+=1;continue;}
   let directory=attributes&0x10!=0;if !directory&&(attributes&0x40!=0||!Path::new(&name).extension().is_some_and(|v|v.eq_ignore_ascii_case("pdf"))){continue;}
   let relative=parent.join(&name);
   let existing=w.nodes.iter().find(|(_,v)|v.relative==relative&&v.directory==directory).map(|(k,_)|k.clone());
   found.push((existing,relative,Entry{id:String::new(),name:name.to_string_lossy().into_owned(),directory}));
  }
  if w.nodes.len()+found.iter().filter(|(id,_,_)|id.is_none()).count()>4096{return Err(budget());}
  let mut entries=Vec::new();for(existing,relative,mut entry)in found{entry.id=existing.unwrap_or_else(||uuid::Uuid::new_v4().to_string());w.nodes.insert(entry.id.clone(),Node{relative,directory:entry.directory});entries.push(entry);}
  entries.sort_by(|a,b|b.directory.cmp(&a.directory).then_with(||a.name.to_lowercase().cmp(&b.name.to_lowercase())).then_with(||a.name.cmp(&b.name)));
  Ok(Listing{entries,skipped_links})
 }
 pub(crate) fn open(&self,owner:&str,id:&str,node:&str,files:&Files)->Result<Descriptor>{
  let mut inner=self.inner.lock().map_err(|_|AppError::io())?;let w=Self::live(&mut inner,owner,id,files)?;
  let leaf=w.nodes.get(node).filter(|n|!n.directory).ok_or_else(AppError::denied)?;
  let path=w.root.join(&leaf.relative);let parent=path.parent().ok_or_else(folder_error)?;let pins=pin(parent)?;
  let actual=final_path(pins.last().ok_or_else(folder_error)?)?;if !actual.starts_with(&w.root){return Err(folder_error());}
  // The established exact-object PDF admission checks type, header, size,
  // reparse flags, hard-link count and final parent before reading content.
  files.authorize_member(owner,&path,&actual,w.epoch)
 }
 pub(crate) fn close(&self,owner:&str,id:&str)->Result<()>{let mut inner=self.inner.lock().map_err(|_|AppError::io())?;if inner.get(id).is_some_and(|w|w.owner!=owner){return Err(AppError::denied());}inner.remove(id);Ok(())}
 pub(crate) fn clear(&self){if let Ok(mut inner)=self.inner.lock(){inner.clear();}}
}

#[tauri::command]pub(crate) async fn pick_workspace(window:WebviewWindow,state:State<'_,crate::Backend>)->Result<Option<Folder>>{
 let owner=crate::local_owner(&window)?;let epoch=state.files.epoch();
 if state.files.picking.swap(true,std::sync::atomic::Ordering::AcqRel){return Err(AppError::busy());}
 struct Picking(std::sync::Arc<Files>);impl Drop for Picking{fn drop(&mut self){self.0.picking.store(false,std::sync::atomic::Ordering::Release);}}
 let _picking=Picking(state.files.clone());
 let Some(selected)=rfd::AsyncFileDialog::new().set_parent(&window).set_title("打开课程文件夹").pick_folder().await else{return Ok(None)};
 state.folders.select(&owner,selected.path(),&state.files,epoch).map(Some)
}
#[tauri::command]pub(crate) fn list_workspace(window:WebviewWindow,state:State<'_,crate::Backend>,workspace_id:String,node_id:String)->Result<Listing>{let owner=crate::local_owner(&window)?;state.folders.list(&owner,&workspace_id,&node_id,&state.files)}
#[tauri::command]pub(crate) fn open_workspace_pdf(window:WebviewWindow,state:State<'_,crate::Backend>,workspace_id:String,node_id:String)->Result<Descriptor>{let owner=crate::local_owner(&window)?;state.folders.open(&owner,&workspace_id,&node_id,&state.files)}
#[tauri::command]pub(crate) fn close_workspace(window:WebviewWindow,state:State<'_,crate::Backend>,workspace_id:String)->Result<()>{let owner=crate::local_owner(&window)?;state.folders.close(&owner,&workspace_id)}

#[cfg(test)]mod tests{
 use super::*;
 thread_local!{static AFTER_LIST_PIN:std::cell::RefCell<Option<Box<dyn FnOnce()>>>=std::cell::RefCell::new(None);}
 pub(super) fn after_list_pin(){let hook=AFTER_LIST_PIN.with(|slot|slot.borrow_mut().take());if let Some(hook)=hook{hook();}}
 // A separate writable OS handle models a concurrent program changing an
 // already-checked empty directory in place, without deleting or renaming it.
 struct InPlaceJunction(File);
 impl InPlaceJunction{
  fn set(path:&Path,target:&Path)->Self{
   use std::{io::Write,os::windows::{ffi::OsStrExt,io::AsRawHandle}};
   let file=OpenOptions::new().write(true).share_mode(7).custom_flags(0x0220_0000).open(path).unwrap();
   let target=std::fs::canonicalize(target).unwrap();let text=target.to_str().unwrap().strip_prefix("\\\\?\\").unwrap();
   let substitute:Vec<u16>=std::ffi::OsStr::new(&format!("\\??\\{text}")).encode_wide().collect();let print:Vec<u16>=std::ffi::OsStr::new(text).encode_wide().collect();
   let mut data=Vec::new();data.write_all(&0xA000_0003u32.to_le_bytes()).unwrap();
   for value in [(12+(substitute.len()+print.len())*2)as u16,0,0,(substitute.len()*2)as u16,(substitute.len()*2+2)as u16,(print.len()*2)as u16]{data.write_all(&value.to_le_bytes()).unwrap();}
   for name in [&substitute,&print]{for value in name.iter().copied().chain(Some(0)){data.write_all(&value.to_le_bytes()).unwrap();}}
   let mut returned=0;assert_ne!(unsafe{windows_sys::Win32::System::IO::DeviceIoControl(file.as_raw_handle(),0x900A4,data.as_ptr().cast(),data.len()as u32,std::ptr::null_mut(),0,&mut returned,std::ptr::null_mut())},0,"in-place junction: {}",std::io::Error::last_os_error());Self(file)
  }
 }
 impl Drop for InPlaceJunction{fn drop(&mut self){use std::os::windows::io::AsRawHandle;let data=[3u8,0,0,0xA0,0,0,0,0];let mut returned=0;assert_ne!(unsafe{windows_sys::Win32::System::IO::DeviceIoControl(self.0.as_raw_handle(),0x900AC,data.as_ptr().cast(),8,std::ptr::null_mut(),0,&mut returned,std::ptr::null_mut())},0,"remove synthetic junction: {}",std::io::Error::last_os_error());}}
 fn fixture()->(PathBuf,PathBuf){let root=crate::save_core::project().join("tests/work/workspace").join(uuid::Uuid::new_v4().to_string());let course=root.join("中文 课程");std::fs::create_dir_all(course.join("chapter")).unwrap();std::fs::write(course.join("lesson.PDF"),std::fs::read(crate::save_core::project().join("tests/fixtures/g1/plain.pdf")).unwrap()).unwrap();std::fs::write(course.join("notes.txt"),"not a PDF").unwrap();(root,course)}
 fn junction(link:&Path,target:&Path){use std::os::windows::process::CommandExt;let command=format!("mklink /J \"{}\" \"{}\"",link.display().to_string().replace('/',"\\"),target.display().to_string().replace('/',"\\"));let result=std::process::Command::new("cmd.exe").args(["/d","/c"]).raw_arg(command).creation_flags(0x08000000).output().unwrap();assert!(result.status.success(),"junction test fixture: {:?}",result.stderr);}
 #[test]fn post_v1_workspace_in_place_junction_cannot_escape_pinned_enumeration(){
  for selected_root in [true,false]{
   let(root,course)=fixture();let chapter=course.join("chapter");let outside=root.join("outside");std::fs::create_dir(&outside).unwrap();std::fs::write(outside.join("outside-secret.pdf"),"synthetic").unwrap();
   let files=Files::default();let folders=Workspaces::default();let folder=folders.select("main",if selected_root{&chapter}else{&course},&files,files.epoch()).unwrap();
   let node=if selected_root{folder.id.clone()}else{folders.list("main",&folder.id,&folder.id,&files).unwrap().entries.into_iter().find(|e|e.name=="chapter").unwrap().id};
   let held=std::rc::Rc::new(std::cell::RefCell::new(None));let hold=held.clone();let path=chapter.clone();let legacy_leak=std::rc::Rc::new(std::cell::Cell::new(false));let leaked=legacy_leak.clone();
   AFTER_LIST_PIN.with(|slot|*slot.borrow_mut()=Some(Box::new(move||{
    *hold.borrow_mut()=Some(InPlaceJunction::set(&path,&outside));
    leaked.set(std::fs::read_dir(&path).unwrap().any(|e|e.unwrap().file_name()=="outside-secret.pdf"));
   })));
   let result=folders.list("main",&folder.id,&node,&files);drop(held.borrow_mut().take());
   assert!(legacy_leak.get(),"the old path enumeration must expose the synthetic outside name");
   if let Ok(list)=result{assert!(list.entries.is_empty(),"outside names leaked after validation (selected_root={selected_root}): {:?}",list.entries.iter().map(|e|&e.name).collect::<Vec<_>>());}
   assert!(folders.list("main",&folder.id,&node,&files).unwrap().entries.is_empty());
   std::fs::write(chapter.join("合法.PDF"),"synthetic").unwrap();let normal=folders.list("main",&folder.id,&node,&files).unwrap();assert_eq!(normal.entries.len(),1);assert_eq!(normal.entries[0].name,"合法.PDF");
   eprintln!("selected_root={selected_root}: in-place junction succeeded; old path exposes outside name; protected listing does not; normal expansion restored");
  }
 }
 #[test]fn post_v1_workspace_handle_enumeration_continues_and_restarts_without_omissions(){
  let(_,course)=fixture();let chapter=course.join("chapter");let mut expected=std::collections::BTreeSet::new();
  for i in 0..400{let name=format!("课程-{i:04}-{}.PDF","wide".repeat(20));std::fs::write(chapter.join(&name),"synthetic").unwrap();expected.insert(name);}
  let files=Files::default();let folders=Workspaces::default();let folder=folders.select("main",&course,&files,files.epoch()).unwrap();let child=folders.list("main",&folder.id,&folder.id,&files).unwrap().entries.into_iter().find(|e|e.name=="chapter").unwrap();
  for _ in 0..2{let list=folders.list("main",&folder.id,&child.id,&files).unwrap();assert_eq!(list.entries.len(),expected.len());assert_eq!(list.entries.into_iter().map(|e|e.name).collect::<std::collections::BTreeSet<_>>(),expected);}
 }
 #[test]fn post_v1_workspace_lazy_scope_owner_epoch_and_independent_open_grants(){
  let(_,course)=fixture();let files=Files::default();let folders=Workspaces::default();let folder=folders.select("main",&course,&files,files.epoch()).unwrap();
  let list=folders.list("main",&folder.id,&folder.id,&files).unwrap();assert_eq!(list.entries.len(),2);assert!(list.entries[0].directory);assert_eq!(list.entries[1].name,"lesson.PDF");
  assert!(folders.list("foreign",&folder.id,&folder.id,&files).is_err());assert!(folders.list("main",&folder.id,"../outside",&files).is_err());
  let child=&list.entries[0];assert!(folders.list("main",&folder.id,&child.id,&files).unwrap().entries.is_empty());
  let grant=folders.open("main",&folder.id,&list.entries[1].id,&files).unwrap();assert!(files.live("main",&grant.grant_id).is_ok());
  assert!(folders.close("foreign",&folder.id).is_err());folders.close("main",&folder.id).unwrap();assert!(folders.list("main",&folder.id,&folder.id,&files).is_err());assert!(files.live("main",&grant.grant_id).is_ok());
  let next=folders.select("main",&course,&files,files.epoch()).unwrap();files.revoke_all();assert!(folders.list("main",&next.id,&next.id,&files).is_err());assert!(files.live("main",&grant.grant_id).is_err());
 }
 #[test]fn post_v1_workspace_reparse_replacement_and_hardlinks_are_refused(){
  let(root,course)=fixture();let outside=root.join("outside");std::fs::create_dir(&outside).unwrap();std::fs::copy(course.join("lesson.PDF"),outside.join("secret.pdf")).unwrap();
  let files=Files::default();let folders=Workspaces::default();let folder=folders.select("main",&course,&files,files.epoch()).unwrap();
  junction(&course.join("linked"),&outside);assert!(folders.select("main",&course.join("linked"),&files,files.epoch()).is_err());
  let list=folders.list("main",&folder.id,&folder.id,&files).unwrap();assert_eq!(list.skipped_links,1);assert!(list.entries.iter().all(|e|e.name!="linked"));
  let chapter=list.entries.iter().find(|e|e.name=="chapter").unwrap();std::fs::rename(course.join("chapter"),course.join("old-chapter")).unwrap();junction(&course.join("chapter"),&outside);
  assert!(folders.list("main",&folder.id,&chapter.id,&files).is_err());
  std::fs::hard_link(outside.join("secret.pdf"),course.join("hardlink.pdf")).unwrap();let list=folders.list("main",&folder.id,&folder.id,&files).unwrap();let hard=list.entries.iter().find(|e|e.name=="hardlink.pdf").unwrap();assert!(folders.open("main",&folder.id,&hard.id,&files).is_err());
  folders.clear();std::fs::remove_dir(course.join("linked")).unwrap();std::fs::remove_dir(course.join("chapter")).unwrap();
 }
 #[test]fn post_v1_workspace_open_revalidates_replaced_files_and_refuses_partial_lists(){
  let(_,course)=fixture();let files=Files::default();let folders=Workspaces::default();let folder=folders.select("main",&course,&files,files.epoch()).unwrap();let list=folders.list("main",&folder.id,&folder.id,&files).unwrap();let pdf=list.entries.iter().find(|e|!e.directory).unwrap();
  std::fs::write(course.join("lesson.PDF"),"replaced with invalid bytes").unwrap();assert!(folders.open("main",&folder.id,&pdf.id,&files).is_err());
  for n in 0..2001{std::fs::write(course.join(format!("ignored-{n}.txt")),[]).unwrap();}
  assert_eq!(folders.list("main",&folder.id,&folder.id,&files).err().unwrap().code,"workspace_budget");
 }
}
