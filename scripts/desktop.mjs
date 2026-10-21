// Project-only process environment. Never changes user/system PATH or proxy.
import { execFileSync, spawn } from 'node:child_process';
import { mkdirSync, existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
const root = path.resolve(fileURLToPath(new URL('../', import.meta.url)));
const rust = 'D:/DevTools/Rust';
const installer = 'C:/Program Files (x86)/Microsoft Visual Studio/Installer/vswhere.exe';
const vs = execFileSync(installer, ['-latest', '-products', '*', '-requires', 'Microsoft.VisualStudio.Component.VC.Tools.x86.x64', '-property', 'installationPath'], { encoding: 'utf8' }).trim();
const toolchain = '1.98.1-x86_64-pc-windows-msvc';
if (!vs || !existsSync(`${rust}/cargo/bin/cargo.exe`) || !existsSync(`${rust}/rustup/toolchains/${toolchain}/bin/rustc.exe`)) {
  throw Error('Required approved desktop tools are missing; do not install automatically');
}
const temp = path.join(root, '.cache/tmp/desktop');
mkdirSync(temp, { recursive: true });
const env = { ...process.env, TEMP: temp, TMP: temp, CARGO_HOME: `${rust}/cargo`, RUSTUP_HOME: `${rust}/rustup`,
  RUSTUP_TOOLCHAIN: toolchain, CARGO_TARGET_DIR: path.join(root, '.cache/tauri-target'), CARGO_BUILD_JOBS: '4',npm_config_cache:path.join(root,'.cache/npm') };
const development = path.join(vs, 'Common7/Tools/VsDevCmd.bat');
const initialized = execFileSync(process.env.ComSpec || 'cmd.exe', ['/d', '/c', `call "${development}" -no_logo -host_arch=x64 -arch=x64 && set`], { env, encoding: 'utf8', windowsVerbatimArguments: true });
for (const line of initialized.split(/\r?\n/)) {
  const index = line.indexOf('=');
  if (index > 0) env[line.slice(0, index)] = line.slice(index + 1);
}
for (const key of Object.keys(env)) if (key.toLowerCase() === 'path') delete env[key];
const pathLine = initialized.split(/\r?\n/).find(line => /^path=/i.test(line));
env.PATH = `${rust}/cargo/bin;${pathLine?.slice(5) || process.env.PATH || ''}`;
const [mode = 'dev', ...args] = process.argv.slice(2);
if (mode === 'm3') { env.M3_ISOLATED_WRITER='1'; env.VITE_M3_COPY='1'; }
if (mode === 'package' || mode === 'package-offline' || mode === 'package-fixed-runtime') { env.VITE_WRITER_CANDIDATE='1'; }
if (mode === 'm4') {
  const testRoot = path.join(root, 'tests/work/m4/desktop-ui');
  mkdirSync(testRoot, { recursive: true });
  env.M4_TEST_ROOT = testRoot;
}
const cargo = `${rust}/cargo/bin/cargo.exe`;
let command, commandArgs;
if(['compile','compile-cli'].includes(mode)){
  command=cargo;commandArgs=['build','--manifest-path',path.join(root,'src-tauri/Cargo.toml'),'--locked',...(mode==='compile-cli'?['--features','worker-isolation-cli']:[]),...args];
} else if (['test', 'check', 'fmt', 'metadata', 'tree', 'fetch'].includes(mode)) {
  command = cargo; commandArgs = [mode, '--manifest-path', path.join(root, 'src-tauri/Cargo.toml'),
    ...(mode === 'fetch' && !args.includes('--target') ? ['--target', 'x86_64-pc-windows-msvc'] : []), ...args];
} else if (['dev', 'build', 'm3', 'm4','package','package-only','package-offline','package-fixed-runtime'].includes(mode)) {
  command = process.execPath; commandArgs = [path.join(root, 'node_modules/@tauri-apps/cli/tauri.js'), ['m3','m4'].includes(mode)?'dev':['package','package-offline','package-fixed-runtime'].includes(mode)?'build':mode==='package-only'?'bundle':mode,
    ...(['package','package-only'].includes(mode)?['--bundles','nsis','--config',path.join(root,'src-tauri/tauri.packaging.conf.json')]:[]),
    ...(mode === 'package-offline' ? ['--bundles','nsis','--config',path.join(root,'src-tauri/tauri.packaging-offline.conf.json')] : []),
    ...(mode === 'package-fixed-runtime' ? ['--bundles','nsis','--config',path.join(root,'src-tauri/tauri.packaging-fixed-runtime.conf.json')] : []),
    ...(mode === 'm3' ? ['--config',path.join(root,'src-tauri/tauri.m3.conf.json')] : []),
    ...(mode === 'build' ? ['--no-bundle'] : []), ...args];
} else throw Error('Unknown desktop command');
if (['package','package-only','package-offline','package-fixed-runtime'].includes(mode)) {
  if(args.some(value=>value.includes('save-chain-lab')||value.includes('worker-isolation-cli')))throw Error('Test-only features cannot be packaged');
  execFileSync(process.execPath, [path.join(root, 'scripts/check-writer-assets.mjs')], { cwd: root, env, stdio: 'inherit' });
  if(mode==='package-only')execFileSync(process.execPath,[path.join(root,'scripts/check-release-artifact.mjs')],{cwd:root,env,stdio:'inherit'});
}
const child = spawn(command, commandArgs, { cwd: root, env, stdio: 'inherit', windowsHide: true });
child.on('error', error => { console.error(error.message); process.exitCode = 1; });
child.on('exit', code => { process.exitCode = code ?? 1; });
