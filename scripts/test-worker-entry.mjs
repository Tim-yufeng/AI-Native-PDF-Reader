// Ordinary-token tests use ONLY inert synthetic DLLs, never the real Worker.
import {spawn} from 'node:child_process';
import {mkdir,copyFile,writeFile} from 'node:fs/promises';
import path from 'node:path';
import assert from 'node:assert/strict';
const root=path.resolve(import.meta.dirname,'..'),built=path.join(root,'.cache/worker-entry');
const work=path.join(root,'tests/work/writer',`entry-${Date.now()}`);
await mkdir(work,{recursive:true});
const cases=['task','probe','version','args','script','old-rust-task','old-rust-probe','without-isolation','excess-args','crt-primary','crt-secondary','python-load','python-symbol'];
for(const mode of cases){
 const dir=path.join(work,`${mode} 中文 space`);await mkdir(dir);
 const exe=path.join(dir,'python.exe');await copyFile(path.join(built,'python.exe'),exe);
 for(const name of ['vcruntime140.dll','vcruntime140_1.dll','python312.dll']){
  if((mode==='crt-primary'&&name==='vcruntime140.dll')||(mode==='crt-secondary'&&name==='vcruntime140_1.dll')||(mode==='python-load'&&name==='python312.dll'))continue;
  await copyFile(path.join(built,mode==='python-symbol'&&name==='python312.dll'?'no-entry.dll':'stub.dll'),path.join(dir,name));
 }
 const args=mode==='version'?['-I','-V']:mode==='args'?['-c','NOT_EXECUTED']:['-I','-B','-X','utf8',path.join(dir,mode==='probe'?'worker/isolation.py':'worker/main.py')];
 if(mode==='script')args[5]=path.join(dir,'unapproved.py');
 // Preserve Rust's former mixed-separator spelling instead of normalizing it
 // through path.join. This reproduces the installed entry-args/87/125 failure.
 if(mode.startsWith('old-rust-'))args[5]=dir+'\\worker/'+(mode==='old-rust-probe'?'isolation.py':'main.py');
 if(mode==='without-isolation')args[0]='-E';
 if(mode==='excess-args')args.push('NOT_EXECUTED');
 const child=spawn(exe,args,{windowsHide:true,stdio:['ignore','pipe','pipe'],env:{SystemRoot:process.env.SystemRoot}});
 const out=[],err=[];let total=0;const budget=4096;
 const collect=target=>chunk=>{total+=chunk.length;if(total>budget)child.kill();else target.push(chunk);};
 child.stdout.on('data',collect(out));child.stderr.on('data',collect(err));
 let timer;const code=await Promise.race([new Promise((resolve,reject)=>{child.once('error',reject);child.once('close',resolve);}),new Promise((_,reject)=>{timer=setTimeout(()=>{child.kill();reject(Error('Synthetic entry timeout'));},3000);})]).finally(()=>clearTimeout(timer));
 const stdout=Buffer.concat(out).toString(),stderr=Buffer.concat(err).toString();assert.ok(total<=budget);
 if(['task','probe','version'].includes(mode)){assert.equal(code,17);assert.equal(stdout,'SYNTHETIC_ENTRY_ARGS_OK');assert.match(stderr,/AUPW_ENTRY_V1 entry-python-main 0/);}
 else{const stage=['script','old-rust-task','old-rust-probe','without-isolation','excess-args'].includes(mode)?'args':mode;assert.equal(code,125);assert.equal(stdout,'');assert.match(stderr,new RegExp(`AUPW_ENTRY_V1 entry-${stage} [1-9][0-9]*`));assert.ok(!stderr.includes(dir));}
}
const report={status:'PASS',checks:cases.length,scope:'synthetic inert DLLs only; no Python, PDF, isolation acceptance or system changes'};
await writeFile(path.join(work,'result.json'),JSON.stringify(report));console.log(JSON.stringify(report));
