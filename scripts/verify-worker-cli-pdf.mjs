// Guest-only independent PDF.js check. No writer, network, or screenshot files.
import {readFile,writeFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
import * as pdfjs from './pdf.mjs';
const lab='C:/Users/WriterCliUser/WriterCliLab',input=lab+'/input';
if(import.meta.dirname.replaceAll('\\','/').toLowerCase()!==input.toLowerCase())throw Error('Guest-only validator');
const hash=b=>createHash('sha256').update(b).digest('hex');
const source=await readFile(input+'/matrix-source.pdf'),candidate=await readFile(input+'/matrix-candidate.pdf'),output=await readFile(lab+'/unicode-output.pdf');
assert.equal(hash(source),'a8a9d4ac31dbcd6d11238dc5d07918da48f826d545f24a3c4e0fdca370d334a9');
assert.equal(hash(candidate),'b5da68da0a0ca6d988ffcc49912bb558ce3596f90c573a7ab82f379cb0877edc');
assert.ok(output.length<=16*1024*1024&&output.subarray(0,candidate.length).equals(candidate));
const options={isEvalSupported:false,enableScripting:false,useSystemFonts:false,useWasm:false,isOffscreenCanvasSupported:false};
const before=pdfjs.getDocument({...options,data:new Uint8Array(source)}),after=pdfjs.getDocument({...options,data:new Uint8Array(output)});
let annotations=0,appearances=0;
try{
 const a=await before.promise,b=await after.promise;assert.equal(a.numPages,4);assert.equal(b.numPages,4);
 for(let i=0;i<4;i++){
  const old=await a.getPage(i+1),page=await b.getPage(i+1);assert.deepEqual(page.view,old.view);assert.equal(page.rotate,old.rotate);
  const original=await old.getTextContent(),text=await page.getTextContent();assert.deepEqual(text.items.map(x=>x.str),original.items.map(x=>x.str));
  await page.getOperatorList();const ann=await page.getAnnotations();assert.equal(ann.length,2);
  for(let box=0;box<2;box++){
   const item=ann[box],value='中文 α + β ≤ 10\n数学 ∫ x² dx ≠ ∞';
   assert.equal(item.subtype,'FreeText');assert.equal(item.hasAppearance,true);assert.equal(item.isEditable,true);
   assert.equal(item.contentsObj.str,value);assert.equal(item.textContent.join('\n'),value);assert.equal(item.defaultAppearanceData.fontSize,12);
   assert.deepEqual(item.rect,[70+box*190,100+box*210,250+box*190,260+box*210]);
   const color=i%2?(box?[240,240,240]:[255,190,180]):(box?[24,100,170]:[200,45,60]);assert.deepEqual(Array.from(item.defaultAppearanceData.fontColor),color);
   annotations++;appearances++;
  }
 }
}finally{await before.destroy();await after.destroy();}
await writeFile(lab+'/validation.json',JSON.stringify({status:'PASS',pages:4,annotations,appearances,bodyPreserved:true,outputHash:hash(output),scope:'independent PDF.js parse/operator/text/annotation validation; no pixel render or GUI claim'}),{flag:'wx'});
