use super::*;
use crate::preferences::{Styles,Style,StyleBundle,EditStyle,InputSettings,ContextSettings};
impl Store {
 pub(crate) fn styles(&self)->Result<Styles>{let raw:Option<String>=self.conn.query_row("SELECT value FROM settings WHERE key='styles-v1'",[],|r|r.get(0)).optional()?;let value:Styles=raw.map(|r|serde_json::from_str(&r)).transpose().map_err(|_|DataError::Refused("style JSON"))?.unwrap_or_default();value.validate()?;Ok(value)}
 fn put_styles(&mut self,value:&Styles)->Result<()>{self.writable()?;value.validate()?;let raw=serde_json::to_string(value).unwrap();if raw.len()>1024*1024{return deny("style budget");}self.conn.execute("INSERT INTO settings VALUES('styles-v1',1,?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[raw])?;Ok(())}
 pub(crate) fn edit_style(&mut self,input:EditStyle)->Result<Styles>{let mut settings=self.styles()?;
  // Unchanged content must not create a version. Clicking save twice is the
  // common way to arrive here, and each click used to add one.
  if let Some(current)=input.id.as_deref().and_then(|id|settings.latest(id)).cloned(){
   if current.name==input.name&&current.aliases==input.aliases&&current.guidelines==input.guidelines&&current.avoid==input.avoid&&current.enabled==input.enabled{return Ok(settings);}}
  let(id,version)=if let Some(id)=input.id{let current=settings.latest(&id).ok_or(DataError::Refused("style missing"))?;(id,current.version.checked_add(1).ok_or(DataError::Refused("style version"))?)}else{(uuid(),1)};
  let next=Style{id:id.clone(),version,name:input.name,aliases:input.aliases,guidelines:input.guidelines,avoid:input.avoid,enabled:input.enabled};settings.revisions.push(next);if settings.selected.as_deref()==Some(&id)&&!settings.latest(&id).unwrap().enabled{settings.selected=None;}self.put_styles(&settings)?;Ok(settings)}
 pub(crate) fn select_style(&mut self,id:Option<String>)->Result<Styles>{let mut settings=self.styles()?;if id.as_ref().is_some_and(|id|settings.latest(id).is_none_or(|s|!s.enabled)){return deny("style disabled");}if let Some(id)=&id{settings.recent.retain(|old|old!=id);settings.recent.insert(0,id.clone());settings.recent.truncate(12);}settings.selected=id;self.put_styles(&settings)?;Ok(settings)}
 /// Removes one saved version. Gaps in the version numbers are allowed, so a
 /// version that earlier answers already reference can be discarded without
 /// renumbering the survivors - renumbering would silently repoint those answers
 /// at different text. The last remaining version of a style is kept: use disable
 /// instead, which leaves its identity and history intact.
 pub(crate) fn delete_style_version(&mut self,id:&str,version:u32)->Result<Styles>{let mut settings=self.styles()?;
  let remaining=settings.revisions.iter().filter(|s|s.id==id).count();
  if remaining<=1{return deny("style last version");}
  let before=settings.revisions.len();
  settings.revisions.retain(|s|!(s.id==id&&s.version==version));
  if settings.revisions.len()==before{return deny("style version missing");}
  if settings.latest(id).is_none_or(|s|!s.enabled){settings.selected=settings.selected.filter(|selected|selected!=id);}
  self.put_styles(&settings)?;Ok(settings)}
 pub(crate) fn import_styles(&mut self,bundle:StyleBundle)->Result<Styles>{bundle.validate()?;let mut settings=self.styles()?;
  for mut next in bundle.styles{if let Some(current)=settings.latest(&next.id){next.version=current.version;if *current==next{continue;}next.version=current.version.checked_add(1).ok_or(DataError::Refused("style version"))?;}settings.revisions.push(next);}
  if settings.selected.as_ref().is_some_and(|id|settings.latest(id).is_some_and(|style|!style.enabled)){settings.selected=None;}
  // Validation precedes one SQLite update, so invalid/oversized bundles cannot
  // partially import and cannot alter the previously selected version.
  self.put_styles(&settings)?;Ok(settings)
 }
 pub(crate) fn inputs(&self)->Result<InputSettings>{let raw:Option<String>=self.conn.query_row("SELECT value FROM settings WHERE key='inputs-v1'",[],|r|r.get(0)).optional()?;let value:InputSettings=raw.map(|r|serde_json::from_str(&r)).transpose().map_err(|_|DataError::Refused("input JSON"))?.unwrap_or_default();value.validate()?;Ok(value)}
 pub(crate) fn set_inputs(&mut self,value:&InputSettings)->Result<()>{self.writable()?;value.validate()?;self.conn.execute("INSERT INTO settings VALUES('inputs-v1',1,?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[serde_json::to_string(value).unwrap()])?;Ok(())}
 pub(crate) fn context(&self)->Result<ContextSettings>{let raw:Option<String>=self.conn.query_row("SELECT value FROM settings WHERE key='context-v1'",[],|r|r.get(0)).optional()?;let value:ContextSettings=raw.map(|r|serde_json::from_str(&r)).transpose().map_err(|_|DataError::Refused("context JSON"))?.unwrap_or_default();value.validate()?;Ok(value)}
 pub(crate) fn set_context(&mut self,value:&ContextSettings)->Result<()>{self.writable()?;value.validate()?;self.conn.execute("INSERT INTO settings VALUES('context-v1',1,?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[serde_json::to_string(value).unwrap()])?;Ok(())}
}
#[cfg(test)]mod tests{use super::*;
 fn root()->PathBuf{let p=crate::save_core::project().join("tests/work/m4").join(uuid());fs::create_dir_all(&p).unwrap();p}
 fn edit(id:Option<String>,name:&str,enabled:bool)->EditStyle{EditStyle{id,name:name.into(),aliases:vec!["简明".into()],guidelines:"简洁准确，保留条件".into(),avoid:"不写故事".into(),enabled}}
 #[test]fn m6_style_save_does_not_add_a_version_when_nothing_changed_and_old_versions_can_be_removed(){
  let root=root();let mut s=Store::open_synthetic(&root,16384).unwrap();
  let created=s.edit_style(EditStyle{id:None,name:"合成角色".into(),aliases:vec!["合成称呼".into()],guidelines:"简明".into(),avoid:"不写剧情".into(),enabled:true}).unwrap();
  let id=created.revisions[0].id.clone();assert_eq!(created.revisions.len(),1);
  // A second save with identical fields is what a double click produces.
  let same=s.edit_style(EditStyle{id:Some(id.clone()),name:"合成角色".into(),aliases:vec!["合成称呼".into()],guidelines:"简明".into(),avoid:"不写剧情".into(),enabled:true}).unwrap();
  assert_eq!(same.revisions.len(),1,"an unchanged save must not create a version");
  // A real change still versions.
  let changed=s.edit_style(EditStyle{id:Some(id.clone()),name:"合成角色".into(),aliases:vec!["合成称呼".into()],guidelines:"更活泼".into(),avoid:"不写剧情".into(),enabled:true}).unwrap();
  assert_eq!(changed.revisions.len(),2);assert_eq!(changed.latest(&id).unwrap().version,2);
  // Deleting the newest keeps the older one, and deleting a middle version leaves
  // a gap rather than renumbering what earlier answers already referenced.
  let removed=s.delete_style_version(&id,2).unwrap();assert_eq!(removed.revisions.len(),1);assert_eq!(removed.latest(&id).unwrap().version,1);
  s.edit_style(EditStyle{id:Some(id.clone()),name:"合成角色".into(),aliases:vec![],guidelines:"v3".into(),avoid:"".into(),enabled:true}).unwrap();
  s.edit_style(EditStyle{id:Some(id.clone()),name:"合成角色".into(),aliases:vec![],guidelines:"v4".into(),avoid:"".into(),enabled:true}).unwrap();
  let before=s.styles().unwrap();assert_eq!(before.revisions.len(),3);
  let gapped=s.delete_style_version(&id,2).unwrap();
  assert_eq!(gapped.revisions.iter().filter(|r|r.id==id).map(|r|r.version).collect::<Vec<_>>(),vec![1,3]);
  // The last remaining version cannot be deleted: disable keeps identity and history.
  s.delete_style_version(&id,1).unwrap();
  assert!(s.delete_style_version(&id,3).is_err(),"the last remaining version must be kept");
  let left=s.styles().unwrap();assert_eq!(left.revisions.iter().filter(|r|r.id==id).count(),1);
  // Deleting the selected version's newest entry must not deselect a still-enabled style.
  assert!(s.select_style(Some(id.clone())).is_ok());
 }
 #[test]fn m6_style_defaults_versions_disable_and_cold_restore(){let root=root();let mut s=Store::open_synthetic(&root,16384).unwrap();assert!(s.styles().unwrap().current().is_none());let styles=s.edit_style(edit(None,"测试表达",true)).unwrap();let id=styles.revisions[0].id.clone();s.select_style(Some(id.clone())).unwrap();let frozen=s.styles().unwrap().current().unwrap();s.edit_style(edit(Some(id.clone()),"新版本",true)).unwrap();assert_eq!(frozen.version,1);assert_eq!(s.styles().unwrap().current().unwrap().version,2);drop(s);let mut s=Store::open_synthetic(&root,16384).unwrap();assert_eq!(s.styles().unwrap().revisions.len(),2);s.edit_style(edit(Some(id),"停用",false)).unwrap();assert!(s.styles().unwrap().current().is_none());assert_eq!(s.styles().unwrap().revisions.len(),3);}
 fn imported(id:&str,name:&str)->Style{Style{id:id.into(),version:1,name:name.into(),aliases:vec!["合成称呼".into(),"Synthetic alias".into()],guidelines:"简明而准确".into(),avoid:"不添故事".into(),enabled:true}}
 #[test]fn m6_private_import_is_atomic_idempotent_versioned_and_recent_choices_restart(){let root=root();let mut s=Store::open_synthetic(&root,16384).unwrap();let a=uuid();let b=uuid();let original=imported(&a,"合成角色甲");
  let bundle=||StyleBundle{schema_version:1,styles:vec![original.clone(),imported(&b,"合成角色乙")]};
  let next=s.import_styles(bundle()).unwrap();assert_eq!(next.revisions.len(),2);assert!(next.current().is_none());
  assert_eq!(s.import_styles(bundle()).unwrap().revisions.len(),2);s.select_style(Some(a.clone())).unwrap();s.select_style(Some(b.clone())).unwrap();s.select_style(Some(a.clone())).unwrap();
  assert_eq!(s.styles().unwrap().recent,vec![a.clone(),b.clone()]);let frozen=s.styles().unwrap().current().unwrap();
  let mut revised=original.clone();revised.guidelines="修订后的合成表达".into();let next=s.import_styles(StyleBundle{schema_version:1,styles:vec![revised]}).unwrap();assert_eq!(next.current().unwrap().version,2);assert_eq!(frozen.version,1);
  let before=serde_json::to_string(&s.styles().unwrap()).unwrap();let invalid=StyleBundle{schema_version:1,styles:vec![imported(&uuid(),"不会部分导入"),imported("invalid","错误项")]};assert!(s.import_styles(invalid).is_err());assert_eq!(serde_json::to_string(&s.styles().unwrap()).unwrap(),before);
  assert!(s.import_styles(StyleBundle{schema_version:1,styles:vec![original.clone(),original]}).is_err());assert_eq!(serde_json::to_string(&s.styles().unwrap()).unwrap(),before);
  s.select_style(None).unwrap();drop(s);let s=Store::open_synthetic(&root,16384).unwrap();let restored=s.styles().unwrap();assert!(restored.current().is_none());assert_eq!(restored.recent,vec![a,b]);assert_eq!(restored.revisions.len(),3);
 }
 #[test]fn m6_import_retains_existing_history_and_refuses_budget_without_partial_changes(){let root=root();let mut s=Store::open_synthetic(&root,16384).unwrap();let(doc,rev)=s.create_document("synthetic-import",&"a".repeat(64),1,"test.pdf").unwrap();let conversation=s.local_conversation(&doc).unwrap();let prompt=s.add_local_prompt(&conversation,&rev,None,"历史合成问题",1).unwrap();
  let id=uuid();s.import_styles(StyleBundle{schema_version:1,styles:vec![imported(&id,"已导入表达")]}).unwrap();
  let too_many=StyleBundle{schema_version:1,styles:(0..101).map(|_|imported(&uuid(),"合成表达")).collect()};assert!(s.import_styles(too_many).is_err());assert_eq!(s.styles().unwrap().revisions.len(),1);assert_eq!(s.local_messages(&conversation).unwrap()[0].id,prompt);
  let old=r#"{"schemaVersion":1,"selected":null,"revisions":[]}"#;let parsed:Styles=serde_json::from_str(old).unwrap();parsed.validate().unwrap();assert!(parsed.recent.is_empty());
 }
 #[test]fn m6_input_conflicts_and_unknown_schema_preserve_last_settings(){let root=root();let mut s=Store::open_synthetic(&root,16384).unwrap();let before=s.inputs().unwrap();let mut bad=before.clone();bad.bindings.insert("region".into(),"Ctrl+S".into());assert!(s.set_inputs(&bad).is_err());bad=before.clone();bad.bindings.insert("region".into(),"Ctrl+Shift+2".into());assert!(s.set_inputs(&bad).is_err());bad=before.clone();bad.schema_version=99;assert!(s.set_inputs(&bad).is_err());assert_eq!(s.inputs().unwrap(),before);let mut next=before;next.bindings.insert("region".into(),String::new());s.set_inputs(&next).unwrap();drop(s);let s=Store::open_synthetic(&root,16384).unwrap();assert_eq!(s.inputs().unwrap(),next);}
 #[test]fn m5_context_settings_default_cold_restore_and_invalid_values_preserve_last(){
  let legacy:ContextSettings=serde_json::from_str(r#"{"schemaVersion":1,"beforePages":0,"afterPages":0}"#).unwrap();assert!(legacy.automatic_current_page&&legacy.inline_full_document);
  let root=root();let mut s=Store::open_synthetic(&root,16384).unwrap();assert_eq!(s.context().unwrap(),ContextSettings::default());
  let next=ContextSettings{schema_version:1,before_pages:2,after_pages:5,automatic_current_page:false,inline_full_document:false,inline_answer_limit:12};s.set_context(&next).unwrap();
  for raw in [r#"{"schemaVersion":1,"beforePages":-1,"afterPages":0}"#,r#"{"schemaVersion":1,"beforePages":0.5,"afterPages":0}"#,r#"{"schemaVersion":1,"beforePages":0,"afterPages":0,"extra":true}"#]{assert!(serde_json::from_str::<ContextSettings>(raw).is_err());assert_eq!(s.context().unwrap(),next);}
  for bad in [ContextSettings{before_pages:6,..next},ContextSettings{after_pages:6,..next},ContextSettings{schema_version:99,..next}]{assert!(s.set_context(&bad).is_err());assert_eq!(s.context().unwrap(),next);}
  drop(s);let s=Store::open_synthetic(&root,16384).unwrap();assert_eq!(s.context().unwrap(),next);
 }
 #[test]fn m5_context_settings_corrupt_storage_is_refused(){let root=root();let mut s=Store::open_synthetic(&root,16384).unwrap();s.set_context(&ContextSettings::default()).unwrap();for raw in [r#"{"schemaVersion":99,"beforePages":0,"afterPages":0}"#,r#"{"schemaVersion":1,"beforePages":6,"afterPages":0}"#,r#"{"schemaVersion":1,"beforePages":-1,"afterPages":0}"#]{s.conn.execute("UPDATE settings SET value=?1 WHERE key='context-v1'",[raw]).unwrap();assert!(s.context().is_err());}}
 #[test]fn m6_history_retains_commit_order_when_wall_clocks_go_back(){
  let root=root();let mut s=Store::open_synthetic(&root,16384).unwrap();let(doc,rev)=s.create_document("synthetic-order",&"a".repeat(64),3,"test.pdf").unwrap();let conv=s.local_conversation(&doc).unwrap();
  let prompt=s.add_local_prompt(&conv,&rev,None,"原问题",1).unwrap();let edit=s.edit_local_prompt(&conv,&prompt,"修改后的问题").unwrap();let answer=s.local_regenerate(&conv,&rev,&edit).unwrap();
  for(id,time)in [(&prompt,3000),(&edit,2000),(&answer,1000)]{s.conn.execute("UPDATE messages SET created_ms=?1 WHERE id=?2",rusqlite::params![time,id]).unwrap();}
  let expected=vec![prompt,edit,answer];assert_eq!(s.local_messages(&conv).unwrap().iter().map(|m|m.id.clone()).collect::<Vec<_>>(),expected);
  assert_eq!(s.messages(&conv).unwrap().iter().map(|m|m.id.clone()).collect::<Vec<_>>(),expected);drop(s);
  let s=Store::open_synthetic(&root,16384).unwrap();assert_eq!(s.local_messages(&conv).unwrap().iter().map(|m|m.id.clone()).collect::<Vec<_>>(),expected);
 }
}
