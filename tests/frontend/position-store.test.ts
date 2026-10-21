import test from 'node:test';
import assert from 'node:assert/strict';
import { PositionStore, POSITION_KEY, contentHash } from '../../src/lib/position-store.ts';
import { zoomPercent, readingKey } from '../../src/lib/reading-controls.ts';
const position = { page: 28, zoom: 'page-fit' as const, top: -.2, left: -.1 };
function memory(value: string | null = null) { return { value, getItem() { return this.value; }, setItem(key: string, next: string) { assert.equal(key, POSITION_KEY); this.value = next; } }; }
test('reading metadata restores by actual content, survives reopening and has a finite size', async () => {
  const signal = new AbortController().signal;
  const one = await contentHash(new Uint8Array([1,2,3]), signal), other = await contentHash(new Uint8Array([1,2,4]), signal);
  assert.notEqual(one, other);
  const storage = memory(), store = new PositionStore(storage); store.save(one, position);
  assert.deepEqual(new PositionStore(storage).get(one), position); assert.equal(store.get(other), undefined);
  for (let i=0;i<100;i++) store.save(i.toString(16).padStart(64,'0'), position);
  const data = JSON.parse(storage.value!); assert.equal(data.entries.length,50); assert.ok(storage.value!.length < 64*1024);
  assert.equal(storage.value!.includes('name'), false); assert.equal(storage.value!.includes('bytes'), false);
});
test('unknown, corrupted and oversized records are not overwritten; quota failures safely fall back', () => {
  for (const raw of ['{broken', JSON.stringify({version:2, entries:[]}), 'x'.repeat(65537), JSON.stringify({version:1,entries:[{hash:'bad',position,updated:1}]})]) {
    const storage=memory(raw), store=new PositionStore(storage); assert.ok(store.warning); assert.equal(store.save('a'.repeat(64),position),false); assert.equal(storage.value,raw);
  }
  const store=new PositionStore({getItem:()=>null,setItem:()=>{throw Error('quota');}});
  assert.equal(store.save('a'.repeat(64),position),false); assert.ok(store.warning); assert.deepEqual(store.get('a'.repeat(64)),position);
});
test('hash completion respects cancellation and position objects do not leak mutable references', async () => {
  const abort=new AbortController(); abort.abort(); await assert.rejects(contentHash(new Uint8Array([1]),abort.signal),{name:'AbortError'});
  const store=new PositionStore(memory()); store.save('a'.repeat(64),position); const read=store.get('a'.repeat(64))!; read.page=1; assert.equal(store.get('a'.repeat(64))!.page,28);
});
test('zoom validates input and reading keys do not steal browser or editable-field shortcuts', () => {
  assert.equal(zoomPercent('137.5%'),1.375); for(const value of ['0','401','1e2','Infinity',''])assert.equal(zoomPercent(value),null);
  const key={key:'ArrowRight',ctrlKey:false,metaKey:false,altKey:false,shiftKey:false,isComposing:false}; assert.equal(readingKey(key,false),'next'); assert.equal(readingKey(key,true),null);
  for(const modifier of ['ctrlKey','metaKey','altKey','shiftKey','isComposing'])assert.equal(readingKey({...key,[modifier]:true},false),null);
  for(const value of ['PageDown','PageUp','F3','Escape','f','r','s','t','w'])assert.equal(readingKey({...key,key:value},false),null);
});
test('extra fields cannot be retained or written into reading-position metadata', () => {
  const raw=JSON.stringify({version:1,entries:[{hash:'b'.repeat(64),position:{...position,pdfContent:'private'},updated:1,path:'private'}]});
  const storage=memory(raw), store=new PositionStore(storage); assert.ok(store.warning); assert.equal(store.save('a'.repeat(64),position),false); assert.equal(storage.value,raw);
  const clean=memory(), extended={...position,pdfContent:'private'}; new PositionStore(clean).save('a'.repeat(64),extended); assert.equal(clean.value!.includes('private'),false);
});
