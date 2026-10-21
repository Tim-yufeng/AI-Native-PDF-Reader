// Fixed, versioned reader resources only. Never copy G1 experiments or samples
// into production. Existing generated files must match; links are refused.
import { mkdir, lstat, readFile, open, realpath, readdir, unlink } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(fileURLToPath(new URL('../', import.meta.url)));
async function regular(filename, directory) {
  const stat = await lstat(filename);
  if (stat.isSymbolicLink() || (directory ? !stat.isDirectory() : !stat.isFile()) ||
      path.normalize(await realpath(filename)).toLowerCase() !== filename.toLowerCase()) {
    throw new Error('Linked or irregular reader resource refused');
  }
}
async function directory(filename) {
  try { await mkdir(filename); } catch (error) { if (error.code !== 'EEXIST') throw error; }
  await regular(filename, true);
}
async function put(filename, bytes) {
  let handle;
  try { handle = await open(filename, 'wx'); }
  catch (error) {
    if (error.code !== 'EEXIST') throw error;
    await regular(filename, false);
    if (!(await readFile(filename)).equals(bytes)) throw new Error('Existing reader resource differs; no overwrite performed');
    return;
  }
  try { await handle.writeFile(bytes); } finally { await handle.close(); }
}
async function copy(source, target) {
  const stat = await lstat(source);
  await regular(source, stat.isDirectory());
  if (stat.isDirectory()) {
    await directory(target);
    for (const name of await readdir(source)) await copy(path.join(source, name), path.join(target, name));
  } else await put(target, await readFile(source));
}
const pdfjs = path.join(root, 'node_modules/pdfjs-dist');
await regular(pdfjs, true);
if (JSON.parse(await readFile(path.join(pdfjs, 'package.json'), 'utf8')).version !== '6.4.299') {
  throw new Error('PDF.js reader resources must match the locked version');
}
await directory(path.join(root, 'public'));
const resources = path.join(root, 'public/pdfjs');
await directory(resources);
for (const name of ['cmaps', 'standard_fonts', 'wasm', 'web/images']) {
  const source = path.join(pdfjs, name);
  await copy(source, path.join(resources, name === 'web/images' ? 'images' : name));
}
await copy(path.join(pdfjs, 'LICENSE'), path.join(resources, 'LICENSE'));
await put(path.join(resources, 'version.json'), Buffer.from('{"version":"6.4.299"}\n'));

// MuPDF.js writer byte layer. The wasm binary is served next to the writer
// module; the pinned Unicode font is copied from the project-owned font folder,
// which is local-only (see .gitignore) and therefore an explicit build input.
const mupdf = path.join(root, 'node_modules/mupdf');
await regular(mupdf, true);
if (JSON.parse(await readFile(path.join(mupdf, 'package.json'), 'utf8')).version !== '1.28.1') {
  throw new Error('MuPDF.js writer resources must match the locked version');
}
const writer = path.join(root, 'public/mupdf');
await directory(writer);
await copy(path.join(mupdf, 'dist/mupdf-wasm.wasm'), path.join(writer, 'mupdf-wasm.wasm'));
await put(path.join(writer, 'version.json'), Buffer.from('{"version":"1.28.1","license":"AGPL-3.0-or-later"}\n'));
const fontRecord = JSON.parse(await readFile(path.join(root,'worker/freetext-font.json'),'utf8'));
const font = path.join(root, 'worker/fonts', fontRecord.name);
await regular(font, false);
await directory(path.join(root, 'public/noto'));
await put(path.join(root, 'public/noto', fontRecord.name), await readFile(font));
await copy(path.join(root,'worker/fonts',fontRecord.licenseFile),path.join(root,'public/noto',fontRecord.licenseFile));
// Retire only the exact former generated public copy, to avoid bundling an
// unused 17 MB Thin font. Keep the source font, installers and all test evidence.
const legacy = path.join(root,'public/noto/NotoSansSC-VF.ttf');
try {
  await regular(legacy,false);
  const expected = JSON.parse(await readFile(path.join(root,'worker/font.json'),'utf8'));
  const bytes = await readFile(legacy);
  if(bytes.length!==expected.bytes||createHash('sha256').update(bytes).digest('hex')!==expected.sha256)throw new Error('Unexpected legacy public font; no removal performed');
  await unlink(legacy);
} catch(error) {if(error.code!=='ENOENT')throw error;}
