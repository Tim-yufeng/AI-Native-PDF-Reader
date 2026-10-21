// Run only the feature-gated, hidden Release lab, never the normal reader/NSIS.
import { spawn, execFile } from 'node:child_process';
import { readdir, readFile, realpath, mkdir, stat, writeFile } from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {saveInputsFingerprint} from './check-release-artifact.mjs';
import { existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
const project=path.resolve(fileURLToPath(new URL('../',import.meta.url)));
const appMode=process.argv.includes('--app'),ipcMode=process.argv.includes('--ipc'),inkOnly=process.argv.includes('--ink-only'),evidence=process.argv.includes('--evidence');let fingerprint;
if(ipcMode&&(appMode||inkOnly||evidence))throw Error('IPC replay is separate from App/package evidence');
if(inkOnly&&!appMode)throw Error('The isolated Ink test requires --app');
if(evidence&&(!appMode||inkOnly))throw Error('Save evidence requires the complete App loop');
if(appMode||ipcMode){
 fingerprint=await saveInputsFingerprint(project);
 const built=JSON.parse(await readFile(path.join(project,ipcMode?'.cache/save-ipc-build.json':'.cache/save-app-build.json'),'utf8'));
 if(built.fingerprint!==fingerprint)throw Error('Rebuild the isolated App after source changes');
 // Diagnostics may reproduce a failure but can never approve a package.
 if(built.diagnostic)throw Error('Diagnostic App builds cannot approve packaging');
 if(!ipcMode&&!!built.evidence!==evidence)throw Error('Evidence and ordinary App builds cannot be interchanged');
}
const exe=path.join(project,'.cache/tauri-target/release/ai-native-pdf-reader.exe');
const runtime=path.join(project,'src-tauri/webview2-fixed-runtime');
if(!existsSync(path.join(runtime,'msedgewebview2.exe')))throw Error('Existing fixed WebView2 runtime is required; do not install automatically');
if((await realpath(exe)).toLowerCase()!==exe.toLowerCase())throw Error('Linked lab executable refused');
// A normal cached Release binary would open the real reader window and never
// exit. Refuse it before creating the real run. A byte probe is not usable
// here: release builds set `strip = true`, so a marker literal that no live
// code reads is legitimately absent from the binary. The lab's own entry point
// instead refuses any argument set other than its two exact lab switches, so an
// argument-free launch must exit immediately with the lab's documented code 64.
async function provesLabEntry(){
 const probe=spawn(exe,[],{cwd:project,windowsHide:true,stdio:'ignore'});
 const outcome=await new Promise(resolve=>{const timer=setTimeout(()=>resolve('timeout'),10000);
  probe.once('error',()=>{clearTimeout(timer);resolve('error');});
  probe.once('exit',code=>{clearTimeout(timer);resolve(code);});});
 if(outcome==='timeout'||outcome==='error'){probe.kill();return false;}
 return outcome===64;
}
if(!(await provesLabEntry()))throw Error('The cached executable is not the explicit hidden lab build; refusing to launch');
const runs=path.join(project,'tests/work/save-chain-release');await mkdir(runs,{recursive:true});
const before=new Set(await readdir(runs));
const temp=path.join(project,'.cache/tmp/save-chain');await mkdir(temp,{recursive:true});
const env={...process.env,TEMP:temp,TMP:temp,WEBVIEW2_BROWSER_EXECUTABLE_FOLDER:runtime,SAVE_APP_INK_ONLY:inkOnly?'1':'0',SAVE_APP_EVIDENCE:evidence?'1':'0'};
const child=spawn(exe,[ipcMode?'--save-ipc-lab':appMode?'--save-app-lab':'--save-chain-lab'],{cwd:project,env,windowsHide:true,stdio:'ignore'});
let timer;
const code=await Promise.race([
 new Promise((resolve,reject)=>{child.once('error',reject);child.once('exit',resolve);}),
 new Promise(resolve=>timer=setTimeout(()=>{
  if(child.exitCode!==null){resolve(child.exitCode);return;}
  execFile('taskkill',['/PID',String(child.pid),'/T','/F'],{windowsHide:true},()=>resolve(124));
 },180000)),
]);clearTimeout(timer);
const created=(await readdir(runs)).filter(name=>!before.has(name)&&/^run-[a-f0-9-]{36}$/.test(name));
if(created.length!==1)throw Error(`Expected exactly one synthetic run; got ${created.length}, exit ${code}`);
const resultPath=path.join(runs,created[0],'result.json');
let report;try{report=JSON.parse(await readFile(resultPath,'utf8'));}catch{report={status:'FAIL',stage:'missing-native-report'};}
if(ipcMode&&code===0&&report.status==='PASS'){
 if(report.runtime!=='real-Wry-Release-identical-bytes-IPC-replay'||report.rows?.length!==6)throw Error('Native replay evidence missing');
 if(fingerprint!==await saveInputsFingerprint(project))throw Error('Replay sources changed during the run');
}else if(appMode&&code===0&&report.status==='PASS'&&inkOnly){
 if(report.runtime!=='production-App-real-Wry-Release-ink-only'||report.scope!=='ink-only')throw Error('Isolated Ink evidence missing');
}else if(appMode&&code===0&&report.status==='PASS'&&evidence){
 if(report.runtime!=='production-App-real-Wry-Release-IPC-and-files'||!report.evidenceOnly||report.saveEvidence?.length!==10)throw Error('Complete save evidence missing');
 if(fingerprint!==await saveInputsFingerprint(project))throw Error('Evidence sources changed during the run');
}else if(appMode&&code===0&&report.status==='PASS'){
 if(report.runtime!=='production-App-real-Wry-Release-IPC-and-files')throw Error('Only the complete App save loop can approve packaging');
 if(fingerprint!==await saveInputsFingerprint(project))throw Error('Save sources changed during verification');
 if(report.saveAsBytes<=report.nativeBytes||report.rows.length<3)throw Error('Rebuild the lab: native Save As verification missing');
 const binaryTime=(await stat(exe)).mtimeMs;
 async function checkNative(directory){for(const e of await readdir(directory,{withFileTypes:true})){const p=path.join(directory,e.name);if(e.isDirectory())await checkNative(p);else if(e.name.endsWith('.rs')&&(await stat(p)).mtimeMs>binaryTime)throw Error('Native save sources changed after compilation');}}
 await checkNative(path.join(project,'src-tauri/src'));
 await writeFile(path.join(project,'.cache/save-app-gate.json'),JSON.stringify({fingerprint,report:path.relative(project,resultPath),reportHash:createHash('sha256').update(await readFile(resultPath)).digest('hex')}));
}
console.log(JSON.stringify({exitCode:code,result:path.relative(project,resultPath),report},null,2));
process.exitCode=code===0&&report.status==='PASS'?0:1;
