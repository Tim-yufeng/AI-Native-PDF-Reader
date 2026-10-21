import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile, mkdir, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import path from 'node:path';
import * as mupdf from 'mupdf';
import * as pdfjs from 'pdfjs-dist/legacy/build/pdf.mjs';
import { appendAnnotationRevision } from '../../src/lib/mupdf-writer.ts';
import { legacyFreeTextFixture } from './legacy-freetext-fixture.ts';

const font = new Uint8Array(await readFile('worker/fonts/NotoSansSC-Regular-v41.ttf'));
const options = { isEvalSupported: false, enableScripting: false, useSystemFonts: false, useWasm: false,
  standardFontDataUrl: path.resolve('node_modules/pdfjs-dist/standard_fonts').replaceAll('\\', '/') + '/' };
const hash = (bytes: Uint8Array | Uint8ClampedArray) => createHash('sha256').update(bytes).digest('hex');
const layout = (text: string, _width: number, size: number) => text.split('\n').map((text, i) => ({ text, x: 0, baseline: size * (1.075 + i * 1.35) }));
const write = (original: Uint8Array, candidate: Uint8Array) =>
  appendAnnotationRevision(mupdf, candidate, font, original, layout);
async function serialize(bytes: Uint8Array, entries: (annotations: any[]) => any[]) {
  const load = pdfjs.getDocument({ ...options, data: bytes.slice() });
  try {
    const doc = await load.promise, annotations = await (await doc.getPage(1)).getAnnotations();
    entries(annotations).forEach((entry, i) => doc.annotationStorage.setValue(`pdfjs_internal_editor_delta_${i}`, { date: '2026-10-20T00:00:00Z', ...entry }));
    return new Uint8Array(await doc.saveDocument());
  } finally { await load.destroy(); }
}
const highlight = { annotationType: 9, pageIndex: 0, rect: [80, 570, 190, 590], rotation: 0, color: [255, 255, 0], opacity: .4, thickness: 12, quadPoints: [80, 590, 190, 590, 80, 570, 190, 570], outlines: [[80, 590, 190, 590, 190, 570, 80, 570]] };
const ink = { annotationType: 15, pageIndex: 0, rect: [75, 470, 175, 530], rotation: 0, color: [25, 51, 77], thickness: 2, opacity: 1, paths: { lines: [[null, null, null, null, 80, 505, 110, 525, 140, 475, 170, 495]], points: [[80, 505, 170, 495]] } };
const textEntry = { annotationType: 3, pageIndex: 0, rect: [80, 320, 300, 380], fontSize: 14, color: [25, 51, 77], rotation: 0, value: '中文 α + β ≤ 2\n数学 √2 ≥ 1' };
function snapshot(bytes: Uint8Array) {
  const doc = mupdf.PDFDocument.openDocument(bytes, 'application/pdf') as mupdf.PDFDocument;
  try {
    assert.equal(doc.wasRepaired(), false, 'complete baseline/output must open without repair');
    return Array.from({ length: doc.countPages() }, (_, page) => doc.loadPage(page).getAnnotations()
      .filter(a => a.getType() === 'FreeText').map(a => {
        const obj = a.getObject(), ap = obj.get('AP').get('N'), pix = a.toPixmap([2, 0, 0, 2, 0, 0], mupdf.ColorSpace.DeviceRGB, false);
        try { return { page, id: obj.toString(), dictionary: obj.resolve().toString(), ap: hash(ap.readStream().asUint8Array()), pixels: hash(pix.getPixels()) }; }
        finally { pix.destroy(); }
      })).flat();
  } finally { doc.destroy(); }
}
function retained(old: ReturnType<typeof snapshot>, saved: Uint8Array, except?: string) {
  const after = snapshot(saved);
  for (const entry of old.filter(e => e.id !== except)) assert.deepEqual(after.find(e => e.page === entry.page && e.id === entry.id), entry);
}

for (const objectStreams of [false, true]) test(`untouched external FreeText survives highlight, Ink and unchanged save; object streams=${objectStreams}`, async () => {
  const original = legacyFreeTextFixture(mupdf, objectStreams), before = snapshot(original);
  for (const [label, entries] of [['highlight', [highlight]], ['ink', [ink]], ['unchanged', []]] as const) {
    const candidate = await serialize(original, () => [...entries]), result = write(original, candidate);
    assert.equal(result.targeted, 0); assert.equal(result.embeddedFont, false);
    assert.deepEqual(candidate.subarray(0, original.length), original);
    assert.deepEqual(result.bytes.subarray(0, candidate.length), candidate);
    retained(before, result.bytes);
    // Hand actual PDF.js candidates to the existing native admission probe.
    const out = 'tests/work/writer-changes'; await mkdir(out, { recursive: true });
    const stem = `${objectStreams ? 'stream' : 'classic'}-${label}`;
    await writeFile(`${out}/${stem}-old.pdf`, original);
    await writeFile(`${out}/${stem}-candidate.pdf`, candidate);
    await writeFile(`${out}/${stem}-output.pdf`, result.bytes);
  }
});

test('only changed object is rewritten despite duplicate names/content; subsequent Chinese/math edit saves', async () => {
  const original = legacyFreeTextFixture(mupdf), before = snapshot(original);
  const candidate = await serialize(original, a => [{ ...textEntry, id: a[0].id }]), result = write(original, candidate);
  assert.equal(result.targeted, 1); retained(before, result.bytes, before[0].id);
  const reopened = await serialize(result.bytes, a => [{ ...textEntry, id: a[0].id, value: '再次编辑 α ≤ β\n中文末行' }]);
  const revised = write(result.bytes, reopened); assert.equal(revised.targeted, 1);
  retained(before, revised.bytes, before[0].id);
  const deleted = await serialize(revised.bytes, a => [{ id: a[0].id, deleted: true, pageIndex: 0, popupRef: '' }, ink]);
  const removed = write(revised.bytes, deleted); assert.equal(removed.targeted, 0);
  assert.equal(snapshot(removed.bytes).length, 7); retained(before, removed.bytes, before[0].id);
});

test('new or edited overflow refuses output; removing the new box lets other annotations save', async () => {
  const original = legacyFreeTextFixture(mupdf), before = snapshot(original);
  const overflow = { ...textEntry, rect: [80, 320, 100, 330], value: '真正溢出的中文文字框' };
  for (const existing of [false, true]) {
    const candidate = await serialize(original, a => [{ ...overflow, ...(existing ? { id: a[0].id } : {}) }, highlight]);
    assert.throws(() => write(original, candidate), { code: 'writer_text_overflow' });
  }
  // Serialize again from the unchanged original after removing the unsaved
  // editor. Failed writer output must never become the next baseline.
  const afterDeletion = await serialize(original, () => [highlight]), result = write(original, afterDeletion);
  assert.equal(result.targeted, 0); retained(before, result.bytes);
});

test('baseline must be the complete matching original, not a /Prev xref offset', async () => {
  const original = legacyFreeTextFixture(mupdf), candidate = await serialize(original, () => [ink]);
  const wrong = original.slice(); wrong[7] ^= 1;
  assert.throws(() => write(wrong, candidate), { code: 'writer_baseline_invalid' });
  assert.throws(() => write(new Uint8Array(), candidate), { code: 'writer_baseline_invalid' });
  const doc = mupdf.PDFDocument.openDocument(candidate, 'application/pdf') as mupdf.PDFDocument;
  const previousXref = doc.getTrailer().get('Prev').asNumber(); doc.destroy();
  assert.ok(previousXref > 0 && previousXref < original.length, '/Prev is an xref offset inside the original');
  assert.throws(() => write(original.subarray(0, previousXref), candidate), { code: 'writer_baseline_invalid' });
  assert.equal(write(original, candidate).targeted, 0);
});
