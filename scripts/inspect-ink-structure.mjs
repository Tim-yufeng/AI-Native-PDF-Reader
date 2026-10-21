// Reports the structure of every Ink annotation in a PDF: subtype, flags, rect,
// whether it carries an /InkList and an /AP appearance. Structure only; no text.
import { readFile } from 'node:fs/promises';

const file = process.argv[2];
const bytes = await readFile(file);
const text = bytes.toString('latin1');
let at = 0, count = 0;
while ((at = text.indexOf('/Subtype/Ink', at)) >= 0 || (at = text.indexOf('/Subtype /Ink', at)) >= 0) {
  if (at < 0) break;
  // Widen to the enclosing object.
  const objStart = text.lastIndexOf(' obj', at);
  const start = objStart < 0 ? Math.max(0, at - 400) : text.lastIndexOf('\n', objStart) + 1;
  const end = text.indexOf('endobj', at);
  const dict = text.slice(start, end < 0 ? at + 600 : end);
  count++;
  const num = (dict.match(/(\d+)\s+0\s+obj/) || [])[1] ?? '?';
  const flags = (dict.match(/\/F\s+(\d+)/) || [])[1] ?? '(absent)';
  const rect = (dict.match(/\/Rect\s*\[([^\]]*)\]/) || [])[1]?.trim() ?? '(absent)';
  const hasInkList = /\/InkList\s*\[/.test(dict);
  const points = (dict.match(/\/InkList\s*\[([\s\S]*?)\]\s*(?:\/|>>)/) || [])[1];
  const coords = points ? (points.match(/-?\d+(\.\d+)?/g) || []).length : 0;
  const hasAP = /\/AP\s*<</.test(dict);
  const apSubtype = (dict.match(/\/Subtype\s*\/(Form|Image)/) || [])[1] ?? '(none)';
  const contents = /\/Contents\s*\(/.test(dict);
  console.log(`ink #${count} obj=${num} flags=${flags} points=${coords} inkList=${hasInkList} ap=${hasAP}/${apSubtype} contents=${contents}`);
  console.log(`   rect=${rect}`);
  at += 12;
}
console.log(count ? `\ntotal ink annotations: ${count}` : 'no Ink annotation found');
