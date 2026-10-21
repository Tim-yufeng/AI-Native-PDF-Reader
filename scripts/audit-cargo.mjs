// Query public package names/versions only; no document content or local paths.
import { readFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
const locked = readFileSync(new URL('../src-tauri/Cargo.lock', import.meta.url), 'utf8');
const packages = locked.split('[[package]]').slice(1).filter(section => /source = "registry\+/.test(section))
  .map(section => ({ name: /^name = "([^"]+)"/m.exec(section)?.[1], version: /^version = "([^"]+)"/m.exec(section)?.[1] }));
if (packages.some(pkg => !pkg.name || !pkg.version)) throw Error('Unable to parse generated Cargo lock');
const findings = [];
for (let offset = 0; offset < packages.length; offset += 100) {
  const batch = packages.slice(offset, offset + 100);
  const response = await fetch('https://api.osv.dev/v1/querybatch', { method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ queries: batch.map(pkg => ({package:{name:pkg.name,ecosystem:'crates.io'},version:pkg.version})) }), signal: AbortSignal.timeout(30000) });
  if (!response.ok) throw Error(`Advisory query failed: ${response.status}`);
  const data = await response.json();
  if (data.results?.length !== batch.length) throw Error('Incomplete advisory results');
  data.results.forEach((result,index) => { if (result.vulns?.length) findings.push({...batch[index], advisories:result.vulns.map(v=>v.id)}); });
}
const tree = execFileSync(process.execPath, [fileURLToPath(new URL('./desktop.mjs', import.meta.url)), 'tree', '--locked',
  '--target', 'x86_64-pc-windows-msvc', '--edges', 'normal,build', '--prefix', 'none', '--format', '{p}'], {encoding:'utf8',env:{...process.env,CARGO_TERM_COLOR:'never'}});
const windows = new Set(tree.split(/\r?\n/).map(line => /^(\S+) v([^\s]+)/.exec(line)).filter(Boolean).map(match=>`${match[1]}@${match[2]}`));
if (!windows.size) throw Error('Unable to resolve Windows target dependency tree');
const applicable = findings.filter(pkg=>windows.has(`${pkg.name}@${pkg.version}`));
console.log(JSON.stringify({checkedAt:new Date().toISOString(),registryPackages:packages.length,windowsPackages:windows.size,findings,
  applicableWindowsFindings:applicable},null,2));
if (applicable.length) process.exitCode = 1;
