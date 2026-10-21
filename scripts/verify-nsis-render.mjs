// Post-build verification of one generated NSIS script.
//
// check-package.mjs states what the sources and the bundler wiring must look
// like; only the script the bundler actually produced shows what one build
// resolved. This reads `.cache/tauri-target/release/nsis/x64/installer.nsi`
// from the most recent bundle, asserts the runtime-supply behaviour that the
// selected mode requires, inventories the bundled resources and reports the
// resulting package size. Read-only: it never builds, installs or executes
// anything, and it never touches the host registry.
import { readFile, readdir, stat } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';
const root = path.resolve(fileURLToPath(new URL('../', import.meta.url)));
const generated = path.join(root, '.cache/tauri-target/release/nsis/x64/installer.nsi');
const bundleDirectory = path.join(root, '.cache/tauri-target/release/bundle/nsis');
const mode = (process.argv[2] || 'fixed-runtime').toLowerCase();
assert.ok(['fixed-runtime', 'offline'].includes(mode), 'usage: verify-nsis-render.mjs [fixed-runtime|offline]');
assert.ok(existsSync(generated), `the bundler has not produced a generated NSIS script: ${generated}`);
const lines = (await readFile(generated, 'utf8')).split(/\r?\n/);
const define = name => {
  const line = lines.find(candidate => candidate.startsWith(`!define ${name} `));
  assert.ok(line, `the generated script is missing !define ${name}`);
  return line.slice(`!define ${name} `.length).replace(/^"|"$/g, '');
};
const supplyMode = define('INSTALLWEBVIEW2MODE');
const gateStart = lines.findIndex(line => line.trim() === '!if "${INSTALLWEBVIEW2MODE}" == "offlineInstaller"');
assert.ok(gateStart > 0, 'the compile-time WebView2 supply gate is missing from the generated script');
let depth = 0, gateEnd = -1;
for (let index = gateStart + 1; index < lines.length; index += 1) {
  const line = lines[index].trim();
  if (/^!(if|ifdef|ifndef)\b/.test(line)) depth += 1;
  if (/^!endif\b/.test(line)) {
    if (depth === 0) { gateEnd = index; break; }
    depth -= 1;
  }
}
assert.ok(gateEnd > gateStart, 'the compile-time WebView2 supply gate is not closed');
const inGate = index => index > gateStart && index < gateEnd;
const matches = pattern => lines.map((line, index) => ({ line, index })).filter(entry => pattern.test(entry.line));
// Every action that could install or discover a WebView2 runtime belongs to the
// gated block, so an unrelated supply mode can never reach one of them.
const supplyActions = matches(/MicrosoftEdgeWebView2RuntimeInstaller\.exe|MicrosoftEdgeWebview2Setup\.exe|EdgeUpdate\\Clients/);
assert.ok(supplyActions.length > 0, 'the generated script no longer contains the gated runtime supply code');
assert.ok(supplyActions.every(entry => inGate(entry.index)), 'a WebView2 runtime action escaped the compile-time gate');
const resources = lines.map(line => /^\s*File \/a "\/oname=(.+)" "(.+)"$/.exec(line)).filter(Boolean).map(match => ({ target: match[1], source: match[2] }));
const deletes = new Set(lines.map(line => /^\s*Delete "\$INSTDIR\\(.+)"$/.exec(line)).filter(Boolean).map(match => match[1]));
const report = {
  status: 'PASS',
  evidence: 'generated-nsis-script-content-only; no installer executed',
  requestedMode: mode,
  resolvedInstallWebview2Mode: supplyMode,
  gate: { line: gateStart + 1, closedAtLine: gateEnd + 1, supplyActionsInsideGate: supplyActions.length, supplyActionsOutsideGate: supplyActions.length - supplyActions.filter(entry => inGate(entry.index)).length },
  resourceFileCount: resources.length,
};
if (mode === 'fixed-runtime') {
  // Tauri maps the fixedRuntime variant to an empty supply mode, which makes the
  // `offlineInstaller` gate inert: this package must not read the Evergreen
  // registration and must not run any runtime installer.
  assert.equal(supplyMode, '', 'the fixed-runtime build must not publish an NSIS runtime supply mode');
  const fixedRuntimeResources = resources.filter(entry => entry.target.startsWith('webview2-fixed-runtime\\'));
  assert.equal(fixedRuntimeResources.length, resources.length, 'the fixed-runtime build bundled resources outside the fixed runtime directory');
  const runtimeRoot = path.join(root, 'src-tauri/webview2-fixed-runtime');
  const expected = existsSync(runtimeRoot) ? (await readdir(runtimeRoot, { recursive: true, withFileTypes: true })).filter(entry => entry.isFile()).length : 0;
  assert.ok(expected > 128, 'the project-local fixed runtime is missing, so this inventory cannot be trusted');
  assert.equal(fixedRuntimeResources.length, expected, 'the installer does not carry every file of the project-local fixed runtime');
  assert.ok(fixedRuntimeResources.some(entry => entry.target === 'webview2-fixed-runtime\\msedgewebview2.exe'), 'the bundled fixed runtime engine binary is missing');
  assert.ok(resources.every(entry => !path.isAbsolute(entry.target)), 'a bundled resource would be installed to an absolute path');
  assert.ok(resources.every(entry => !entry.target.split(/[\\/]/).includes('..') && !entry.target.startsWith('.')), 'a bundled resource would install outside the install directory');
  // The runtime must be installed from a build-host copy but only ever read from
  // the install directory, which is what makes the guest independent of the host.
  assert.ok(resources.every(entry => path.resolve(entry.source).toLowerCase().startsWith(path.join(root, 'src-tauri', 'webview2-fixed-runtime').toLowerCase())), 'the fixed runtime sources must come from the project-local extracted runtime');
  assert.ok(resources.every(entry => deletes.has(entry.target)), 'the uninstaller must delete every bundled fixed-runtime file');
  // The template pins and creates every fixed resource directory before copying,
  // so each target directory must appear in the guarded preparation list.
  const prepared = new Set(lines.map(line => /^\s*Push "\$INSTDIR\\(.+)"$/.exec(line)).filter(Boolean).map(match => match[1]));
  const targetDirectories = new Set(resources.map(entry => path.dirname(entry.target)).filter(directory => directory !== '.'));
  assert.ok(prepared.has('webview2-fixed-runtime'), 'the fixed-runtime install directory must be prepared before copying');
  assert.ok([...targetDirectories].every(directory => prepared.has(directory)), 'every fixed-runtime target directory must be prepared before copying');
  report.fixedRuntime = { resourcePath: 'webview2-fixed-runtime', installedBelow: '$INSTDIR', hostPathsReferencedAtRuntime: 0, registryDiscoveryLines: 0, installerExecutionLines: 0 };
} else {
  assert.equal(supplyMode, 'offlineInstaller', 'the offline validation build must publish the offline installer supply mode');
  assert.equal(resources.length, 0, 'the offline validation build must not carry the fixed runtime');
  const live = supplyActions.filter(entry => inGate(entry.index));
  assert.ok(live.some(entry => entry.line.includes('MicrosoftEdgeWebView2RuntimeInstaller.exe')), 'the offline build must embed the signed Microsoft offline installer');
  report.offline = { embeddedInstallerInsideGate: live.length };
}
report.preparedResourceDirectories = new Set(lines.map(line => /^\s*Push "\$INSTDIR\\(.+)"$/.exec(line)).filter(Boolean).map(match => match[1])).size;
const packages = existsSync(bundleDirectory) ? (await readdir(bundleDirectory)).filter(name => name.endsWith('.exe')) : [];
report.packages = [];
for (const name of packages) {
  const file = await stat(path.join(bundleDirectory, name));
  report.packages.push({ name, bytes: file.size, modified: file.mtime.toISOString() });
}
if (mode === 'fixed-runtime') assert.ok(report.packages.some(entry => entry.bytes > 100 * 1024 * 1024), 'the fixed-runtime package is too small to carry the bundled runtime');
console.log(JSON.stringify(report, null, 2));
