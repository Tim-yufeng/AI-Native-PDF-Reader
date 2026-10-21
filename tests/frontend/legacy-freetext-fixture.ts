import { editorFixture } from './editor-fixture.ts';

/** Disposable external-style annotations. Their own compact appearance fits,
 * but the Contents/DA/Rect intentionally overflow the current Reader layout.
 * Duplicate /NM and identical text prevent matching by name/content/order. */
export function legacyFreeTextFixture(mupdf: any, objectStreams = false): Uint8Array {
  const doc = mupdf.PDFDocument.openDocument(editorFixture(), 'application/pdf');
  try {
    for (let p = 0; p < 4; p++) {
      const page = doc.loadPage(p);
      for (let n = 0; n < 2; n++) {
        const annot = page.createAnnotation('FreeText'), object = annot.getObject();
        annot.setRect([70 + n * 130, 130, 170 + n * 130, 160]);
        annot.setContents('Historical external text\nwith its own compact appearance\nand a deliberately different layout.');
        annot.setDefaultAppearance('Helv', 18, [0, 0, 0]);
        object.put('NM', doc.newString('duplicate-name'));
        const font = doc.addSimpleFont(new mupdf.Font('Helvetica'));
        const ap = doc.addStream('q 0.9 0.95 1 rg 0 0 100 30 re f Q\nBT /Legacy 5 Tf 0 0 0 rg 1 0 0 1 2 23 Tm (Historical external text) Tj 0 -7 Td (with its own compact appearance) Tj 0 -7 Td (and a deliberately different layout.) Tj ET');
        ap.put('Type', doc.newName('XObject')); ap.put('Subtype', doc.newName('Form'));
        ap.put('BBox', [0, 0, 100, 30]); ap.put('Resources', { Font: { Legacy: font } });
        object.put('AP', { N: ap });
      }
    }
    return doc.saveToBuffer(objectStreams ? { compress: true, objstms: true } : {}).asUint8Array().slice();
  } finally { doc.destroy(); }
}
