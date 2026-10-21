import test from 'node:test';
import assert from 'node:assert/strict';
import { desktopSource, desktopError, writerError, DesktopSourceError, type DesktopBridge } from '../../src/lib/desktop-source.ts';
import { DocumentTabs } from '../../src/lib/document-tabs.ts';
const descriptor = { grantId: '3ee1a7ee-82db-4554-8d84-751dd97b8880', name: '中文 空格.pdf', size: 5 };
test('incremental refusal preserves a bounded rule number without leaking backend text',()=>{
 const code='writer_original_structure_invalid';
 const wrapped=new DesktopSourceError(new DesktopSourceError({code,message:'T09_free_entry'},'original'),'original');
 assert.match(writerError(wrapped),/检查编号：T09/);
 for(const message of ['C:\\private\\lesson.pdf','I07_source_dictionary_syntax secret content','I99_unknown','S00_unknown']){
  const output=writerError(new DesktopSourceError({code,message},'original'));
  assert.ok(!output.includes(message));assert.ok(!output.includes('检查编号'));
 }
});
// Deterministic stand-in for the MuPDF.js byte layer; these tests cover the
// bridge contract (payload shape, cancellation, release) only.
const stubWriter = async (candidate: Uint8Array) => new Uint8Array([...candidate, 10, 37, 37, 69, 79, 70]);

test('both save routes pass the complete grant-sized original; a short candidate never reaches the writer', async () => {
  const originals: Uint8Array[] = [], calls: string[] = [], bytes = new Uint8Array([37,80,68,70,45,10,20]);
  const writer = async (candidate: Uint8Array, _signal: AbortSignal, original: Uint8Array) => {
    originals.push(original.slice()); return stubWriter(candidate);
  };
  const bridge: DesktopBridge = async<T>(command: string) => {
    calls.push(command);
    if(command==='begin_unicode_export')return 'synthetic-task' as T;
    if(command==='finish_unicode_export')return null as T;
    if(command==='save_original_pdf')return {descriptor:null,hash:'a'.repeat(64),binding:null,historyPending:false,reopenRequired:true} as T;
    return undefined as T;
  };
  const signal=new AbortController().signal;
  await desktopSource(descriptor,bridge,writer).exportCopy!(bytes,signal);
  await desktopSource(descriptor,bridge,writer,async()=>{}).saveOriginal!(bytes,signal);
  assert.deepEqual(originals,[bytes.slice(0,5),bytes.slice(0,5)]);
  await assert.rejects(desktopSource(descriptor,bridge,writer).exportCopy!(bytes.slice(0,4),signal),{code:'writer_baseline_invalid'});
  assert.equal(originals.length,2);assert.equal(calls.filter(c=>c==='save_original_pdf').length,1);
});
test('candidate export cancellation while reserving never sends bytes or a renderer path',async()=>{
 const controller=new AbortController();let finish!:(id:string)=>void;const calls:{command:string,args?:Parameters<DesktopBridge>[1]}[]=[];
 const bridge:DesktopBridge=async<T>(command:string,args?:Parameters<DesktopBridge>[1],options?:Parameters<DesktopBridge>[2])=>{calls.push({command,args});if(command==='begin_unicode_export')return await new Promise<string>(resolve=>finish=resolve)as T;return undefined as T;};
 const source=desktopSource(descriptor,bridge,stubWriter),pending=source.exportCopy!(new Uint8Array([37,80,68,70,45]),controller.signal);controller.abort();finish('synthetic-task');await assert.rejects(pending);
 assert.ok(calls.some(c=>c.command==='cancel_unicode_export'));assert.ok(!calls.some(c=>c.command==='finish_unicode_export'));assert.ok(calls.every(c=>!Object.hasOwn(c.args??{},'path')));
});
test('candidate export returns a new grant, leaves old source live, and releases a late cancelled output',async()=>{
 let finish!:(value:unknown)=>void;const calls:string[]=[];const controller=new AbortController();
 const bridge:DesktopBridge=async<T>(command:string)=>{calls.push(command);if(command==='begin_unicode_export')return 'synthetic-task'as T;if(command==='finish_unicode_export')return await new Promise<unknown>(resolve=>finish=resolve)as T;return undefined as T;};
 const source=desktopSource(descriptor,bridge,stubWriter),pending=source.exportCopy!(new Uint8Array([37,80,68,70,45]),controller.signal);await new Promise(resolve=>setImmediate(resolve));controller.abort();finish({...descriptor,grantId:'6ba10989-2294-44a8-a2e8-9356ae9a74d0'});await assert.rejects(pending);assert.ok(calls.includes('release_pdf'));
 const accept:DesktopBridge=async<T>(command:string)=>command==='begin_unicode_export'?'next'as T:command==='finish_unicode_export'?null as T:undefined as T;
 assert.equal(await desktopSource(descriptor,accept,stubWriter).exportCopy!(new Uint8Array([37,80,68,70,45]),new AbortController().signal),null);
});

test('isolated copy bridge carries an opaque grant and bytes, never an output path, and refuses a released source', async()=>{
  const calls:{command:string;args?:Parameters<DesktopBridge>[1]}[]=[];
  const bridge:DesktopBridge=async<T>(command:string,args?:Parameters<DesktopBridge>[1],options?:Parameters<DesktopBridge>[2])=>{
    calls.push({command,args});return {...descriptor,grantId:'6ba10989-2294-44a8-a2e8-9356ae9a74d0'} as T;
  };
  const source=desktopSource(descriptor,bridge,stubWriter),bytes=new Uint8Array([37,80,68,70,45]);
  const copy=await source.testCopy!(bytes);
  assert.deepEqual(calls[0],{command:'save_m3_copy',args:{grantId:descriptor.grantId,bytes:Array.from(bytes)}});
  assert.notEqual(copy,source);await source.release!();await assert.rejects(source.testCopy!(bytes));
  assert.equal(calls.filter(c=>c.command==='save_m3_copy').length,1);await copy.release!();
});
test('desktop sources expose grants only, use binary reads, and release once on tab close', async () => {
  const calls: { command: string; args?: Parameters<DesktopBridge>[1] }[] = [];
  const bridge: DesktopBridge = async <T>(command: string, args?: Parameters<DesktopBridge>[1]): Promise<T> => {
    calls.push({command,args}); return (command === 'begin_pdf_read' ? 'task' : command === 'read_pdf' ? new ArrayBuffer(5) : undefined) as T;
  };
  const source = desktopSource(descriptor, bridge); const tabs = new DocumentTabs(); tabs.add([source]);
  assert.equal((await source.read(new AbortController().signal)).length, 5);
  assert.deepEqual(calls[0], {command:'begin_pdf_read', args:{grantId:descriptor.grantId}});
  assert.ok(calls.every(call => !Object.hasOwn(call.args || {}, 'path')));
  tabs.close(tabs.activeId!); await source.release?.();
  assert.equal(calls.filter(call => call.command === 'release_pdf').length, 1);
  await assert.rejects(source.read(new AbortController().signal));
});
test('cancellation while registering a desktop read cancels the returned reservation before reading', async () => {
  const abort = new AbortController(); const calls: string[] = [];
  let complete!: (task: string) => void;
  const bridge: DesktopBridge = async <T>(command: string): Promise<T> => {
    calls.push(command); if (command === 'begin_pdf_read') return await new Promise<string>(resolve => { complete = resolve; }) as T;
    return undefined as T;
  };
  const pending = desktopSource(descriptor, bridge).read(abort.signal); abort.abort(); complete('task');
  await assert.rejects(pending); assert.ok(calls.includes('cancel_pdf_read')); assert.ok(!calls.includes('read_pdf'));
});
test('late binary responses after cancellation/release cannot attach, and invalid lengths are rejected', async () => {
  let finish!: (bytes: ArrayBuffer) => void;
  const bridge: DesktopBridge = async <T>(command: string): Promise<T> => {
    if (command === 'begin_pdf_read') return 'task' as T;
    if (command === 'read_pdf') return await new Promise<ArrayBuffer>(resolve => {finish = resolve;}) as T;
    return undefined as T;
  };
  const source = desktopSource(descriptor, bridge), abort = new AbortController();
  const pending = source.read(abort.signal); await new Promise(resolve => setImmediate(resolve));
  await source.release?.(); finish(new ArrayBuffer(5)); await assert.rejects(pending);
  const invalid: DesktopBridge = async <T>(command: string): Promise<T> => (command === 'begin_pdf_read' ? 'task' : new ArrayBuffer(4)) as T;
  await assert.rejects(desktopSource(descriptor, invalid).read(new AbortController().signal));
  assert.ok(!desktopError({code:'unknown',message:'private/path'}).includes('private'));
});
test('tab switching retains grants and closing inactive tabs or clearing releases each source', () => {
  const released: number[] = []; const tabs = new DocumentTabs();
  tabs.add([1,2].map(id => ({name: `${id}.pdf`,size:1,read:async()=>new Uint8Array(1),release:()=>{released.push(id);}})));
  tabs.activate(tabs.tabs[0].id); assert.deepEqual(released, []);
  tabs.close(tabs.tabs[1].id); tabs.clear(); tabs.clear(); assert.deepEqual(released,[2,1]);
});

test('original save validates before native commit and refreshes the consumed file grant',async()=>{
 const calls:string[]=[];let checked=false;
 const next={...descriptor,grantId:'2ace1000-2294-44a8-a2e8-9356ae9a74d0',size:9};
 const bridge:DesktopBridge=async<T>(command:string,args?:Parameters<DesktopBridge>[1],options?:Parameters<DesktopBridge>[2])=>{calls.push(command);
  if(command==='save_original_pdf'){assert.equal(checked,true);assert.ok(args instanceof Uint8Array);assert.deepEqual(args,await stubWriter(new Uint8Array([37,80,68,70,45])));assert.equal(options?.headers['x-ai-save-candidate-bytes'],'5');assert.equal(options?.headers['x-ai-save-binding'],undefined);return {descriptor:next,hash:'a'.repeat(64),binding:null,historyPending:false,reopenRequired:false}as T;}
  return 'synthetic-task'as T;
 };
 const source=desktopSource(descriptor,bridge,stubWriter,async()=>{checked=true;});
 const result=await source.saveOriginal!(new Uint8Array([37,80,68,70,45]),new AbortController().signal);
 assert.equal(result.source?.grantId,next.grantId);assert.equal(result.hash,'a'.repeat(64));
 await assert.rejects(source.read(new AbortController().signal));
 assert.deepEqual(calls,['begin_unicode_export','save_original_pdf','cancel_unicode_export']);
});
test('failed original validation never commits, cancellation and ambiguous responses retain truthful status',async()=>{
 const calls:string[]=[];const bridge:DesktopBridge=async<T>(command:string)=>{calls.push(command);return 'unexpected-string'as T;};
 const bytes=new Uint8Array([37,80,68,70,45]);
 const failed=desktopSource(descriptor,bridge,stubWriter,async()=>{throw {code:'writer_original_unsupported'};});
 await assert.rejects(failed.saveOriginal!(bytes,new AbortController().signal),e=>/writer_original_unsupported/.test(writerError(e)));
 assert.ok(!calls.includes('save_original_pdf'));
 const invalid=desktopSource(descriptor,bridge,stubWriter,async()=>{});
 await assert.rejects(invalid.saveOriginal!(bytes,new AbortController().signal),e=>/writer_response_invalid/.test(writerError(e)));
 const abort=new AbortController();abort.abort();const count=calls.length;await assert.rejects(invalid.saveOriginal!(bytes,abort.signal));assert.equal(calls.length,count);
});

test('an explicitly supplied invalid binding is not silently converted into an unbound save',async()=>{
 let seen=false;const bridge:DesktopBridge=async<T>(command:string,args?:Parameters<DesktopBridge>[1],options?:Parameters<DesktopBridge>[2])=>{
  if(command==='save_original_pdf'){seen=true;assert.equal(options?.headers['x-ai-save-binding'],'');throw {code:'unauthorized'};}
  return 'synthetic-task' as T;
 };
 await assert.rejects(desktopSource(descriptor,bridge,stubWriter,async()=>{}).saveOriginal!(new Uint8Array([37,80,68,70,45]),new AbortController().signal,''),e=>/unauthorized/.test(writerError(e)));
 assert.equal(seen,true);
});

test('save-as never treats a native string success as a file descriptor or claims the file was unchanged',async()=>{
 const calls:string[]=[];
 const bridge:DesktopBridge=async<T>(command:string)=>{calls.push(command);return (command==='begin_unicode_export'?'synthetic-task':command==='finish_unicode_export'?'原件已安全写入':undefined)as T;};
 await assert.rejects(desktopSource(descriptor,bridge,stubWriter).exportCopy!(new Uint8Array([37,80,68,70,45]),new AbortController().signal),error=>{
  const text=writerError(error);assert.match(text,/writer_response_invalid/);assert.match(text,/保存结果尚未确认/);assert.ok(!text.includes('原 PDF 未修改'));return true;
 });
 assert.deepEqual(calls,['begin_unicode_export','finish_unicode_export','cancel_unicode_export']);
});

test('malformed descriptors release only a valid opaque grant without evaluating response getters',async()=>{
 let touched=false;const released:string[]=[];
 const invalid=Object.defineProperty({grantId:'6ba10989-2294-44a8-a2e8-9356ae9a74d0',size:5},'name',{get(){touched=true;throw Error('private-data');}});
 const bridge:DesktopBridge=async<T>(command:string,args?:Parameters<DesktopBridge>[1],options?:Parameters<DesktopBridge>[2])=>{
  if(command==='begin_unicode_export')return 'synthetic-task'as T;
  if(command==='finish_unicode_export')return invalid as T;
  if(command==='release_pdf')released.push(String((args instanceof Uint8Array?undefined:args)?.grantId));return undefined as T;
 };
 await assert.rejects(desktopSource(descriptor,bridge,stubWriter).exportCopy!(new Uint8Array([37,80,68,70,45]),new AbortController().signal),error=>/writer_response_invalid/.test(writerError(error)));
 assert.equal(touched,false);assert.deepEqual(released,[invalid.grantId]);
});

test('revoked grant refusal does not run the writer or finish a save',async()=>{
 const calls:string[]=[];let writerCalls=0;
 const bridge:DesktopBridge=async<T>(command:string)=>{calls.push(command);throw {code:'unauthorized'};};
 await assert.rejects(desktopSource(descriptor,bridge,async bytes=>{writerCalls++;return stubWriter(bytes);}).exportCopy!(new Uint8Array([37,80,68,70,45]),new AbortController().signal),error=>/unauthorized/.test(writerError(error)));
 assert.deepEqual(calls,['begin_unicode_export']);assert.equal(writerCalls,0);
});

test('closing a source during save releases a late valid output grant and cannot attach it',async()=>{
 let finish!:(value:unknown)=>void;const released:string[]=[];
 const output={...descriptor,grantId:'6ba10989-2294-44a8-a2e8-9356ae9a74d0'};
 const bridge:DesktopBridge=async<T>(command:string,args?:Parameters<DesktopBridge>[1],options?:Parameters<DesktopBridge>[2])=>{
  if(command==='begin_unicode_export')return 'synthetic-task'as T;
  if(command==='finish_unicode_export')return await new Promise<unknown>(resolve=>finish=resolve)as T;
  if(command==='release_pdf')released.push(String((args instanceof Uint8Array?undefined:args)?.grantId));return undefined as T;
 };
 const source=desktopSource(descriptor,bridge,stubWriter),pending=source.exportCopy!(new Uint8Array([37,80,68,70,45]),new AbortController().signal);
 await new Promise(resolve=>setImmediate(resolve));await source.release?.();finish(output);
 await assert.rejects(pending);assert.deepEqual(released,[descriptor.grantId,output.grantId]);
});

test('save-as accepts the exact 20 MiB writer bound and refuses the next byte before IPC',async()=>{
 const candidate=new Uint8Array([37,80,68,70,45]);let sent=0;
 const bridge:DesktopBridge=async<T>(command:string,args?:Parameters<DesktopBridge>[1],options?:Parameters<DesktopBridge>[2])=>{
  if(command==='begin_unicode_export')return 'synthetic-task'as T;
  if(command==='finish_unicode_export'){sent++;assert.ok(args instanceof Uint8Array);assert.equal(args.length,20*1024*1024);assert.deepEqual(args.subarray(0,candidate.length),candidate);assert.equal(options?.headers['x-ai-save-candidate-bytes'],String(candidate.length));return {...descriptor,size:20*1024*1024}as T;}
  return undefined as T;
 };
 const exact=desktopSource(descriptor,bridge,async()=>{const output=new Uint8Array(20*1024*1024);output.set(candidate);return output;});
 const copy=await exact.exportCopy!(candidate,new AbortController().signal);assert.equal(copy?.size,20*1024*1024);await copy?.release?.();
 await assert.rejects(desktopSource(descriptor,bridge,async()=>new Uint8Array(20*1024*1024+1)).exportCopy!(candidate,new AbortController().signal),error=>/writer_standards_blocked/.test(writerError(error)));
 assert.equal(sent,1);
});

test('failed append retains the editor and receives only a fresh opaque recovery authority',async()=>{
 const recovery={...descriptor,grantId:'2ace1000-2294-44a8-a2e8-9356ae9a74d0'};
 const bridge:DesktopBridge=async<T>(command:string)=>{if(command==='save_original_pdf')throw {code:'writer_original_interrupted',recoveryDescriptor:recovery};return 'synthetic-task'as T;};
 const source=desktopSource(descriptor,bridge,stubWriter,async()=>{});
 await assert.rejects(source.saveOriginal!(new Uint8Array([37,80,68,70,45]),new AbortController().signal),error=>{
   assert.match(writerError(error),/writer_original_interrupted/);assert.equal((error as {recoverySource?:{grantId:string}}).recoverySource?.grantId,recovery.grantId);return true;
 });
 await assert.rejects(source.read(new AbortController().signal));
});
