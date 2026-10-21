// Colour choices beside the two stroke tools: defaults, memory and rejection of
// anything that is not one of the offered swatches.
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import * as mupdf from 'mupdf';
import { appendAnnotationRevision } from '../../src/lib/mupdf-writer.ts';
import { editorFixture } from './editor-fixture.ts';
import { toolPalette, defaultToolColors, parseToolColors, serializeToolColors, validToolColor } from '../../src/lib/annotation-colors.ts';

test('highlight defaults to yellow and ink to black, with five or six choices each', () => {
  assert.deepEqual(defaultToolColors(), { highlight: '#FFFF00', ink: '#000000' });
  for (const kind of ['highlight', 'ink'] as const) {
    const colors = toolPalette[kind];
    assert.ok(colors.length >= 5 && colors.length <= 6, `${kind} needs five or six choices`);
    assert.equal(new Set(colors.map(c => c.color.toLowerCase())).size, colors.length, `${kind} colours must be distinct`);
    assert.equal(colors[0].color.toLowerCase(), defaultToolColors()[kind].toLowerCase(), `${kind} default is first`);
  }
});

test('a remembered colour survives a round trip and anything unknown is ignored', () => {
  const chosen = { highlight: '#FFA6D5', ink: '#2C6FD0' };
  assert.deepEqual(parseToolColors(serializeToolColors(chosen)), chosen);
  // Absent, malformed, unknown and injected values all fall back per tool.
  assert.deepEqual(parseToolColors(null), defaultToolColors());
  assert.deepEqual(parseToolColors('not json'), defaultToolColors());
  assert.deepEqual(parseToolColors('{"highlight":"#123456"}'), defaultToolColors(), 'an unlisted colour is refused');
  assert.deepEqual(parseToolColors('{"ink":"#D0342C","extra":{"a":1}}'), { highlight: '#FFFF00', ink: '#D0342C' });
  assert.ok(validToolColor('ink', '#d0342c'), 'colour matching is case-insensitive');
  assert.ok(!validToolColor('ink', 'red'));
  assert.ok(!validToolColor('ink', '#FFFF00'), 'a highlight colour is not an ink colour');
});

test('the writer keeps the colour of existing Highlight and Ink annotations', async () => {
  const font = new Uint8Array(await readFile('worker/fonts/NotoSansSC-Regular-v41.ttf'));
  const doc = mupdf.PDFDocument.openDocument(editorFixture(), 'application/pdf');
  assert.ok(doc instanceof mupdf.PDFDocument);
  const page = doc.loadPage(0);
  const highlight = page.createAnnotation('Highlight');
  // ul, ur, ll, lr as flat coordinate pairs.
  highlight.addQuadPoint([80, 620, 240, 620, 80, 600, 240, 600]);
  highlight.setColor([1, 0.65, 0.84]);
  const ink = page.createAnnotation('Ink');
  // MuPDF exposes an Ink path as an array member, not vertices.
  ink.setInkList([[[80, 560], [200, 560]]]);
  ink.setColor([0.816, 0.204, 0.173]);
  page.update();
  const candidate = new Uint8Array(doc.saveToBuffer({}).asUint8Array());
  doc.destroy();

  const result = appendAnnotationRevision(mupdf, candidate, font, candidate);
  const out = mupdf.PDFDocument.openDocument(result.bytes, 'application/pdf');
  assert.ok(out instanceof mupdf.PDFDocument);
  try {
    const written = out.loadPage(0).getAnnotations();
    const byType = (type: string) => written.find(a => a.getType() === type)!;
    const color = (annot: mupdf.PDFAnnotation) => annot.getObject().get('C').asJS().map((n: number) => Math.round(n * 1000) / 1000);
    assert.deepEqual(color(byType('Highlight')), [1, 0.65, 0.84], 'highlight colour must survive the save');
    assert.deepEqual(color(byType('Ink')), [0.816, 0.204, 0.173], 'ink colour must survive the save');
    // The save is still append-only: every candidate byte is preserved.
    assert.deepEqual(result.bytes.subarray(0, candidate.length), candidate);
  } finally { out.destroy(); }
});
