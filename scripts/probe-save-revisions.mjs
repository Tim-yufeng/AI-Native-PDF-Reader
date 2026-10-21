// Isolated regression generator: synthetic input, production PDF.js/MuPDF
// byte writers, no UI, credentials, provider requests, or original user files.
import {readFile, writeFile, mkdir} from 'node:fs/promises';
import path from 'node:path';
import assert from 'node:assert/strict';
import * as pdfjs from 'pdfjs-dist/legacy/build/pdf.mjs';
import * as mupdf from 'mupdf';
import {editorFixture} from '../tests/frontend/editor-fixture.ts';
import {appendAnnotationRevision} from '../src/lib/mupdf-writer.ts';
const root=path.resolve(import.meta.dirname,'..');
const work=path.join(root,'tests/work/stability-save-probe');await mkdir(work,{recursive:true});
const font=new Uint8Array(await readFile(path.join(root,'worker/fonts/NotoSansSC-VF.ttf')));
const options={isEvalSupported:false,enableScripting:false,useSystemFonts:false,useWasm:false,isOffscreenCanvasSupported:false,
 standardFontDataUrl:path.join(root,'node_modules/pdfjs-dist/standard_fonts').replaceAll('\\','/')+'/'};
const encoder=new TextEncoder();
function fixture(label){
 const source=editorFixture();
 if(label.startsWith('classic-')){source[7]=label.endsWith('13')?51:52;return source;}
 if(label==='literal-delimiter'){
  const text=Buffer.from(source).toString('latin1');
  return encoder.encode(text.replace('/Root 1 0 R >>\nstartxref','/Root 1 0 R /ID [(012345>>89abcdef)(0123456789abcdef)] >>\nstartxref'));
 }
 if(label==='xref-stream'||label==='stream-delete'){
  const doc=mupdf.PDFDocument.openDocument(source,'application/pdf');
  try{return doc.saveToBuffer('compress=yes,objstms=yes').asUint8Array().slice();}finally{doc.destroy();}
 }
 return source;
}
const rows=[];
for(const label of ['classic-13','classic-14','xref-stream','literal-delimiter','ordinary','classic-delete','stream-delete']){
 let current=fixture(label);await writeFile(path.join(work,`${label}-source.pdf`),current);
 for(let round=1;round<=10;round++){
  const old=current.slice(),load=pdfjs.getDocument({...options,data:old.slice()}),pdf=await load.promise;
  let candidate;
  try{
   const annotations=await(await pdf.getPage(1)).getAnnotations(),id=kind=>annotations.find(a=>a.subtype===kind)?.id??null;
   const value=`中文 α + β ≤ ${10+round}`;
   const entries=[{annotationType:pdfjs.AnnotationEditorType.FREETEXT,pageIndex:0,id:id('FreeText'),rect:[120+round,460,360+round,530],fontSize:12,color:[24,100,170],rotation:0,value},
    {annotationType:pdfjs.AnnotationEditorType.HIGHLIGHT,pageIndex:0,id:id('Highlight'),rect:[120,640,320,668],rotation:0,color:[255,255,0],opacity:.4,thickness:12,quadPoints:[120,640,320,640,120,668,320,668],outlines:[[120,640,320,640,320,668,120,668]]},
    {annotationType:pdfjs.AnnotationEditorType.INK,pageIndex:0,id:id('Ink'),rect:[140,560,300,620],rotation:0,color:[200,45,60],opacity:1,thickness:3,paths:{lines:[[140,560,160,620,300,560]],points:[[140,560,160,620,300,560]]}}];
   if(label.endsWith('-delete')&&round%2===0)annotations.forEach((annotation,index)=>pdf.annotationStorage.setValue(`pdfjs_internal_editor_probe_${index}`,{id:annotation.id,deleted:true,pageIndex:0,popupRef:''}));
   else entries.forEach((entry,index)=>pdf.annotationStorage.setValue(`pdfjs_internal_editor_probe_${index}`,{...entry,date:'2026-10-17T00:00:00Z'}));
   candidate=new Uint8Array(await pdf.saveDocument());
  }finally{await load.destroy();}
  // This byte/protocol probe intentionally uses a short, single-line fixture.
  // Browser wrapping/font appearance is exercised by font-appearance.html.
  current=appendAnnotationRevision(mupdf,candidate,font,old,(text,width,size)=>[{text,x:0,baseline:size*1.075}]).bytes;
  assert.ok(current.length>candidate.length&&candidate.length>old.length);
  assert.deepEqual(current.subarray(0,candidate.length),candidate);assert.deepEqual(candidate.subarray(0,old.length),old);
  const reopened=pdfjs.getDocument({...options,data:current.slice()}),opened=await reopened.promise;
  try{const annotations=await(await opened.getPage(1)).getAnnotations();assert.equal(annotations.length,label.endsWith('-delete')&&round%2===0?0:3);for(const a of annotations)assert.ok(a.hasAppearance&&a.isEditable);if(annotations.length)assert.equal(annotations.find(a=>a.subtype==='FreeText').contentsObj.str,`中文 α + β ≤ ${10+round}`);}finally{await reopened.destroy();}
  const stem=`${label}-${round}`;
  await writeFile(path.join(work,`${stem}-old.pdf`),old);await writeFile(path.join(work,`${stem}-candidate.pdf`),candidate);await writeFile(path.join(work,`${stem}-output.pdf`),current);
  rows.push({label,round,stem,oldBytes:old.length,candidateBytes:candidate.length,outputBytes:current.length});
 }
}
await writeFile(path.join(work,'manifest.json'),JSON.stringify(rows,null,2));
console.log(JSON.stringify({scope:'function-level actual byte writers; production Rust admission must be checked separately',pairs:rows.length,work,rows:rows.filter(r=>[1,2,3,5,10].includes(r.round))},null,2));
