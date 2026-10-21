//! SQLite-backed session/version selection. This module never writes PDFs.
use super::*;
use super::model::{MessageMetadata,desktop::HistoryMessage};
use rusqlite::params;
use serde::{Deserialize,Serialize};
use std::collections::{BTreeMap,HashMap,HashSet};
const MAX_NODES:usize=4096;
#[cfg(test)]#[path="chat_view_tests.rs"]mod tests;
#[derive(Serialize)]#[serde(rename_all="camelCase")]
pub(crate) struct ChatView{pub session_id:Option<String>,pub leaf_id:Option<String>,pub sessions:Vec<Session>,pub path:Vec<HistoryMessage>,pub versions:Vec<Version>}
#[derive(Serialize)]#[serde(rename_all="camelCase")]
pub(crate) struct Session{pub id:String,pub title:String,pub archived:bool,pub leaf_id:String}
#[derive(Serialize)]#[serde(rename_all="camelCase")]
pub(crate) struct Version{pub message_id:String,pub index:usize,pub total:usize,pub previous_id:Option<String>,pub next_id:Option<String>}
#[derive(Clone,Serialize,Deserialize)]#[serde(rename_all="camelCase",deny_unknown_fields)]
pub(super) struct ChatState{schema_version:u32,generation:u64,active_session:Option<String>,active_leaf:Option<String>,archived:Vec<String>,remembered:BTreeMap<String,String>,session_leaves:BTreeMap<String,String>}
struct Node{id:String,parent:Option<String>,edit:Option<String>,role:String,title:String,hidden:bool,session:String,family:String}
pub(super) struct Graph{nodes:Vec<Node>,lookup:HashMap<String,usize>}
fn key(conv:&str)->String{format!("chat-view-v1:{conv}")}
fn clean_title(raw:&str)->String{let value:String=raw.chars().filter(|c|!c.is_control()&&!matches!(*c,'\u{061c}'|'\u{200e}'|'\u{200f}'|'\u{2028}'|'\u{2029}'|'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}')).take(80).collect();if value.trim().is_empty(){"未命名会话".into()}else{value}}
impl Graph{
 pub(super) fn load(conn:&Connection,conv:&str)->Result<Self>{
  valid_uuid(conv)?;let doc:String=conn.query_row("SELECT document_id FROM conversations WHERE id=?1",[conv],|r|r.get(0))?;
  let mut stmt=conn.prepare("SELECT id,parent_id,edited_from,role,substr(content,1,160),EXISTS(SELECT 1 FROM data_meta WHERE key='message-hidden:'||m.id),document_id FROM messages m WHERE conversation_id=?1 ORDER BY rowid LIMIT 4097")?;
  let rows=stmt.query_map([conv],|r|Ok((r.get::<_,String>(0)?,r.get::<_,Option<String>>(1)?,r.get::<_,Option<String>>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,bool>(5)?,r.get::<_,String>(6)?)))?;
  let mut graph=Self{nodes:Vec::new(),lookup:HashMap::new()};
  for row in rows{let(id,parent,edit,role,title,hidden,owner)=row?;if graph.nodes.len()>=MAX_NODES{return deny("chat node budget");}valid_uuid(&id)?;if owner!=doc{return deny("chat graph ownership");}
   let up=parent.as_ref().or(edit.as_ref());let session=if let Some(up)=up{graph.node(up)?.session.clone()}else{if role!="user"{return deny("chat root role");}id.clone()};
   let family=if let Some(edit)=&edit{let old=graph.node(edit)?;if role!="user"||old.role!="user"||old.parent!=parent{return deny("chat edit relationship");}old.family.clone()}else{id.clone()};
   if role=="assistant"&&parent.is_none(){return deny("chat assistant parent");}
   graph.lookup.insert(id.clone(),graph.nodes.len());graph.nodes.push(Node{id,parent,edit,role,title:if hidden{"已删除的提问".into()}else{clean_title(&title)},hidden,session,family});
  }Ok(graph)
 }
 fn node(&self,id:&str)->Result<&Node>{self.lookup.get(id).map(|n|&self.nodes[*n]).ok_or(DataError::Refused("chat node outside binding"))}
 fn path(&self,id:&str)->Result<Vec<String>>{let mut result=Vec::new();let mut next=Some(id);while let Some(id)=next{if result.len()>=MAX_NODES{return deny("chat node budget");}let node=self.node(id)?;result.push(node.id.clone());next=node.parent.as_deref();}result.reverse();Ok(result)}
 fn descendant(&self,leaf:&str,node:&str)->bool{self.path(leaf).is_ok_and(|p|p.iter().any(|id|id==node))}
 fn newest_leaf(&self,id:&str)->Result<String>{let mut current=id.to_string();for _ in 0..MAX_NODES{if let Some(child)=self.nodes.iter().rev().find(|n|n.parent.as_deref()==Some(&current)&&!n.hidden){current=child.id.clone();}else{return Ok(current);}}deny("chat node budget")}
 fn roots(&self)->Vec<&Node>{self.nodes.iter().filter(|n|n.id==n.session).collect()}
 pub(super) fn budget(&self,additional:usize)->Result<()>{if self.nodes.len()+additional>MAX_NODES{return deny("chat node budget");}Ok(())}
}
impl ChatState{
 pub(super) fn read(conn:&Connection,conv:&str,graph:&Graph)->Result<Self>{
  let raw:Option<String>=conn.query_row("SELECT value FROM data_meta WHERE key=?1",[key(conv)],|r|r.get(0)).optional()?;
  let state=if let Some(raw)=raw{if raw.len()>1024*1024{return deny("chat metadata budget");}serde_json::from_str(&raw).map_err(|_|DataError::Refused("chat metadata"))?}else{
   let leaf=graph.nodes.iter().rev().find(|n|!n.hidden);Self{schema_version:1,generation:0,active_session:leaf.map(|n|n.session.clone()),active_leaf:leaf.map(|n|n.id.clone()),archived:Vec::new(),remembered:BTreeMap::new(),session_leaves:BTreeMap::new()}
  };state.validate(graph)?;Ok(state)
 }
 fn validate(&self,graph:&Graph)->Result<()>{
  if self.schema_version!=1||self.archived.len()>MAX_NODES||self.remembered.len()>MAX_NODES||self.session_leaves.len()>MAX_NODES||self.active_session.is_some()!=self.active_leaf.is_some(){return deny("chat metadata");}
  let mut seen=HashSet::new();for id in &self.archived{valid_uuid(id)?;if graph.node(id)?.session!=*id||!seen.insert(id){return deny("chat metadata");}}
  if let(Some(session),Some(leaf))=(&self.active_session,&self.active_leaf){valid_uuid(session)?;valid_uuid(leaf)?;if graph.node(leaf)?.session!=*session||self.archived.contains(session){return deny("chat metadata");}}
  for(node,leaf)in &self.remembered{valid_uuid(node)?;valid_uuid(leaf)?;if !graph.descendant(leaf,node){return deny("chat metadata");}}
  for(session,leaf)in &self.session_leaves{valid_uuid(session)?;valid_uuid(leaf)?;if graph.node(session)?.session!=*session||graph.node(leaf)?.session!=*session{return deny("chat metadata");}}Ok(())
 }
 pub(super) fn generation(&self)->u64{self.generation}
 fn remember(&mut self,graph:&Graph,leaf:&str)->Result<()>{let session=graph.node(leaf)?.session.clone();if self.archived.contains(&session){return deny("chat archived");}for id in graph.path(leaf)?{self.remembered.insert(id,leaf.into());}self.session_leaves.insert(session.clone(),leaf.into());self.active_session=Some(session);self.active_leaf=Some(leaf.into());Ok(())}
 fn select(&mut self,graph:&Graph,id:Option<&str>)->Result<()>{if let Some(id)=id{let node=graph.node(id)?;if node.hidden{return deny("chat hidden node");}if self.archived.contains(&node.session){return deny("chat archived");}let leaf=self.remembered.get(id).filter(|leaf|graph.descendant(leaf,id)&&graph.node(leaf).is_ok_and(|n|!n.hidden)).cloned().map(Ok).unwrap_or_else(||graph.newest_leaf(id))?;self.remember(graph,&leaf)?;}else{self.active_session=None;self.active_leaf=None;}Ok(())}
 pub(super) fn write(&mut self,conn:&Connection,conv:&str,graph:&Graph)->Result<()>{self.validate(graph)?;self.generation=self.generation.checked_add(1).ok_or(DataError::Refused("chat generation"))?;let raw=serde_json::to_string(self).map_err(|_|DataError::Refused("chat metadata"))?;if raw.len()>1024*1024{return deny("chat metadata budget");}conn.execute("INSERT INTO data_meta VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![key(conv),raw])?;Ok(())}
 pub(super) fn after_begin(&mut self,conn:&Connection,conv:&str,answer:&str)->Result<()>{let graph=Graph::load(conn,conv)?;self.remember(&graph,answer)?;self.write(conn,conv,&graph)}
 fn reset_active(&mut self,graph:&Graph)->Result<()>{self.active_session=None;self.active_leaf=None;let next=graph.nodes.iter().rev().find(|n|!n.hidden&&!self.archived.contains(&n.session)).map(|n|n.id.clone());if let Some(next)=next{self.remember(graph,&next)?;}Ok(())}
}
impl Store{
 pub(crate) fn validate_chat_select(&self,conv:&str,id:Option<&str>)->Result<()>{let graph=Graph::load(&self.conn,conv)?;let state=ChatState::read(&self.conn,conv,&graph)?;if let Some(id)=id{valid_uuid(id)?;let node=graph.node(id)?;if node.hidden{return deny("chat hidden node");}if state.archived.contains(&node.session){return deny("chat archived");}}Ok(())}
 pub(crate) fn validate_chat_session(&self,conv:&str,id:&str)->Result<()>{valid_uuid(id)?;let graph=Graph::load(&self.conn,conv)?;ChatState::read(&self.conn,conv,&graph)?;if graph.node(id)?.session!=id{return deny("chat session target");}Ok(())}
 pub(crate) fn chat_view(&self,conv:&str)->Result<ChatView>{let graph=Graph::load(&self.conn,conv)?;let state=ChatState::read(&self.conn,conv,&graph)?;self.chat_render(conv,&graph,&state)}
 fn chat_render(&self,conv:&str,graph:&Graph,state:&ChatState)->Result<ChatView>{
  let mut sessions=Vec::new();for root in graph.roots(){let leaf=state.session_leaves.get(&root.id).cloned().unwrap_or_else(||graph.nodes.iter().rev().find(|n|n.session==root.id&&!n.hidden).map(|n|n.id.clone()).unwrap_or_else(||root.id.clone()));sessions.push(Session{id:root.id.clone(),title:root.title.clone(),archived:state.archived.contains(&root.id),leaf_id:leaf});}sessions.reverse();
  let mut path=Vec::new();let mut versions=Vec::new();let mut bytes=0usize;
  for id in state.active_leaf.as_ref().map(|id|graph.path(id)).transpose()?.unwrap_or_default(){let node=graph.node(&id)?;let family=graph.nodes.iter().filter(|n|!n.hidden&&n.role==node.role&&if node.role=="user"{n.family==node.family}else{n.parent==node.parent}).collect::<Vec<_>>();let index=family.iter().position(|n|n.id==id);
   if let Some(index)=index{versions.push(Version{message_id:id.clone(),index:index+1,total:family.len(),previous_id:index.checked_sub(1).map(|i|family[i].id.clone()),next_id:family.get(index+1).map(|n|n.id.clone())});}
   let item=self.conn.query_row("SELECT content,status,created_ms,(SELECT page FROM anchors WHERE message_id=m.id OR message_id=m.parent_id OR message_id=m.edited_from ORDER BY (message_id=m.id) DESC,rowid LIMIT 1),metadata FROM messages m WHERE id=?1 AND conversation_id=?2",params![id,conv],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,i64>(2)?,r.get::<_,Option<u32>>(3)?,r.get::<_,String>(4)?)))?;
   let metadata:MessageMetadata=serde_json::from_str(&item.4).map_err(|_|DataError::Refused("metadata JSON"))?;metadata.validate()?;bytes+=item.0.len();if bytes>16*1024*1024{return deny("chat path budget");}path.push(HistoryMessage{id,parent:node.parent.clone(),edited_from:node.edit.clone(),content:if node.hidden{String::new()}else{item.0},status:item.1,role:node.role.clone(),created_ms:item.2,page:item.3,deleted:node.hidden,metadata,truncated:false});
  }Ok(ChatView{session_id:state.active_session.clone(),leaf_id:state.active_leaf.clone(),sessions,path,versions})
 }
 pub(crate) fn chat_select(&mut self,conv:&str,id:Option<&str>)->Result<ChatView>{self.writable()?;let graph=Graph::load(&self.conn,conv)?;let mut state=ChatState::read(&self.conn,conv,&graph)?;state.select(&graph,id)?;let view=self.chat_render(conv,&graph,&state)?;state.write(&self.conn,conv,&graph)?;Ok(view)}
 pub(crate) fn chat_archive(&mut self,conv:&str,session:&str,archived:bool)->Result<ChatView>{self.writable()?;valid_uuid(session)?;let graph=Graph::load(&self.conn,conv)?;if graph.node(session)?.session!=session{return deny("chat session target");}let mut state=ChatState::read(&self.conn,conv,&graph)?;state.archived.retain(|id|id!=session);if archived{state.archived.push(session.into());if state.active_session.as_deref()==Some(session){state.reset_active(&graph)?;}}let view=self.chat_render(conv,&graph,&state)?;state.write(&self.conn,conv,&graph)?;Ok(view)}
 pub(crate) fn chat_generation(&self,conv:&str)->Result<u64>{let graph=Graph::load(&self.conn,conv)?;Ok(ChatState::read(&self.conn,conv,&graph)?.generation())}
 pub(crate) fn chat_session_open(&self,conv:&str,id:&str)->Result<()>{let graph=Graph::load(&self.conn,conv)?;let state=ChatState::read(&self.conn,conv,&graph)?;if state.archived.contains(&graph.node(id)?.session){return deny("chat archived");}Ok(())}
 pub(crate) fn chat_delete(&mut self,conv:&str,session:&str)->Result<usize>{self.chat_delete_impl(conv,session,false)}
 #[cfg(test)]pub(crate) fn chat_delete_interrupted(&mut self,conv:&str,session:&str)->Result<usize>{self.chat_delete_impl(conv,session,true)}
 fn chat_delete_impl(&mut self,conv:&str,session:&str,interrupt:bool)->Result<usize>{
  self.writable()?;valid_uuid(session)?;let graph=Graph::load(&self.conn,conv)?;if graph.node(session)?.session!=session{return deny("chat session target");}let mut state=ChatState::read(&self.conn,conv,&graph)?;let ids=graph.nodes.iter().filter(|n|n.session==session).map(|n|n.id.clone()).collect::<HashSet<_>>();
  let reset=state.active_session.as_deref()==Some(session);state.archived.retain(|id|id!=session);state.remembered.retain(|node,leaf|!ids.contains(node)&&!ids.contains(leaf));state.session_leaves.remove(session);if reset{state.active_session=None;state.active_leaf=None;}
  let raw=serde_json::to_string(&ids).map_err(|_|DataError::Refused("chat delete IDs"))?;let tx=self.conn.transaction()?;
  if tx.prepare("SELECT 1 FROM anchors a JOIN messages m ON m.id=a.message_id WHERE m.id IN(SELECT value FROM json_each(?1)) AND a.document_id<>m.document_id")?.exists([&raw])?{return deny("inconsistent history anchor ownership");}
  for prefix in ["message-hidden:","ai-context:","ai-usage:"]{tx.execute("DELETE FROM data_meta WHERE key IN(SELECT ?1||value FROM json_each(?2))",params![prefix,raw])?;}
  tx.execute("DELETE FROM message_search WHERE message_id IN(SELECT value FROM json_each(?1))",[&raw])?;tx.execute("DELETE FROM anchors WHERE message_id IN(SELECT value FROM json_each(?1))",[&raw])?;tx.execute("DELETE FROM message_chunks WHERE message_id IN(SELECT value FROM json_each(?1))",[&raw])?;
  // Delete the entire closed graph in one statement; FK enforcement stays on.
  tx.execute("DELETE FROM messages WHERE conversation_id=?1 AND id IN(SELECT value FROM json_each(?2))",params![conv,raw])?;
  if interrupt{return deny("chat delete interrupted");}let remaining=Graph::load(&tx,conv)?;if reset{state.reset_active(&remaining)?;}state.write(&tx,conv,&remaining)?;integrity(&tx)?;tx.commit()?;Ok(ids.len())
 }
}
