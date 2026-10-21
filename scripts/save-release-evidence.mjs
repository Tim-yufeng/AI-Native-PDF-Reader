// Bounded synthetic-only evidence. Never invokes Edge, a Provider, an installer
// or the original-save command. No PDF input is modified.
import {readFile,writeFile,mkdir,readdir,stat,lstat,realpath} from 'node:fs/promises';
import {createHash,randomUUID} from 'node:crypto';
import path from 'node:path';
import assert from 'node:assert/strict';
import * as pdfjs from 'pdfjs-dist/legacy/build/pdf.mjs';
import * as mupdf from 'mupdf';
import {appendAnnotationRevision} from '../src/lib/mupdf-writer.ts';
const root=path.resolve(import.meta.dirname,'..'),base=path.join(root,'tests/work/save-interop-performance');
const sha=bytes=>createHash('sha256').update(bytes).digest('hex');
const json=async file=>JSON.parse(await readFile(file,'utf8'));
const save=async(file,value)=>writeFile(file,JSON.stringify(value,null,2)+'\n');
async function regular(file){const s=await lstat(file);assert.ok(s.isFile()&&!s.isSymbolicLink());assert.equal((await realpath(file)).toLowerCase(),path.resolve(file).toLowerCase());assert.ok(s.size<=20*1024*1024);return s;}
async function pinned(file,bytes){try{assert.equal(sha(await readFile(file)),sha(bytes),'Existing evidence differs; use a new run');}catch(e){if(e.code!=='ENOENT')throw e;await writeFile(file,bytes,{flag:'wx'});}}
async function runInput(input,runtime='production-App-real-Wry-Release-IPC-and-files'){const dir=path.resolve(root,input);assert.equal(path.dirname(dir).toLowerCase(),path.join(root,'tests/work/save-chain-release').toLowerCase());assert.match(path.basename(dir),/^run-[a-f0-9-]{36}$/);assert.equal((await realpath(dir)).toLowerCase(),dir.toLowerCase());const report=await json(path.join(dir,'result.json'));assert.equal(report.status,'PASS');assert.equal(report.runtime,runtime);return{dir,report};}
const options={isEvalSupported:false,enableScripting:false,useSystemFonts:false,useWasm:false,isOffscreenCanvasSupported:false,stopAtErrors:true,standardFontDataUrl:path.join(root,'node_modules/pdfjs-dist/standard_fonts').replaceAll('\\','/')+'/'};
async function inspectPdf(file,png){
 await regular(file);const bytes=new Uint8Array(await readFile(file));const result={bytes:bytes.length,sha256:sha(bytes)};
 const load=pdfjs.getDocument({...options,data:bytes.slice()});
 try{const doc=await load.promise;assert.ok(doc.numPages<=20);const annotations=[];for(let i=1;i<=doc.numPages;i++){const page=await doc.getPage(i);for(const a of await page.getAnnotations())annotations.push({page:i,id:a.id,type:a.subtype,text:a.contentsObj?.str,rect:a.rect,isEditable:a.isEditable,hasAppearance:a.hasAppearance});await page.getTextContent();await page.getOperatorList();}result.pdfjs={status:'PASS',pages:doc.numPages,annotations};}catch(e){result.pdfjs={status:'FAIL',error:String(e)};}finally{await load.destroy();}
 let doc;
 try{
  doc=mupdf.PDFDocument.openDocument(bytes,'application/pdf');assert.ok(!doc.needsPassword()&&doc.countPages()<=20&&doc.countObjects()<=20000);
  const appearances=[],fonts=new Map();
  for(let p=0;p<doc.countPages();p++){const page=doc.loadPage(p);for(const a of page.getAnnotations()){const o=a.getObject(),ap=o.get('AP').get('N');appearances.push({page:p+1,type:a.getType(),flags:o.get('F').asNumber(),text:o.get('Contents').isString()?o.get('Contents').asString():null,rect:o.get('Rect').asJS(),normalAppearance:ap.isStream()?'stream':ap.isDictionary()?'states':'missing',bbox:ap.get('BBox').asJS(),matrix:ap.get('Matrix').isNull()?[1,0,0,1,0,0]:ap.get('Matrix').asJS()});}page.destroy();}
  for(let n=1;n<doc.countObjects();n++){const o=doc.newIndirect(n);for(const key of ['FontFile','FontFile2','FontFile3']){const f=o.get(key);if(f.isStream()){const decoded=f.readStream().asUint8Array();fonts.set(f.isIndirect()?f.asIndirect():`direct-${n}-${key}`,{bytes:decoded.length,sha256:sha(decoded)});}}}
  if(png){const page=doc.loadPage(0),pix=page.toPixmap([1.25,0,0,1.25,0,0],mupdf.ColorSpace.DeviceRGB,false,true);await writeFile(png,pix.asPNG());pix.destroy();page.destroy();}
  result.mupdf={status:'PASS',repaired:doc.wasRepaired(),pages:doc.countPages(),objects:doc.countObjects(),appearances,fontStreams:fonts.size,fontDecodedBytes:[...fonts.values()].reduce((s,f)=>s+f.bytes,0),uniqueFontHashes:[...new Set([...fonts.values()].map(f=>f.sha256))]};
 }catch(e){result.mupdf={status:'FAIL',error:String(e)};}finally{doc?.destroy();}
 return result;
}
async function capturePairs(dir,report){
 const names=await readdir(dir),outputs=names.filter(n=>/^[a-f0-9-]{36}-output\.pdf$/.test(n));const byHash=new Map();
 for(const name of outputs){const file=path.join(dir,name);await regular(file);byHash.set(sha(await readFile(file)),name);}
 return Promise.all(report.rows.filter(r=>r.test.startsWith('App Ctrl+S round ')).map(async row=>{const name=byHash.get(row.sha256);assert.ok(name,'Native output capture missing');const id=name.slice(0,-11),output=path.join(dir,name),candidate=path.join(dir,`${id}-candidate.pdf`);const capture=await json(path.join(dir,`${id}-capture.json`)).catch(e=>{if(e.code==='ENOENT')return null;throw e;});return{round:Number(row.test.split(' ').at(-1)),output,candidate,candidateBytes:(await stat(candidate)).size,outputBytes:(await stat(output)).size,captureMs:capture?.captureMs??null};}));
}
async function summarize(input){
 const{dir,report}=await runInput(input),pairs=await capturePairs(dir,report),out=path.join(base,path.basename(dir));await mkdir(out,{recursive:true});const rows=[];
 for(const metric of report.metrics){
  const pair=pairs.find(p=>p.round===metric.round),extra=report.saveEvidence?.find(p=>p.round===metric.round),s=metric.stages;
  const nativeKeys=['validationMs','backupMs','commitMs','historyMs','housekeepingMs'],nativeMeasuredMs=nativeKeys.reduce((n,k)=>n+(s['pdf-save:native-'+k]??0),0),arrayMs=(extra?.stages.candidateArray??0)+(extra?.stages.outputArray??0);
  rows.push({round:metric.round,candidateBytes:pair.candidateBytes,outputBytes:pair.outputBytes,wallMs:metric.wallMs,readyWallMs:extra?.readyWallMs??null,writerMs:s['pdf-save:writer'],readerValidationMs:s['pdf-save:validation'],ipcInclusiveMs:s['pdf-save:ipc'],arrayConversionMs:extra?arrayMs:null,nativeMeasuredMs,labCaptureMs:pair.captureMs,ipcResidualMs:extra&&pair.captureMs!==null?s['pdf-save:ipc']-arrayMs-nativeMeasuredMs-pair.captureMs:null,rawHeadersMs:extra?.stages.rawHeadersMs??null,nativeDecodeMs:s['pdf-save:native-decodeMs']??null,nativeQueueMs:s['pdf-save:native-queueMs']??null,nativeCommandMs:s['pdf-save:native-commandMs']??null,nativeCommitTotalMs:s['pdf-save:native-commitTotalMs']??null,frameworkTransportResidualMs:s['pdf-save:native-commandMs']===undefined?null:s['pdf-save:ipc']-s['pdf-save:native-commandMs']-(extra?.stages.rawHeadersMs??0),app:extra?.stages??null,maxFrameGapMs:extra?.maxFrameGapMs??null,operations:extra?.counts??null,writer:extra?.writer??null,output:path.relative(root,pair.output)});
 }
 const entries=await readdir(dir),retained=[];
 for(const d of await readdir(path.join(dir,'data-v1'))){const archive=path.join(dir,'data-v1',d,'pdf-backups');try{for(const group of await readdir(archive)){for(const name of await readdir(path.join(archive,group))){for(const leaf of await readdir(path.join(archive,group,name))){const f=path.join(archive,group,name,leaf);if((await stat(f)).isFile())retained.push({file:path.relative(dir,f),bytes:(await stat(f)).size});}}}}catch(e){if(e.code!=='ENOENT')throw e;}}
 const inventory={sourceFolderTransactions:entries.filter(n=>n.startsWith('.ainative-save-')),sourceFolderLegacyBackups:entries.filter(n=>n.startsWith('.ainative-backup-')),labPdfCaptures:entries.filter(n=>/-(candidate|output)\.pdf$/.test(n)).length,labCaptureBytes:pairs.reduce((s,p)=>s+p.candidateBytes+p.outputBytes,0),retainedBackupBytes:retained.reduce((s,f)=>s+f.bytes,0),retainedBackupFiles:retained,retentionCount:report.backupCount,browserProfile:'Excluded: lab-only browser data, never source-folder residuals'};
 const evidence={scope:'Actual hidden Release App. Observation-only timers; no Edge native PDF editing or public-release performance claim.',run:path.relative(root,dir),evidenceOnly:!!report.evidenceOnly,rounds:rows,inventory,limitations:['First save is WASM cold; the font has already been loaded by editing. OS cache is not controlled.','IPC includes numeric-array conversion. The residual also includes codec/transport/queue, grant acquisition and unmeasured native work; it is NOT pure disk time.','Native five timings are subintervals of IPC, not additional wall time. Lab capture time is test-only and excluded from production native timings.']};
 await save(path.join(out,'timings.json'),evidence);return{out,evidence,pairs,report};
}
async function prepare(input){
 const{out,evidence,pairs,report}=await summarize(input),kit=path.join(out,'roundtrip');await mkdir(kit,{recursive:true});const first=pairs.find(p=>p.round===1),source=path.join(kit,'01-ai-reader-saved.pdf'),working=path.join(kit,'02-edge-working.pdf');
 await pinned(source,await readFile(first.output));await pinned(working,await readFile(first.output));
 const control=path.join(root,'tests/fixtures/g1/freetext-control.pdf'),manifest=await json(path.join(root,'tests/fixtures/g1/manifest.json'));assert.equal(sha(await readFile(control)),manifest.files.find(f=>f.name==='freetext-control.pdf').sha256);
 await pinned(path.join(kit,'control-before-edge.pdf'),await readFile(control));
 const baseline=await inspectPdf(source,path.join(kit,'ai-reader-baseline.png')),standard=await inspectPdf(path.join(kit,'control-before-edge.pdf'),path.join(kit,'control-baseline.png'));
 assert.equal(baseline.pdfjs.status,'PASS');assert.equal(baseline.mupdf.status,'PASS');assert.equal(baseline.mupdf.repaired,false);
 const protocol={schema:1,run:evidence.run,sourceReportSha256:sha(await readFile(path.join(root,evidence.run,'result.json'))),source:'Actual App original-save round 1, fixed disposable synthetic source; not a private PDF.',baseline,control:standard,steps:[
 'Use the user-chosen test environment. Record Edge version. Open control-before-edge.pdf, verify existing FreeText appearance and try edit/move/save. Keep control-before-edge.pdf unchanged; Save As control-edge-returned.pdf.',
 'Open 02-edge-working.pdf in Edge. Compare Chinese/math FreeText, Highlight and Ink with ai-reader-baseline.png. Record appearance separately from editability.',
 'Try editing/moving existing FreeText. If unsupported, record unsupported L3 rather than a failed save. Add an ASCII text EDGE_RETURN_123 and an Ink stroke as a separate operation; Save As 03-edge-returned.pdf. Record exact save success/error, then close Edge.',
 'Run node scripts/save-release-evidence.mjs inspect '+path.relative(root,kit).replaceAll('\\','/')+'. This verifies file changes and rereads with PDF.js/MuPDF; native Edge provenance remains a user observation.',
 'In AI Reader open 03-edge-returned.pdf. Separately verify appearance, select/edit existing FreeText, move/resize it and edit Ink/Highlight. Save explicitly as 04-ai-reader-resaved.pdf, close/reopen and record the exact phase of any failure.',
 'Repeat inspect. Distinguish Edge save failure, parser/reopen failure, appearance mismatch, and inability to edit again. File existence or a PDF appearance dictionary alone proves none of the other phases.'
 ],status:{aiReaderInitialSave:'PASS',edgeSave:'USER CHECK',edgeAppearance:'USER CHECK',aiReaderReturnRead:'USER CHECK',aiReaderReturnAppearance:'USER CHECK',aiReaderReturnReeditSave:'USER CHECK'}};
 await save(path.join(kit,'protocol.json'),protocol);try{await writeFile(path.join(kit,'user-observations.json'),JSON.stringify({edgeVersion:null,externalSave:{success:null,error:null},existingFreeTextEditing:null,appearance:{edge:null,aiReaderReturn:null},aiReaderReeditSave:{success:null,error:null},note:'Fill only after actual manual operations; no Agent GUI or synthetic success claims.'},null,2)+'\n',{flag:'wx'});}catch(e){if(e.code!=='EEXIST')throw e;}
 console.log(JSON.stringify({status:'PREPARED',kit:path.relative(root,kit),baselineTypes:baseline.pdfjs.annotations.map(a=>a.type),edge:'USER CHECK',rows:report.metrics.length}));
}
async function inspectReturn(input){
 const kit=path.resolve(root,input);assert.ok(kit.toLowerCase().startsWith(base.toLowerCase()+path.sep)&&path.basename(kit)==='roundtrip');assert.equal((await realpath(kit)).toLowerCase(),kit.toLowerCase());const protocol=await json(path.join(kit,'protocol.json')),manual=await json(path.join(kit,'user-observations.json'));
 const reported=value=>value===true?'PASS (USER REPORTED)':value===false?'FAIL (USER REPORTED)':'USER CHECK';
 const result={scope:'Read-only synthetic roundtrip inspection; never automates or attests Edge UI',manualObservations:manual,phases:{externalSave:{status:reported(manual.externalSave?.success),error:manual.externalSave?.error??null},edgeAppearance:{status:reported(manual.appearance?.edge)},aiReaderAppearance:{status:reported(manual.appearance?.aiReaderReturn)},aiReaderReeditSave:{status:reported(manual.aiReaderReeditSave?.success),error:manual.aiReaderReeditSave?.error??null}}};
 for(const [phase,name] of [['externalReturn','03-edge-returned.pdf'],['aiReaderResave','04-ai-reader-resaved.pdf'],['controlExternalReturn','control-edge-returned.pdf']]){
  const file=path.join(kit,name);try{await regular(file);}catch(e){if(e.code==='ENOENT'){result.phases[phase]={status:phase==='externalReturn'&&manual.externalSave?.success===false?'BLOCKED: external save failed (USER REPORTED)':'USER CHECK',filePresent:false};continue;}throw e;}
  let comparison=phase==='controlExternalReturn'?protocol.control.sha256:protocol.baseline.sha256;
  if(phase==='aiReaderResave'){try{const external=path.join(kit,'03-edge-returned.pdf');await regular(external);comparison=sha(await readFile(external));}catch(e){if(e.code!=='ENOENT')throw e;comparison=null;}}
  const reading=await inspectPdf(file,path.join(kit,phase+'.png'));result.phases[phase]={filePresent:true,bytesChanged:comparison===null?null:reading.sha256!==comparison,comparisonSha256:comparison,reread:reading.pdfjs.status==='PASS'&&reading.mupdf.status==='PASS'?(reading.mupdf.repaired?'PARTIAL: repaired by MuPDF':'PASS'):'FAIL',reading,externalSave:'USER CHECK: existence/change cannot attest which application saved',appearance:'USER CHECK: compare rendered PNG and actual app view',reeditSave:'USER CHECK: isEditable/AP is not proof of actual UI editing'};
 }
 await save(path.join(kit,'inspection.json'),result);console.log(JSON.stringify(result));
}
async function growth(){
 const out=path.join(base,'growth-'+randomUUID());await mkdir(out,{recursive:true});const source=await readFile(path.join(root,'tests/fixtures/g1/plain.pdf')),font=new Uint8Array(await readFile(path.join(root,'worker/fonts/NotoSansSC-VF.ttf'))),rows=[];
 for(const [series,texts] of Object.entries({sameGlyphs:['中文 α ≥ 0123','3210 ≥ α 文中','中文 α ≥ 2301','1032 ≥ α 中文'],newGlyphs:['中文 α ≥ 0','中文 α ≥ 1','中文 α ≥ 2','中文 α ≥ 3']})){
  let current=new Uint8Array(source);for(let i=0;i<texts.length;i++){
   const old=current.slice(),load=pdfjs.getDocument({...options,data:old.slice()});let candidate;
   try{const doc=await load.promise,id=(await(await doc.getPage(1)).getAnnotations()).find(a=>a.subtype==='FreeText')?.id??null;doc.annotationStorage.setValue('pdfjs_internal_editor_evidence',{annotationType:pdfjs.AnnotationEditorType.FREETEXT,pageIndex:0,id,rect:[72,600,400,650],fontSize:12,color:[24,100,170],rotation:0,value:texts[i],date:'2026-10-18T00:00:00Z'});candidate=new Uint8Array(await doc.saveDocument());}finally{await load.destroy();}
   const result=appendAnnotationRevision(mupdf,candidate,font,current,(text,w,size)=>[{text,x:0,baseline:size*1.075}]);current=result.bytes;const file=path.join(out,`${series}-${i+1}-output.pdf`);await pinned(file,current);
   const structure=await inspectPdf(file);rows.push({series,round:i+1,oldBytes:old.length,candidateBytes:candidate.length,outputBytes:current.length,embeddedFont:result.embeddedFont,fontBytes:result.fontBytes,fontStreams:structure.mupdf.fontStreams,uniqueFonts:structure.mupdf.uniqueFontHashes?.length,reread:structure.pdfjs.status});
   if(i===texts.length-1){await pinned(path.join(out,`${series}-last-old.pdf`),old);await pinned(path.join(out,`${series}-last-candidate.pdf`),candidate);}
  }
 }
 await save(path.join(out,'growth.json'),{scope:'Function-level actual PDF.js/MuPDF writers, short single-line layout only; not Edge/GUI/native commit evidence',rows});console.log(JSON.stringify({out:path.relative(root,out),rows}));
}
async function collect(inputs){
 assert.ok(inputs.length>=2);const runs=[];for(const input of inputs)runs.push((await summarize(input)).evidence);
 const rows=runs.flatMap(r=>r.rounds),warm=rows.filter(r=>r.round>1),range=values=>({min:Math.min(...values),max:Math.max(...values)});
 const index={scope:'Evidence only: two isolated one-page synthetic ten-save runs, not a public-release performance guarantee',runs:runs.map(r=>r.run),tools:{node:process.version,pdfjs:pdfjs.version,mupdf:(await json(path.join(root,'node_modules/mupdf/package.json'))).version},firstSaveWallMs:range(rows.filter(r=>r.round===1).map(r=>r.wallMs)),tenthSaveWallMs:range(rows.filter(r=>r.round===10).map(r=>r.wallMs)),tenthIpcMs:range(rows.filter(r=>r.round===10).map(r=>r.ipcInclusiveMs)),tenthIpcResidualMs:range(rows.filter(r=>r.round===10).map(r=>r.ipcResidualMs)),warmWriterMs:range(warm.map(r=>r.writerMs)),writerAssetCheckMs:range(rows.map(r=>r.app['app.writerCheck'])),maxFrameGapMs:range(rows.filter(r=>r.round===10).map(r=>r.maxFrameGapMs)),nativeMeasuredLastMs:range(rows.filter(r=>r.round===10).map(r=>r.nativeMeasuredMs)),savedFileLastBytes:range(rows.filter(r=>r.round===10).map(r=>r.outputBytes)),sourceFolderResiduals:runs.map(r=>({run:r.run,transactions:r.inventory.sourceFolderTransactions,backups:r.inventory.sourceFolderLegacyBackups,retainedVersions:r.inventory.retentionCount,labCaptureBytes:r.inventory.labCaptureBytes})),unknown:'IPC residual includes serialization/transport/queue and native intervals not separately timed. No causal percentage is claimed for those subparts.',edge:'USER CHECK: no actual Edge-returned file was provided'};
 await save(path.join(base,'index.json'),index);console.log(JSON.stringify(index));
}
async function compare(inputs){
 assert.ok(inputs.length>=3&&inputs.length<=4);const replay=await runInput(inputs[0],'real-Wry-Release-identical-bytes-IPC-replay'),runs=[];
 for(const input of inputs.slice(1))runs.push((await summarize(input)).evidence);
 const median=values=>{const a=values.toSorted((x,y)=>x-y);return(a[Math.floor((a.length-1)/2)]+a[Math.floor(a.length/2)])/2;};
 const stats=mode=>{const rows=replay.report.rows.filter(r=>r.mode===mode);assert.equal(rows.length,3);const fields=['packingMs','headerMs','encodingMs','invokeMs','ipcInclusiveMs','frameworkTransportResidualMs'];return Object.fromEntries([...fields.map(k=>[k,median(rows.map(r=>r[k]))]),...['decodeMs','queueMs','commandMs','validationMs','commitTotalMs','backupMs','commitMs','historyMs','housekeepingMs'].map(k=>[k,median(rows.map(r=>r.timings[k]))])]);};
 const before=stats('json'),after=stats('raw'),baseline=await json(path.join(base,'index.json'));
 const first=runs.map(r=>r.rounds[0]),last=runs.map(r=>r.rounds.at(-1)),range=(rows,key)=>({min:Math.min(...rows.map(r=>r[key])),max:Math.max(...rows.map(r=>r[key]))});
 const result={scope:'Identical-byte native IPC replay isolates transport; complete App comparison includes resource cache, font compression and raw IPC. No Edge compatibility claim.',replay:path.relative(root,replay.dir),replayMedian:{before,after,savedMs:before.ipcInclusiveMs-after.ipcInclusiveMs,reductionPercent:(1-after.ipcInclusiveMs/before.ipcInclusiveMs)*100,target50Percent:after.ipcInclusiveMs<=before.ipcInclusiveMs*.5},baseline,app:{runs:runs.map(r=>r.run),firstSaveWallMs:range(first,'wallMs'),tenthSaveWallMs:range(last,'wallMs'),tenthReadyMs:range(last,'readyWallMs'),tenthIpcMs:range(last,'ipcInclusiveMs'),tenthNativeCommandMs:range(last,'nativeCommandMs'),tenthQueueMs:range(last,'nativeQueueMs'),tenthDecodeMs:range(last,'nativeDecodeMs'),tenthReaderValidationMs:range(last,'readerValidationMs'),tenthWriterMs:range(last,'writerMs'),tenthFrameGapMs:range(last,'maxFrameGapMs'),tenthAssetMs:range(last.map(r=>({ms:r.app['app.writerCheck']})),'ms'),tenthFileBytes:range(last,'outputBytes'),inventories:runs.map(r=>r.inventory)},limitations:['Native command timings include lab capture/fsync. Subintervals are not added to command total.','Replay residual includes framework JSON parsing, transport and callback scheduling; there is no assertion that it is pure transmission.','First save is cold; repeated-edit comparison uses the same operations, with compressed PDF bytes intentionally smaller.']};
 const out=path.join(base,'ipc-'+path.basename(replay.dir));await mkdir(out,{recursive:true});await save(path.join(out,'comparison.json'),result);console.log(JSON.stringify({out:path.relative(root,out),...result}));
}
await mkdir(base,{recursive:true});const[command,...inputs]=process.argv.slice(2),input=inputs[0];
if(command==='summarize'){const r=await summarize(input);console.log(JSON.stringify({out:path.relative(root,r.out),rounds:r.evidence.rounds.length,first:r.evidence.rounds[0],last:r.evidence.rounds.at(-1),sourceFolderTransactions:r.evidence.inventory.sourceFolderTransactions,retentionCount:r.evidence.inventory.retentionCount}));}
else if(command==='prepare')await prepare(input);else if(command==='inspect')await inspectReturn(input);else if(command==='growth')await growth();else if(command==='collect')await collect(inputs);else if(command==='compare')await compare(inputs);else throw Error('Use summarize RUN, prepare RUN, inspect KIT, growth, collect RUN RUN or compare IPC_RUN APP_RUN APP_RUN (synthetic-only)');
