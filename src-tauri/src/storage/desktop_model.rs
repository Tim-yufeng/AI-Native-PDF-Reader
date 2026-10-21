//! Typed desktop operations. SQL, paths and arbitrary metadata keys are private.
use super::*;
#[path="save_receipt.rs"] pub(crate) mod saves;
#[derive(Clone,Debug,Serialize,Deserialize,PartialEq)]
#[serde(untagged)]
pub(crate) enum Zoom { Fit(String), Scale(f64) }
#[derive(Clone,Debug,Serialize,Deserialize,PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReadingPosition {pub page:u32,pub zoom:Zoom,pub top:f64,pub left:f64}
impl ReadingPosition {
    fn validate(&self,pages:u32)->Result<()> {
        let zoom=match &self.zoom{Zoom::Fit(v)=>matches!(v.as_str(),"page-width"|"page-fit"),Zoom::Scale(v)=>v.is_finite()&&(0.25..=4.0).contains(v)};
        if self.page==0||self.page>pages||!zoom||!self.top.is_finite()||!(-2.0..=2.0).contains(&self.top)||!self.left.is_finite()||!(-2.0..=2.0).contains(&self.left){return deny("reading position");}Ok(())
    }
}
#[derive(Serialize)]
#[serde(rename_all="camelCase")]
pub(crate) struct HistoryMessage {
    pub id:String,pub parent:Option<String>,pub edited_from:Option<String>,pub content:String,pub status:String,
    pub role:String,pub created_ms:i64,pub page:Option<u32>,pub deleted:bool,pub metadata:MessageMetadata,pub truncated:bool,
}
#[derive(Serialize)] pub(crate) struct DraftRecord {pub id:String,pub json:String,pub updated:i64}
#[derive(Serialize,Deserialize)] struct DraftRef {file:String,updated:i64}
fn now()->i64 {std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis().min(i64::MAX as u128) as i64}
fn draft_key(doc:&str,revision:&str,id:&str)->String {format!("draft:{doc}:{revision}:{id}")}
impl Store {
    pub(crate) fn current_revision(&self,doc:&str)->Result<(String,u32)> {
        Ok(self.conn.query_row("SELECT r.id,r.pages FROM documents d JOIN revisions r ON r.id=d.current_revision AND r.document_id=d.id WHERE d.id=?1",[doc],|r|Ok((r.get(0)?,r.get(1)?)))?)
    }
    pub(crate) fn local_conversation(&mut self,doc:&str)->Result<String> {
        let existing:Option<String>=self.conn.query_row("SELECT id FROM conversations WHERE document_id=?1 ORDER BY rowid LIMIT 1",[doc],|r|r.get(0)).optional()?;
        existing.map(Ok).unwrap_or_else(||self.conversation(doc))
    }
    pub(crate) fn desktop_position(&self,doc:&str,rev:&str)->Result<Option<ReadingPosition>> {
        let raw:Option<String>=self.conn.query_row("SELECT value FROM data_meta WHERE key=?1",[format!("reader-position:{doc}:{rev}")],|r|r.get(0)).optional()?;
        raw.map(|json|{let value:ReadingPosition=serde_json::from_str(&json).map_err(|_|DataError::Refused("position JSON"))?;let pages:u32=self.conn.query_row("SELECT pages FROM revisions WHERE id=?1 AND document_id=?2",params![rev,doc],|r|r.get(0))?;value.validate(pages)?;Ok(value)}).transpose()
    }
    pub(crate) fn put_desktop_position(&mut self,doc:&str,rev:&str,value:&ReadingPosition)->Result<()> {
        self.writable()?;
        let pages:u32=self.conn.query_row("SELECT pages FROM revisions WHERE id=?1 AND document_id=?2",params![rev,doc],|r|r.get(0))?;value.validate(pages)?;
        let zoom=match value.zoom{Zoom::Scale(v)=>v*100.0,Zoom::Fit(_)=>100.0};
        let tx=self.conn.transaction()?;
        tx.execute("INSERT INTO positions VALUES(?1,?2,?3,?4,?5) ON CONFLICT(document_id) DO UPDATE SET revision_id=excluded.revision_id,page=excluded.page,zoom=excluded.zoom,scroll=excluded.scroll",params![doc,rev,value.page,zoom,value.top+2.0])?;
        tx.execute("INSERT INTO data_meta VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![format!("reader-position:{doc}:{rev}"),serde_json::to_string(value).unwrap()])?;
        tx.commit()?;Ok(())
    }
    pub(crate) fn check_message_owner(&self,conv:&str,id:&str)->Result<()> {
        valid_uuid(id)?;
        if !self.conn.prepare("SELECT 1 FROM messages WHERE id=?1 AND conversation_id=?2 AND NOT EXISTS(SELECT 1 FROM data_meta WHERE key='message-hidden:'||messages.id)")?.exists(params![id,conv])?{return deny("message outside document binding/hidden");}Ok(())
    }
    pub(crate) fn local_messages(&self,conv:&str)->Result<Vec<HistoryMessage>> {
        // Display committed order; OS/SQLite wall clocks can differ or move back.
        let mut statement=self.conn.prepare("SELECT m.id,m.parent_id,m.edited_from,substr(m.content,1,16384),m.status,m.role,m.created_ms,(SELECT page FROM anchors WHERE message_id=m.id OR message_id=m.parent_id OR message_id=m.edited_from ORDER BY (message_id=m.id) DESC,rowid LIMIT 1),EXISTS(SELECT 1 FROM data_meta WHERE key='message-hidden:'||m.id),m.metadata,length(m.content)>16384 FROM messages m WHERE conversation_id=?1 ORDER BY rowid DESC LIMIT 100")?;
        let rows=statement.query_map([conv],|r|Ok((r.get::<_,String>(0)?,r.get(1)?,r.get(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,String>(5)?,r.get::<_,i64>(6)?,r.get(7)?,r.get::<_,bool>(8)?,r.get::<_,String>(9)?,r.get::<_,bool>(10)?)))?;
        let mut result=Vec::new();
        for row in rows {let(id,parent,edited_from,content,status,role,created_ms,page,deleted,meta,truncated)=row?;let metadata:MessageMetadata=serde_json::from_str(&meta).map_err(|_|DataError::Refused("metadata JSON"))?;
            result.push(HistoryMessage{id,parent,edited_from,content:if deleted{String::new()}else{content},status,role,created_ms,page,deleted,metadata,truncated});}
        result.reverse();
        Ok(result)
    }
    pub(crate) fn edit_local_prompt(&mut self,conv:&str,id:&str,text:&str)->Result<String>{
        self.check_message_owner(conv,id)?;
        super::super::chat_view::Graph::load(&self.conn,conv)?.budget(1)?;self.chat_session_open(conv,id)?;
        if text.trim().is_empty()||text.len()>16*1024{return deny("local prompt length");}self.writable()?;
        let next=uuid();let tx=self.conn.transaction()?;
        if tx.execute("INSERT INTO messages(id,conversation_id,document_id,revision_id,parent_id,edited_from,role,status,content,metadata,created_ms) SELECT ?1,conversation_id,document_id,revision_id,parent_id,id,role,'complete',?2,metadata,?3 FROM messages WHERE id=?4 AND role='user' AND status='complete'",params![next,text,now(),id])?!=1{return deny("not editable prompt");}
        tx.execute("INSERT INTO anchors SELECT ?1,?2,document_id,revision_id,page,rect FROM anchors WHERE message_id=?3 ORDER BY rowid LIMIT 1",params![uuid(),next,id])?;tx.commit()?;Ok(next)
    }
    pub(crate) fn default_zoom(&self)->Result<Option<String>>{Ok(self.conn.query_row("SELECT value FROM data_meta WHERE key='reader-default-zoom'",[],|r|r.get(0)).optional()?)}
    pub(crate) fn set_default_zoom(&mut self,value:&str)->Result<()>{self.writable()?;if !matches!(value,"page-width"|"page-fit"|"actual-size"){return deny("default zoom");}
        self.conn.execute("INSERT INTO data_meta VALUES('reader-default-zoom',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[value])?;Ok(())}
    pub(crate) fn add_local_prompt(&mut self,conv:&str,rev:&str,parent:Option<&str>,text:&str,page:u32)->Result<String> {
        super::super::chat_view::Graph::load(&self.conn,conv)?.budget(1)?;if let Some(id)=parent{self.chat_session_open(conv,id)?;}
        if text.trim().is_empty()||text.len()>16*1024{return deny("local prompt length");}
        if let Some(id)=parent{self.check_message_owner(conv,id)?;}
        let meta=MessageMetadata{provider:Some("synthetic".into()),model:Some("local-fixture".into()),preset_version:None,request_id:None,sent_context_hash:None,slot:None};
        let pages:u32=self.conn.query_row("SELECT pages FROM revisions WHERE id=?1",[rev],|r|r.get(0))?;
        if page==0||page>pages{return deny("anchor page");}
        self.writable()?;
        // Message and anchor commit together; failed anchor never leaves a half UI record.
        let doc:String=self.conn.query_row("SELECT document_id FROM conversations WHERE id=?1",[conv],|r|r.get(0))?;
        let id=uuid();let tx=self.conn.transaction()?;
        tx.execute("INSERT INTO messages(id,conversation_id,document_id,revision_id,parent_id,role,status,content,metadata,created_ms) VALUES(?1,?2,?3,?4,?5,'user','complete',?6,?7,?8)",params![id,conv,doc,rev,parent,text,serde_json::to_string(&meta).unwrap(),now()])?;
        tx.execute("INSERT INTO anchors VALUES(?1,?2,?3,?4,?5,'[0,0,1,1]')",params![uuid(),id,doc,rev,page])?;tx.commit()?;Ok(id)
    }
    pub(crate) fn local_regenerate(&mut self,conv:&str,rev:&str,parent:&str)->Result<String> {
        self.check_message_owner(conv,parent)?;
        super::super::chat_view::Graph::load(&self.conn,conv)?.budget(1)?;self.chat_session_open(conv,parent)?;
        let role:String=self.conn.query_row("SELECT role FROM messages WHERE id=?1",[parent],|r|r.get(0))?;
        if role!="user"{return deny("regeneration parent is not a prompt");}
        let request=uuid();let meta=MessageMetadata{provider:Some("synthetic".into()),model:Some("local-fixture".into()),preset_version:None,request_id:Some(request.clone()),sent_context_hash:None,slot:None};
        let id=self.message(conv,rev,Some(parent),"",true,&meta)?;
        self.append_chunk(&id,&request,0,"本地合成记录，用于验证历史分支；")?;
        self.append_chunk(&id,&request,1,"没有调用 AI。")?;self.finish(&id,"complete")?;Ok(id)
    }
    pub(crate) fn hide_local_message(&mut self,conv:&str,id:&str)->Result<()> {
        self.check_message_owner(conv,id)?;self.writable()?;
        let tx=self.conn.transaction()?;
        tx.execute("INSERT INTO data_meta VALUES(?1,?2) ON CONFLICT(key) DO NOTHING",params![format!("message-hidden:{id}"),now().to_string()])?;
        tx.execute("DELETE FROM message_search WHERE message_id=?1",[id])?;tx.commit()?;Ok(())
    }
    /// User-requested logical deletion of the bound document's entire history.
    /// Empty conversation IDs remain usable by existing tabs. This is not secure
    /// erasure: retained backups and database/WAL free pages may contain old bytes.
    pub(crate) fn delete_document_history(&mut self,document:&str,active_conversation:&str)->Result<usize>{
        self.delete_document_history_impl(document,active_conversation,false)
    }
    #[cfg(test)]
    pub(crate) fn delete_document_history_interrupted(&mut self,document:&str,active_conversation:&str)->Result<usize>{
        self.delete_document_history_impl(document,active_conversation,true)
    }
    fn delete_document_history_impl(&mut self,document:&str,active_conversation:&str,interrupt:bool)->Result<usize>{
        valid_uuid(document)?;valid_uuid(active_conversation)?;self.writable()?;
        let tx=self.conn.transaction()?;
        if !tx.prepare("SELECT 1 FROM conversations WHERE id=?1 AND document_id=?2")?.exists(params![active_conversation,document])?{return deny("history deletion outside document binding");}
        if tx.prepare("SELECT 1 FROM anchors a JOIN messages m ON m.id=a.message_id WHERE m.document_id=?1 AND a.document_id<>?1")?.exists([document])?{return deny("inconsistent history anchor ownership");}
        // Precise message-ID sets only. Never match document strings against
        // generic metadata keys: that would also match reading/draft state.
        tx.execute("DELETE FROM message_search WHERE message_id IN(SELECT id FROM messages WHERE document_id=?1)",[document])?;
        tx.execute("DELETE FROM data_meta WHERE key IN(SELECT 'message-hidden:'||id FROM messages WHERE document_id=?1)",[document])?;
        tx.execute("DELETE FROM data_meta WHERE key IN(SELECT 'ai-context:'||id FROM messages WHERE document_id=?1) OR key IN(SELECT 'ai-usage:'||id FROM messages WHERE document_id=?1)",[document])?;
        tx.execute("DELETE FROM data_meta WHERE key IN(SELECT 'chat-view-v1:'||id FROM conversations WHERE document_id=?1)",[document])?;
        tx.execute("DELETE FROM anchors WHERE message_id IN(SELECT id FROM messages WHERE document_id=?1)",[document])?;
        tx.execute("DELETE FROM message_chunks WHERE message_id IN(SELECT id FROM messages WHERE document_id=?1)",[document])?;
        if interrupt {
            #[cfg(test)]
            {tx.execute_batch("INSERT INTO nonexistent_history_delete_fault VALUES(1)")?;}
            #[cfg(not(test))]
            {return deny("unavailable history deletion interruption");}
        }
        // All branches are deleted together. The schema's NO ACTION parent/edit
        // constraints are checked at statement end, without disabling FK checks.
        let count=tx.execute("DELETE FROM messages WHERE document_id=?1",[document])?;
        integrity(&tx)?;tx.commit()?;Ok(count)
    }
    pub(crate) fn put_local_draft(&mut self,doc:&str,rev:&str,id:&str,json:&str)->Result<()> {
        valid_uuid(id)?;self.writable()?;
        if json.len()>256*1024{return deny("draft size");}
        let value:serde_json::Value=serde_json::from_str(json).map_err(|_|DataError::Refused("draft JSON"))?;
        let pages:u32=self.conn.query_row("SELECT pages FROM revisions WHERE id=?1 AND document_id=?2",params![rev,doc],|r|r.get(0))?;
        validate_draft(&value,pages)?;
        let key=draft_key(doc,rev,id);
        if let Some(existing)=self.conn.query_row("SELECT value FROM data_meta WHERE key=?1",[&key],|r|r.get::<_,String>(0)).optional()?{
            let reference:DraftRef=serde_json::from_str(&existing).map_err(|_|DataError::Refused("draft reference"))?;
            if self.recovery_get(&reference.file)?==json.as_bytes(){return Ok(());}
        }
        // Immutable recovery bytes are synced first. SQL publication failure keeps
        // the old pointer and the new recovery material rather than discarding either.
        let file=self.recovery_put(json.as_bytes())?;
        let reference=serde_json::to_string(&DraftRef{file,updated:now()}).unwrap();
        self.conn.execute("INSERT INTO data_meta VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![key,reference])?;Ok(())
    }
    pub(crate) fn local_drafts(&self,doc:&str,rev:&str)->Result<Vec<DraftRecord>> {
        let prefix=format!("draft:{doc}:{rev}:");
        let mut statement=self.conn.prepare("SELECT key,value FROM data_meta WHERE key LIKE ?1 ORDER BY rowid LIMIT 32")?;
        let rows=statement.query_map([format!("{prefix}%")],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?;
        let mut result=Vec::new();
        for row in rows {let(key,raw)=row?;let id=key.strip_prefix(&prefix).ok_or(DataError::Refused("draft key"))?.to_string();valid_uuid(&id)?;
            let reference:DraftRef=serde_json::from_str(&raw).map_err(|_|DataError::Refused("draft reference"))?;
            let json=String::from_utf8(self.recovery_get(&reference.file)?).map_err(|_|DataError::Refused("draft encoding"))?;result.push(DraftRecord{id,json,updated:reference.updated});}
        Ok(result)
    }
    pub(crate) fn drop_local_draft(&mut self,doc:&str,rev:&str,id:&str)->Result<()> {
        valid_uuid(id)?;self.writable()?;
        // Explicit dismissal removes only the live mapping. Recovery snapshots stay
        // available in retained backup material and are never cache quota victims.
        self.conn.execute("DELETE FROM data_meta WHERE key=?1",[draft_key(doc,rev,id)])?;Ok(())
    }
}
pub(crate) fn validate_draft(v:&serde_json::Value,pages:u32)->Result<()> {
    let entries=v.get("entries").and_then(|x|x.as_array()).ok_or(DataError::Refused("draft entries"))?;
    if v.get("schema").and_then(|x|x.as_u64())!=Some(1)||v.get("pdfjs").and_then(|x|x.as_str())!=Some("6.4.299")||entries.len()>500{return deny("draft schema");}
    fn inspect(v:&serde_json::Value,depth:u32)->Result<()> {
        if depth>12{return deny("draft depth");}
        if let Some(object)=v.as_object(){for(k,v)in object{if matches!(k.as_str(),"__proto__"|"constructor"|"prototype"){return deny("draft field");}inspect(v,depth+1)?;}}
        if let Some(array)=v.as_array(){for v in array{inspect(v,depth+1)?;}}
        if let Some(n)=v.as_f64(){if !n.is_finite()||n.abs()>1e7{return deny("draft number");}}Ok(())
    }
    inspect(v,0)?;
    for e in entries {
        if !matches!(e.get("annotationType").and_then(|x|x.as_u64()),Some(3|9|15))||e.get("pageIndex").and_then(|x|x.as_u64()).is_none_or(|p|p>=pages as u64)||!matches!(e.get("rotation").and_then(|x|x.as_u64()),Some(0|90|180|270))||e.get("id").is_some_and(|x|!x.is_null())||e.get("annotationElementId").is_some_and(|x|!x.is_null())||e.get("deleted").and_then(|x|x.as_bool()).unwrap_or(false){return deny("draft entry");}
        if e.get("rect").and_then(|x|x.as_array()).is_none_or(|a|a.len()!=4||a.iter().any(|x|x.as_f64().is_none())){return deny("draft rect");}
    }Ok(())
}
