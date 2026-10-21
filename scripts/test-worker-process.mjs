// Lifecycle smoke tests of the packaged process with synthetic partial packets.
// Ordinary test token ONLY; these results do not prove LPAC or production cancel.
import {spawn} from 'node:child_process';import {writeFile} from 'node:fs/promises';import path from 'node:path';
const root=path.resolve(import.meta.dirname,'..'),results=[];
for(const mode of ['bad-protocol','truncated-packet','timeout','cancel']){
 const child=spawn(path.join(root,'.cache/python-worker/python.exe'),['-I','-B','-X','utf8',path.join(root,'.cache/python-worker/worker/main.py')],{cwd:root,windowsHide:true,stdio:['pipe','pipe','pipe'],env:{SystemRoot:process.env.SystemRoot,TEMP:path.join(root,'.cache/tmp'),TMP:path.join(root,'.cache/tmp')}});
 let stdout=0,stderr=0;const errors=[];child.stdout.on('data',b=>{stdout+=b.length;});child.stderr.on('data',b=>{stderr+=b.length;if(stderr<8192)errors.push(b);});child.stdin.on('error',()=>{});
 const closed=new Promise((resolve,reject)=>{child.once('close',(code,signal)=>resolve({code,signal}));child.once('error',reject);});
 const partial=Buffer.from([65,85,80,87,1,0,0,0]);let stopped=false;
 if(mode==='bad-protocol')child.stdin.end(Buffer.from('bad packet'));
 else if(mode==='truncated-packet')child.stdin.end(partial);
 else{child.stdin.write(partial);setTimeout(()=>{stopped=child.kill();},200);}
 let timer;try{
  const outcome=await Promise.race([closed,new Promise((_,reject)=>{timer=setTimeout(()=>{child.kill();reject(Error('Own test process did not close within budget'));},3000);})]);
  if(stdout!==0||stderr>8192||outcome.code===0||(['timeout','cancel'].includes(mode)&&!stopped))throw Error('Unexpected lifecycle result');
  if(!['timeout','cancel'].includes(mode)){const error=JSON.parse(Buffer.concat(errors));if(error.ok!==false||!['protocol','packet_truncated'].includes(error.error))throw Error('Unsafe error protocol');}
  results.push({mode,pass:true,stdoutBytes:stdout,processClosed:true,controlledStop:stopped});
 }finally{clearTimeout(timer);if(child.exitCode===null&&child.signalCode===null)child.kill();}
}
await writeFile(path.join(root,'tests/work/writer/process.json'),JSON.stringify({status:'PASS',scope:'ordinary-token synthetic CLI smoke only; not isolation/production IPC validation',results},null,2));console.log(JSON.stringify({status:'PASS',checks:results.length,scope:'packaged ordinary-token CLI only'}));
