// Static wiring check only. Runtime ACL/IPC is tested separately with Wry.
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
export function verifyContract({manifest,handler,reader,enabled,invoked}) {
  const declared=new Set(manifest),registered=new Set(handler);
  assert.equal(declared.size,manifest.length,'Duplicate manifest command');
  assert.equal(registered.size,handler.length,'Duplicate handler');
  assert.deepEqual([...declared].sort(),[...registered].sort(),'Manifest and registered handlers differ');
  assert.deepEqual(enabled,['reader'],'Only the reviewed reader capability may be enabled in release');
  assert.equal(reader.local,true);assert.equal(reader.remote,undefined);
  assert.deepEqual(reader.windows,['main']);
  const allowed=new Set(reader.permissions.filter(p=>typeof p==='string'&&p.startsWith('allow-')).map(p=>p.slice(6).replaceAll('-','_')));
  for(const command of allowed)assert.ok(declared.has(command),`Permission has no registered command: ${command}`);
  // This sole debug command is declared but can only be granted by the explicit
  // M3 debug overlay. Do not borrow its capability to approve release calls.
  for(const command of declared)if(command!=='save_m3_copy')assert.ok(allowed.has(command),`Reader cannot invoke ${command}`);
  for(const command of invoked)if(command!=='save_m3_copy')assert.ok(allowed.has(command)&&registered.has(command),`Invocation is not allowed/registered: ${command}`);
  for(const command of ['check_unicode_writer','begin_unicode_export','finish_unicode_export','cancel_unicode_export','save_original_pdf','pending_pdf_recovery','export_pdf_recovery','begin_pdf_read','read_pdf','release_pdf'])assert.ok(allowed.has(command)&&registered.has(command),`Save contract missing ${command}`);
  assert.ok(!allowed.has('save_m3_copy'),'Release cannot grant the debug-only writer');
}
export async function checkCommandContract(root=path.resolve(fileURLToPath(new URL('../',import.meta.url)))) {
  const read=p=>readFile(path.join(root,p),'utf8');
  const [build,main,readerText,configText,desktop]=await Promise.all(['src-tauri/build.rs','src-tauri/src/main.rs','src-tauri/capabilities/reader.json','src-tauri/tauri.conf.json','src/lib/desktop-source.ts'].map(read));
  const block=/\.commands\(&\[([\s\S]*?)\]\)/.exec(build)?.[1];assert.ok(block,'Manifest command list missing');
  const manifest=[...block.matchAll(/"([a-z_0-9]+)"/g)].map(m=>m[1]);
  const handlers=[...main.matchAll(/tauri::generate_handler!\[([^\]]+)\]/g)];assert.ok(handlers.length,'Handler list missing');
  const handler=handlers[0][1].split(',').map(n=>n.trim().split('::').at(-1)).filter(Boolean);
  const data={manifest,handler,reader:JSON.parse(readerText),enabled:JSON.parse(configText).app.security.capabilities,invoked:[...desktop.matchAll(/\b(?:invoke|bridge)(?:<[^>]*>)?\(\s*['"]([a-z_0-9]+)['"]/g)].map(m=>m[1])};
  verifyContract(data);
  // Mutating only in-memory inputs proves checks do not accept the historic
  // missing permission, missing handler, unrelated window or extra capability.
  for(const mutation of [
    d=>d.manifest=d.manifest.filter(n=>n!=='save_original_pdf'),
    d=>d.handler=d.handler.filter(n=>n!=='finish_unicode_export'),
    d=>d.reader.permissions=d.reader.permissions.filter(n=>n!=='allow-finish-unicode-export'),
    d=>d.reader.windows=['other'],d=>d.reader.local=false,
    d=>d.enabled=['reader','reader-m3'],d=>d.reader.permissions.push('allow-save-m3-copy'),
  ]){const negative=structuredClone(data);mutation(negative);assert.throws(()=>verifyContract(negative),'Invalid command contract accepted');}
  return {commands:manifest.length,negativeControls:7,scope:'static release reader wiring; not IPC execution'};
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url))console.log(JSON.stringify({status:'PASS',...await checkCommandContract()}));
