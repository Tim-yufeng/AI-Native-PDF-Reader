// Read-only final gate: never bundle a cached, feature-gated lab executable.
import {readFile,stat,readdir} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';
import path from 'node:path';
import assert from 'node:assert/strict';
export async function saveInputsFingerprint(root) {
  const files=['package.json','package-lock.json','vite.config.ts','src-tauri/Cargo.toml','src-tauri/Cargo.lock','src-tauri/build.rs','src-tauri/tauri.conf.json','src-tauri/writer-assets.lock.json','tests/headless/save-app.html','tests/headless/save-ipc.html','scripts/build-save-chain-lab.mjs','scripts/save-lab-instrumentation.mjs'];
  async function walk(directory){for(const e of await readdir(path.join(root,directory),{withFileTypes:true})){const p=directory+'/'+e.name;if(e.isDirectory())await walk(p);else if(e.isFile())files.push(p);else throw Error('Linked save input refused');}}
  for(const directory of ['src','src-tauri/src','src-tauri/permissions','src-tauri/capabilities'])await walk(directory);
  const hash=createHash('sha256');for(const file of files.sort()){hash.update(file+'\0');hash.update(await readFile(path.join(root,file)));}return hash.digest('hex');
}
export function ordinaryRelease(bytes){
  assert.ok(bytes.length>1024*1024&&bytes[0]===77&&bytes[1]===90,'Release executable missing/invalid');
  assert.ok(!bytes.includes(Buffer.from('AI_NATIVE_SAVE_CHAIN_LAB_ONLY_v1')),'Refusing to bundle the hidden save-chain lab; rebuild the normal Release first');
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){
  const root=path.resolve(fileURLToPath(new URL('../',import.meta.url)));
  const gate=JSON.parse(await readFile(path.join(root,'.cache/save-app-gate.json'),'utf8'));
  assert.equal(gate.fingerprint,await saveInputsFingerprint(root),'Save inputs changed after actual App/native verification; rerun the existing isolated Release App loop before packaging');
  const reportPath=path.resolve(root,gate.report);
  assert.ok(reportPath.startsWith(path.join(root,'tests/work/save-chain-release')+path.sep)&&path.basename(reportPath)==='result.json','Unexpected save evidence path');
  const reportBytes=await readFile(reportPath),report=JSON.parse(reportBytes);
  assert.equal(createHash('sha256').update(reportBytes).digest('hex'),gate.reportHash,'Save evidence changed');
  assert.equal(report.status,'PASS');assert.equal(report.runtime,'production-App-real-Wry-Release-IPC-and-files');assert.ok(report.rows.length>=3&&report.backupCount>=2&&report.saveAsBytes>report.nativeBytes,'Incomplete actual App save evidence');
  // Refuse a frontend edited after Vite finished, rather than package stale UI.
  const [source,built]=await Promise.all(['src/App.svelte','dist/index.html'].map(p=>stat(path.join(root,p))));
  assert.ok(source.mtimeMs<=built.mtimeMs,'Frontend changed after build; rebuild before bundling');
  ordinaryRelease(await readFile(path.join(root,'.cache/tauri-target/release/ai-native-pdf-reader.exe')));
  const fake=Buffer.alloc(1024*1024+1);fake.write('MZ');ordinaryRelease(fake);
  Buffer.from('AI_NATIVE_SAVE_CHAIN_LAB_ONLY_v1').copy(fake,128);
  assert.throws(()=>ordinaryRelease(fake),'A lab marker must always refuse bundling');
  console.log(JSON.stringify({status:'PASS',scope:'cached ordinary Release and in-memory lab rejection only; no installer generated'}));
}
