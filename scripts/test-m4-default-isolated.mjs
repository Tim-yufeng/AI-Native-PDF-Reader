// Exercise the default-locator factory/restart using an injected project root.
// Does NOT claim a real C LocalAppData or Sandbox GUI persistence test.
import {spawn} from 'node:child_process';import {mkdir} from 'node:fs/promises';import path from 'node:path';
const root=path.resolve(import.meta.dirname,'..'),local=path.join(root,'tests/work/m4/default-location-regression');await mkdir(local,{recursive:true});
const child=spawn(process.execPath,[path.join(root,'scripts/desktop.mjs'),'test','--locked','--features','unicode-writer-candidate','--','--exact','storage::directories_tests::default_localappdata_persistence_across_actual_process_restart','--ignored','--nocapture'],{cwd:root,windowsHide:true,stdio:'inherit',env:{...process.env,LOCALAPPDATA:local,M4_DEFAULT_VALIDATION_RUN:'1'}});
child.once('error',()=>process.exitCode=1);child.once('exit',code=>process.exitCode=code??1);
