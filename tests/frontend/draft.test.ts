import { test } from 'node:test';
import assert from 'node:assert/strict';
import { emptyDraft,validateDraft } from '../../src/lib/annotation-draft.ts';
import { DraftStore,DRAFT_PREFIX,recoveryBranch } from '../../src/lib/draft-store.ts';
const hash='a'.repeat(64),id='11111111-1111-4111-8111-111111111111';
const draft=()=>({...emptyDraft(),entries:[{annotationType:3,pageIndex:0,rect:[10,20,100,50],rotation:0,color:[0,0,0],fontSize:12,value:'中文 α ≤ 10',id:null}]});
class Memory {
  values=new Map<string,string>();fail=false;
  get length(){return this.values.size;} key(i:number){return [...this.values.keys()][i]??null;}getItem(key:string){return this.values.get(key)??null;}
  setItem(key:string,value:string){if(this.fail)throw Error('quota');this.values.set(key,value);}removeItem(key:string){this.values.delete(key);}
}
test('Unicode draft survives a new store, only matches exact source bytes and preserves distinct copies',()=>{
  const memory=new Memory(),store=new DraftStore(memory);assert.ok(store.save(id,hash,draft()));
  assert.equal(new DraftStore(memory).list(hash)[0].draft.entries[0].value,'中文 α ≤ 10');assert.equal(store.list('b'.repeat(64)).length,0);
  assert.ok(store.save('22222222-2222-4222-8222-222222222222',hash,draft()));assert.equal(store.list(hash).length,2);
  assert.equal(store.save(id,'b'.repeat(64),draft()),false);
});
test('quota failures and future or corrupt records keep existing recovery bytes',()=>{
  const memory=new Memory(),store=new DraftStore(memory);store.save(id,hash,draft());const old=memory.getItem(DRAFT_PREFIX+id);
  memory.fail=true;assert.equal(store.save(id,hash,{...draft(),entries:[{...draft().entries[0],value:'changed'}]}),false);assert.equal(memory.getItem(DRAFT_PREFIX+id),old);
  memory.fail=false;memory.setItem(DRAFT_PREFIX+id,JSON.stringify({...JSON.parse(old!),version:2}));const future=memory.getItem(DRAFT_PREFIX+id);
  assert.equal(store.save(id,hash,draft()),false);assert.equal(memory.getItem(DRAFT_PREFIX+id),future);
  memory.setItem(DRAFT_PREFIX+id,'broken');assert.deepEqual(store.list(hash),[]);assert.equal(memory.getItem(DRAFT_PREFIX+id),'broken');
});
test('malformed, foreign annotation, excessive or executable draft data is rejected',()=>{
  for(const entry of [{...draft().entries[0],id:'7R'},{...draft().entries[0],rect:[NaN,1,2,3]}, {...draft().entries[0],fontSize:-1},{...draft().entries[0],annotationType:13},{...draft().entries[0],pageIndex:2000}])assert.throws(()=>validateDraft({...draft(),entries:[entry]}));
  assert.throws(()=>validateDraft({...draft(),entries:Array(501).fill(draft().entries[0])}));
  assert.throws(()=>validateDraft(JSON.parse('{"schema":1,"pdfjs":"6.4.299","entries":[{"annotationType":3,"pageIndex":0,"rect":[1,2,3,4],"rotation":0,"color":[0,0,0],"fontSize":12,"value":"x","__proto__":{}}]}')));
});
test('drawing arrays keep their coordinates through JSON recovery, explicit discard is scoped',()=>{
  const record={annotationType:15,pageIndex:1,rect:[10,10,20,20],rotation:90,color:[0,0,0],thickness:2,opacity:1,paths:{lines:[new Float32Array([NaN,NaN,NaN,NaN,1,2])],points:[new Float32Array([1,2])]},id:null};
  assert.deepEqual(validateDraft({...emptyDraft(),entries:[record]}).entries[0].paths,{lines:[[null,null,null,null,1,2]],points:[[1,2]]});
  assert.throws(()=>validateDraft({...emptyDraft(),entries:[{annotationType:15,pageIndex:0,rect:[0,0,1,1],rotation:0}]}));
  const memory=new Memory();memory.setItem('unrelated','keep');const store=new DraftStore(memory);store.save(id,hash,draft());assert.ok(store.discard(id));assert.equal(memory.getItem('unrelated'),'keep');
});
test('restoring identical documents creates independent branches and retains the old recovery',()=>{
  const memory=new Memory(),store=new DraftStore(memory);store.save(id,hash,draft());const record=store.list(hash)[0];
  const a=recoveryBranch(record,hash),b=recoveryBranch(record,hash);assert.notEqual(a.id,b.id);assert.notEqual(a.id,id);
  a.draft.entries[0].value='first tab';store.save(a.id,hash,a.draft);store.save(b.id,hash,b.draft);store.discard(a.id);
  assert.equal(store.list(hash).find(r=>r.id===id)?.draft.entries[0].value,'中文 α ≤ 10');
  assert.equal(store.list(hash).find(r=>r.id===b.id)?.draft.entries[0].value,'中文 α ≤ 10');assert.throws(()=>recoveryBranch(record,'b'.repeat(64)));
});
test('explicitly abandoning a new draft is possible when unrelated recovery records are incompatible',()=>{
  const memory=new Memory(),store=new DraftStore(memory);assert.ok(store.save(id,hash,draft()));
  const futureId='33333333-3333-4333-8333-333333333333';
  const future=JSON.stringify({...JSON.parse(memory.getItem(DRAFT_PREFIX+id)!),id:futureId,version:2});
  memory.setItem(DRAFT_PREFIX+futureId,future);
  const freshId='44444444-4444-4444-8444-444444444444';
  assert.equal(store.save(freshId,hash,draft()),false);assert.ok(store.discard(freshId));
  assert.ok(store.discard(id));assert.equal(memory.getItem(DRAFT_PREFIX+id),null);
  assert.equal(store.discard(futureId),false);assert.equal(memory.getItem(DRAFT_PREFIX+futureId),future);
  assert.ok(new DraftStore(null).discard(freshId));
});
