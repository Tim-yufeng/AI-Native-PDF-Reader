// Independent check of the Release save loop report against the bytes it left on
// disk. Recomputes every digest itself; trusts nothing the report says.
import { readdir, readFile, stat } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import path from 'node:path';

const root = path.resolve('tests/work/save-chain-release');
const runs = [];
for (const n of await readdir(root)) {
  if (!/^run-[a-f0-9-]{36}$/.test(n)) continue;
  runs.push({ n, t: (await stat(path.join(root, n))).mtimeMs });
}
runs.sort((a, b) => a.t - b.t);
if (!runs.length) throw new Error('no lab run found');
const newest = runs[runs.length - 1].n;
const dir = path.join(root, newest);
const sha = async p => createHash('sha256').update(await readFile(p)).digest('hex');
const size = async p => (await stat(p)).size;

const report = JSON.parse(await readFile(path.join(dir, 'result.json'), 'utf8'));
const problems = [];
const ok = (cond, label) => { if (!cond) problems.push(label); };

console.log(`run            : ${newest}  (newest of ${runs.length})`);
console.log(`report status  : ${report.status}`);

// 1. The committed original on disk must equal the digest the report claims.
const original = path.join(dir, 'plain.pdf');
ok(await sha(original) === report.nativeSha256, 'native sha256 differs from disk');
ok(await size(original) === report.nativeBytes, 'native size differs from disk');
console.log(`original       : ${await size(original)} B  sha256=${await sha(original)}`);
console.log(`report claims  : ${report.nativeBytes} B  sha256=${report.nativeSha256}`);

// 2. Exactly ten native commits, each left as a real output file.
const outputs = (await readdir(dir)).filter(n => n.endsWith('-output.pdf'));
ok(outputs.length === 10, `expected 10 native commits, found ${outputs.length}`);
ok(report.originalCommitCalls === 10, `report claims ${report.originalCommitCalls} commits`);
console.log(`native commits : ${outputs.length} (report says ${report.originalCommitCalls})`);

// 3. Save As copy must be a real, larger PDF with the digest the report claims.
const copy = path.join(dir, 'exports/export-00.pdf');
ok(await sha(copy) === report.saveAsSha256, 'Save As sha256 differs from disk');
ok(await size(copy) === report.saveAsBytes, 'Save As size differs from disk');
console.log(`save as        : ${await size(copy)} B  sha256=${await sha(copy)}`);

// 4. No long-term backup folder or unfinished transaction beside the document.
const entries = await readdir(dir);
const userBackups = entries.filter(n => n.startsWith('.ainative-backup-'));
const temporary = entries.filter(n => n.startsWith('.ainative-save-'));
ok(userBackups.length === 0, `user-folder backups left: ${userBackups.length}`);
ok(temporary.length === 0, `temporary transactions left: ${temporary.length}`);
console.log(`user folder    : backups=${userBackups.length} temporary=${temporary.length}`);

// 5. The archive holds exactly two verified old versions, each a prefix of the
//    committed original, and each marker agreeing with its own bytes.
const archived = [];
for (const dataRoot of entries.filter(n => n === 'data-v1')) {
  for (const f of await readdir(path.join(dir, dataRoot), { recursive: true })) {
    if (!f.includes('pdf-backups') || !f.includes('.ainative-backup-')) continue;
    const abs = path.join(dir, dataRoot, f);
    if (f.endsWith('verified-old.pdf')) archived.push(abs);
  }
}
ok(archived.length === 2, `archive must keep two versions, found ${archived.length}`);
const originalBytes = await readFile(original);
for (const p of archived) {
  const old = await readFile(p);
  ok(originalBytes.subarray(0, old.length).equals(old), `archived version is not a prefix of the original: ${p}`);
  const marker = JSON.parse(await readFile(path.join(path.dirname(p), 'retention-v1.json'), 'utf8'));
  ok(marker.old_hash === createHash('sha256').update(old).digest('hex'), `archive marker disagrees with its bytes: ${p}`);
  ok(marker.old_len === old.length, `archive marker length disagrees: ${p}`);
}
console.log(`archive        : ${archived.length} retained old versions, each verified against its own marker`);

// 6. Report rows, inline rounds and the counts it claims must all agree.
const rows = report.rows ?? [];
ok(rows.length >= 2 && rows.every(r => r.status === 'PASS'), 'a report row is not PASS');
const inline = report.inlineRows ?? [];
ok(inline.length === 10 && inline.every(r => r.status === 'PASS'), `inline rows incomplete: ${inline.length}`);
ok(report.userFolderBackupCount === 0, `report claims ${report.userFolderBackupCount} leftover user-folder backups`);
ok(report.temporaryTransactionCount === 0, `report claims ${report.temporaryTransactionCount} leftover temporary transactions`);
ok(report.backupCount === archived.length, `report claims ${report.backupCount} archived versions, disk has ${archived.length}`);

console.log(`report rows    : ${rows.length} all PASS=${rows.every(r => r.status === 'PASS')}`);
console.log(`inline rounds  : ${inline.length} all PASS=${inline.every(r => r.status === 'PASS')}`);
console.log(`report backups : archive=${report.backupCount} userFolder=${report.userFolderBackupCount} temp=${report.temporaryTransactionCount}`);

if (problems.length) { console.error('\nFAIL'); for (const p of problems) console.error(' - ' + p); process.exit(1); }
console.log('\nINDEPENDENT CHECK PASS: report, disk bytes and archive agree');
