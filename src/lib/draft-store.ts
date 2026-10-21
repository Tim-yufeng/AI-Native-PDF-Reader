import { validateDraft, type AnnotationDraft } from './annotation-draft.ts';
export const DRAFT_PREFIX='pdf-reader.recovery.v1.';
interface Store { readonly length:number; key(index:number):string|null; getItem(key:string):string|null; setItem(key:string,value:string):void; removeItem(key:string):void; }
export interface RecoveryDraft { version:1; id:string; hash:string; updated:number; draft:AnnotationDraft; }
// Restoring creates an independent branch. Two identical open files never share a key.
export function recoveryBranch(record:RecoveryDraft,sourceHash:string):{id:string;draft:AnnotationDraft}{
  if(record.version!==1||record.hash!==sourceHash||!/^[a-f0-9]{64}$/.test(sourceHash))throw Error('恢复草稿与文档不匹配。');
  return{id:crypto.randomUUID(),draft:validateDraft(record.draft)};
}
const hashValid=(hash:string)=>/^[a-f0-9]{64}$/.test(hash);
const idValid=(id:string)=>/^[a-f0-9-]{36}$/.test(id);
function parseRecord(raw:string,key:string):RecoveryDraft {
  if(raw.length>2*1024*1024)throw Error('recovery quota');
  const record=JSON.parse(raw) as RecoveryDraft;
  if(record.version!==1||!idValid(record.id)||key!==DRAFT_PREFIX+record.id||!hashValid(record.hash)||!Number.isSafeInteger(record.updated)||record.updated<0)throw Error('unknown recovery schema');
  record.draft=validateDraft(record.draft);return record;
}
export class DraftStore {
  warning='';
  private storage:Store|null;
  constructor(storage:Store|null) {this.storage=storage;}
  private records() {
    if (!this.storage) throw Error('no storage');
    const records:RecoveryDraft[]=[]; let size=0;
    for(let i=0;i<this.storage.length;i++) {
      const key=this.storage.key(i); if(!key?.startsWith(DRAFT_PREFIX)) continue;
      const raw=this.storage.getItem(key); if(!raw) continue; size+=raw.length;
      if(size>2*1024*1024||records.length>=32) throw Error('recovery quota');
      records.push(parseRecord(raw,key));
    }
    return records;
  }
  list(hash:string):RecoveryDraft[] {
    if(!hashValid(hash))return[];
    try{return this.records().filter(record=>record.hash===hash).sort((a,b)=>b.updated-a.updated);}
    catch{this.warning='恢复区不可用或版本不兼容，已有草稿保持不变。';return[];}
  }
  save(id:string,hash:string,draft:AnnotationDraft):boolean {
    try{
      if(!idValid(id)||!hashValid(hash))throw Error('invalid recovery identity');
      draft=validateDraft(draft); if(!draft.entries.length)return true;
      const records=this.records(),old=records.find(r=>r.id===id);
      if(old&&old.hash!==hash)throw Error('different source');
      const record:RecoveryDraft={version:1,id,hash,updated:Date.now(),draft};
      const raw=JSON.stringify(record);
      if((!old&&records.length>=32)||records.filter(r=>r.id!==id).reduce((n,r)=>n+JSON.stringify(r).length,0)+raw.length>2*1024*1024)throw Error('recovery quota');
      this.storage!.setItem(DRAFT_PREFIX+id,raw); this.warning='';return true;
    }catch{this.warning='未能保存恢复草稿；请保留窗口。已有草稿不会自动清理。';return false;}
  }
  discard(id:string):boolean {
    if(!idValid(id))return false;
    try{
      // Explicitly abandoning this draft must not depend on unrelated records.
      // A failed first checkpoint has no persistent key to remove.
      if(!this.storage)return true;
      const key=DRAFT_PREFIX+id,raw=this.storage.getItem(key);
      if(raw===null)return true;
      parseRecord(raw,key);this.storage.removeItem(key);return true;
    }
    catch{this.warning='草稿未能移除，已有内容保持不变。';return false;}
  }
}
