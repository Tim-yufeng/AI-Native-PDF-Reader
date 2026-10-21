// MuPDF.js byte-generation layer for the PDF writer.
//
// Contract:
//   input  : complete original snapshot and candidate bytes = the renderer's serialization (authorized
//            original bytes + exactly one renderer revision)
//   output : candidate bytes + exactly one appended revision in which changed
//            FreeText annotations carry /AP with vector text using one
//            embedded, subsetted Unicode font. /Contents, /Rect, /Rotate and
//            all original bytes stay untouched.
//
// The appearance is written here on purpose: MuPDF's own FreeText generator
// ignores the annotation /DR font and embeds multi-megabyte built-in fallbacks,
// which would bloat every saved copy. MuPDF is used only as the font subsetter
// (its subsetter walks page content) and as the PDF object writer.
//
// Annotation coverage: the reader offers FreeText, Highlight and Ink, and
// PDF.js serializes all three into the candidate bytes with self-contained
// vector appearances (Highlight/Ink need no font at all). Only FreeText has to
// be rewritten with the pinned Unicode font when newly created or edited.
// Untouched FreeText retains its original appearance, including external fonts
// and layouts. Other annotations are re-asserted unchanged.

import { WriterRefusal, writerStep } from './writer-refusal.ts';
import { measureFreeText, type FreeTextLayouter } from './freetext-layout.ts';

// Glyph IDs/outlines differ from the former Thin variable font. Never reuse
// its subsets, including when it already covers all characters being saved.
const MARKER = 'AUPWUnicodeRegularV41';
const RESOURCE = 'AUPW';
const PDF_HEAD_FONT = 'NotoSansSC-Regular';
// eslint-disable-next-line @typescript-eslint/no-explicit-any
type Mupdf = any;

export interface WriterResult {
  bytes: Uint8Array;
  /** FreeText appearances rewritten with the pinned Unicode font. */
  targeted: number;
  /** Annotations re-asserted unchanged (Highlight, Ink, original annotations). */
  asserted: number;
  embeddedFont: boolean;
  fontBytes: number;
}

let loading: Promise<Mupdf> | null = null;

/** Load MuPDF.js once, pointing its wasm fetch at the packaged asset. */
export function loadMupdf(wasmBase = '/mupdf/'): Promise<Mupdf> {
  if (!loading) {
    (globalThis as unknown as Record<string, unknown>)['$libmupdf_wasm_Module'] = {
      locateFile: (name: string) => `${wasmBase}${name}`,
    };
    loading = import('mupdf').catch((error) => { loading = null; throw error; });
  }
  return loading;
}

const pad4 = (value: number) => value.toString(16).padStart(4, '0');

/** MuPDF's newArray() takes no arguments; every element must be pushed. */
function arrayOf(doc: Mupdf, items: unknown[]): Mupdf {
  const array = doc.newArray();
  for (const item of items) array.push(item);
  return array;
}

function realArray(doc: Mupdf, values: number[]): Mupdf {
  const array = doc.newArray();
  for (const value of values) array.push(Math.abs(value % 1) < 1e-9 ? doc.newInteger(value) : doc.newReal(value));
  return array;
}

/** Minimal one-page PDF used only as a scratch document for font subsetting. */
function scratchPdf(): Uint8Array {
  const objects = [
    '<</Type/Catalog/Pages 2 0 R>>',
    '<</Type/Pages/Kids[3 0 R]/Count 1>>',
    '<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]/Resources<</Font<<>>>>/Contents 4 0 R>>',
    '<</Length 4>>\nstream\nq Q\nendstream',
  ];
  let body = '%PDF-1.7\n';
  const offsets: number[] = [];
  objects.forEach((object, index) => {
    offsets.push(body.length);
    body += `${index + 1} 0 obj\n${object}\nendobj\n`;
  });
  const xref = body.length;
  body += `xref\n0 ${objects.length + 1}\n0000000000 65535 f \n`;
  for (const offset of offsets) body += `${String(offset).padStart(10, '0')} 00000 n \n`;
  body += `trailer\n<</Size ${objects.length + 1}/Root 1 0 R>>\nstartxref\n${xref}\n%%EOF\n`;
  const bytes = new Uint8Array(body.length);
  for (let i = 0; i < body.length; i++) bytes[i] = body.charCodeAt(i);
  return bytes;
}

function latin1(bytes: Uint8Array): string {
  let text = '';
  for (let i = 0; i < bytes.length; i += 8192) text += String.fromCharCode(...bytes.subarray(i, i + 8192));
  return text;
}

function asBytes(value: Uint8Array | ArrayBuffer): Uint8Array {
  return value instanceof Uint8Array ? value : new Uint8Array(value);
}

/**
 * MuPDF returns views into the wasm heap; the heap can be detached or grown
 * later, so every value that outlives the call is copied out immediately.
 */
function wasmBytes(view: { asUint8Array(): Uint8Array }): Uint8Array {
  return Uint8Array.from(view.asUint8Array());
}

function readEmbeddedFontFile(doc: Mupdf, fontObj: Mupdf): Uint8Array | null {
  const descendant = fontObj.get('DescendantFonts')?.get?.(0)?.resolve?.() ?? fontObj;
  const descriptor = descendant.get('FontDescriptor')?.resolve?.();
  for (const key of ['FontFile2', 'FontFile3', 'FontFile']) {
    const ref = descriptor?.get?.(key);
    if (!ref) continue;
    try {
      const target = ref.isStream?.() ? ref : ref.resolve();
      return wasmBytes(target.readStream());
    } catch { /* try the next key */ }
  }
  return null;
}

/** Reliable extraction: save the scratch document and read the subset back. */
function extractSubsetViaSave(mupdf: Mupdf, doc: Mupdf): Uint8Array | null {
  const saved = wasmBytes(doc.saveToBuffer({ garbage: true }));
  const reopened = mupdf.PDFDocument.openDocument(saved, 'application/pdf');
  for (let i = 1; i < reopened.countObjects(); i++) {
    let obj: Mupdf;
    try { obj = reopened.newIndirect(i)?.resolve?.(); } catch { continue; }
    if (!obj?.get) continue;
    let subtype: string | undefined;
    try { subtype = obj.get('Subtype')?.asName?.(); } catch { continue; }
    if (subtype !== 'CIDFontType2' && subtype !== 'TrueType') continue;
    const descriptor = obj.get('FontDescriptor')?.resolve?.();
    for (const key of ['FontFile2', 'FontFile3']) {
      const ref = descriptor?.get?.(key);
      if (!ref) continue;
      try {
        const target = ref.isStream?.() ? ref : ref.resolve();
        const bytes = wasmBytes(target.readStream());
        if (bytes.length > 1024) return bytes;
      } catch { /* try next */ }
    }
  }
  return null;
}

/** Subset the pinned font down to the given glyph ids (MuPDF walks page content). */
export function subsetFontBytes(mupdf: Mupdf, fullFontBytes: Uint8Array, glyphIds: number[]): Uint8Array {
  const scratch = mupdf.PDFDocument.openDocument(scratchPdf(), 'application/pdf');
  const full = new mupdf.Font(PDF_HEAD_FONT, fullFontBytes);
  const fontObj = scratch.addFont(full);
  const page = scratch.loadPage(0);
  const pageObj = page.getObject();
  pageObj.get('Resources').get('Font').put('SUBSET', fontObj);
  let hex = '';
  for (const gid of glyphIds) hex += pad4(gid);
  const stream = scratch.addStream(`q BT /SUBSET 24 Tf 1 0 0 1 5 5 Tm <${hex}> Tj ET Q`);
  const contents = pageObj.get('Contents');
  const previous: unknown[] = contents.isArray() ? contents.asJS() : [contents];
  pageObj.put('Contents', scratch.newArray());
  const list = pageObj.get('Contents');
  for (const item of previous) list.push(item);
  list.push(stream);
  scratch.subsetFonts();
  let bytes = readEmbeddedFontFile(scratch, fontObj);
  if (!bytes || bytes.length >= fullFontBytes.length) bytes = extractSubsetViaSave(mupdf, scratch);
  if (!bytes || bytes.length < 1024) throw new WriterRefusal('writer_subset_failed', 'appearance');
  if (bytes.length >= fullFontBytes.length) throw new WriterRefusal('writer_subset_failed', 'appearance');
  return bytes;
}

/** Advances come from the subset's own hmtx; subsetting keeps original glyph ids. */
function readAdvances(subsetBytes: Uint8Array) {
  const view = new DataView(subsetBytes.buffer, subsetBytes.byteOffset, subsetBytes.byteLength);
  const numTables = view.getUint16(4);
  const tables: Record<string, { offset: number; length: number }> = {};
  for (let i = 0; i < numTables; i++) {
    const offset = 12 + i * 16;
    const tag = String.fromCharCode(view.getUint8(offset), view.getUint8(offset + 1), view.getUint8(offset + 2), view.getUint8(offset + 3));
    tables[tag] = { offset: view.getUint32(offset + 8), length: view.getUint32(offset + 12) };
  }
  if (!tables.head || !tables.hhea || !tables.hmtx) throw new WriterRefusal('writer_subset_failed', 'appearance');
  const unitsPerEm = view.getUint16(tables.head.offset + 18);
  const metrics = view.getUint16(tables.hhea.offset + 34);
  return {
    unitsPerEm,
    width(gid: number) {
      const index = gid < metrics ? gid : metrics - 1;
      return view.getUint16(tables.hmtx.offset + index * 4);
    },
  };
}

function toUnicodeCMap(pairs: { gid: number; cp: number }[]): string {
  const lines = [
    '/CIDInit /ProcSet findresource begin', '12 dict begin', 'begincmap',
    '/CIDSystemInfo <</Registry (Adobe) /Ordering (UCS) /Supplement 0>> def',
    `/CMapName /${MARKER} def`, '/CMapType 2 def', '1 begincodespacerange', '<0000> <FFFF>', 'endcodespacerange',
  ];
  for (let i = 0; i < pairs.length; i += 100) {
    const chunk = pairs.slice(i, i + 100);
    lines.push(`${chunk.length} beginbfchar`);
    for (const { gid, cp } of chunk) lines.push(`<${pad4(gid)}> <${cp.toString(16).padStart(4, '0')}>`);
    lines.push('endbfchar');
  }
  lines.push('endcmap', 'CMapName currentdict /CMap defineresource pop', 'end', 'end');
  return lines.join('\n');
}

function rotationBox(width: number, height: number, rotation: number) {
  const swapped = rotation === 90 || rotation === 270;
  const box = swapped ? [0, 0, height, width] : [0, 0, width, height];
  const matrices: Record<number, number[]> = {
    0: [1, 0, 0, 1, 0, 0],
    90: [0, 1, -1, 0, width, 0],
    180: [-1, 0, 0, -1, width, height],
    270: [0, -1, 1, 0, 0, height],
  };
  return { box, matrix: matrices[rotation] ?? matrices[0] };
}

function coveredCodePoints(fontObj: Mupdf): Set<number> {
  const covered = new Set<number>();
  try {
    const ref = fontObj.get('ToUnicode');
    if (!ref) return covered;
    const target = ref.isStream?.() ? ref : ref.resolve();
    const text = latin1(wasmBytes(target.readStream()));
    if (!text.includes(`/${MARKER} `)) return covered;
    for (const block of text.split('endbfchar')) {
      for (const m of block.matchAll(/<([0-9a-fA-F]+)>\s*<([0-9a-fA-F]+)>/g)) {
        if (m[1].length <= 4 && m[2].length <= 4) covered.add(parseInt(m[2], 16));
      }
    }
  } catch { /* treat as not reusable */ }
  return covered;
}

function findReusableFont(doc: Mupdf, required: Set<number>): Mupdf | null {
  for (let i = 1; i < doc.countObjects(); i++) {
    let fontObj: Mupdf;
    try { fontObj = doc.newIndirect(i); } catch { continue; }
    let resolved: Mupdf;
    try { resolved = fontObj.resolve(); } catch { continue; }
    if (!resolved?.get) continue;
    try { if (resolved.get('Subtype')?.asName?.() !== 'Type0') continue; } catch { continue; }
    const covered = coveredCodePoints(resolved);
    if (covered.size && [...required].every((cp) => covered.has(cp))) return fontObj;
  }
  return null;
}

function addUnicodeFont(doc: Mupdf, subsetBytes: Uint8Array, pairs: { gid: number; cp: number }[], advances: ReturnType<typeof readAdvances>): Mupdf {
  const fontFile = doc.addStream(subsetBytes);
  fontFile.put('Length1', doc.newInteger(subsetBytes.length));
  const descriptor = doc.newDictionary();
  descriptor.put('Type', doc.newName('FontDescriptor'));
  descriptor.put('FontName', doc.newName(`AAAAAA+${PDF_HEAD_FONT}`));
  descriptor.put('Flags', doc.newInteger(4));
  // Actual head/hhea metrics of the pinned static Regular font (1000 upem).
  descriptor.put('FontBBox', realArray(doc, [-1002, -1048, 2928, 1808]));
  descriptor.put('ItalicAngle', doc.newInteger(0));
  descriptor.put('Ascent', doc.newInteger(1160));
  descriptor.put('Descent', doc.newInteger(-288));
  descriptor.put('CapHeight', doc.newInteger(733));
  descriptor.put('StemV', doc.newInteger(80));
  descriptor.put('FontWeight', doc.newInteger(400));
  descriptor.put('MissingWidth', doc.newInteger(1000));
  descriptor.put('FontFile2', fontFile);
  const descriptorRef = doc.addObject(descriptor);
  const widths: unknown[] = [];
  for (const { gid } of pairs) {
    widths.push(doc.newInteger(gid));
    widths.push(realArray(doc, [Math.round((advances.width(gid) * 1000) / advances.unitsPerEm)]));
  }
  const cid = doc.newDictionary();
  cid.put('Type', doc.newName('Font'));
  cid.put('Subtype', doc.newName('CIDFontType2'));
  cid.put('BaseFont', doc.newName(`AAAAAA+${PDF_HEAD_FONT}`));
  const info = doc.newDictionary();
  info.put('Registry', doc.newString('Adobe'));
  info.put('Ordering', doc.newString('Identity'));
  info.put('Supplement', doc.newInteger(0));
  cid.put('CIDSystemInfo', info);
  cid.put('FontDescriptor', descriptorRef);
  cid.put('DW', doc.newInteger(1000));
  cid.put('W', arrayOf(doc, widths));
  cid.put('CIDToGIDMap', doc.newName('Identity'));
  const cidRef = doc.addObject(cid);
  const type0 = doc.newDictionary();
  type0.put('Type', doc.newName('Font'));
  type0.put('Subtype', doc.newName('Type0'));
  type0.put('BaseFont', doc.newName(`AAAAAA+${PDF_HEAD_FONT}`));
  type0.put('Encoding', doc.newName('Identity-H'));
  type0.put('DescendantFonts', arrayOf(doc, [cidRef]));
  type0.put('ToUnicode', doc.addStream(toUnicodeCMap(pairs)));
  return doc.addObject(type0);
}

interface FreeTextTarget {
  object: Mupdf; rect: number[]; rotation: number; text: string; fontSize: number; color: number[];
}

function annotationState(object: Mupdf): string {
  // PDF.js keeps the annotation reference on edit and allocates a new /AP.
  // Compare the entire dictionary (including AP/DR/RC), not /NM, text alone,
  // array order or an editor dirty flag. Resolve layout values as well: they
  // may be indirect in an external PDF.
  return JSON.stringify([object.resolve().toString(),
    ...['Contents', 'Rect', 'Rotate', 'DA'].map(key => {
      const value = object.get(key);
      return (value.isIndirect() ? value.resolve() : value).toString();
    })]);
}

/** Match only objects still on the same page; deleted objects are never targets. */
function collectAnnotations(doc: Mupdf, original: Mupdf) {
  const targets: FreeTextTarget[] = [];
  const asserted: Mupdf[] = [];
  const required = new Set<number>();
  const pageCount = doc.countPages();
  if (pageCount !== original.countPages()) throw new WriterRefusal('writer_baseline_invalid', 'serialize');
  for (let pageIndex = 0; pageIndex < pageCount; pageIndex++) {
    const before = new Map<string, string>();
    const oldPage = original.loadPage(pageIndex);
    for (const annot of oldPage.getAnnotations()) {
      const object = annot.getObject();
      if (annot.getType() === 'FreeText' && object.isIndirect()) before.set(object.toString(), annotationState(object));
    }
    oldPage.destroy();
    const page = doc.loadPage(pageIndex);
    for (const annot of page.getAnnotations()) {
      const object = annot.getObject();
      // Highlight, Ink and every annotation of the original document already
      // carry their own appearance (PDF.js writes vector /AP for the editor
      // types it saves; original annotations keep the bytes they were opened
      // with). None of them needs the pinned font, so they are re-asserted
      // unchanged instead of being rewritten.
      if (annot.getType() !== 'FreeText' ||
          (object.isIndirect() && before.get(object.toString()) === annotationState(object))) {
        asserted.push(object); continue;
      }
      const text = String(annot.getContents() ?? '');
      const da = String(object.get('DA')?.asString?.() ?? '').replace(/\\/g, '');
      const sizeMatch = /([\d.]+)\s+Tf/.exec(da);
      const colorMatch = /([\d.]+)\s+([\d.]+)\s+([\d.]+)\s+rg/.exec(da);
      const rect = ((object.get('Rect')?.asJS?.() ?? annot.getRect()) as unknown[]).map(Number);
      const rotation = Number(object.get('Rotate')?.asNumber?.() ?? 0);
      for (const ch of text) required.add(ch.codePointAt(0) ?? 0);
      targets.push({
        object, rect, rotation, text,
        fontSize: sizeMatch ? Number(sizeMatch[1]) : 12,
        color: colorMatch ? [Number(colorMatch[1]), Number(colorMatch[2]), Number(colorMatch[3])] : [0, 0, 0],
      });
    }
    page.destroy();
  }
  return { targets, asserted, required };
}

/** Write one FreeText appearance using the pinned, subsetted Unicode font. */
function writeFreeTextAppearance(doc: Mupdf, target: FreeTextTarget, full: Mupdf, fontObj: Mupdf, layout:FreeTextLayouter) {
  const width = target.rect[2] - target.rect[0];
  const height = target.rect[3] - target.rect[1];
  if (!(width > 0) || !(height > 0) || !(target.fontSize >= 1 && target.fontSize <= 400)) throw new WriterRefusal('writer_geometry_refused', 'appearance');
  const { box, matrix } = rotationBox(width, height, target.rotation);
  const leading = target.fontSize * 1.35;
  const lines=layout(target.text,box[2],target.fontSize);
  const parts = ['1 0 0 1 0 0 cm', `${target.color[0]} ${target.color[1]} ${target.color[2]} rg`];
  for (const line of lines) {
    let hex = '';
    let advance=0;
    for (const ch of line.text) {const gid=Number(full.encodeCharacter(ch.codePointAt(0)??0));hex+=pad4(gid);advance+=full.advanceGlyph(gid)*target.fontSize;}
    if(!Number.isFinite(line.x)||!Number.isFinite(line.baseline)||line.x < -0.02||line.x+advance>box[2]+0.05||line.baseline-target.fontSize< -0.05||line.baseline+target.fontSize*.2>box[3]+0.05)
      throw new WriterRefusal('writer_text_overflow','appearance');
    if(hex)parts.push(`BT 1 0 0 1 ${line.x.toFixed(4)} ${(box[3]-line.baseline).toFixed(4)} Tm /${RESOURCE} ${target.fontSize} Tf ${leading.toFixed(2)} TL <${hex}> Tj ET`);
  }
  if (parts.length === 2) throw new WriterRefusal('writer_encode_failed', 'appearance');
  const stream = doc.addStream(parts.join('\n'));
  stream.put('Type', doc.newName('XObject'));
  stream.put('Subtype', doc.newName('Form'));
  stream.put('FormType', doc.newInteger(1));
  stream.put('BBox', realArray(doc, box));
  stream.put('Matrix', realArray(doc, matrix));
  const resources = doc.newDictionary();
  const fonts = doc.newDictionary();
  fonts.put(RESOURCE, fontObj);
  resources.put('Font', fonts);
  resources.put('ProcSet', arrayOf(doc, [doc.newName('PDF'), doc.newName('Text')]));
  stream.put('Resources', resources);
  const appearance = doc.newDictionary();
  appearance.put('N', stream);
  target.object.put('AP', appearance);
  target.object.put('DA', doc.newString(`/${RESOURCE} ${target.fontSize} Tf ${target.color[0]} ${target.color[1]} ${target.color[2]} rg`));
  const dr = doc.newDictionary();
  const drFonts = doc.newDictionary();
  drFonts.put(RESOURCE, fontObj);
  dr.put('Font', drFonts);
  target.object.put('DR', dr);
  target.object.put('Rect', realArray(doc, target.rect));
}

/**
 * Append exactly one revision: changed FreeText appearances use the pinned
 * Unicode font, every other annotation is re-asserted with the geometry and
 * appearance it already has. Throws a coded WriterRefusal (without producing
 * bytes) when a step cannot be completed, so a blank or partial revision can
 * never be written and no failure is reported without its step.
 * Browser callers prepareFreeTextFont() before invoking this synchronous byte
 * layer. Non-browser protocol probes supply an explicit layout; they are not
 * substitutes for the browser appearance regression.
 */
export function appendAnnotationRevision(mupdf: Mupdf, candidate: Uint8Array, fullFontBytes: Uint8Array, original: Uint8Array, layout:FreeTextLayouter=measureFreeText): WriterResult {
  // The original is a complete PDF, never bytes cut at /Prev (an xref offset).
  // Desktop callers use the authorized descriptor's byte length; the native
  // save service independently checks this prefix against its pinned snapshot.
  if (!original.length || original.length > candidate.length || !original.every((v, i) => candidate[i] === v))
    throw new WriterRefusal('writer_baseline_invalid', 'serialize');
  const doc = writerStep('serialize', () => mupdf.PDFDocument.openDocument(asBytes(candidate), 'application/pdf'));
  try {
  const { targets, asserted, required } = writerStep('serialize', () => {
    const baseline = mupdf.PDFDocument.openDocument(asBytes(original), 'application/pdf');
    try {
      if (baseline.wasRepaired() || doc.wasRepaired()) throw new WriterRefusal('writer_baseline_invalid', 'serialize');
      return collectAnnotations(doc, baseline);
    } finally { baseline.destroy(); }
  });
  if (!targets.length && !asserted.length) {
    // Deleting the last annotation still produces a valid PDF.js candidate.
    // Re-assert the existing catalog reference to emit a real incremental
    // revision without inventing an annotation, flattening, or changing pages.
    writerStep('appearance', () => {
      const catalog = doc.getTrailer().get('Root');
      catalog.put('Pages', catalog.get('Pages'));
    });
  }
  required.delete(0x0a);

  let embeddedFont = false;
  let fontBytes = 0;
  if (targets.length) {
    let full: Mupdf;
    try { full = new mupdf.Font(PDF_HEAD_FONT, fullFontBytes); }
    catch (cause) { throw new WriterRefusal('writer_font_load_failed', 'font', cause); }
    const pairs = writerStep('appearance', () => {
      const encoded = [...required]
        .map((cp) => ({ cp, gid: Number(full.encodeCharacter(cp)) }))
        .filter((entry) => entry.gid)
        .sort((a, b) => a.gid - b.gid);
      if (encoded.length !== required.size) throw new WriterRefusal('writer_glyph_missing', 'appearance');
      return encoded;
    });
    // A previously embedded subset that already covers every code point is
    // reused as-is; only otherwise is the font subset and embedded.
    let fontObj: Mupdf = writerStep('appearance', () => findReusableFont(doc, required));
    if (!fontObj) {
      const subset = writerStep('appearance', () => subsetFontBytes(mupdf, fullFontBytes, pairs.map((entry) => entry.gid)));
      fontBytes = subset.length;
      fontObj = writerStep('appearance', () => addUnicodeFont(doc, subset, pairs, readAdvances(subset)));
      embeddedFont = true;
    }
    for (const target of targets) writerStep('appearance', () => writeFreeTextAppearance(doc, target, full, fontObj,layout));
  }

  // Re-assert what was verified instead of rewriting it: Rect (always required)
  // and the existing appearance. This keeps the revision a real appended
  // revision even when the document has no FreeText annotation at all.
  let assertedCount = 0;
  for (const object of asserted) {
    writerStep('appearance', () => {
      if (typeof object?.get !== 'function' || typeof object?.put !== 'function') return;
      const rect = object.get('Rect');
      if (!rect) return;
      object.put('Rect', rect);
      const appearance = object.get('AP');
      if (appearance) object.put('AP', appearance);
      assertedCount++;
    });
  }

  // Standard lossless Flate compression changes storage only: retain the
  // candidate prefix, font glyphs/metrics, appearance commands and editability.
  const bytes = writerStep('revision', () => wasmBytes(doc.saveToBuffer({ incremental: true, 'compress-fonts': true })));
  if (bytes.length <= candidate.length) throw new WriterRefusal('writer_revision_missing', 'revision');
  return { bytes, targeted: targets.length, asserted: assertedCount, embeddedFont, fontBytes };
  } finally { doc.destroy(); }
}
