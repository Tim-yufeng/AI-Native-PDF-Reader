import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { getDocument, version } from 'pdfjs-dist/legacy/build/pdf.mjs';
import { securityFixture } from './security-fixture.ts';

const fixtureRoot = new URL('../fixtures/g1/', import.meta.url);
const sha = (bytes: Uint8Array) => createHash('sha256').update(bytes).digest('hex');
const standardFontDataUrl = fileURLToPath(new URL('../../node_modules/pdfjs-dist/standard_fonts/', import.meta.url)) + '/';

test('the locked reader parses all existing controls without modifying their bytes', async () => {
  assert.equal(version, '6.4.299');
  const manifest = JSON.parse(await readFile(new URL('manifest.json', fixtureRoot), 'utf8'));
  for (const fixture of manifest.files) {
    const filename = new URL(fixture.name, fixtureRoot);
    const before = await readFile(filename); assert.equal(sha(before), fixture.sha256);
    const task = getDocument({ data: new Uint8Array(before),
      standardFontDataUrl });
    try {
      const document = await task.promise; assert.equal(document.numPages, fixture.pageCount);
      const text: string[] = [];
      for (let number = 1; number <= document.numPages; number++) {
        const page = await document.getPage(number);
        assert.equal(page.rotate, fixture.pageGeometry[number - 1].rotate);
        assert.deepEqual(page.view, fixture.pageGeometry[number - 1].cropBox);
        const content = await page.getTextContent();
        text.push(...content.items.filter(item => 'str' in item).map(item => 'str' in item ? item.str : ''));
        const operators = await page.getOperatorList(); assert.ok(operators.fnArray.length > 0);
        const size = page.getViewport({ scale: 1 });
        const doubled = page.getViewport({ scale: 2 });
        assert.equal(doubled.width, size.width * 2); assert.equal(doubled.height, size.height * 2);
        const annotations = await page.getAnnotations();
        text.push(...annotations.map(annotation => annotation.contentsObj?.str || ''));
        if (fixture.name === 'freetext-control.pdf') {
          assert.deepEqual(annotations.map(annotation => annotation.subtype).sort(), ['FreeText', 'Highlight', 'Ink']);
        }
      }
      for (const marker of fixture.markers) assert.ok(text.join(' ').includes(marker), `Missing ${marker}`);
    } finally { await task.destroy(); }
    assert.equal(sha(await readFile(filename)), fixture.sha256);
  }
});

test('the actual reader rejects invalid PDF bytes; a subsequent valid read still works', async () => {
  const broken = getDocument({ data: new TextEncoder().encode('not a PDF') });
  await assert.rejects(broken.promise, { name: 'InvalidPDFException' }); await broken.destroy();
  const valid = getDocument({ data: new Uint8Array(await readFile(new URL('plain.pdf', fixtureRoot))) });
  try { assert.equal((await valid.promise).numPages, 1); } finally { await valid.destroy(); }
});

test('the synthetic safety control contains a real external link and PDF JavaScript action', async () => {
  const task = getDocument({ data: securityFixture(), standardFontDataUrl });
  try {
    const document = await task.promise;
    assert.equal(document.numPages, 1);
    const actions = await document.getJSActions();
    assert.ok(actions instanceof Map);
    assert.match(JSON.stringify(actions.get('OpenAction')), /M2_SCRIPT_MUST_NOT_RUN/);
    const annotations = await (await document.getPage(1)).getAnnotations();
    assert.equal(annotations[0].subtype, 'Link');
    assert.equal(annotations[0].url, 'https://example.invalid/m2-a1-link');
  } finally { await task.destroy(); }
});

test('an invalid first-page object can fail after document parsing succeeds', async () => {
  const task = getDocument({ data: securityFixture(true), standardFontDataUrl });
  try {
    const document = await task.promise;
    assert.equal(document.numPages, 1);
    await assert.rejects(document.getPage(1));
  } finally { await task.destroy(); }
});
