// Preprocess only: execute the cached compiler, never an installer or its code.
import assert from 'node:assert/strict';
import { readFileSync, writeFileSync, mkdirSync, realpathSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const compiler = path.join(root, '.cache/tauri-target/.tauri/NSIS/makensis.exe');
const generated = path.join(root, '.cache/tauri-target/release/nsis/x64/installer.nsi');
const work = path.join(root, 'tests/work/m7-directory-type');
for (const file of [compiler, generated]) {
  assert.equal(realpathSync(file).toLowerCase(), file.toLowerCase());
}
mkdirSync(work, { recursive: true });
const temporary = path.join(work, 'compiler-tmp');
mkdirSync(temporary, { recursive: true });
function preprocess(file) {
  const result = spawnSync(compiler, ['/PPO', '/V1', file], {
    cwd: root, windowsHide: true, encoding: 'utf8', maxBuffer: 8 * 1024 * 1024,
    env: { ...process.env, TEMP: temporary, TMP: temporary },
  });
  assert.ifError(result.error);
  assert.equal(result.status, 0, result.stderr);
  return result.stdout.replaceAll('\r\n', '\n');
}
function directoryComparison(expanded, name) {
  const beginning = `Function ${name}\n`;
  const start = expanded.indexOf(beginning);
  assert.ok(start >= 0, `Missing ${name}`);
  const end = expanded.indexOf('\nFunctionEnd', start);
  assert.ok(end > start);
  const instructions = expanded.slice(start + beginning.length, end).split('\n')
    .map(line => line.trim()).filter(line => line && !line.startsWith('!') && !line.startsWith(';'));
  const mask = instructions.indexOf('IntOp $0 $0 & 0x410');
  assert.ok(mask >= 0, 'Missing exact source directory mask');
  return instructions[mask + 1];
}
const source = readFileSync(generated, 'utf8');
assert.equal(source.split('${If} $0 <> 0x10').length - 1, 1);
const expanded = preprocess(generated);
const comparisons = ['PinDirectory', 'un.PinDirectory'].map(name => {
  const instruction = directoryComparison(expanded, name);
  assert.ok(instruction.startsWith('IntCmp `$0` `0x10` '), instruction);
  return { name, instruction };
});
// A negative control compiles to text only. It reproduces the former string
// comparison, proving that a C#/native numeric assertion alone missed it.
const negative = path.join(work, 'legacy-comparison.nsi');
writeFileSync(negative, source.replace('${If} $0 <> 0x10', '${If} $0 != 0x10'));
const legacy = preprocess(negative);
for (const { name } of comparisons) {
  assert.ok(directoryComparison(legacy, name).startsWith('StrCmp `$0` `0x10` '));
}
const report = { status: 'PASS', scope: 'cached compiler preprocessor only; no installation', comparisons,
  legacyStringComparisonDetected: true, installerExecuted: false, registryWrites: 0, aclChanges: 0 };
writeFileSync(path.join(work, 'logic-check.json'), JSON.stringify(report, null, 2));
console.log(JSON.stringify(report));
