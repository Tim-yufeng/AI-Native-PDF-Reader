// Build a project-only page. The production dist and the installer are untouched.
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { writeFile,readFile,mkdir } from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {saveInputsFingerprint} from './check-release-artifact.mjs';
import {saveLabInstrumentation} from './save-lab-instrumentation.mjs';
const root = path.resolve(fileURLToPath(new URL('../', import.meta.url)));
const app = process.argv.includes('--app');
const ipc=process.argv.includes('--ipc');
if(app&&ipc)throw Error('Choose the App loop or fixed IPC replay');
const evidence=process.argv.includes('--evidence');
if(evidence&&!app)throw Error('Save evidence requires the isolated App');
if(app)process.env.VITE_WRITER_CANDIDATE='1'; // Match the candidate's production build.
await build({
  configFile: false, root, publicDir: false,
  define:{__SAVE_EVIDENCE__:JSON.stringify(evidence)},
  plugins: app ? [...(evidence?[saveLabInstrumentation()]:[]),svelte(), ...(process.argv.includes('--diagnostic') ? [{
    name: 'synthetic-original-admission-diagnostic',
    enforce: 'pre',
    // Diagnostic is compiled ONLY into this isolated page, never dist/NSIS.
    transform(code, id) {
      if (!id.replaceAll('\\', '/').endsWith('/src/lib/original-output.ts')) return;
      const marker = 'if(!sameSavedAnnotations(annotations(xa),annotations(ya)))';
      if (!code.includes(marker)) throw Error('Admission diagnostic target changed');
      return code.replace(marker, 'globalThis.__SAVE_APP_DIAGNOSTIC__={before:annotations(xa),after:annotations(ya)};'+marker);
    },
  }] : [])] : [],
  cacheDir: path.join(root, '.cache/save-chain-vite'),
  build: { target: 'es2022', outDir: path.join(root, '.cache/save-chain-web'), emptyOutDir: false,
    assetsDir: '__save_chain_lab', sourcemap: false,
    rollupOptions: { input: path.join(root, `tests/headless/${ipc?'save-ipc':app?'save-app':'save-chain'}.html`) } },
});
if(app)await writeFile(path.join(root,'.cache/save-app-build.json'),JSON.stringify({fingerprint:await saveInputsFingerprint(root),diagnostic:process.argv.includes('--diagnostic'),evidence}));
if(ipc){
 const baseline='tests/work/save-interop-performance/run-9ecaa314-70e3-4781-a612-e382cb5235ec/timings.json',report=JSON.parse(await readFile(path.join(root,baseline),'utf8'));
 const old=report.rounds.find(r=>r.round===9),last=report.rounds.find(r=>r.round===10);
 const dest=path.join(root,'.cache/save-chain-web/__save_chain_lab');await mkdir(dest,{recursive:true});
 for(const [name,relative]of [['ipc-old.pdf',old.output],['ipc-candidate.pdf',last.output.replace('-output.pdf','-candidate.pdf')],['ipc-output.pdf',last.output]]){
  const source=path.resolve(root,relative),allowed=path.join(root,'tests/work/save-chain-release/run-9ecaa314-70e3-4781-a612-e382cb5235ec');if(path.dirname(source).toLowerCase()!==allowed.toLowerCase())throw Error('Unexpected replay fixture');
  const bytes=await readFile(source);if(bytes.length>20*1024*1024)throw Error('Oversized replay fixture');
  if(name!=='ipc-candidate.pdf'&&createHash('sha256').update(bytes).digest('hex')!==(name==='ipc-old.pdf'?'080e96270e2fc153fddca2ec7f4fda201f6f5d6e79a42f5d65d5630731e5047a':'a4c3c7fcee7f0c65cd0d4bc15821f224cd40151fff8158778d403b086268538e'))throw Error('Replay fixture hash differs');
  await writeFile(path.join(dest,name),bytes);
 }
 await writeFile(path.join(root,'.cache/save-ipc-build.json'),JSON.stringify({fingerprint:await saveInputsFingerprint(root)}));
}
