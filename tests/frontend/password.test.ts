import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { getDocument, PasswordResponses } from 'pdfjs-dist/legacy/build/pdf.mjs';
test('actual AES PDF retries wrong password and opens with temporary correct password', async () => {
  const bytes = new Uint8Array(await readFile(new URL('../work/m3/fixtures/password.pdf', import.meta.url)));
  const task = getDocument({data:bytes}); const reasons:number[]=[];
  task.onPassword = (update:(value:string)=>void,reason:number) => { reasons.push(reason); update(reasons.length===1?'wrong':'m3-test'); };
  try { const pdf=await task.promise; assert.equal(pdf.numPages,3); assert.deepEqual(reasons,[PasswordResponses.NEED_PASSWORD,PasswordResponses.INCORRECT_PASSWORD]); }
  finally { await task.destroy(); }
});
test('cancelling an actual password request releases its parser', async () => {
  const task=getDocument({data:new Uint8Array(await readFile(new URL('../work/m3/fixtures/password.pdf',import.meta.url)))});
  let called!:()=>void; const request=new Promise<void>(resolve=>called=resolve); task.onPassword=()=>called();
  const failed=assert.rejects(task.promise); await request; await task.destroy(); await failed;
});
