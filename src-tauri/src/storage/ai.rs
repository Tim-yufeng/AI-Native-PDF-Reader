//! M5 uses existing schema-backed metadata; no new migration or plaintext key column.
use super::*;use super::model::MessageMetadata;use rusqlite::params;
impl Store {
 pub(crate) fn ai_config(&self)->Result<crate::ai_config::Config>{
  let raw:Option<String>=self.conn.query_row("SELECT value FROM settings WHERE key='ai-v1'",[],|r|r.get(0)).optional()?;
  let value=if let Some(raw)=raw{serde_json::from_str(&raw).map_err(|_|DataError::Refused("ai config"))?}else{crate::ai_config::Config::default()};
  value.validate().map_err(|_|DataError::Refused("ai config"))?;Ok(value)
 }
 pub(crate) fn ai_set_config(&mut self,config:&crate::ai_config::Config)->Result<()>{self.writable()?;config.validate().map_err(|_|DataError::Refused("ai config"))?;
  self.conn.execute("INSERT INTO settings VALUES('ai-v1',1,?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[serde_json::to_string(config).unwrap()])?;Ok(())
 }
 pub(crate) fn ai_begin(&mut self,conv:&str,rev:&str,prompt:&str,parent:Option<&str>,edit:Option<&str>,metadata:&MessageMetadata,manifest:&str,page:u32)->Result<(String,String)>{
  self.writable()?;metadata.validate()?;if prompt.trim().is_empty()||prompt.len()>16384||manifest.len()>65536{return deny("ai prompt/context length");}
  if let Some(parent)=parent{self.check_message_owner(conv,parent)?;}if let Some(edit)=edit{self.check_message_owner(conv,edit)?;}
  let graph=super::chat_view::Graph::load(&self.conn,conv)?;graph.budget(2)?;let mut chat=super::chat_view::ChatState::read(&self.conn,conv,&graph)?;
  if let Some(id)=edit.or(parent){self.chat_session_open(conv,id)?;}
  let pages:u32=self.conn.query_row("SELECT pages FROM revisions WHERE id=?1",[rev],|r|r.get(0))?;if page==0||page>pages{return deny("ai page");}
  let sources:Vec<crate::ai_context::Source>=serde_json::from_str(manifest).map_err(|_|DataError::Refused("ai source manifest"))?;
  let rect=serde_json::to_string(&sources.first().and_then(|s|s.rect).unwrap_or([0.,0.,1.,1.])).unwrap();
  let doc:String=self.conn.query_row("SELECT document_id FROM conversations WHERE id=?1",[conv],|r|r.get(0))?;
  let user=uuid();let answer=uuid();let tx=self.conn.transaction()?;
  let effective_parent:Option<String>=if let Some(id)=edit{let(role,parent):(String,Option<String>)=tx.query_row("SELECT role,parent_id FROM messages WHERE id=?1 AND conversation_id=?2",params![id,conv],|r|Ok((r.get(0)?,r.get(1)?)))?;if role!="user"{return deny("edit role");}parent}else{parent.map(str::to_string)};
  let meta=serde_json::to_string(metadata).unwrap();
  tx.execute("INSERT INTO messages(id,conversation_id,document_id,revision_id,parent_id,edited_from,role,status,content,metadata,created_ms) VALUES(?1,?2,?3,?4,?5,?6,'user','complete',?7,?8,CAST(unixepoch('subsec')*1000 AS INTEGER))",params![user,conv,doc,rev,effective_parent,edit,prompt,meta])?;
  tx.execute("INSERT INTO messages(id,conversation_id,document_id,revision_id,parent_id,role,status,content,metadata,created_ms) VALUES(?1,?2,?3,?4,?5,'assistant','streaming','',?6,CAST(unixepoch('subsec')*1000 AS INTEGER))",params![answer,conv,doc,rev,user,meta])?;
  tx.execute("INSERT INTO anchors VALUES(?1,?2,?3,?4,?5,?6)",params![uuid(),user,doc,rev,page,rect])?;
  tx.execute("INSERT INTO data_meta VALUES(?1,?2)",params![format!("ai-context:{answer}"),manifest])?;chat.after_begin(&tx,conv,&answer)?;tx.commit()?;Ok((user,answer))
 }
 pub(crate) fn ai_check(&self,conv:&str,answer:&str)->Result<()>{self.check_message_owner(conv,answer)?;
  self.chat_session_open(conv,answer)?;
  let(parent,status):(String,String)=self.conn.query_row("SELECT parent_id,status FROM messages WHERE id=?1 AND conversation_id=?2",params![answer,conv],|r|Ok((r.get(0)?,r.get(1)?)))?;self.check_message_owner(conv,&parent)?;if status!="streaming"{return deny("ai terminal");}Ok(())
 }
 pub(crate) fn ai_cancel(&mut self,conv:&str,answer:&str)->Result<()>{self.writable()?;self.conn.execute("UPDATE messages SET status='cancelled' WHERE id=?1 AND conversation_id=?2 AND status='streaming'",params![answer,conv])?;Ok(())}
 pub(crate) fn ai_edit_parent(&self,conv:&str,id:&str)->Result<Option<String>>{self.check_message_owner(conv,id)?;self.chat_session_open(conv,id)?;Ok(self.conn.query_row("SELECT parent_id FROM messages WHERE id=?1 AND conversation_id=?2 AND role='user'",params![id,conv],|r|r.get(0))?)}
 pub(crate) fn ai_append(&mut self,conv:&str,answer:&str,request:&str,sequence:u32,text:&str)->Result<()>{self.ai_check(conv,answer)?;self.append_chunk(answer,request,sequence,text)}
 pub(crate) fn ai_finish(&mut self,conv:&str,answer:&str,status:&str,usage:&serde_json::Value)->Result<()>{
  self.ai_check(conv,answer)?;self.writable()?;if !matches!(status,"complete"|"cancelled"|"failed"){return deny("ai terminal status");}let tx=self.conn.transaction()?;
  if tx.execute("UPDATE messages SET status=?1 WHERE id=?2 AND status='streaming'",params![status,answer])?!=1{return deny("ai already terminal");}
  tx.execute("INSERT INTO data_meta VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![format!("ai-usage:{answer}"),serde_json::to_string(usage).unwrap()])?;tx.commit()?;Ok(())
 }
 pub(crate) fn ai_ancestry(&self,conv:&str,parent:Option<&str>,allowed:&[String])->Result<Vec<serde_json::Value>>{
  if let Some(parent)=parent{self.chat_session_open(conv,parent)?;}
  let mut output=Vec::new();let mut next=parent.map(str::to_owned);let mut total=0;
  for _ in 0..32{let Some(id)=next.take() else{break;};self.check_message_owner(conv,&id)?;
   let(role,content,status,up):(String,String,String,Option<String>)=self.conn.query_row("SELECT role,content,status,parent_id FROM messages WHERE id=?1 AND conversation_id=?2",params![id,conv],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
   if status!="complete"{return deny("incomplete history context");}
   if let Some(raw)=self.conn.query_row("SELECT value FROM data_meta WHERE key=?1",[format!("ai-context:{id}")],|r|r.get::<_,String>(0)).optional()?{
    let sources:Vec<crate::ai_context::Source>=serde_json::from_str(&raw).map_err(|_|DataError::Refused("context manifest"))?;if sources.iter().any(|s|s.file_key.as_ref().is_some_and(|k|!allowed.contains(&format!("{k}:{}",s.hash.as_deref().unwrap_or(""))))){return deny("revoked history sources; start fresh conversation");}}
   if total+content.len()>48000{break;}total+=content.len();output.push(serde_json::json!({"role":role,"content":content}));next=up;
  }output.reverse();Ok(output)
 }
 pub(crate) fn ai_details(&self,conv:&str,id:&str)->Result<serde_json::Value>{self.check_message_owner(conv,id)?;
  let get=|key:String|->Result<Option<serde_json::Value>>{let raw:Option<String>=self.conn.query_row("SELECT value FROM data_meta WHERE key=?1",[key],|r|r.get(0)).optional()?;raw.map(|r|serde_json::from_str(&r).map_err(|_|DataError::Refused("ai details"))).transpose()};
  Ok(serde_json::json!({"sources":get(format!("ai-context:{id}"))?,"usage":get(format!("ai-usage:{id}"))?}))
 }
}
