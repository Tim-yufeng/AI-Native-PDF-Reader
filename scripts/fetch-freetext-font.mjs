// The pinned Unicode font is a build input, not a committed binary. Its manifest
// records the exact URL and SHA-256, so a fresh clone can obtain the same bytes and
// verify them instead of trusting whatever a download may return.
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const root = path.resolve(fileURLToPath(new URL('../', import.meta.url)));
const record = JSON.parse(await readFile(path.join(root, 'worker/freetext-font.json'), 'utf8'));
const target = path.join(root, 'worker/fonts', record.name);
const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');

export async function freetextFont() {
  let bytes;
  try {
    bytes = await readFile(target);
  } catch {
    console.log(`fetching ${record.name} (${record.bytes} B) from the pinned URL`);
    let response;
    try {
      response = await fetch(record.url, { redirect: 'follow' });
    } catch (cause) {
      throw new Error(
        `Cannot obtain ${record.name}: ${cause.message}. ` +
        `Download it from ${record.url} and place it at worker/fonts/${record.name}; ` +
        `it must be ${record.bytes} bytes with SHA-256 ${record.sha256}.`);
    }
    if (!response.ok) throw new Error(`Cannot obtain ${record.name}: HTTP ${response.status} from ${record.url}`);
    bytes = Buffer.from(await response.arrayBuffer());
    if (bytes.length !== record.bytes || sha256(bytes) !== record.sha256) {
      throw new Error(`${record.name} does not match the manifest (bytes ${bytes.length}/${record.bytes}, sha256 differs); refusing to use it`);
    }
    await mkdir(path.dirname(target), { recursive: true });
    await writeFile(target, bytes);
  }
  if (bytes.length !== record.bytes || sha256(bytes) !== record.sha256) {
    throw new Error(`${record.name} does not match worker/freetext-font.json; delete it and let the build fetch it again`);
  }
  return target;
}

// The licence text must travel with the font and is verified the same way.
export async function freetextFontLicense() {
  const licence = path.join(root, 'worker/fonts', record.licenseFile);
  const text = await readFile(licence);
  if (sha256(text) !== record.licenseSha256) throw new Error(`${record.licenseFile} does not match the manifest`);
  return licence;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const file = await freetextFont();
  await freetextFontLicense();
  console.log(JSON.stringify({ status: 'PASS', font: path.relative(root, file), bytes: record.bytes, sha256: record.sha256, license: record.license }));
}
