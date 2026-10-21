//! Internal PDF-commit receipts only. No renderer command or JSON receipt input.
//! The PDF operation precedes this SQLite transaction: a database failure means
//! pending association, never that the already committed PDF was unchanged.
use super::*;
use crate::original_commit::VerifiedReceipt;
use sha2::{Digest,Sha256};

/// Captured from an existing immutable recovery record, not renderer assertions.
/// The private fields and lack of Deserialize prevent an IPC caller from
/// substituting this recovery snapshot. Capture does NOT prove PDF inclusion.
pub(crate) struct SaveDraftSnapshot {
    document:String,revision:String,id:String,digest:String,
}
#[derive(Debug,PartialEq)]
pub(crate) struct AppliedSave {
    pub revision:String,pub is_current:bool,
    /// Always false until an independent inclusion check binds the frozen
    /// annotation snapshot to the committed PDF. No live draft retires here.
    pub draft_acknowledged:bool,
    /// These complete old-base drafts are retained, NOT automatically reapplied
    /// to the new PDF: they may contain annotations already in that PDF.
    pub pending_rebase:usize,
}
// AppliedSave describes that transaction, not the current editor generation.
// A replay must never be used to clear ANY live draft or editor dirty state.
// Production must quiesce/revoke old bindings and compare the frozen editor
// generation before publishing a successful save to an active tab.
#[derive(Serialize,Deserialize,Debug,PartialEq)]
#[serde(deny_unknown_fields)]
struct SnapshotIdentity {id:String,digest:String}
#[derive(Serialize,Deserialize,Debug,PartialEq)]
#[serde(deny_unknown_fields)]
struct PendingDraft {id:String,digest:String,file:String,updated:i64}
#[derive(Serialize,Deserialize,Debug,PartialEq)]
#[serde(deny_unknown_fields)]
struct Association {
    schema:u32,document:String,old_revision:String,new_revision:String,
    old_hash:String,new_hash:String,file_key:String,pages:u32,
    snapshot:Option<SnapshotIdentity>,draft_acknowledged:bool,
    pending_rebase:Vec<PendingDraft>,
}
struct DraftBefore {key:String,raw:String,id:String,digest:String,reference:DraftRef}
#[derive(Clone,Copy)]
enum AssociationFault {None,#[cfg(test)] Sql,#[cfg(test)] NewDraft}
fn digest(bytes:&[u8])->String {format!("{:x}",Sha256::digest(bytes))}
fn association_key(event:&str)->String {format!("verified-pdf-save-v1:{event}")}

impl Store {
    pub(crate) fn capture_save_draft(&self,doc:&str,revision:&str,id:&str)->Result<SaveDraftSnapshot> {
        valid_uuid(doc)?;valid_uuid(revision)?;valid_uuid(id)?;
        let(current,pages)=self.current_revision(doc)?;
        if current!=revision{return deny("stale save draft capture");}
        let raw:String=self.conn.query_row("SELECT value FROM data_meta WHERE key=?1",[draft_key(doc,revision,id)],|r|r.get(0))?;
        let reference:DraftRef=serde_json::from_str(&raw).map_err(|_|DataError::Refused("draft reference"))?;
        let bytes=self.recovery_get(&reference.file)?;
        let value:serde_json::Value=serde_json::from_slice(&bytes).map_err(|_|DataError::Refused("draft JSON"))?;
        validate_draft(&value,pages)?;
        Ok(SaveDraftSnapshot{document:doc.into(),revision:revision.into(),id:id.into(),digest:digest(&bytes)})
    }

    /// Apply only a receipt minted by the guarded file commit after flush and
    /// byte verification. This is intentionally not wired to a production IPC.
    /// No draft mapping is deleted: the file receipt proves bytes were committed,
    /// but does not establish that `frozen` was included in that PDF. All old-base
    /// drafts remain pending until a future independent inclusion check exists.
    pub(crate) fn apply_verified_save(&mut self,receipt:&VerifiedReceipt,doc:&str,old_revision:&str,pages:u32,frozen:Option<&SaveDraftSnapshot>)->Result<AppliedSave> {
        self.apply_verified_save_inner(receipt,doc,old_revision,pages,frozen,AssociationFault::None)
    }
    fn apply_verified_save_inner(&mut self,receipt:&VerifiedReceipt,doc:&str,old_revision:&str,pages:u32,frozen:Option<&SaveDraftSnapshot>,_fault:AssociationFault)->Result<AppliedSave> {
        self.writable()?;valid_uuid(doc)?;valid_uuid(old_revision)?;valid_uuid(receipt.event_id())?;
        valid_hash(receipt.old_hash())?;valid_hash(receipt.new_hash())?;
        if receipt.old_hash()==receipt.new_hash()||pages==0||receipt.file_key().is_empty()||receipt.file_key().len()>256{return deny("verified save metadata");}
        if frozen.is_some_and(|s|s.document!=doc||s.revision!=old_revision){return deny("save snapshot outside binding");}
        let snapshot=frozen.map(|s|SnapshotIdentity{id:s.id.clone(),digest:s.digest.clone()});
        // An idempotent replay never rewinds the document if a later save was
        // already associated. It also never acknowledges a newer live draft.
        if let Some(raw)=self.conn.query_row("SELECT value FROM data_meta WHERE key=?1",[association_key(receipt.event_id())],|r|r.get::<_,String>(0)).optional()? {
            if raw.len()>32*1024{return deny("save association size");}
            let saved:Association=serde_json::from_str(&raw).map_err(|_|DataError::Refused("save association JSON"))?;
            if saved.schema!=1||saved.draft_acknowledged||saved.pending_rebase.len()>32||saved.document!=doc||saved.old_revision!=old_revision||saved.old_hash!=receipt.old_hash()||saved.new_hash!=receipt.new_hash()||saved.file_key!=receipt.file_key()||saved.pages!=pages||saved.snapshot!=snapshot{return deny("save event UUID reused");}
            let row:(String,String,String,String,String,String,u32,String,String,Option<String>)=self.conn.query_row("SELECT e.document_id,e.old_revision,e.new_revision,e.new_hash,e.new_file_key,o.hash,n.pages,n.source,n.hash,n.parent_id FROM save_events e JOIN revisions o ON o.id=e.old_revision AND o.document_id=e.document_id JOIN revisions n ON n.id=e.new_revision AND n.document_id=e.document_id WHERE e.event_id=?1",[receipt.event_id()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?,r.get(9)?)))?;
            if row!=(doc.into(),old_revision.into(),saved.new_revision.clone(),receipt.new_hash().into(),receipt.file_key().into(),receipt.old_hash().into(),pages,"verified-append-v1".into(),receipt.new_hash().into(),Some(old_revision.into())){return deny("save association mismatch");}
            let current=self.current_revision(doc)?.0;
            return Ok(AppliedSave{revision:saved.new_revision.clone(),is_current:current==saved.new_revision,draft_acknowledged:saved.draft_acknowledged,pending_rebase:saved.pending_rebase.len()});
        }
        if self.conn.prepare("SELECT 1 FROM save_events WHERE event_id=?1")?.exists([receipt.event_id()])?{return deny("save event without verified association");}
        let(current,old_hash,old_pages):(String,String,u32)=self.conn.query_row("SELECT r.id,r.hash,r.pages FROM documents d JOIN revisions r ON r.id=d.current_revision AND r.document_id=d.id WHERE d.id=?1",[doc],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
        // Annotation-only commits cannot silently change page topology. The
        // backend binding provides the page count; a renderer cannot mint this
        // receipt or substitute another document with a similar filename.
        if current!=old_revision||old_hash!=receipt.old_hash()||old_pages!=pages{return deny("stale verified save binding");}
        if !self.conn.prepare("SELECT 1 FROM locations WHERE document_id=?1 AND file_key=?2 AND hash=?3")?.exists(params![doc,receipt.file_key(),receipt.old_hash()])?{return deny("verified save file identity");}
        let prefix=format!("draft:{doc}:{old_revision}:");
        let rows=self.conn.prepare("SELECT key,value FROM data_meta WHERE key LIKE ?1 ORDER BY key LIMIT 33")?.query_map([format!("{prefix}%")],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        // The existing reader exposes at most 32 live recovery records. Refuse
        // an unbounded association rather than silently dropping hidden drafts.
        if rows.len()>32{return deny("save draft association limit");}
        let mut drafts=Vec::new();
        for(key,raw)in rows {
            let id=key.strip_prefix(&prefix).ok_or(DataError::Refused("draft key"))?.to_string();valid_uuid(&id)?;
            let reference:DraftRef=serde_json::from_str(&raw).map_err(|_|DataError::Refused("draft reference"))?;
            let bytes=self.recovery_get(&reference.file)?;
            let value:serde_json::Value=serde_json::from_slice(&bytes).map_err(|_|DataError::Refused("draft JSON"))?;validate_draft(&value,pages)?;
            drafts.push(DraftBefore{key,raw,id,digest:digest(&bytes),reference});
        }
        // Even an unchanged captured digest is only a saved-claim candidate:
        // it has not been bound to the actual PDF annotation snapshot. Preserve
        // every live mapping and immutable file, with explicit old/new lineage.
        let pending=drafts.iter().map(|d|PendingDraft{id:d.id.clone(),digest:d.digest.clone(),file:d.reference.file.clone(),updated:d.reference.updated}).collect();
        let next=uuid();
        let association=Association{schema:1,document:doc.into(),old_revision:old_revision.into(),new_revision:next.clone(),old_hash:receipt.old_hash().into(),new_hash:receipt.new_hash().into(),file_key:receipt.file_key().into(),pages,snapshot,draft_acknowledged:false,pending_rebase:pending};
        #[cfg(test)]
        if matches!(_fault,AssociationFault::NewDraft) {
            // A second SQLite connection commits a new live draft after the
            // recovery list was read and before our write transaction starts.
            let other=Connection::open(self.root.path.join("history.sqlite"))?;
            let raw=drafts.first().ok_or(DataError::Refused("race fixture requires draft"))?.raw.clone();
            other.execute("INSERT INTO data_meta VALUES(?1,?2)",params![draft_key(doc,old_revision,&uuid()),raw])?;
        }
        let tx=self.conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let actual:(String,String)=tx.query_row("SELECT r.id,r.hash FROM documents d JOIN revisions r ON r.id=d.current_revision AND r.document_id=d.id WHERE d.id=?1",[doc],|r|Ok((r.get(0)?,r.get(1)?)))?;
        if actual!=(old_revision.into(),receipt.old_hash().into()){return deny("save association changed");}
        let name_hint:Option<String>=tx.query_row("SELECT name_hint FROM locations WHERE document_id=?1 AND file_key=?2 AND hash=?3",params![doc,receipt.file_key(),receipt.old_hash()],|r|r.get(0)).optional()?;
        let name_hint=name_hint.ok_or(DataError::Refused("verified save file identity changed"))?;
        let actual_count:i64=tx.query_row("SELECT count(*) FROM data_meta WHERE key LIKE ?1",[format!("{prefix}%")],|r|r.get(0))?;
        if actual_count!=drafts.len() as i64{return deny("save draft set changed during association");}
        let position:Option<String>=tx.query_row("SELECT value FROM data_meta WHERE key=?1",[format!("reader-position:{doc}:{old_revision}")],|r|r.get(0)).optional()?;
        let position=position.map(|raw|{let value:ReadingPosition=serde_json::from_str(&raw).map_err(|_|DataError::Refused("position JSON"))?;value.validate(pages)?;Ok::<_,DataError>(value)}).transpose()?;
        // Compare immutable pointers under SQLite's write lock. If another
        // connection changed a draft while it was read, retain everything.
        for d in &drafts {
            let actual:Option<String>=tx.query_row("SELECT value FROM data_meta WHERE key=?1",[&d.key],|r|r.get(0)).optional()?;
            if actual.as_deref()!=Some(d.raw.as_str()){return deny("save draft changed during association");}
        }
        tx.execute("INSERT INTO revisions VALUES(?1,?2,?3,?4,?5,'verified-append-v1')",params![next,doc,receipt.new_hash(),pages,old_revision])?;
        tx.execute("UPDATE documents SET current_revision=?1 WHERE id=?2",params![next,doc])?;
        tx.execute("INSERT INTO save_events VALUES(?1,?2,?3,?4,?5,?6)",params![receipt.event_id(),doc,old_revision,next,receipt.new_hash(),receipt.file_key()])?;
        tx.execute("DELETE FROM locations WHERE document_id=?1",[doc])?;
        tx.execute("INSERT INTO locations VALUES(?1,?2,?3,?4)",params![doc,receipt.file_key(),receipt.new_hash(),name_hint])?;
        tx.execute("UPDATE positions SET revision_id=?1 WHERE document_id=?2 AND revision_id=?3",params![next,doc,old_revision])?;
        if let Some(value)=position {
            tx.execute("INSERT INTO data_meta VALUES(?1,?2)",params![format!("reader-position:{doc}:{next}"),serde_json::to_string(&value).map_err(|_|DataError::Refused("position JSON"))?])?;
        }
        tx.execute("INSERT INTO data_meta VALUES(?1,?2)",params![association_key(receipt.event_id()),serde_json::to_string(&association).map_err(|_|DataError::Refused("save association JSON"))?])?;
        #[cfg(test)]
        if matches!(_fault,AssociationFault::Sql) {tx.execute_batch("INSERT INTO verified_save_association_fault VALUES(1)")?;}
        integrity(&tx)?;tx.commit()?;
        Ok(AppliedSave{revision:next,is_current:true,draft_acknowledged:false,pending_rebase:association.pending_rebase.len()})
    }
}

#[cfg(test)]
#[path="save_receipt_tests.rs"] mod tests;
