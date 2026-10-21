// Pre-packaging writer resource gate.
//
// The renderer fetches the MuPDF.js engine and the pinned Unicode font as
// ordinary frontend assets, and an installed package serves them from the
// frontend Tauri embedded at build time. This checks, before any NSIS package
// can be produced, that every copy that reaches the package is the one the
// application's compiled-in lock expects: the pinned input itself, the Vite
// public input and the built frontend. Any difference aborts packaging instead
// of shipping a package whose writer cannot start.
//
// The numbers in the lock are never recomputed from the files being checked: a
// mismatch is reported with the observed values, and updating the lock stays a
// deliberate, reviewable change of the pinned inputs.
import { lstat, readFile, realpath } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';

const root = path.resolve(fileURLToPath(new URL('../', import.meta.url)));
const lockPath = path.join(root, 'src-tauri/writer-assets.lock.json');
const lock = JSON.parse(await readFile(lockPath, 'utf8'));
assert.equal(lock.schema, 1, 'unsupported writer asset lock schema');
assert.ok(Array.isArray(lock.assets) && lock.assets.length > 0, 'the writer asset lock lists no assets');
const writerSource = await readFile(path.join(root, 'src/lib/mupdf-writer.ts'), 'utf8');
const fontSource = await readFile(path.join(root, 'src/lib/freetext-layout.ts'), 'utf8');
const desktopSource = await readFile(path.join(root, 'src/lib/desktop-source.ts'), 'utf8');
assert.ok(desktopSource.includes("from './freetext-layout.ts'") && desktopSource.includes('loadFreeTextFont()'), 'the writer must use the shared font loader');

async function regular(filename) {
  const stat = await lstat(filename);
  if (stat.isSymbolicLink() || !stat.isFile()) throw new Error(`writer asset is not a regular file: ${filename}`);
  if (path.normalize(await realpath(filename)).toLowerCase() !== path.normalize(filename).toLowerCase()) {
    throw new Error(`writer asset resolves through a link: ${filename}`);
  }
}

async function digest(filename) {
  const bytes = await readFile(filename);
  return { bytes: bytes.length, sha256: createHash('sha256').update(bytes).digest('hex') };
}

const report = { status: 'PASS', lock: path.relative(root, lockPath).replace(/\\/g, '/'), assets: [] };
const basename = filename => filename.replace(/\\/g, '/').split('/').pop();

for (const asset of lock.assets) {
  assert.ok(asset.path && asset.request && asset.source && asset.sha256 && asset.bytes > 0 && asset.license, `incomplete lock entry for ${asset.path}`);
  assert.equal(asset.request, `/${asset.path}`, `the renderer request for ${asset.path} must be its own path`);
  // The served name must be the pinned input's own name, so a rename of either
  // side cannot silently point the renderer at a different file.
  assert.equal(basename(asset.source), basename(asset.path), `the lock must serve ${path.posix.basename(asset.source)} under its own name`);
  // And the renderer must really request it: the loader points MuPDF.js at
  // /mupdf/ (locateFile keeps the package's own file name), the writer fetches
  // the font by its packaged path.
  if (asset.path.startsWith('mupdf/')) assert.ok(writerSource.includes("'/mupdf/'") && asset.request.startsWith('/mupdf/'), 'the MuPDF.js loader must fetch the locked wasm directory');
  else assert.ok(fontSource.includes(`'${asset.request}'`), `the writer must fetch ${asset.request}`);
  const entry = { request: asset.request, path: asset.path, source: asset.source, expectedBytes: asset.bytes, expectedSha256: asset.sha256, copies: {} };
  // The pinned input, what Vite publishes, and the built frontend the package
  // embeds: all three must be byte-identical to the lock.
  for (const [label, filename] of [
    ['source', path.join(root, asset.source)],
    ['public', path.join(root, 'public', ...asset.path.split('/'))],
    ['dist', path.join(root, 'dist', ...asset.path.split('/'))],
  ]) {
    await regular(filename);
    const actual = await digest(filename);
    const matches = actual.bytes === asset.bytes && actual.sha256 === asset.sha256;
    entry.copies[label] = { bytes: actual.bytes, sha256: actual.sha256, matchesLock: matches };
    assert.ok(matches, `${label} copy of ${asset.path} does not match the lock: expected ${asset.bytes} bytes ${asset.sha256}, found ${actual.bytes} bytes ${actual.sha256}`);
  }
  // A locked asset that comes from a package must match the version the project
  // locks, so a dependency upgrade cannot slip past as a hash-only change.
  const packaged = /^node_modules\/([^/]+)\//.exec(asset.source.replace(/\\/g, '/'));
  if (packaged) {
    const declared = JSON.parse(await readFile(path.join(root, 'package.json'), 'utf8')).dependencies?.[packaged[1]];
    const installed = JSON.parse(await readFile(path.join(root, 'node_modules', packaged[1], 'package.json'), 'utf8')).version;
    assert.equal(declared, installed, `the installed ${packaged[1]} version must be the locked one`);
    entry.pinnedPackage = `${packaged[1]}@${installed}`;
  }
  report.assets.push(entry);
}

// The font must still be the file the project recorded when it was downloaded.
const fontRecord = JSON.parse(await readFile(path.join(root, 'worker/freetext-font.json'), 'utf8'));
const lockedFont = lock.assets.find(asset => asset.path === `noto/${fontRecord.name}`);
assert.ok(lockedFont, 'the lock must pin the writer font');
assert.equal(fontRecord.sha256, lockedFont.sha256, 'the locked font differs from worker/freetext-font.json');
assert.equal(fontRecord.bytes, lockedFont.bytes, 'the locked font size differs from worker/freetext-font.json');
const font = await readFile(path.join(root,lockedFont.source));
let weight;const tables=[];
for(let i=0;i<font.readUInt16BE(4);i++){const p=12+16*i,tag=font.toString('ascii',p,p+4);tables.push(tag);if(tag==='OS/2')weight=font.readUInt16BE(font.readUInt32BE(p+8)+4);}
assert.equal(weight,fontRecord.weight,'actual font weight must match the editor');
assert.ok(fontRecord.static&&!tables.includes('fvar'),'the embedded font must be a static instance');
for(const dir of ['worker/fonts','public/noto','dist/noto'])assert.equal((await digest(path.join(root,dir,fontRecord.licenseFile))).sha256,fontRecord.licenseSha256,'font licence copy differs');
report.fontRecord = { name: fontRecord.name, license: fontRecord.license, weight, static: true, matchesFontRecord: true };
report.verifiedAt = new Date().toISOString();
console.log(JSON.stringify(report, null, 2));
