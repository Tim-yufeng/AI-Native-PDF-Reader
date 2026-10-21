// Read-only accounting of the installed VS catalog. No installer invocation.
import { readFileSync, existsSync, readdirSync, openSync, readSync, closeSync, statSync, lstatSync } from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';

const vswhere = 'C:/Program Files (x86)/Microsoft Visual Studio/Installer/vswhere.exe';
if (!existsSync(vswhere)) throw new Error('vswhere is missing');
const instances = JSON.parse(execFileSync(vswhere, ['-all', '-prerelease', '-products', '*', '-format', 'json', '-utf8'], { encoding: 'utf8' }));
const identity = p => [p.id.toLowerCase(), p.version, p.chip || '', p.language?.toLowerCase() || '', p.productArch || ''].join('|');
const sum = (packages, key) => packages.reduce((n, p) => n + (Number(p.installSizes?.[key]) || 0), 0);
const bytes = p => (p.payloads || []).reduce((n, file) => n + (Number(file.size) || 0), 0);
const cacheRoot = 'C:/ProgramData/Microsoft/VisualStudio/Packages';
const cacheFolders = readdirSync(cacheRoot, { withFileTypes: true }).filter(p => p.isDirectory()).map(p => p.name);
function cached(pkg) {
  if (pkg.payloads?.length !== 1) return null;
  const payload = pkg.payloads[0];
  const prefix = `${pkg.id},version=${pkg.version}`.toLowerCase();
  const folders = cacheFolders.filter(name => name.toLowerCase().startsWith(prefix) &&
    (!pkg.language || name.toLowerCase().includes(`language=${pkg.language.toLowerCase()}`)) &&
    (!pkg.chip || name.toLowerCase().includes(`chip=${pkg.chip.toLowerCase()}`)));
  for (const folder of folders) {
    const original = path.basename(payload.fileName);
    for (const name of [original, `payload${path.extname(original)}`]) {
      const file = path.join(cacheRoot, folder, name);
      if (!existsSync(file) || !lstatSync(file).isFile()) continue;
      const hash = createHash('sha256'), fd = openSync(file, 'r'), block = Buffer.alloc(1024 * 1024);
      try { let count; while ((count = readSync(fd, block))) hash.update(block.subarray(0, count)); }
      finally { closeSync(fd); }
      if (hash.digest('hex') === payload.sha256.toLowerCase()) return { bytes: statSync(file).size, sha256Verified: true };
    }
  }
  return null;
}
const reports = instances.map(instance => {
  if (!/^[a-z0-9]+$/i.test(instance.instanceId)) throw new Error('Invalid instance ID');
  const root = path.join('C:/ProgramData/Microsoft/VisualStudio/Packages/_Instances', instance.instanceId);
  const load = name => JSON.parse(readFileSync(path.join(root, name), 'utf8').replace(/^\uFEFF/, ''));
  const catalog = load('catalog.json'), state = load('state.packages.json');
  const errors = existsSync(path.join(root, 'state.errors.json')) ? load('state.errors.json').errors || {} : {};
  const selected = load('state.json').selectedPackages || [];
  const languages = new Set(state.packages.map(p => p.language?.toLowerCase()).filter(Boolean));
  const installed = new Set(state.packages.map(identity));
  const pending = [...(errors.failedPackages || []), ...(errors.skippedPackages || [])];
  const pendingKeys = new Set(pending.map(identity));
  const byId = new Map();
  for (const pkg of catalog.packages) {
    // This machine is x64; language-specific resources must match the instance.
    if (pkg.machineArch && !['x64', 'neutral'].includes(pkg.machineArch.toLowerCase())) continue;
    if (pkg.language && !languages.has(pkg.language.toLowerCase()) && pkg.language.toLowerCase() !== 'neutral') continue;
    const id = pkg.id.toLowerCase();
    if (!byId.has(id)) byId.set(id, []);
    byId.get(id).push(pkg);
  }
  const seen = new Set(), closure = [];
  function visit(id) {
    id = id.toLowerCase();
    if (seen.has(id)) return;
    seen.add(id);
    for (const pkg of byId.get(id) || []) {
      closure.push(pkg);
      for (const [depId, dep] of Object.entries(pkg.dependencies || {})) {
        if (typeof dep === 'object' && ['optional', 'recommended'].includes(dep.type?.toLowerCase())) continue;
        visit(depId);
      }
    }
  }
  visit('Microsoft.VisualStudio.Component.VC.Tools.x86.x64');
  // Only count packages explicitly reported failed/skipped by this installer.
  // This avoids pretending our graph traversal is the installer applicability resolver.
  const missing = closure.filter(p => pendingKeys.has(identity(p)) && !installed.has(identity(p)));
  const missingIds = new Set(missing.map(p => p.id.toLowerCase()));
  const unresolved = pending.filter(p => seen.has(p.id.toLowerCase()) && !installed.has(identity(p)) && !missingIds.has(p.id.toLowerCase()));
  const rows = missing.map(p => ({ id: p.id, version: p.version, language: p.language,
    targetBytes: Number(p.installSizes?.targetDrive) || 0, systemBytes: Number(p.installSizes?.systemDrive) || 0,
    downloadBytes: bytes(p), existingCache: cached(p) }));
  const outside = catalog.packages.filter(p => pendingKeys.has(identity(p)) && !installed.has(identity(p)) && !missingIds.has(p.id.toLowerCase()));
  const versionFile = path.join(instance.installationPath, 'VC/Auxiliary/Build/Microsoft.VCToolsVersion.default.txt');
  const toolVersion = existsSync(versionFile) ? readFileSync(versionFile, 'utf8').trim() : '(not installed)';
  const tool = path.join(instance.installationPath, 'VC/Tools/MSVC', toolVersion);
  return { instanceId: instance.instanceId, path: instance.installationPath, version: instance.installationVersion,
    complete: instance.isComplete, toolVersion, catalogVersion: catalog.info?.buildVersion,
    component: 'Microsoft.VisualStudio.Component.VC.Tools.x86.x64',
    selectedComponents: selected.filter(p => p.type === 'Component').map(p => p.id),
    actualFiles: Object.fromEntries(['bin/Hostx64/x64/cl.exe', 'bin/Hostx64/x64/link.exe',
      'bin/Hostx86/x64/cl.exe', 'lib/x64/libcmt.lib', 'lib/x64/libvcruntime.lib',
      'lib/x64/msvcrt.lib', 'lib/x64/vcruntime.lib'].map(file => [file, existsSync(path.join(tool, file))])),
    failed: (errors.failedPackages || []).map(p => ({ id: p.id, action: p.action, code: p.returnCode, details: p.details })),
    missing: rows, unresolved, totals: { targetBytes: sum(missing, 'targetDrive'), systemBytes: sum(missing, 'systemDrive'),
      downloadBytes: missing.reduce((n, p) => n + bytes(p), 0),
      downloadBytesAfterVerifiedCacheReuse: rows.reduce((n, p) => n + (p.existingCache ? 0 : p.downloadBytes), 0) },
    pendingOutsideComponent: { ids: outside.map(p => p.id), targetBytes: sum(outside, 'targetDrive'),
      systemBytes: sum(outside, 'systemDrive'), downloadBytes: outside.reduce((n, p) => n + bytes(p), 0),
      installedAsanPackages: state.packages.filter(p => /asan/i.test(p.id)).map(p => p.id) },
    caveat: 'Exact catalog bytes for recorded missing packages, not an executed installer plan or measured disk allocation. Installer servicing, cache reuse, temp space and existing partial files may change the actual delta.' };
});
console.log(JSON.stringify({ observedAt: new Date().toISOString(), reports }, null, 2));
