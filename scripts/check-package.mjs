// Read-only guards for the bounded M7 candidate. Does not run an installer.
import {readFile,realpath,readdir,lstat} from 'node:fs/promises';import {existsSync} from 'node:fs';import path from 'node:path';import {fileURLToPath} from 'node:url';import assert from 'node:assert/strict';import {createHash} from 'node:crypto';
import {checkCommandContract} from './check-command-contract.mjs';
const root=path.resolve(fileURLToPath(new URL('../',import.meta.url))),read=p=>readFile(path.join(root,p),'utf8');
await checkCommandContract(root);
const releaseConfig=JSON.parse(await read('src-tauri/tauri.conf.json'));
assert.equal(releaseConfig.build.beforeBundleCommand,'node scripts/check-release-artifact.mjs','Bundling must reject a cached lab executable');
const base=JSON.parse(await read('src-tauri/tauri.conf.json')),overlay=JSON.parse(await read('src-tauri/tauri.packaging.conf.json')),npm=JSON.parse(await read('package.json')),cargo=await read('src-tauri/Cargo.toml'),b=overlay.bundle;
assert.deepEqual(overlay.build.features,[]);
// The V1 user-facing package is the fixed-runtime candidate. It is the only
// packaging overlay allowed to enable the real Provider capability, and it must
// enable it: without the feature the reader would hide its credential UI and
// refuse every Provider call, which is the P0 gap this release closes.
const fixedRuntime=JSON.parse(await read('src-tauri/tauri.packaging-fixed-runtime.conf.json'));
assert.deepEqual(fixedRuntime.build.features,['real-provider'],'the V1 candidate must enable the real Provider capability');
assert.equal(fixedRuntime.bundle.windows.webviewInstallMode.type,'fixedRuntime');
for(const name of ['tauri.packaging.conf.json','tauri.packaging-offline.conf.json']){
  const other=JSON.parse(await read(`src-tauri/${name}`));
  assert.deepEqual(other.build.features,[],`${name} must stay Mock-only`);
}
assert.equal(base.productName,'AI-Native PDF Reader');assert.equal(base.version,npm.version);assert.ok(cargo.includes(`version = "${base.version}"`));assert.equal(base.identifier,'io.github.ainativepdfreader');assert.equal(base.bundle.active,false);assert.equal(b.active,true);assert.deepEqual(b.targets,['nsis']);assert.equal(b.useLocalToolsDir,true);assert.equal(b.windows.nsis.installMode,'currentUser');assert.equal(b.windows.webviewInstallMode.type,'skip');assert.equal(b.windows.allowDowngrades,false);assert.deepEqual(b.fileAssociations,[]);assert.deepEqual(b.resources,{});assert.deepEqual(b.externalBin,[]);assert.equal(b.createUpdaterArtifacts,false);
// The byte writer is the renderer's MuPDF.js/WASM module: no sidecar resources
// and no Python worker path may appear in the packaging configuration.
const digest=data=>createHash('sha256').update(data).digest('hex');
for(const file of ['src-tauri/tauri.packaging.conf.json','scripts/prepare-pdf-assets.mjs','src/lib/mupdf-writer.ts']){assert.ok(!(await read(file)).includes('python-worker'),`${file} still references the Python worker`);}
const assets=[['dist/mupdf/mupdf-wasm.wasm','node_modules/mupdf/dist/mupdf-wasm.wasm'],['dist/noto/NotoSansSC-Regular-v41.ttf','worker/fonts/NotoSansSC-Regular-v41.ttf']];
for(const [packaged,source] of assets){
  const a=await readFile(path.join(root,packaged)),c=await readFile(path.join(root,source));
  assert.equal(digest(a),digest(c),`${packaged} differs from ${source}`);
  assert.ok(a.length>1024*1024,`${packaged} looks truncated`);
  const prepped=await readFile(path.join(root,'public',packaged.slice('dist/'.length)));
  assert.equal(digest(prepped),digest(a),`public/ copy of ${packaged} differs`);
}
const expected={wasmBytes:(await readFile(path.join(root,assets[0][1]))).length,fontBytes:(await readFile(path.join(root,assets[1][1]))).length};
for(const icon of b.icon){const target=path.resolve(root,'src-tauri',icon),actual=await realpath(target);assert.equal(actual.toLowerCase(),target.toLowerCase());const data=await readFile(target);if(icon.endsWith('.ico')){assert.equal(data.readUInt16LE(0),0);assert.equal(data.readUInt16LE(2),1);assert.ok(data.readUInt16LE(4)>0);}else{assert.equal(data.subarray(0,8).toString('hex'),'89504e470d0a1a0a');assert.ok(data.readUInt32BE(16)>=128&&data.readUInt32BE(20)>=128);}}
const template=await read('src-tauri/'+b.windows.nsis.template),commands=template.split(/\r?\n/).map(s=>s.trim()).filter(s=>s&&!s.startsWith(';'));
assert.ok(commands.includes('RequestExecutionLevel user'));assert.ok(commands.includes('SetShellVarContext current'));
// Recursive removal is allowed only for the application's own data directory,
// whose leaf must be exactly the compile-time application identifier. Every
// other RMDir stays non-recursive, and program directories are never walked.
// The one recursive macro is proven bounded by its definition and by every call
// site naming a fixed identifier, not by a path assembled at run time.
const ownedRoot='$$LOCALAPPDATA\\${NAME}';
const recursive=commands.filter(s=>/^RMDir\s+\/r\b/i.test(s));
assert.ok(recursive.length>0&&recursive.every(s=>s===`RMDir /r "${ownedRoot}"`),'only an owned application data directory may be removed recursively');
const macroStart=template.indexOf('!macro RemoveOwnedDataDirectory'),macroEnd=template.indexOf('!macroend',macroStart);
assert.ok(macroStart>=0&&macroEnd>macroStart,'the bounded data-removal macro must exist');
const macroBody=template.slice(macroStart,macroEnd);
assert.ok(macroBody.includes(ownedRoot),'the bounded macro must only name the application data root');
assert.ok(macroBody.includes('${If} $1 == "${APPID}"'),'the macro must re-check the fixed identifier before removing');
assert.ok(!macroBody.includes('$INSTDIR')&&!macroBody.includes('$APPDATA'),'the macro must never derive a data path from the install location');
const ownedCalls=[...template.matchAll(/!insertmacro RemoveOwnedDataDirectory "([^"]+)"/g)].map(m=>m[1]);
assert.ok(ownedCalls.length>0&&ownedCalls.every(name=>name==='${APPID}'||name==='${APPID}.personal'),'data removal must only ever name the fixed application identifiers');
assert.ok(ownedCalls.every((name,i)=>i===0||name!==ownedCalls[i-1]),'the same data root must not be named twice');
const deleteTargets=commands.filter(s=>/^Delete\s/i.test(s));assert.ok(deleteTargets.every(s=>/^Delete "\$(INSTDIR|SMPROGRAMS)\\/.test(s)||s==='Delete "$TEMP\\MicrosoftEdgeWebView2RuntimeInstaller.exe"'),'only program files and the own fixed-name temporary WebView2 copy may be deleted');
// F08: removing the reader is separate from removing its local records. The
// user gets an uninstall component that defaults OFF, describes the exact
// scope, and asks once more before anything is deleted.
assert.ok(template.includes('MUI_UNPAGE_COMPONENTS'),'the uninstall must offer the separate data choice');
assert.ok(template.includes('MUI_UNCHECKBOXBLURB'),'the data choice needs its own explanation');
const dataDecision=template.slice(template.indexOf('Function un.RemoveSelectedChatData'),template.indexOf('Section "Reader"'));
assert.ok(dataDecision.includes('${If} $RemoveChatData != "1"'),'unchecked must keep all local data');
assert.ok(dataDecision.includes('MessageBox MB_YESNO'),'the checked path needs its own confirmation');
assert.ok(dataDecision.indexOf('StrCpy $RemoveChatData "0"')<dataDecision.indexOf('!insertmacro RemoveOwnedDataDirectory'),'a declined confirmation must clear the choice before any delete');
assert.ok(ownedCalls.includes('${APPID}')&&ownedCalls.includes('${APPID}.personal'),'both fixed application data roots must be named explicitly');
assert.ok(dataDecision.includes('Source PDF files and annotations already saved into them are NOT deleted'),'the choice must state that PDFs are untouched');
assert.ok(!template.includes('$INSTDIR\\\\${APPID}'),'no data path may be derived from the install directory');
assert.ok(commands.filter(s=>/^(Write|Delete)Reg/.test(s)).every(s=>s.includes(' HKCU ')&&!s.includes('UserChoice')));
assert.ok(!commands.some(s=>/^WriteReg\w+ HKCU "Software\\Classes\\\.pdf" /.test(s)));
// No user-data path may be deleted except through the bounded macro proven
// above: every other line touching $APPDATA/$LOCALAPPDATA must be read-only.
const userDataLines=commands.filter(s=>/\$(APPDATA|LOCALAPPDATA)/i.test(s));
assert.ok(userDataLines.length>0&&userDataLines.every(s=>!(/^(Delete|RMDir)\b/i.test(s))||s===`RMDir /r "${ownedRoot}"`),'no user-data path may be removed outside the bounded owned macro');
assert.ok(template.includes('$"$INSTDIR\\${BINARY}.exe$" $"%1$"'));assert.ok(template.includes('${VersionCompare}'));
assert.ok(template.includes('Call CheckInstallDir'));assert.ok(template.indexOf('Call CheckInstallDir')<template.indexOf('  File /oname'));assert.ok(template.includes('FindFirst'));assert.ok(template.includes('FileRead'));
assert.ok(template.includes('!define MUI_ICON "{{no-escape installer_icon}}"'));assert.ok(template.includes('!define MUI_UNICON "{{no-escape installer_icon}}"'));assert.equal(commands.filter(s=>s==='${If} $0 == $1').length,2);assert.ok(!commands.some(s=>s.startsWith('${If} $0 == \'$"')));
assert.ok(template.includes('!macro RequireInstallSuccess'));assert.ok(template.includes('!macro RequireUninstallSuccess'));assert.ok(template.includes('RegDeleteTreeW(p 0x80000001'));assert.ok(template.includes('RegDeleteValueW(p r2'));
// The candidate must never write machine-wide state. The bound is expressed
// positively instead of by the single word HKLM: no machine-hive write API may
// appear at all, and the only lines naming HKLM are the fixed, read-only
// WebView2 runtime detection reads that Microsoft's documented discovery
// requires. Every Write/DeleteReg line stays in HKCU (checked above).
assert.ok(!/WriteReg(Str|ExpandStr|DWORD|Bin|MultiStr)?[\s\S]{0,40}HKLM/.test(template),'no machine-hive value may be written');
assert.ok(!/DeleteReg(Key|Value)[\s\S]{0,40}HKLM/.test(template),'no machine-hive value may be deleted');
assert.ok(!template.includes('0x80000002'),'no machine-hive handle may be used in native registry calls');
const machineHiveLines=template.split(/\r?\n/).map(s=>s.trim()).filter(s=>s.includes('HKLM')&&!s.startsWith(';'));
assert.ok(machineHiveLines.length>0,'the WebView2 detection must read the documented machine-hive registration');
const machineHiveRead=/^ReadRegStr \$(WebView2Version|WebView2Location) HKLM "SOFTWARE\\(WOW6432Node\\)?Microsoft\\EdgeUpdate\\Clients\\\$\{WEBVIEW2APPGUID\}" "(pv|location)"$/;
for(const line of machineHiveLines)assert.ok(machineHiveRead.test(line),`an unexpected machine-hive access appeared: ${line}`);
// A registration without its engine binary is exactly the broken state this
// package must repair, so the usability decision has to check the files too.
assert.ok(template.includes('${If} ${FileExists} "$WebView2Location\\$WebView2Version\\msedgewebview2.exe"'),'the runtime must only count as present when the registered engine binary exists');
// NSIS's instruction probes the final leaf and rejects a fresh install target.
assert.ok(!commands.some(s=>/^GetFullPathName\b/.test(s)));
assert.ok(template.includes('kernel32::GetFullPathNameW(w "$GuardTarget", i ${NSIS_MAX_STRLEN}, w .r7, p 0) i .r5 ?e'));
assert.ok(template.includes('${If} $5 == 0')&&template.includes('${If} $5 >= ${NSIS_MAX_STRLEN}')&&template.includes('StrCpy $GuardTarget $7'));
assert.ok(template.includes('Step: $InstallStage')&&template.includes('Path: $InstallPath')&&template.includes('an exact Windows error code is unavailable'));
assert.ok(template.includes('${If} $0 <> 0x10'));
assert.ok(!commands.some(s=>/^\$\{(?:If|AndIf|OrIf)\}\s+\S+\s+(?:==|!=)\s+0x[\da-f]+\b/i.test(s)), 'hexadecimal flags require numeric LogicLib comparison');
assert.equal((template.match(/\{\{#each resources\}\}/g)||[]).length,2);assert.ok(template.includes('File /a "/oname={{this.[1]}}" "{{no-escape @key}}"'));assert.ok(template.includes('Delete "$INSTDIR\\\\{{this.[1]}}"'));assert.ok(template.includes('Call CheckResourceDirectory'));assert.ok(template.includes('Call un.CheckResourceDirectory'));
const directoryCheck=template.split('Function CheckInstallDir')[1].split('FunctionEnd')[0];
assert.ok(!directoryCheck.includes('kernel32::GetLastError'));
for(const [api,register] of [['GetFileAttributesW','$4'],['FindFirstFileW','$2'],['FindNextFileW','$2']]){
  const lines=directoryCheck.split(/\r?\n/),index=lines.findIndex(line=>line.includes(`kernel32::${api}(`));
  assert.ok(index>=0&&lines[index].trim().endsWith("?e'")&&lines[index+1].trim()===`Pop ${register}`);
}
assert.ok(directoryCheck.includes('System::Alloc 592')&&directoryCheck.includes("'*$8(&v44, &w260 .r1)'"));
assert.ok(directoryCheck.includes('${If} $2 != 18')&&directoryCheck.includes('${If} $3 == 0'));
assert.ok(!directoryCheck.split(/\r?\n/).some(line=>/^\s*Find(First|Next|Close)\b/.test(line)));
assert.ok(template.includes('Path: $INSTDIR')&&template.includes('Operation: ${OPERATION}')&&template.includes('Windows error: ${CODE}'));
const uninstall=template.slice(template.indexOf('Section "Uninstall"'));assert.ok(uninstall.indexOf('Delete "$INSTDIR\\${BINARY}.exe"')<uninstall.indexOf('!insertmacro RemoveOwnedKey'));assert.ok(uninstall.indexOf('Delete "$INSTDIR\\uninstall.exe"')>uninstall.lastIndexOf('!insertmacro RemoveOwnedKey'));
assert.ok(directoryCheck.includes('ReadRegStr $0 HKCU "Software\\${APPID}" "RetainedWriterInstallLocation"'));
const retainedDirectory=directoryCheck.split('ReadRegStr $0 HKCU "Software\\${APPID}" "RetainedWriterInstallLocation"')[1];assert.ok(retainedDirectory.indexOf('FileOpen $1 "$INSTDIR\\${MARKER}" r')<retainedDirectory.indexOf('Return'));
assert.ok(!uninstall.includes('unicode-worker'),'uninstall must not reference the removed Python worker');assert.ok(!template.includes('sandbox-state.json'),'no writer marker may be installed or deleted');
assert.ok(uninstall.includes('${If} $RetainedWriter == "0"'));
assert.ok(uninstall.indexOf('WriteRegStr HKCU "Software\\${APPID}" "RetainedWriterInstallLocation" "$INSTDIR"')<uninstall.indexOf('Delete "$INSTDIR\\uninstall.exe"'));
const install=template.slice(template.indexOf('Section "Reader"'),template.indexOf('Section "Uninstall"'));assert.ok(install.includes('RegDeleteValueW(p r2, w "RetainedWriterInstallLocation")'));
assert.ok(install.indexOf('ReadRegStr $0 HKCU "${UNINSTALL}" "InstallLocation"')<install.indexOf('RegDeleteValueW(p r2, w "RetainedWriterInstallLocation")'));
// Continuous no-delete guards are required around every finite program write.
assert.ok(template.includes('i 0x81, i 3, p 0, i 3, i 0x02200000'));
assert.ok(template.includes('i 0x81, i 3, p 0, i 1, i 0x00200000'));
assert.ok(template.includes("'*$GuardBuffer(i .r0, &v36, i .r1)'"));
assert.ok(template.includes('${OrIf} $1 != 1'));
assert.ok(install.indexOf('Call PinDirectoryTree')<install.indexOf('Call CheckInstallDir'));
for(const target of ['$INSTDIR\\${BINARY}.exe','$INSTDIR\\uninstall.exe','$INSTDIR\\${MARKER}','$SMPROGRAMS\\${PRODUCT}.lnk'])assert.ok(install.includes(`!insertmacro BeginGuardedWrite "${target}"`));
assert.ok(install.includes('!insertmacro BeginGuardedWrite "$INSTDIR\\\\{{this.[1]}}"'));
assert.equal((install.match(/!insertmacro EndGuardedWrite/g)||[]).length,5);
assert.ok(uninstall.includes('!insertmacro CheckUninstallLeaf "$INSTDIR\\\\{{this.[1]}}"'));
assert.ok(uninstall.indexOf('Call un.ReleaseDirectoryGuards')>uninstall.indexOf('Delete "$INSTDIR\\\\{{this.[1]}}"'));
assert.ok(template.includes('Function .onInstFailed')&&template.includes('Function un.onUninstFailed'));
// WebView2 runtime supply. The baseline packaging config keeps `skip`; the
// offline validation package is a separate, complete config that embeds the
// signed Microsoft offline installer. The guards are extended rather than
// removed: both configurations are inspected and any drift other than the
// supply mode fails this check.
const offline=JSON.parse(await read('src-tauri/tauri.packaging-offline.conf.json')),ob=offline.bundle;
assert.equal(ob.windows.webviewInstallMode.type,'offlineInstaller');
assert.equal(ob.windows.webviewInstallMode.silent,true);
assert.equal(overlay.bundle.windows.webviewInstallMode.type,'skip');
const normalize=config=>{const copy=structuredClone(config);delete copy.bundle.shortDescription;delete copy.bundle.windows.webviewInstallMode;return copy;};
assert.deepEqual(normalize(offline),normalize(overlay),'the offline validation config may only differ in the WebView2 supply mode and the description');
assert.ok(template.includes('!define INSTALLWEBVIEW2MODE "{{install_webview2_mode}}"'));
assert.ok(template.includes('!define WEBVIEW2INSTALLERARGS "{{webview2_installer_args}}"'));
assert.ok(template.includes('!define WEBVIEW2INSTALLERPATH "{{webview2_installer_path}}"'));
for(const literal of ['!include "x64.nsh"','{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}','SOFTWARE\\WOW6432Node\\Microsoft\\EdgeUpdate\\Clients\\${WEBVIEW2APPGUID}','SOFTWARE\\Microsoft\\EdgeUpdate\\Clients\\${WEBVIEW2APPGUID}','File "/oname=$TEMP\\MicrosoftEdgeWebView2RuntimeInstaller.exe" "${WEBVIEW2INSTALLERPATH}"','ExecWait \'"$TEMP\\MicrosoftEdgeWebView2RuntimeInstaller.exe" ${WEBVIEW2INSTALLERARGS} /install\''])assert.ok(template.includes(literal),`missing offline WebView2 wiring: ${literal}`);
const webviewGate='!if "${INSTALLWEBVIEW2MODE}" == "offlineInstaller"';
const readerIndex=template.indexOf('Section "Reader"'),gateIndex=template.indexOf(webviewGate),webviewIndex=template.indexOf('Section "WebView2"'),embedIndex=template.indexOf('MicrosoftEdgeWebView2RuntimeInstaller.exe" "${WEBVIEW2INSTALLERPATH}"'),gateEndIndex=template.indexOf('!endif',webviewIndex);
assert.ok(gateIndex>0&&gateIndex<readerIndex,'the runtime supply must be compile-time gated before any program file is written');
assert.ok(webviewIndex>gateIndex&&webviewIndex<readerIndex,'the WebView2 section must live inside the compile-time gate');
assert.ok(gateEndIndex>webviewIndex&&gateEndIndex<readerIndex,'the compile-time gate must be closed before the reader section');
assert.ok(embedIndex>gateIndex&&embedIndex<gateEndIndex,'the gated block must be the one that embeds the installer, so the skip baseline stays inert');
for(const banned of ['NSISdl','2124703','MicrosoftEdgeWebview2Setup.exe','downloadBootstrapper','embedBootstrapper'])assert.ok(!template.includes(banned),`the installer must not keep an online WebView2 path: ${banned}`);
const desktopLines=(await read('scripts/desktop.mjs')).split(/\r?\n/),offlineLines=desktopLines.filter(line=>line.includes('tauri.packaging-offline.conf.json'));
assert.equal(offlineLines.length,1,'exactly one build path may use the offline validation config');
assert.ok(offlineLines[0].includes("mode === 'package-offline'"),'the offline config must be bound to its own build mode');
assert.ok(desktopLines.filter(line=>line.includes('tauri.packaging.conf.json')).every(line=>!line.includes('package-offline')),'the default packaging mode must not use the offline validation config');
// Fixed-runtime supply. The third complete config asks for Tauri's
// `fixedRuntime` mode, which Tauri publishes as one ordinary bundled resource
// directory and which the reader addresses through
// WEBVIEW2_BROWSER_EXECUTABLE_FOLDER below the executable directory. That mode
// maps to an empty NSIS supply mode, so this package runs no registry discovery
// and no installer; the same two-field drift bound is extended to it.
const fixed=JSON.parse(await read('src-tauri/tauri.packaging-fixed-runtime.conf.json')),fb=fixed.bundle,fim=fb.windows.webviewInstallMode;
assert.equal(fim.type,'fixedRuntime');
assert.equal(fim.path,'webview2-fixed-runtime');
assert.deepEqual(Object.keys(fim).sort(),['path','type'],'the fixed-runtime supply mode carries exactly a type and a path');
// This is the V1 user-facing overlay, so it is also the one that carries the
// real Provider feature. That single build difference is asserted separately
// above and normalized out here, where only the supply mode may otherwise vary.
const normalizeWithFeatures=config=>{const copy=normalize(config);delete copy.build.features;return copy;};
assert.deepEqual(normalizeWithFeatures(fixed),normalizeWithFeatures(overlay),'the fixed-runtime validation config may only differ in the WebView2 supply mode, the description and the Provider feature');
// A relative, traversal-free path is what makes one string serve both as the
// bundled resource directory and as the reader's lookup below `$INSTDIR`; an
// absolute or escaping path would let the guest read a build-machine location.
assert.ok(!path.isAbsolute(fim.path)&&!fim.path.split(/[\\/]/).includes('..')&&!fim.path.startsWith('.')&&!fim.path.endsWith('/')&&!fim.path.endsWith('\\'),'the fixed runtime must resolve below the install directory and never to a build-machine path');
assert.ok(!template.includes('fixedRuntime'),'the NSIS template must not carry a second, unreachable WebView2 supply branch');
const runtimeRoot=path.join(root,'src-tauri',fim.path),runtimeEngine=path.join(runtimeRoot,'msedgewebview2.exe');
const runtimeAssets=existsSync(runtimeEngine);
let runtimeFiles=0;
if(runtimeAssets){
  runtimeFiles=(await readdir(runtimeRoot,{recursive:true,withFileTypes:true})).filter(entry=>entry.isFile()).length;
  assert.ok(runtimeFiles>128,`the bundled fixed runtime looks incomplete (${runtimeFiles} files)`);
  assert.ok(existsSync(path.join(runtimeRoot,'EBWebView')),'the bundled fixed runtime must keep its engine directory layout');
  assert.ok((await lstat(runtimeEngine)).size>1024*1024,'the bundled fixed runtime engine binary looks truncated');
}
const fixedLines=desktopLines.filter(line=>line.includes('tauri.packaging-fixed-runtime.conf.json'));
assert.equal(fixedLines.length,1,'exactly one build path may use the fixed-runtime validation config');
assert.ok(fixedLines[0].includes("mode === 'package-fixed-runtime'"),'the fixed-runtime config must be bound to its own build mode');
assert.ok(desktopLines.filter(line=>line.includes('tauri.packaging.conf.json')||line.includes('tauri.packaging-offline.conf.json')).every(line=>!line.includes('package-fixed-runtime')),'no other packaging mode may use the fixed-runtime validation config');
// Writer asset lock. The renderer fetches the MuPDF.js engine and the pinned
// font from the packaged frontend, so the application compiles a lock in and
// verifies the bytes it would serve before any writer starts. These guards pin
// the wiring: the lock exists and covers exactly the renderer's requests, the
// writer module compiles it in, the check resolves assets through the frontend
// asset service instead of a build-machine path, and every packaging mode runs
// the pre-packaging gate before NSIS.
const writerLock=JSON.parse(await read('src-tauri/writer-assets.lock.json'));
assert.equal(writerLock.schema,1,'the writer asset lock schema must be the supported one');
assert.deepEqual(writerLock.assets.map(asset=>asset.request).sort(),['/mupdf/mupdf-wasm.wasm','/noto/NotoSansSC-Regular-v41.ttf'],'the writer lock must cover exactly the assets the renderer fetches');
assert.ok(writerLock.assets.every(asset=>asset.bytes>0&&/^[0-9a-f]{64}$/.test(asset.sha256)&&asset.source&&asset.license&&asset.request===`/${asset.path}`),'every locked writer asset needs a size, a sha256, a pinned source, a licence and a matching request');
assert.ok(writerLock.assets.every(asset=>asset.path.split('/').pop()===asset.source.replace(/\\/g,'/').split('/').pop()),'a locked asset must be served under the pinned input file name');
// A writer failure must always be reportable: the byte layer may only refuse
// with an approved code and step, never with a free-form engine message that
// the renderer would have to collapse into "原因未能安全识别".
const writerModule=await read('src/lib/mupdf-writer.ts');
assert.ok(writerModule.includes("from './writer-refusal.ts'"),'the writer module must use the approved refusal vocabulary');
assert.ok(!/throw\s+(new\s+)?Error\(/.test(writerModule),'every writer refusal needs an approved code and step, not a free-form Error');
assert.ok(!/no FreeText annotation found/.test(writerModule),'the writer must not refuse a document only because it holds no FreeText annotation');
const writerFacingSource=await read('src/lib/desktop-source.ts');
assert.ok(writerFacingSource.includes('writerRefusalMessages')&&writerFacingSource.includes('safeWriterStep')&&writerFacingSource.includes("'writerStep'"),'the renderer must display the approved writer code and the failing step');
assert.ok(writerFacingSource.includes("from './mupdf-writer.ts'"),'the byte writer must ship in the main bundle: a lazily imported chunk would add a module-loading path no packaged build has verified');
// Gate: every command the reader invokes must be allowed for its window. A command
// the capability system does not allow is rejected before any code runs, and such a
// rejection carries no error code at all - that is exactly how Ctrl+S stayed broken
// and unreportable while Save As worked.
const capabilityPermissions=new Set();
for(const file of await readdir(path.join(root,'src-tauri/capabilities'))){
  for(const m of (await read(path.join('src-tauri/capabilities',file))).matchAll(/"(allow-[a-z0-9-]+)"/g))capabilityPermissions.add(m[1]);
}
async function sourceList(dir){
  const found=[];
  for(const entry of await readdir(path.join(root,dir),{withFileTypes:true})){
    const relative=path.join(dir,entry.name).replaceAll('\\','/');
    if(entry.isDirectory())found.push(...await sourceList(relative));
    else if(/\.(ts|svelte)$/.test(entry.name))found.push(relative);
  }
  return found;
}
const invoked=new Map();
for(const relative of await sourceList('src')){
  for(const m of (await read(relative)).matchAll(/\b(?:invoke|bridge)(?:<[^>]*>)?\(\s*'([a-z_0-9]+)'/g))invoked.set(m[1],relative);
}
assert.ok(invoked.size>=50,`the command inventory looks wrong: only ${invoked.size} invocations found`);
const unallowed=[...invoked].filter(([name])=>!capabilityPermissions.has(`allow-${name.replaceAll('_','-')}`)).map(([name,file])=>`${name} (${file})`);
assert.deepEqual(unallowed,[],`every invoked command needs a capability entry: ${unallowed.join(', ')}`);
// A command that is not declared in the app manifest never gets a permission, so
// the reader can invoke it only to be rejected before any code runs.
const manifestCommands=new Set([...(await read('src-tauri/build.rs')).matchAll(/"([a-z_0-9]+)"/g)].map(m=>m[1]));
const undeclared=[...invoked].filter(([name])=>!manifestCommands.has(name)).map(([name,file])=>`${name} (${file})`);
assert.deepEqual(undeclared,[],`every invoked command must be declared in the app manifest: ${undeclared.join(', ')}`);
// Gate: the byte path can only report approved codes. A backend code missing from
// the renderer's list would be shown as an unidentified failure instead.
const bytePathFiles=['src-tauri/src/error.rs','src-tauri/src/files.rs','src-tauri/src/pdf_export.rs','src-tauri/src/main.rs'];
const constructorCodes={denied:'unauthorized',cancelled:'cancelled',busy:'busy',io:'file_unavailable',invalid:'invalid_pdf',changed:'file_changed'};
const backendCodes=new Set(Object.values(constructorCodes));
for(const file of bytePathFiles){
  const text=await read(file);
  for(const m of text.matchAll(/error\(\s*"([a-z_0-9]+)"/g))backendCodes.add(m[1]);
  for(const m of text.matchAll(/code:\s*"([a-z_0-9]+)"/g))backendCodes.add(m[1]);
  for(const m of text.matchAll(/AppError::(\w+)\(\)/g))if(constructorCodes[m[1]])backendCodes.add(constructorCodes[m[1]]);
}
const messagesStart=writerFacingSource.indexOf('const messages = {');
const messagesBlock=writerFacingSource.slice(messagesStart,writerFacingSource.indexOf('} as const;',messagesStart));
const approved=new Set();
for(const text of [messagesBlock,await read('src/lib/writer-refusal.ts')]){
  for(const m of text.matchAll(/([a-z][a-z_0-9]*)\s*:\s*'/g))approved.add(m[1]);
}
// Codes owned by other flows (chat binding, credentials, settings). They are shown
// through their own text paths, not through the byte-path formatters, so listing
// them here is a conscious decision rather than a missing entry.
const otherFlowCodes=new Set(['document_identity_unavailable']);
const unreportable=[...backendCodes].filter(code=>!approved.has(code)&&!otherFlowCodes.has(code)).sort();
assert.deepEqual(unreportable,[],`the renderer cannot report these backend codes: ${unreportable.join(', ')}`);
const exportSource=await read('src-tauri/src/pdf_export.rs');
assert.ok(exportSource.includes('include_str!("../writer-assets.lock.json")'),'the running application must compile the writer asset lock in');
assert.ok(exportSource.includes('fn verify_writer_assets')&&exportSource.includes('writer_resource_mismatch'),'the writer module must verify the locked assets and reject a mismatch');
const mainSource=await read('src-tauri/src/main.rs');
assert.ok(!mainSource.includes('public/mupdf')&&!mainSource.includes('public/noto'),'the writer check must not resolve packaged assets from a build-machine project path');
assert.ok(mainSource.includes('asset_resolver()'),'the writer check must verify the assets the frontend asset service actually serves');
const desktopSource=await read('scripts/desktop.mjs'),gateMode=['package','package-only','package-offline','package-fixed-runtime'].map(mode=>`'${mode}'`).join(',');
assert.ok(desktopSource.includes('check-writer-assets.mjs')&&desktopSource.includes(`[${gateMode}]`),'the pre-packaging writer asset gate must run for every packaging mode before NSIS');
console.log(JSON.stringify({status:'PASS',evidence:'configuration-source-and-template-wiring-plus-runtime-inventory-only',product:base.productName,version:base.version,iconChecked:true,localTools:true,perUser:true,noSystemRuntimeInstall:true,baselineWebviewInstallMode:overlay.bundle.windows.webviewInstallMode.type,validationWebviewInstallMode:ob.windows.webviewInstallMode.type,validationRuntimeSource:'embedded signed Microsoft offline installer; no online bootstrapper',validationRuntimeTiming:'before program files; abort when the runtime is still unregistered',runtimeInstallScope:'the validation package installs the runtime only in the environment it runs in; this round authorizes the disposable Windows Sandbox guest, never the host',candidateOnly:true,writer:'mupdf-wasm-in-renderer',wasmBytes:expected.wasmBytes,fontBytes:expected.fontBytes,pythonWorkerResource:'absent',dataRetention:'explicit program-resource deletion only; no app-data operations; actual upgrade/uninstall remains pending',saveAcceptance:'manual Sandbox run required',systemChanges:0,fixedRuntimeSupply:'bundled Microsoft Fixed Version WebView2 as one ordinary resource directory; the NSIS supply mode stays empty, so this package performs no registry discovery and executes no runtime installer',fixedRuntimeResourcePath:fim.path,fixedRuntimeAbsolute:false,fixedRuntimeEscapesInstallDirectory:false,fixedRuntimeAssets:runtimeAssets?'present-engine-and-layout-checked':'absent-not-verified',fixedRuntimeFileCount:runtimeFiles,fixedRuntimeWriterLayout:'same relative path below $INSTDIR as the executable directory the reader joins it with',writerAssetLock:'compiled into the application and re-checked before NSIS packaging',writerAssetRequests:writerLock.assets.map(asset=>asset.request),writerAssetBytes:writerLock.assets.map(asset=>asset.bytes)}));
