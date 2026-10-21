import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile,writeFile,mkdir} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import path from 'node:path';
import * as mupdf from 'mupdf';
import * as pdfjs from 'pdfjs-dist/legacy/build/pdf.mjs';
import {appendAnnotationRevision} from '../../src/lib/mupdf-writer.ts';

const out='tests/work/save-interop-performance/worker-font-regular';
const hash=(bytes:Uint8Array|Uint8ClampedArray)=>createHash('sha256').update(bytes).digest('hex');
const options={isEvalSupported:false,enableScripting:false,useSystemFonts:false,useWasm:false,isOffscreenCanvasSupported:false,stopAtErrors:true,standardFontDataUrl:path.resolve('node_modules/pdfjs-dist/standard_fonts').replaceAll('\\','/')+'/'};
// A/B changes only the final storage option, using exactly the same Writer.
const uncompressed={...mupdf,PDFDocument:{openDocument(...args:Parameters<typeof mupdf.PDFDocument.openDocument>){
 const doc=mupdf.PDFDocument.openDocument(...args);assert.ok(doc instanceof mupdf.PDFDocument);const save=doc.saveToBuffer.bind(doc);
 doc.saveToBuffer=(opts:Record<string,unknown>)=>save({...opts,'compress-fonts':false});return doc;
}}};
const layout=(text:string,_width:number,size:number)=>[{text,x:0,baseline:size*1.075}];
async function candidate(bytes:Uint8Array,text:string){
 const loading=pdfjs.getDocument({...options,data:bytes.slice()});
 try{const doc=await loading.promise,id=(await(await doc.getPage(1)).getAnnotations()).find(a=>a.subtype==='FreeText')?.id??null;
  doc.annotationStorage.setValue('pdfjs_internal_editor_compression',{annotationType:pdfjs.AnnotationEditorType.FREETEXT,pageIndex:0,id,rect:[72,600,400,650],fontSize:12,color:[24,100,170],rotation:0,value:text,date:'2026-10-19T00:00:00Z'});
  return new Uint8Array(await doc.saveDocument());
 }finally{await loading.destroy();}
}
function snapshot(bytes:Uint8Array){
 const doc=mupdf.PDFDocument.openDocument(bytes,'application/pdf');
 assert.ok(doc instanceof mupdf.PDFDocument);
 try{
  assert.equal(doc.wasRepaired(),false);const page=doc.loadPage(0),pix=page.toPixmap([1,0,0,1,0,0],mupdf.ColorSpace.DeviceRGB,false,true);
  const pixelHash=hash(pix.getPixels());pix.destroy();
  const annotations=page.getAnnotations().map(a=>{const o=a.getObject(),ap=o.get('AP').get('N');return{type:a.getType(),text:a.getContents(),rect:o.get('Rect').asJS(),rotation:o.get('Rotate').asJS(),bbox:ap.get('BBox').asJS(),matrix:ap.get('Matrix').asJS(),ap:hash(ap.readStream().asUint8Array())};});page.destroy();
  const fonts=[];for(let i=1;i<doc.countObjects();i++){const font=doc.newIndirect(i).get('FontFile2');if(font.isStream())fonts.push({decoded:hash(font.readStream().asUint8Array()),filter:font.get('Filter').asJS(),stored:font.readRawStream().asUint8Array().length});}
  return{pixelHash,annotations,fonts};
 }finally{doc.destroy();}
}
async function verifyRead(bytes:Uint8Array){
 const load=pdfjs.getDocument({...options,data:bytes.slice()});
 try{const doc=await load.promise,page=await doc.getPage(1);await page.getOperatorList();return(await page.getAnnotations()).map(a=>({id:a.id,type:a.subtype,text:a.contentsObj?.str,rect:a.rect}));}finally{await load.destroy();}
}

test('font compression preserves decoded font/AP, pixels, native annotations, prefix and subsequent edit saves',async()=>{
 await mkdir(out,{recursive:true});const font=new Uint8Array(await readFile('worker/fonts/NotoSansSC-Regular-v41.ttf'));
 const source=new Uint8Array(await readFile('tests/fixtures/g1/plain.pdf'));let raw:Uint8Array=source,compressed:Uint8Array=source;const rows=[];
 for(const [index,text] of ['中文 α ≥ 0','中文 α ≥ 1','中文 α ≥ 2','中文 α ≥ 3','3 ≥ α 文中'].entries()){
  const rawCandidate=await candidate(raw,text),compressedCandidate=await candidate(compressed,text);
  let start=performance.now();const a=appendAnnotationRevision(uncompressed,rawCandidate,font,raw,layout);const rawMs=performance.now()-start;
  start=performance.now();const b=appendAnnotationRevision(mupdf,compressedCandidate,font,compressed,layout);const compressedMs=performance.now()-start;
  assert.deepEqual(a.bytes.subarray(0,rawCandidate.length),rawCandidate);assert.deepEqual(b.bytes.subarray(0,compressedCandidate.length),compressedCandidate);
  const x=snapshot(a.bytes),y=snapshot(b.bytes);assert.equal(x.pixelHash,y.pixelHash);assert.deepEqual(x.annotations,y.annotations);
  assert.deepEqual(x.fonts.map(f=>f.decoded),y.fonts.map(f=>f.decoded));assert.ok(y.fonts.every(f=>f.filter==='FlateDecode'));
  assert.deepEqual(await verifyRead(a.bytes),await verifyRead(b.bytes));
  if(index===4){assert.equal(b.embeddedFont,false);assert.ok(b.bytes.length-compressed.length<10000,'reuse must not re-append old font streams');}
  else assert.ok(b.bytes.length-compressedCandidate.length<(a.bytes.length-rawCandidate.length)/4,'font append should fall by at least 75% on this fixed sample');
  rows.push({round:index+1,rawBytes:a.bytes.length,compressedBytes:b.bytes.length,rawMs,compressedMs,embeddedFont:b.embeddedFont,pixelsEqual:true,decodedFontsEqual:true});raw=a.bytes;compressed=b.bytes;
 }
 await writeFile(out+'/compression-raw.pdf',raw);await writeFile(out+'/compression-output.pdf',compressed);
 // Existing uncompressed documents retain every historical byte. Verify the
 // first compressed edit as well as a subsequent edit that reuses its font.
 const legacyCandidate=await candidate(raw,'中文 α ≥ 4'),legacyRaw=appendAnnotationRevision(uncompressed,legacyCandidate,font,raw,layout),legacyCompressed=appendAnnotationRevision(mupdf,legacyCandidate,font,raw,layout);
 assert.deepEqual(legacyCompressed.bytes.subarray(0,legacyCandidate.length),legacyCandidate);
 const legacyA=snapshot(legacyRaw.bytes),legacyB=snapshot(legacyCompressed.bytes);
 assert.equal(legacyA.pixelHash,legacyB.pixelHash);assert.deepEqual(legacyA.annotations,legacyB.annotations);assert.deepEqual(legacyA.fonts.map(f=>f.decoded),legacyB.fonts.map(f=>f.decoded));
 assert.deepEqual(await verifyRead(legacyRaw.bytes),await verifyRead(legacyCompressed.bytes));
 const reuseCandidate=await candidate(legacyCompressed.bytes,'4 ≥ α 文中'),reuse=appendAnnotationRevision(mupdf,reuseCandidate,font,legacyCompressed.bytes,layout);
 assert.equal(reuse.embeddedFont,false);assert.ok(reuse.bytes.length-legacyCompressed.bytes.length<10000);
 await writeFile(out+'/compression.json',JSON.stringify({scope:'Function-level A/B only, fixed short lines; actual UI/full save performance remains for WORKER',rows,legacy:{candidateBytes:legacyCandidate.length,rawBytes:legacyRaw.bytes.length,compressedBytes:legacyCompressed.bytes.length,reuseAppendBytes:reuse.bytes.length-legacyCompressed.bytes.length}},null,2)+'\n');
});

test('compression retains mixed annotations and every existing rotation without changing pixels',async()=>{
 const font=new Uint8Array(await readFile('worker/fonts/NotoSansSC-Regular-v41.ttf'));
 const source=new Uint8Array(await readFile('tests/fixtures/g1/freetext-control.pdf'));
 const d=mupdf.PDFDocument.openDocument(source,'application/pdf');assert.ok(d instanceof mupdf.PDFDocument);const page=d.loadPage(0);
 // Square boxes fit the fixed single-line probe in every orientation.
 for(const a of page.getAnnotations().filter(a=>a.getType()==='FreeText')){a.setRect([72,80,272,280]);a.setContents('中文 α ≥ 0');a.setDefaultAppearance('Helv',12,[0,0,0]);}
 for(const rotation of [90,180,270]){const a=page.createAnnotation('FreeText');a.setRect([300,80+rotation,500,280+rotation]);a.setContents('中文 α ≥ 0');a.setDefaultAppearance('Helv',12,[0,0,0]);a.getObject().put('Rotate',d.newInteger(rotation));}
 page.destroy();const bytes=d.saveToBuffer({incremental:true}).asUint8Array().slice();d.destroy();
 const a=appendAnnotationRevision(uncompressed,bytes,font,source,layout),b=appendAnnotationRevision(mupdf,bytes,font,source,layout),x=snapshot(a.bytes),y=snapshot(b.bytes);
 assert.deepEqual(b.bytes.subarray(0,bytes.length),bytes);assert.equal(x.pixelHash,y.pixelHash);assert.deepEqual(x.annotations,y.annotations);assert.deepEqual(await verifyRead(a.bytes),await verifyRead(b.bytes));
 assert.ok(y.annotations.some(a=>a.type==='Ink'));assert.ok(y.annotations.some(a=>a.type==='Highlight'));
 assert.deepEqual(y.annotations.filter(a=>a.type==='FreeText').map(a=>a.rotation??0).sort((a,b)=>Number(a)-Number(b)),[0,90,180,270]);
});

test('Regular uses real static outlines, retains glyph coverage and refuses legacy Thin subset reuse',async()=>{
 const oldFont=new Uint8Array(await readFile('worker/fonts/NotoSansSC-VF.ttf'));
 const regular=new Uint8Array(await readFile('worker/fonts/NotoSansSC-Regular-v41.ttf'));
 const thin=new mupdf.Font('Thin',oldFont),font=new mupdf.Font('Regular',regular);
 let covered=0;for(let cp=32;cp<=0xffff;cp++)if(thin.encodeCharacter(cp)){assert.ok(font.encodeCharacter(cp),`lost U+${cp.toString(16)}`);covered++;}
 function ink(f:mupdf.Font){const pix=new mupdf.Pixmap(mupdf.ColorSpace.DeviceRGB,[0,0,1800,160],false);pix.clear(255);const d=new mupdf.DrawDevice([1,0,0,1,0,0],pix),text=new mupdf.Text();text.showString(f,[64,0,0,64,10,100],'中文字数学 α ≤ β H I M A',0);d.fillText(text,[1,0,0,1,0,0],mupdf.ColorSpace.DeviceRGB,[0,0,0],1);d.close();let sum=0;for(const v of pix.getPixels())sum+=(255-v)/765;text.destroy();d.destroy();pix.destroy();return sum;}
 const thickness=ink(font)/ink(thin);assert.ok(thickness>=2&&thickness<=3,`ink ratio ${thickness}`);
 // Create an actual Thin subset and mark it with the former ToUnicode identity.
 // Its characters match, but its glyph IDs and outlines must not be reused.
 const source=new Uint8Array(await readFile('tests/fixtures/g1/plain.pdf'));
 const bytes=await candidate(source,'中文 α ≥ 0');
 const old=appendAnnotationRevision(mupdf,bytes,oldFont,source,layout).bytes;
 const doc=mupdf.PDFDocument.openDocument(old,'application/pdf');assert.ok(doc instanceof mupdf.PDFDocument);
 for(let n=1;n<doc.countObjects();n++){const cmap=doc.newIndirect(n).get('ToUnicode');if(cmap.isStream()){const content=cmap.readStream().asString();if(content.includes('/AUPWUnicodeRegularV41 '))cmap.writeStream(content.replaceAll('/AUPWUnicodeRegularV41 ','/AUPWUnicode '));}}
 const legacy=doc.saveToBuffer({incremental:true}).asUint8Array().slice();doc.destroy();
 const edited=await candidate(legacy,'中文 α ≥ 1');
 const upgraded=appendAnnotationRevision(mupdf,edited,regular,legacy,layout);assert.equal(upgraded.embeddedFont,true);assert.deepEqual(upgraded.bytes.subarray(0,legacy.length),legacy);
 const again=await candidate(upgraded.bytes,'1 ≥ α 文中');
 const reused=appendAnnotationRevision(mupdf,again,regular,upgraded.bytes,layout);assert.equal(reused.embeddedFont,false);
 const check=mupdf.PDFDocument.openDocument(upgraded.bytes,'application/pdf');assert.ok(check instanceof mupdf.PDFDocument);const ap=check.loadPage(0).getAnnotations().find(a=>a.getType()==='FreeText')!.getObject().get('AP').get('N');
 const embedded=ap.get('Resources').get('Font').get('AUPW');assert.equal(embedded.get('DescendantFonts').get(0).get('FontDescriptor').get('FontWeight').asNumber(),400);
 assert.ok(!/\b(?:Tr|w)\b/.test(ap.readStream().asString()),'appearance must use filled outlines, not stroked text');check.destroy();
 await writeFile(out+'/regular-weight.json',JSON.stringify({status:'PASS',bmpGlyphsRetained:covered,inkRatio:thickness,legacySubsetRejected:true,regularSubsetReused:true},null,2)+'\n');
 thin.destroy();font.destroy();
});
