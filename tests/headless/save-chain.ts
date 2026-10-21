// This page runs in a separate hidden Wry Release window, not a browser mock.
// No fake bridge is passed to desktopSource: every call uses Tauri's actual IPC.
import { invoke } from '@tauri-apps/api/core';
import { desktopSource, type DesktopDescriptor } from '../../src/lib/desktop-source.ts';
import {saveHeaders} from '../../src/lib/save-transport.ts';
import { loadMupdf } from '../../src/lib/mupdf-writer.ts';
import { getDocument, AnnotationEditorType, GlobalWorkerOptions } from 'pdfjs-dist/build/pdf.mjs';
import workerUrl from 'pdfjs-dist/build/pdf.worker.mjs?url';

GlobalWorkerOptions.workerSrc = workerUrl;
type Kind = 'freetext-only'|'highlight-only'|'ink-only'|'mixed';
type Row = { test:string; status:'PASS'|'FAIL'; code?:string; bytes?:number; annotations?:string[] };
const rows:Row[]=[];
function check(value:unknown,code:string):asserts value{if(!value)throw {code};}
const descriptors=(globalThis as unknown as {__SAVE_CHAIN_INPUTS__:Record<string,DesktopDescriptor>}).__SAVE_CHAIN_INPUTS__;
const safeCode=(error:unknown)=>{
  if(error&&typeof error==='object'){
    const entry=Object.getOwnPropertyDescriptor(error,'code');
    if(entry&&'value'in entry&&typeof entry.value==='string'&&/^[a-z0-9_-]{1,80}$/.test(entry.value))return entry.value;
  }
  return 'unclassified';
};
const options={isEvalSupported:false,enableScripting:false,useSystemFonts:false,useWasm:false,isOffscreenCanvasSupported:false,standardFontDataUrl:'/pdfjs/standard_fonts/'};
function entries(kind:Kind,round:number,annotations:any[]=[]){
  const id=(type:string)=>annotations.find(a=>a.subtype===type)?.id??null;
  const out:any[]=[];
  if(kind==='freetext-only'||kind==='mixed')out.push({annotationType:AnnotationEditorType.FREETEXT,pageIndex:0,id:id('FreeText'),rect:[120+round*8,460,360+round*8,530],fontSize:round?14:12,color:round?[180,30,70]:[24,100,170],rotation:0,value:round?'修改后 中文 β ≥ 20':'中文 α + β ≤ 10',date:'2026-10-15T00:00:00Z'});
  if(kind==='highlight-only'||kind==='mixed')out.push({annotationType:AnnotationEditorType.HIGHLIGHT,pageIndex:0,id:id('Highlight'),rect:[120,640,320,668],rotation:0,color:round?[100,255,100]:[255,255,0],opacity:0.4,thickness:12,quadPoints:[120,640,320,640,120,668,320,668],outlines:[[120,640,320,640,320,668,120,668]],date:'2026-10-15T00:00:00Z'});
  if(kind==='ink-only'||kind==='mixed')out.push({annotationType:AnnotationEditorType.INK,pageIndex:0,id:id('Ink'),rect:[140,560,300,620],rotation:0,color:round?[20,80,180]:[200,45,60],opacity:1,thickness:round?4:3,paths:{lines:[[140,560,160,620,300,560]],points:[[140,560,160,620,300,560]]},date:'2026-10-15T00:00:00Z'});
  return out;
}
async function inspect(bytes:Uint8Array,kind:Kind,round:number,serializedFontColor:number[]|null){
  const task=getDocument({...options,data:bytes.slice()}),pdf=await task.promise;
  try{
    check(pdf.numPages===4,'page_count');
    const page=await pdf.getPage(1);await page.getOperatorList();
    const annotations=await page.getAnnotations();
    const expected=entries(kind,round);
    check(annotations.length===expected.length,'annotation_count');
    for(const a of annotations){
      check(a.hasAppearance&&a.isEditable,'annotation_not_editable');
      const type=a.subtype==='FreeText'?AnnotationEditorType.FREETEXT:a.subtype==='Highlight'?AnnotationEditorType.HIGHLIGHT:AnnotationEditorType.INK;
      const wanted=expected.find(e=>e.annotationType===type);
      if(a.subtype==='FreeText'){
        // /C is annotation background/border, not the text color. PDF.js rounds
        // the serialized /DA RGB (e.g. 24,100,170 -> 23,99,171), so require the
        // Writer output to preserve the candidate /DA exactly. A second-round
        // candidate can still carry the OLD /AP, so PDF.js getAnnotations()
        // is not a valid source for the NEW /DA color until the Writer rebuilds
        // that appearance. The baseline below therefore reads raw /DA.
        const actual=a.defaultAppearanceData?.fontColor;
        if(serializedFontColor)check(serializedFontColor.length===3&&actual?.length===3&&Array.from(actual).every((v,i)=>v===serializedFontColor[i]),'annotation_color');
      }else{
        check(wanted&&a.color?.length===3&&Array.from(a.color).every((v,i)=>v===wanted.color[i]),'annotation_color');
      }
      check(wanted&&a.rect.every((v:number,i:number)=>Math.abs(v-wanted.rect[i])<1e-4),'annotation_rect');
      if(a.subtype==='FreeText'){
        check(a.contentsObj?.str===expected.find(e=>e.annotationType===AnnotationEditorType.FREETEXT)?.value,'unicode_content');
        check(a.defaultAppearanceData?.fontSize===(round?14:12),'font_size');
        check(a.rect[0]===120+round*8,'position');
      }
    }
    for(let p=1;p<=4;p++){
      const body=await(await pdf.getPage(p)).getTextContent();
      check(body.items.some((item:any)=>item.str?.includes('Original synthetic editor geometry')),'original_body_missing');
    }
    return annotations.map(a=>a.subtype);
  }finally{await task.destroy();}
}
async function expectRefusal(test:string,operation:()=>Promise<unknown>,code:string){
  try{await operation();rows.push({test,status:'FAIL',code:'unexpected_success'});}
  catch(error){const got=safeCode(error);rows.push({test,status:got===code?'PASS':'FAIL',code:got});}
}
async function run(){
  check(descriptors&&Object.keys(descriptors).length===7,'missing_native_inputs');
  const checked=await invoke<string>('check_unicode_writer');check(typeof checked==='string','resource_response');
  rows.push({test:'release-resource-resolver',status:'PASS'});
  for(const kind of ['freetext-only','highlight-only','ink-only','mixed'] as Kind[]){
    let source=desktopSource(descriptors[kind]);
    try{
      for(let round=0;round<2;round++){
        const original=await source.read(new AbortController().signal);
        const task=getDocument({...options,data:original.slice()}),pdf=await task.promise;
        let candidate:Uint8Array;
        try{
          const page=await pdf.getPage(1),existing=await page.getAnnotations();
          entries(kind,round,existing).forEach((entry,index)=>pdf.annotationStorage.setValue(`pdfjs_internal_editor_release_${index}`,entry));
          candidate=new Uint8Array(await pdf.saveDocument());
        }finally{await task.destroy();}
        let serializedFontColor:number[]|null=null;
        if(kind==='freetext-only'||kind==='mixed'){
          const mupdf=await loadMupdf(),reference=mupdf.PDFDocument.openDocument(candidate.slice(),'application/pdf');
          try{
            const text=reference.loadPage(0).getAnnotations().find((a:any)=>a.getType()==='FreeText');
            const da=text?.getObject().get('DA').asString();
            const rgb=typeof da==='string'?/([\d.]+)\s+([\d.]+)\s+([\d.]+)\s+rg/.exec(da):null;
            check(rgb,'candidate_font_color_missing');
            serializedFontColor=rgb.slice(1).map(v=>Math.round(Number(v)*255));
            const requested=entries(kind,round).find(e=>e.annotationType===AnnotationEditorType.FREETEXT).color;
            // Exact conversion used by locked PDF.js getPdfColor and
            // core_utils_numberToString (pdf.worker.mjs), not a tolerance.
            const requestedSerialized=requested.map((value:number)=>{
              const channel=value/255,rounded=Math.round(channel*100);
              const decimal=rounded%100===0?rounded/100:Number(channel.toFixed(rounded%10===0?1:2));
              return Math.round(decimal*255);
            });
            check(serializedFontColor.every((v,i)=>v===requestedSerialized[i]),'candidate_font_color_changed');
          }finally{reference.destroy();}
        }
        const copy=await source.exportCopy!(candidate,new AbortController().signal);check(copy,'missing_export_descriptor');
        await source.release?.();source=copy;
        const saved=await source.read(new AbortController().signal);
        const types=await inspect(saved,kind,round,serializedFontColor);
        rows.push({test:`${kind}-round-${round+1}`,status:'PASS',bytes:saved.length,annotations:types});
      }
    }catch(error){rows.push({test:kind,status:'FAIL',code:safeCode(error)});}
    finally{await source.release?.();}
  }
  let original=desktopSource(descriptors.original);
  let binding=await invoke<any>('m4_bind_document',{grantId:original.grantId,pageCount:4});
  const documentId=binding.documentId;
  await invoke('m4_add_message',{bindingId:binding.bindingId,content:'Synthetic history survives original save',page:1,parentId:null});
  await invoke('m4_position',{bindingId:binding.bindingId,position:{page:2,zoom:'page-width',top:.2,left:0}});
  for(let round=0;round<2;round++){
    const bytes=await original.read(new AbortController().signal),loading=getDocument({...options,data:bytes.slice()}),pdf=await loading.promise;
    let candidate:Uint8Array;
    try{const existing=await(await pdf.getPage(1)).getAnnotations();entries('mixed',round,existing).forEach((entry,index)=>pdf.annotationStorage.setValue('pdfjs_internal_editor_original_'+index,entry));candidate=new Uint8Array(await pdf.saveDocument());}finally{await loading.destroy();}
    const oldBinding=binding.bindingId;
    const result=await original.saveOriginal!(candidate,new AbortController().signal,binding.bindingId);
    check(result.source&&result.binding&&!result.historyPending,'original_handoff');original=result.source;binding=result.binding;
    check((await invoke<any[]>('pending_pdf_recovery',{grantId:original.grantId})).length===0,'normal_commit_misclassified_pending');
    check(binding.documentId===documentId&&binding.bindingId!==oldBinding&&binding.position?.page===2,'history_identity_or_position');
    const messages=await invoke<any[]>('m4_messages',{bindingId:binding.bindingId});check(messages.some(m=>m.content==='Synthetic history survives original save'),'history_lost');
    await expectRefusal('old-binding-round-'+round,()=>invoke('m4_messages',{bindingId:oldBinding}),'unauthorized');
    const saved=await original.read(new AbortController().signal);
    const annotations=await inspect(saved,'mixed',round,null);
    rows.push({test:'original-round-'+(round+1),status:'PASS',bytes:saved.length,annotations,historyRestored:true});
  }
  await original.release?.();
  const retired=descriptors.revoked;
  await invoke('release_pdf',{grantId:retired.grantId});
  await expectRefusal('revoked-grant',()=>invoke('begin_unicode_export',{grantId:retired.grantId}),'unauthorized');
  const unknown='00000000-0000-0000-0000-000000000099';
  await expectRefusal('unknown-task',()=>invoke('finish_unicode_export',new Uint8Array([1,2]),saveHeaders(unknown,1)),'unauthorized');
  // The normal command must refuse original writes before showing any dialog.
  await expectRefusal('original-save-unknown-task',()=>invoke('save_original_pdf',new Uint8Array([1,2]),saveHeaders(unknown,1)),'unauthorized');
  const taskId=await invoke<string>('begin_unicode_export',{grantId:descriptors.negative.grantId});
  await expectRefusal('invalid-byte-relation',()=>invoke('finish_unicode_export',new Uint8Array([37,80,68,70,0]),saveHeaders(taskId,4)),'writer_conversion_refused');
  const cancelId=await invoke<string>('begin_unicode_export',{grantId:descriptors.negative.grantId});
  await invoke('cancel_unicode_export',{taskId:cancelId});
  await expectRefusal('cancelled-task',()=>invoke('finish_unicode_export',new Uint8Array([1,2]),saveHeaders(cancelId,1)),'unauthorized');
  await invoke('release_pdf',{grantId:descriptors.negative.grantId});
}
run().catch(error=>rows.push({test:'harness',status:'FAIL',code:safeCode(error)})).finally(()=>{
  document.title='SAVE_CHAIN_RESULT:'+JSON.stringify({schema:1,status:rows.some(r=>r.status==='FAIL')?'FAIL':'PASS',rows});
});
