// One command for a formal update: verify the sources, run the real App save
// loop, package the fixed-runtime installer, and stage exactly one versioned
// candidate with its hashes. Never installs, never touches a running app, and
// never deletes a previous candidate.
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(fileURLToPath(new URL('../', import.meta.url)));
const sourceCommit=execFileSync('git',['rev-parse','HEAD'],{cwd:root,encoding:'utf8'}).trim();
if(execFileSync('git',['status','--porcelain'],{cwd:root,encoding:'utf8'}).trim())throw Error('Commit the reviewed sources before preparing the versioned candidate');
const version = JSON.parse(readFileSync(path.join(root, 'package.json'), 'utf8')).version;
const stage = path.join(root, '.cache', `v${version}-candidate`);
// Same values the NSIS template and the Tauri config must agree on.
const product = JSON.parse(readFileSync(path.join(root, 'src-tauri/tauri.conf.json'), 'utf8')).productName;
const installer = `${product}_${version}_x64-setup.exe`;
const built = path.join(root, '.cache/tauri-target/release/bundle/nsis', installer);
const steps = [];

function run(label, command, args) {
  process.stdout.write(`\n=== ${label} ===\n`);
  execFileSync(command, args, { cwd: root, stdio: 'inherit' });
  steps.push(label);
}
const node = process.execPath;
const sha256 = file => createHash('sha256').update(readFileSync(file)).digest('hex');
const size = file => statSync(file).size;

// 1. The identities every artifact and the installer carry must agree.
const cargo = readFileSync(path.join(root, 'src-tauri/Cargo.toml'), 'utf8');
const lock = readFileSync(path.join(root, 'src-tauri/Cargo.lock'), 'utf8');
for (const [what, text] of [['Cargo.toml', cargo], ['Cargo.lock', lock]]) {
  if (!text.includes(`version = "${version}"`)) throw Error(`${what} does not carry version ${version}`);
}
const identifier = JSON.parse(readFileSync(path.join(root, 'src-tauri/tauri.conf.json'), 'utf8')).identifier;
if (!identifier) throw Error('the Tauri config has no application identifier');
console.log(`candidate ${version} · ${identifier} · ${installer}`);

// 2. Everything that can fail cheaply fails before the three-minute build.
run('types', node, [path.join(root, 'node_modules/typescript/bin/tsc'), '-p', 'tsconfig.tests.json']);
run('frontend tests', node, ['--test', ...readdirSync(path.join(root,'tests/frontend')).filter(name=>name.endsWith('.test.ts')).sort().map(name=>'tests/frontend/'+name)]);
for(const test of ['classroom','workspace-mode','settings','text-autogrow','font-appearance'])run(`classroom gate: ${test}`,node,['tests/headless/post-v1.run.mjs',test,'2']);
run('annotation interaction gate',node,['tests/headless/annotation-recovery.run.mjs','2']);

// 3. The real product build, then the isolated evidence the packaging gate
//    requires: a normal build, the hidden App lab on top of it, and the save
//    chain that proves the five Ctrl+S rounds against the current sources.
//    The toolchain lives in one PowerShell helper because the project's Node
//    desktop script locates Visual Studio through a piped child process.
const powershell = process.env.ComSpec
  ? path.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32', 'WindowsPowerShell', 'v1.0', 'powershell.exe')
  : 'powershell.exe';
const release = (purpose, quiet = true) => run(`${purpose} build`, powershell,
  ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', path.join(root, 'scripts/build-release.ps1'), '-Purpose', purpose, ...(quiet ? ['-Quiet'] : [])]);

process.env.VITE_WRITER_CANDIDATE = '1';
run('frontend build', node, ['node_modules/vite/bin/vite.js', 'build']);
run('isolated App page', node, ['scripts/build-save-chain-lab.mjs', '--app']);
release('release', false);
release('lab');
run('real App save loop', node, ['scripts/run-save-chain-lab.mjs', '--app']);
// The lab binary must never be the one that gets packaged.
release('release', false);
run('package the versioned installer', node, ['scripts/desktop.mjs', 'package-fixed-runtime']);

if (!existsSync(built)) throw Error(`the packager produced no installer at ${built}`);

// 4. Exactly one staged candidate, replaced only when it is byte different.
mkdirSync(stage, { recursive: true });
const staged = path.join(stage, installer);
const hash = sha256(built);
const previous = existsSync(staged) ? sha256(staged) : null;
if (previous !== hash) copyFileSync(built, staged);
const appExe = path.join(root, '.cache/tauri-target/release/ai-native-pdf-reader.exe');
const info = {
  version, sourceCommit, identifier, installer, bytes: size(staged), sha256: hash,
  appBytes: existsSync(appExe) ? size(appExe) : null,
  appSha256:existsSync(appExe)?sha256(appExe):null,
  saveFingerprint:JSON.parse(readFileSync(path.join(root,'.cache/save-app-gate.json'),'utf8')).fingerprint,
  saveEvidence:JSON.parse(readFileSync(path.join(root,'.cache/save-app-gate.json'),'utf8')).report,
  features: ['real-provider'], webviewInstallMode: 'fixedRuntime', signed: false,
  builtAt: new Date(statSync(staged).mtimeMs).toISOString(),
  evidence: 'run-save-chain-lab --app must report PASS before this step runs',
};
writeFileSync(path.join(stage, 'SHA256.txt'), `${hash}  ${installer}\r\n`, 'ascii');
writeFileSync(path.join(stage, 'package-info.json'), `${JSON.stringify(info, null, 2)}\n`, 'utf8');
// Never clean unrelated files or old candidates as part of a build.
for (const name of readdirSync(stage)) if (name !== installer && name !== 'SHA256.txt' && name !== 'package-info.json') throw Error('Unexpected candidate material preserved: '+name);

console.log('\n=== candidate ===');
console.log(`path   ${staged}`);
console.log(`bytes  ${info.bytes}`);
console.log(`sha256 ${hash}`);
console.log(`steps  ${steps.join(' | ')}`);
console.log('Not installed. Install it yourself, or keep using npm run desktop:dev.');
