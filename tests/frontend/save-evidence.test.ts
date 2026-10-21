import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile,mkdir,writeFile,copyFile} from 'node:fs/promises';
import {execFileSync} from 'node:child_process';
import {randomUUID,createHash} from 'node:crypto';
import path from 'node:path';
import {saveLabInstrumentation} from '../../scripts/save-lab-instrumentation.mjs';

test('save observations reject missing targets and skip Svelte virtual CSS',async()=>{
 const observer=saveLabInstrumentation();assert.equal(observer.transform('body{}','/src/App.svelte?svelte&type=style'),undefined);
 for(const file of ['src/App.svelte','src/lib/desktop-source.ts','src/lib/original-output.ts']){
  const source=await readFile(file,'utf8'),instrumented=observer.transform(source,'/'+file);
  assert.ok(instrumented?.includes('__SAVE_PROBE__'));assert.throws(()=>observer.transform('', '/'+file),/target changed/);
 }
});

test('roundtrip evidence separates user-reported save failure, parser failure and pending UI checks',async()=>{
 const relative=`tests/work/save-interop-performance/classification-${randomUUID()}/roundtrip`,kit=path.resolve(relative);
 await mkdir(kit,{recursive:true});const source=path.resolve('tests/fixtures/g1/freetext-control.pdf'),bytes=await readFile(source),hash=createHash('sha256').update(bytes).digest('hex');
 await writeFile(path.join(kit,'protocol.json'),JSON.stringify({baseline:{sha256:hash},control:{sha256:hash}}));
 const observations={externalSave:{success:null as boolean|null,error:null as string|null},appearance:{edge:null,aiReaderReturn:null},aiReaderReeditSave:{success:null}};
 const observe=()=>writeFile(path.join(kit,'user-observations.json'),JSON.stringify(observations));
 const inspect=()=>execFileSync(process.execPath,['scripts/save-release-evidence.mjs','inspect',relative],{windowsHide:true,stdio:'pipe'});
 const result=async()=>JSON.parse(await readFile(path.join(kit,'inspection.json'),'utf8'));
 await observe();inspect();assert.equal((await result()).phases.externalSave.status,'USER CHECK');assert.equal((await result()).phases.externalReturn.filePresent,false);
 observations.externalSave={success:false,error:'Synthetic Edge save error'};await observe();inspect();assert.equal((await result()).phases.externalSave.status,'FAIL (USER REPORTED)');assert.match((await result()).phases.externalReturn.status,/BLOCKED/);
 observations.externalSave={success:null,error:null};await observe();await writeFile(path.join(kit,'03-edge-returned.pdf'),'%PDF-1.7\ninvalid synthetic');inspect();assert.equal((await result()).phases.externalReturn.reread,'FAIL');assert.equal((await result()).phases.externalSave.status,'USER CHECK');
 await copyFile(source,path.join(kit,'03-edge-returned.pdf'));await copyFile(source,path.join(kit,'04-ai-reader-resaved.pdf'));inspect();const valid=await result();assert.equal(valid.phases.externalReturn.reread,'PASS');assert.equal(valid.phases.externalReturn.bytesChanged,false);assert.equal(valid.phases.aiReaderResave.bytesChanged,false);assert.match(valid.phases.aiReaderResave.reeditSave,/USER CHECK/);assert.equal(valid.phases.aiReaderReeditSave.status,'USER CHECK');
 assert.equal(createHash('sha256').update(await readFile(source)).digest('hex'),hash,'immutable control changed');
});
