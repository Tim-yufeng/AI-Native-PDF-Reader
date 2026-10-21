// Prints the dictionary keys of every Ink annotation in a PDF plus the shape of
// its appearance stream: /AP /N subtype, /BBox, /Matrix. Structure only.
import { readFile } from 'node:fs/promises';

const bytes = await readFile(process.argv[2]);
const text = bytes.toString('latin1');
const objects = new Map();
for (const m of text.matchAll(/(\d+)\s+0\s+obj([\s\S]{0,4000}?)endobj/g)) {
  objects.set(m[1], m[2]);
}
let shown = 0;
for (const [num, dict] of objects) {
  if (!/\/Subtype\s*\/Ink/.test(dict)) continue;
  shown++;
  const keys = [...dict.matchAll(/\/([A-Za-z]+)/g)].map(m => m[1]);
  console.log(`ink object ${num}`);
  console.log(`  keys        : ${[...new Set(keys)].join(' ')}`);
  console.log(`  flags       : ${(dict.match(/\/F\s+(\d+)/) || [, 'absent'])[1]}`);
  console.log(`  appearance  : ${/\/AP\s*<</.test(dict) ? 'present' : 'ABSENT'}`);
  const ap = dict.match(/\/AP\s*<<\s*\/N\s+(\d+)\s+0\s+R/);
  if (ap) {
    const target = objects.get(ap[1]) ?? '';
    console.log(`  AP stream   : object ${ap[1]} subtype=${(target.match(/\/Subtype\s*\/(\w+)/) || [, '?'])[1]} bbox=${(target.match(/\/BBox\s*\[([^\]]*)\]/) || [, 'absent'])[1]} matrix=${(target.match(/\/Matrix\s*\[([^\]]*)\]/) || [, 'absent'])[1]}`);
    console.log(`  AP stream   : resources=${/\/Resources\s*<</.test(target) ? 'present' : 'absent'} length=${(target.match(/\/Length\s+(\d+)/) || [, '?'])[1]}`);
  } else {
    console.log('  AP stream   : /AP /N is not an indirect reference into this revision');
  }
  console.log(`  printable   : ${/\/F\s+\d+/.test(dict) ? ((Number((dict.match(/\/F\s+(\d+)/) || [, 0])[1]) & 4) ? 'yes (Print)' : 'no') : 'default'}`);
  if (shown >= 1) break;
}
if (!shown) console.log('no Ink annotation object found in this revision');
