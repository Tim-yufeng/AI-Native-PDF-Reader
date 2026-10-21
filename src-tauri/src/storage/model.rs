use super::*;
use rusqlite::params;
use serde::{Deserialize,Serialize};
#[path="desktop_model.rs"] pub(crate) mod desktop;
#[derive(Debug,PartialEq)] pub(crate) enum IdentityMatch { Known(String), Confirm(Vec<String>), New }
#[derive(Clone,Debug,Serialize,Deserialize,PartialEq)]
pub(crate) struct MessageMetadata {
    pub provider:Option<String>,pub model:Option<String>,pub preset_version:Option<String>,
    pub request_id:Option<String>,pub sent_context_hash:Option<String>,
    // Which configured slot answered this round ("main" or "auxiliary"). The
    // provider/model pair already names the exact model, and the slot is kept so
    // a reader can tell an explicit auxiliary answer from a main-model one.
    #[serde(default)] pub slot:Option<String>,
}
impl MessageMetadata {
    pub(super) fn validate(&self)->Result<()> {
        for s in [&self.provider,&self.model,&self.preset_version]{if s.as_ref().is_some_and(|x|x.len()>256){return deny("metadata too long");}}
        if self.slot.as_ref().is_some_and(|s|s!="main"&&s!="auxiliary"){return deny("metadata slot");}
        if let Some(id)=&self.request_id{valid_uuid(id)?;}
        if let Some(hash)=&self.sent_context_hash{valid_hash(hash)?;}Ok(())
    }
}
#[derive(Debug,PartialEq)] pub(crate) struct Message {
    pub id:String,pub parent:Option<String>,pub edited_from:Option<String>,pub content:String,pub status:String,
}
#[derive(Debug,Serialize,Deserialize,PartialEq)] pub(crate) struct ReaderSettings {
    pub schema_version:u32,pub thumbnail_open:bool,pub cache_bytes:u64,
    #[serde(flatten)] pub extensions:std::collections::BTreeMap<String,serde_json::Value>,
}
impl ReaderSettings {
    fn validate(&self)->Result<()> {
        if self.schema_version!=1{return deny("future settings schema");}
        // Unknown fields from existing settings survive typed updates. No general
        // key/value setter or credential field is offered to renderer callers.
        if serde_json::to_vec(self).map_err(|_|DataError::Refused("settings JSON"))?.len()>64*1024{return deny("settings too large");}Ok(())
    }
}
/// Deliberately synthetic: not a receipt for a real PDF transaction. No serde/IPC.
pub(crate) struct SyntheticSaveEvent {
    pub event_id:String,pub document:String,pub old_revision:String,pub old_hash:String,
    pub new_hash:String,pub pages:u32,pub new_file_key:String,
}
impl Store {
    pub(crate) fn create_document(&mut self,file_key:&str,hash:&str,pages:u32,name_hint:&str)->Result<(String,String)> {
        self.writable()?;valid_hash(hash)?;
        if file_key.is_empty()||file_key.len()>256||name_hint.len()>1024||pages==0{return deny("document metadata");}
        let doc=uuid();let rev=uuid();let tx=self.conn.transaction()?;
        tx.execute("INSERT INTO documents VALUES(?1,NULL)",[&doc])?;
        tx.execute("INSERT INTO revisions VALUES(?1,?2,?3,?4,NULL,'opened')",params![rev,doc,hash,pages])?;
        tx.execute("UPDATE documents SET current_revision=?1 WHERE id=?2",params![rev,doc])?;
        tx.execute("INSERT INTO locations VALUES(?1,?2,?3,?4)",params![doc,file_key,hash,name_hint])?;
        tx.commit()?;Ok((doc,rev))
    }
    pub(crate) fn identify(&self,file_key:&str,hash:&str)->Result<IdentityMatch> {
        valid_hash(hash)?;
        let exact=self.conn.prepare("SELECT document_id FROM locations WHERE file_key=?1 AND hash=?2 ORDER BY document_id")?
            .query_map(params![file_key,hash],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        if exact.len()==1{return Ok(IdentityMatch::Known(exact[0].clone()));}
        let candidates=self.conn.prepare("SELECT DISTINCT document_id FROM locations WHERE file_key=?1 OR hash=?2 ORDER BY document_id")?
            .query_map(params![file_key,hash],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(if candidates.is_empty(){IdentityMatch::New}else{IdentityMatch::Confirm(candidates)})
    }
    pub(crate) fn conversation(&mut self,doc:&str)->Result<String> {
        self.writable()?;valid_uuid(doc)?;let id=uuid();
        self.conn.execute("INSERT INTO conversations VALUES(?1,?2)",params![id,doc])?;Ok(id)
    }
    pub(crate) fn message(&mut self,conversation:&str,revision:&str,parent:Option<&str>,content:&str,assistant:bool,meta:&MessageMetadata)->Result<String> {
        self.writable()?;valid_uuid(conversation)?;valid_uuid(revision)?;meta.validate()?;
        if content.len()>1024*1024{return deny("message too large");}
        let id=uuid();let doc:String=self.conn.query_row("SELECT document_id FROM conversations WHERE id=?1",[conversation],|r|r.get(0))?;
        self.conn.execute("INSERT INTO messages(id,conversation_id,document_id,revision_id,parent_id,role,status,content,metadata,created_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,CAST(unixepoch('subsec')*1000 AS INTEGER))",
            params![id,conversation,doc,revision,parent,if assistant{"assistant"}else{"user"},if assistant{"streaming"}else{"complete"},content,serde_json::to_string(meta).unwrap()])?;
        Ok(id)
    }
    pub(crate) fn edit_prompt(&mut self,id:&str,content:&str)->Result<String> {
        self.writable()?;
        if content.len()>1024*1024{return deny("message too large");}
        let next=uuid();let tx=self.conn.transaction()?;
        let affected=tx.execute("INSERT INTO messages(id,conversation_id,document_id,revision_id,parent_id,edited_from,role,status,content,metadata,created_ms) SELECT ?1,conversation_id,document_id,revision_id,parent_id,id,role,'complete',?2,metadata,CAST(unixepoch('subsec')*1000 AS INTEGER) FROM messages WHERE id=?3 AND role='user' AND status='complete'",params![next,content,id])?;
        if affected!=1{return deny("not editable prompt");}tx.commit()?;Ok(next)
    }
    pub(crate) fn append_chunk(&mut self,message:&str,request:&str,sequence:u32,content:&str)->Result<()> {
        self.writable()?;valid_uuid(request)?;
        if content.len()>64*1024{return deny("chunk too large");}
        let tx=self.conn.transaction()?;
        let (status,next,metadata,length):(String,u32,String,i64)=tx.query_row("SELECT status,next_seq,metadata,length(CAST(content AS BLOB)) FROM messages WHERE id=?1 AND role='assistant'",[message],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
        let meta:MessageMetadata=serde_json::from_str(&metadata).map_err(|_|DataError::Refused("metadata JSON"))?;
        if status!="streaming"||next!=sequence||meta.request_id.as_deref()!=Some(request)||length+content.len() as i64>1024*1024{return deny("late/repeated/out-of-order chunk");}
        tx.execute("INSERT INTO message_chunks VALUES(?1,?2,?3)",params![message,sequence,content])?;
        tx.execute("UPDATE messages SET content=content||?1,next_seq=next_seq+1 WHERE id=?2",params![content,message])?;
        tx.commit()?;Ok(())
    }
    pub(crate) fn finish(&mut self,message:&str,status:&str)->Result<()> {
        self.writable()?;if !matches!(status,"complete"|"cancelled"|"failed"){return deny("invalid terminal status");}
        if self.conn.execute("UPDATE messages SET status=?1 WHERE id=?2 AND status='streaming'",params![status,message])?!=1{return deny("already terminal");}Ok(())
    }
    pub(crate) fn messages(&self,conversation:&str)->Result<Vec<Message>> {
        let items=self.conn.prepare("SELECT id,parent_id,edited_from,content,status FROM messages WHERE conversation_id=?1 ORDER BY rowid")?
            .query_map([conversation],|r|Ok(Message{id:r.get(0)?,parent:r.get(1)?,edited_from:r.get(2)?,content:r.get(3)?,status:r.get(4)?}))?.collect::<rusqlite::Result<Vec<_>>>()?;Ok(items)
    }
    pub(crate) fn search(&self,query:&str)->Result<Vec<String>> {
        if query.len()>1024{return deny("query length");}
        Ok(self.conn.prepare("SELECT message_id FROM message_search WHERE message_search MATCH ?1 AND NOT EXISTS(SELECT 1 FROM data_meta WHERE key='message-hidden:'||message_search.message_id) LIMIT 100")?
            .query_map([query],|r|r.get(0))?.collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub(crate) fn anchor(&mut self,message:&str,revision:&str,page:u32,rect:[f64;4])->Result<String> {
        self.writable()?;
        if page==0||rect.iter().any(|v|!v.is_finite())||rect[0]>rect[2]||rect[1]>rect[3]{return deny("invalid anchor");}
        let(doc,pages):(String,u32)=self.conn.query_row("SELECT m.document_id,r.pages FROM messages m JOIN revisions r ON r.document_id=m.document_id AND r.id=?1 WHERE m.id=?2",params![revision,message],|r|Ok((r.get(0)?,r.get(1)?)))?;
        if page>pages{return deny("anchor page");}
        let id=uuid();self.conn.execute("INSERT INTO anchors VALUES(?1,?2,?3,?4,?5,?6)",params![id,message,doc,revision,page,serde_json::to_string(&rect).unwrap()])?;Ok(id)
    }
    pub(crate) fn anchor_current(&self,id:&str)->Result<bool> {
        Ok(self.conn.query_row("SELECT a.revision_id=d.current_revision FROM anchors a JOIN documents d ON d.id=a.document_id WHERE a.id=?1",[id],|r|r.get(0))?)
    }
    pub(crate) fn save_event_synthetic(&mut self,event:&SyntheticSaveEvent)->Result<String> {
        self.writable()?;valid_uuid(&event.event_id)?;valid_hash(&event.old_hash)?;valid_hash(&event.new_hash)?;
        if event.pages==0||event.new_file_key.is_empty()||event.new_file_key.len()>256{return deny("save event metadata");}
        let tx=self.conn.transaction()?;
        let prior:Option<(String,String,String,String,String)>=tx.query_row("SELECT new_revision,document_id,old_revision,new_hash,new_file_key FROM save_events WHERE event_id=?1",[&event.event_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?;
        if let Some((revision,doc,old,hash,key))=prior {
            let(old_hash,pages):(String,u32)=tx.query_row("SELECT o.hash,n.pages FROM revisions o JOIN revisions n ON n.id=?1 WHERE o.id=?2",params![revision,old],|r|Ok((r.get(0)?,r.get(1)?)))?;
            if doc!=event.document||old!=event.old_revision||hash!=event.new_hash||key!=event.new_file_key||old_hash!=event.old_hash||pages!=event.pages{return deny("event UUID reused");}return Ok(revision);
        }
        let current:(String,String)=tx.query_row("SELECT r.id,r.hash FROM documents d JOIN revisions r ON r.id=d.current_revision AND r.document_id=d.id WHERE d.id=?1",[&event.document],|r|Ok((r.get(0)?,r.get(1)?)))?;
        if current!=(event.old_revision.clone(),event.old_hash.clone()){return deny("stale save event");}
        let new_revision=uuid();
        tx.execute("INSERT INTO revisions VALUES(?1,?2,?3,?4,?5,'synthetic-save')",params![new_revision,event.document,event.new_hash,event.pages,event.old_revision])?;
        tx.execute("UPDATE documents SET current_revision=?1 WHERE id=?2",params![new_revision,event.document])?;
        tx.execute("INSERT INTO save_events VALUES(?1,?2,?3,?4,?5,?6)",params![event.event_id,event.document,event.old_revision,new_revision,event.new_hash,event.new_file_key])?;
        // The event establishes lineage; stale file IDs cease to be auto matches.
        tx.execute("DELETE FROM locations WHERE document_id=?1",[&event.document])?;
        tx.execute("INSERT INTO locations VALUES(?1,?2,?3,'synthetic receipt')",params![event.document,event.new_file_key,event.new_hash])?;
        tx.commit()?;Ok(new_revision)
    }
    pub(crate) fn position(&mut self,doc:&str,revision:&str,page:u32,zoom:f64,scroll:f64)->Result<()> {
        self.writable()?;
        let pages:u32=self.conn.query_row("SELECT pages FROM revisions WHERE id=?1 AND document_id=?2",params![revision,doc],|r|r.get(0))?;
        if page==0||page>pages||!zoom.is_finite()||!(25.0..=400.0).contains(&zoom)||!scroll.is_finite()||scroll<0.0{return deny("invalid reading position");}
        self.conn.execute("INSERT INTO positions VALUES(?1,?2,?3,?4,?5) ON CONFLICT(document_id) DO UPDATE SET revision_id=excluded.revision_id,page=excluded.page,zoom=excluded.zoom,scroll=excluded.scroll",params![doc,revision,page,zoom,scroll])?;Ok(())
    }
    pub(crate) fn read_position(&self,doc:&str,revision:&str)->Result<Option<(u32,f64,f64)>> {
        Ok(self.conn.query_row("SELECT page,zoom,scroll FROM positions WHERE document_id=?1 AND revision_id=?2",params![doc,revision],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?)
    }
    pub(crate) fn settings(&self)->Result<Option<ReaderSettings>> {
        let raw:Option<(u32,String)>=self.conn.query_row("SELECT schema_version,value FROM settings WHERE key='reader'",[],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        raw.map(|(version,s)|{if version!=1{return deny("future reader settings");}let value:ReaderSettings=serde_json::from_str(&s).map_err(|_|DataError::Refused("settings JSON"))?;value.validate()?;Ok(value)}).transpose()
    }
    pub(crate) fn update_settings(&mut self,thumbnail_open:bool,cache_bytes:u64)->Result<()> {
        self.writable()?;
        let existing:Option<(u32,String)>=self.conn.query_row("SELECT schema_version,value FROM settings WHERE key='reader'",[],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        let mut value=if let Some((version,raw))=existing{if version!=1{return deny("future settings");}serde_json::from_str::<ReaderSettings>(&raw).map_err(|_|DataError::Refused("settings JSON"))?}else{ReaderSettings{schema_version:1,thumbnail_open,cache_bytes,extensions:Default::default()}};
        value.thumbnail_open=thumbnail_open;value.cache_bytes=cache_bytes;value.validate()?;
        self.conn.execute("INSERT INTO settings VALUES('reader',1,?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[serde_json::to_string(&value).unwrap()])?;
        self.quota=cache_bytes;self.trim_cache()?;Ok(())
    }
}
