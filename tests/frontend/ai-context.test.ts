import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createCanvas, DOMMatrix, ImageData, Path2D, loadImage } from '@napi-rs/canvas';
Object.assign(globalThis, { DOMMatrix, ImageData, Path2D, document: { createElement: () => createCanvas(1, 1) } });
const { getDocument } = await import('pdfjs-dist/legacy/build/pdf.mjs');
const { extractPages, pageImage, normalizeRect } = await import('../../src/lib/ai-context.ts');
import type { PageReader } from '../../src/lib/ai-context.ts';
import path from 'node:path';
import { readingFixture } from './reading-fixture.ts';
import {indexInlineContext} from '../../src/lib/inline-context.ts';
import {defaultContext} from '../../src/lib/preferences.ts';

test('inline default indexes every page including distant text; local-only setting and cancellation stay bounded',async()=>{
 const read:number[]=[],published:{page:number;text:string}[]=[];
 const reader={getPage:async(n:number)=>{read.push(n);return {getTextContent:async()=>({items:[{str:n===27?'distant definition':'page '+n,hasEOL:true}]})};}} as unknown as PageReader;
 await indexInlineContext(reader,30,2,undefined,defaultContext(),new AbortController().signal,async pages=>{published.push(...pages);},()=>{});
 assert.deepEqual(read,Array.from({length:30},(_,i)=>i+1));assert.equal(published.length,30);assert.ok(published[26].text.includes('distant definition'));
 read.length=0;await indexInlineContext(reader,30,10,undefined,{...defaultContext(),inlineFullDocument:false,beforePages:1,afterPages:1},new AbortController().signal,async()=>{},()=>{});assert.deepEqual(read,[10,9,11]);
 const cancelled=new AbortController();cancelled.abort();read.length=0;await assert.rejects(()=>indexInlineContext(reader,30,1,undefined,defaultContext(),cancelled.signal,async()=>{},()=>{}));assert.deepEqual(read,[]);
});

test('real control PDF renders rotated region with PDF coordinates and extracts pages in order, without writing files', async () => {
  const task = getDocument({ data: new Uint8Array(await readFile('tests/fixtures/g1/geometry.pdf')), useSystemFonts: false, standardFontDataUrl: path.resolve("node_modules/pdfjs-dist/standard_fonts") + "/" });
  const pdf = await task.promise, reader = { getPage: (page: number) => pdf.getPage(page) } as PageReader;
  try {
    const batches: { page: number; text: string }[][] = [];
    await extractPages(reader, pdf.numPages, new AbortController().signal, async pages => { batches.push(pages); }, () => {});
    assert.deepEqual(batches.flat().map(p => p.page), [1, 2]);
    for (const n of [1, 2]) {
      const page = await pdf.getPage(n), v = page.getViewport({ scale: 1 });
      const rect = normalizeRect(v.convertToPdfPoint(v.width * .2, v.height * .2), v.convertToPdfPoint(v.width * .6, v.height * .6));
      const result = await pageImage(reader, { id: 'S1', grantId: 'test', name: 'geometry.pdf', page: n, text: '', image: true, rect }, new AbortController().signal);
      assert.ok(result.startsWith('data:image/png;base64,'));
      const image = await loadImage(result);
      assert.ok(Math.abs(image.width - v.width * .4 * 1.5) <= 2); assert.ok(Math.abs(image.height - v.height * .4 * 1.5) <= 2);
      assert.ok(image.width * image.height < 2560000);
      // Compare actual crop pixels with the same rectangle in an independent
      // full-page render, including the rotated fixture's second page.
      const scaled = page.getViewport({ scale: 1.5 }), full = createCanvas(Math.ceil(scaled.width), Math.ceil(scaled.height));
      await page.render({ canvas: full as unknown as HTMLCanvasElement, viewport: scaled }).promise;
      const p1 = scaled.convertToViewportPoint(rect[0], rect[1]), p2 = scaled.convertToViewportPoint(rect[2], rect[3]);
      const left = Math.floor(Math.min(p1[0], p2[0])), top = Math.floor(Math.min(p1[1], p2[1]));
      const crop = createCanvas(image.width, image.height); crop.getContext('2d').drawImage(image, 0, 0);
      const actual = crop.getContext('2d').getImageData(2, 2, image.width - 4, image.height - 4).data;
      const expected = full.getContext('2d').getImageData(left + 2, top + 2, image.width - 4, image.height - 4).data;
      let totalDifference = 0, maximumDifference = 0, shifted = 0;
      for (let i = 0; i < actual.length; i++) { const difference = Math.abs(actual[i] - expected[i]); totalDifference += difference; maximumDifference = Math.max(maximumDifference, difference); if ((actual[i] < 240) !== (expected[i] < 240)) shifted++; }
      // Canvas clipping can change antialias gray levels; geometry and meaningful
      // dark/light boundaries must agree rather than demanding identical PNGs.
      assert.ok(totalDifference / actual.length < 0.5, `page ${n}: mean difference ${totalDifference / actual.length}, max ${maximumDifference}`);
      assert.ok(shifted / actual.length < .001, `page ${n}: shifted dark/light boundaries ${shifted / actual.length}`);
    }
    const abort = new AbortController(); abort.abort();
    await assert.rejects(() => pageImage(reader, { id: 'S1', grantId: 'test', name: 'geometry.pdf', page: 1, text: '', image: true }, abort.signal));
  } finally { await task.destroy(); }
});

test('extraction refuses over-budget text instead of silently truncating pages', async () => {
  const reader = { getPage: async () => ({ getTextContent: async () => ({ items: [{ str: '中'.repeat(12000), hasEOL: false }] }) }) } as unknown as PageReader;
  let published = false;
  await assert.rejects(() => extractPages(reader, 1, new AbortController().signal, async () => { published = true; }, () => {}), /预算/);
  assert.equal(published, false);
});

test('thirty-page PDF supplies distant basis material and non-contiguous image pages', async () => {
  const task = getDocument({ data: readingFixture(30, false, { 22: 'Linear space basis definition: v = a e1 + b e2', 27: 'Apply the basis transformation matrix A to vector v' }), standardFontDataUrl: path.resolve('node_modules/pdfjs-dist/standard_fonts') + '/' });
  const pdf = await task.promise, reader = { getPage: (n: number) => pdf.getPage(n) };
  try {
    const texts: { page: number; text: string }[] = [];
    await extractPages(reader, 30, new AbortController().signal, async pages => { texts.push(...pages); }, () => {});
    assert.ok(texts.find(p => p.page === 22)?.text.includes('basis definition'));
    assert.ok(texts.find(p => p.page === 27)?.text.includes('basis transformation'));
    for (const n of [21, 24, 27]) assert.ok((await pageImage(reader, { id: `S${n}`, grantId: 'test', name: 'cross-page.pdf', page: n, text: '', image: true }, new AbortController().signal)).startsWith('data:image/png;base64,'));
  } finally { await task.destroy(); }
});
