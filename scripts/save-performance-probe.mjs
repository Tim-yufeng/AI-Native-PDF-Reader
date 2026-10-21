// CPU-only diagnostic on already captured synthetic saves. No IPC, save command,
// desktop, credentials or Provider. Uses the installed, locked Tauri encoder.
import {readFile,writeFile,mkdir} from 'node:fs/promises';
import path from 'node:path';
import {runInThisContext} from 'node:vm';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root=path.resolve(import.meta.dirname,'..');
const out=path.join(root,'tests/work/save-interop-performance/expert-01eabfb');
await mkdir(out,{recursive:true});
const encoderPath='D:/DevTools/Rust/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/tauri-2.12.3/scripts/process-ipc-message-fn.js';
const encoderSource=await readFile(encoderPath,'utf8');
const encode=runInThisContext(encoderSource,{filename:encoderPath});
const timing=JSON.parse(await readFile(path.join(root,'tests/work/save-interop-performance/run-9ecaa314-70e3-4781-a612-e382cb5235ec/timings.json'),'utf8'));
const rows=[];
for(const round of [1,10]){
 const record=timing.rounds.find(r=>r.round===round),output=new Uint8Array(await readFile(path.join(root,record.output))),candidate=new Uint8Array(await readFile(path.join(root,record.output.replace('-output.pdf','-candidate.pdf'))));
 assert.equal(output.length,record.outputBytes);assert.equal(candidate.length,record.candidateBytes);assert.deepEqual(output.subarray(0,candidate.length),candidate);
 for(let repeat=0;repeat<3;repeat++){
  let start=performance.now();const args={taskId:'00000000-0000-0000-0000-000000000000',bindingId:null,candidate:Array.from(candidate),output:Array.from(output)};const arrayMs=performance.now()-start;
  start=performance.now();const json=encode(args);const encodeMs=performance.now()-start;
  start=performance.now();const raw=encode(output);const rawEncodeMs=performance.now()-start;
  assert.equal(raw.data,output);assert.equal(raw.contentType,'application/octet-stream');assert.equal(json.contentType,'application/json');
  rows.push({round,repeat,arrayMs,encodeMs,rawEncodeMs,jsonBytes:Buffer.byteLength(json.data),rawBytes:output.length,candidateBytes:candidate.length});
  if(round===10&&repeat===2){await writeFile(path.join(out,'wire.json'),json.data);await writeFile(path.join(out,'wire-output.bin'),output);}
 }
}
const result={scope:'Node CPU probe of exact Tauri 2.12.3 JS encoder; not WebView IPC latency or end-to-end save',encoderSha256:createHash('sha256').update(encoderSource).digest('hex'),rows};
await writeFile(path.join(out,'js-wire.json'),JSON.stringify(result,null,2)+'\n');console.log(JSON.stringify(result));
