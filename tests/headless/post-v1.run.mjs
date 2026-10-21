// Disposable headless profile and synthetic files only. No desktop automation,
// production data, credential access or remote Provider calls.
import {spawn,execFile} from 'node:child_process';
import {mkdir,readFile,writeFile,readdir,lstat,realpath} from 'node:fs/promises';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {randomUUID} from 'node:crypto';
import {createServer} from 'node:http';
import {build} from 'vite';
import {svelte} from '@sveltejs/vite-plugin-svelte';
const requested=process.argv[2],test=requested==='workspace-independent'?'workspace-mode':requested,dpr=process.argv[3]??'1';
if(!['welcome-drop','font-appearance','text-autogrow','workspace-mode','full-context','classroom','settings'].includes(test)||!['1','2'].includes(dpr))throw Error('Unknown isolated test');
const root=path.resolve(fileURLToPath(new URL('../../',import.meta.url))),work=path.join(root,'tests/work/post-v1');
const out=path.join(root,'.cache/post-v1-build'),profile=path.join(root,'.cache/post-v1-headless',randomUUID()),temp=path.join(root,'.cache/tmp');
await Promise.all([mkdir(work,{recursive:true}),mkdir(profile,{recursive:true}),mkdir(temp,{recursive:true})]);
process.env.TEMP=temp;process.env.TMP=temp;
await build({configFile:false,root,publicDir:false,plugins:[svelte()],cacheDir:path.join(root,'.cache/post-v1-vite'),logLevel:'error',build:{outDir:out,emptyOutDir:false,target:'es2022',rollupOptions:{input:path.join(root,`tests/headless/${test}.html`)}}});
const allowed=new Map();
async function inventory(base,prefix,dir=base){for(const entry of await readdir(dir,{withFileTypes:true})){const p=path.join(dir,entry.name),stat=await lstat(p);if(stat.isSymbolicLink()||(await realpath(p)).toLowerCase()!==p.toLowerCase())continue;if(stat.isDirectory())await inventory(base,prefix,p);else if(stat.isFile())allowed.set(prefix+path.relative(base,p).replaceAll('\\','/'),p);}}
await inventory(out,'/');await inventory(path.join(root,'public/pdfjs'),'/pdfjs/');
await inventory(path.join(root,'public/noto'),'/noto/');
if(['font-appearance','text-autogrow','classroom','workspace-mode'].includes(test))await inventory(path.join(root,'public/mupdf'),'/mupdf/');
const nonce=randomUUID();let finished;
const resultPromise=new Promise(resolve=>finished=resolve);
const artifacts=new Set(['font-appearance.png','font-appearance.pdf']);
const server=createServer(async(request,response)=>{
 if(request.method==='POST'&&request.url===`/__test-result/${nonce}`){let raw='';for await(const chunk of request){raw+=chunk;if(raw.length>32768){response.writeHead(413).end();return;}}try{const result=JSON.parse(raw);if(!['PASS','FAIL'].includes(result.status))throw Error('invalid result');response.end('ok');finished(result);}catch{response.writeHead(400).end();}return;}
 if(test==='font-appearance'&&request.method==='POST'&&request.url?.startsWith(`/__artifact/${nonce}/`)){const name=request.url.slice(`/__artifact/${nonce}/`.length);if(!artifacts.has(name)){response.writeHead(404).end();return;}const chunks=[];let size=0;for await(const chunk of request){size+=chunk.length;if(size>16*1024*1024){response.writeHead(413).end();return;}chunks.push(chunk);}await writeFile(path.join(work,name),Buffer.concat(chunks));response.end('ok');return;}
 const file=allowed.get((request.url??'').split('?')[0]);if(request.method!=='GET'||!file){response.writeHead(404).end();return;}
 response.setHeader('Content-Security-Policy',"default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline'; img-src 'self' blob: data:; connect-src 'self'; worker-src 'self' blob:; font-src 'self' blob: data:; object-src 'none'; base-uri 'none'");
 response.setHeader('Content-Type',({'.html':'text/html','.js':'text/javascript','.mjs':'text/javascript','.css':'text/css','.wasm':'application/wasm'}[path.extname(file)])??'application/octet-stream');
 try{response.end(await readFile(file));}catch{response.writeHead(404).end();}
});
await new Promise((resolve,reject)=>{server.once('error',reject);server.listen(0,'127.0.0.1',resolve);});
let browser;
try{
 browser=spawn('C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',['--headless=new',`--user-data-dir=${profile}`,`--force-device-scale-factor=${dpr}`,'--window-size=1400,1100','--no-first-run','--no-default-browser-check','--disable-sync','--disable-extensions','--disable-background-networking',`http://127.0.0.1:${server.address().port}/tests/headless/${test}.html?nonce=${nonce}${requested==='workspace-independent'?'&independent=1':''}`],{windowsHide:true,env:{...process.env,TEMP:temp,TMP:temp},stdio:['ignore','ignore','pipe']});
 let errors='';browser.stderr.on('data',b=>{if(errors.length<100000)errors+=b;});browser.once('error',e=>finished({status:'FAIL',error:String(e)}));browser.once('exit',code=>finished({status:'FAIL',error:'headless browser exited',code}));
 let timeout;const result=await Promise.race([resultPromise,new Promise(resolve=>timeout=setTimeout(()=>resolve({status:'FAIL',error:'headless result timed out'}),90000))]);clearTimeout(timeout);
 await writeFile(path.join(work,`${requested}-dpr${dpr}.json`),JSON.stringify(result,null,2));console.log(JSON.stringify(result));
 if(result.status!=='PASS'){await writeFile(path.join(work,`${test}-startup.txt`),errors);process.exitCode=1;}
}finally{if(browser?.pid&&browser.exitCode===null)await new Promise(resolve=>execFile('taskkill',['/PID',String(browser.pid),'/T','/F'],{windowsHide:true},()=>resolve()));await new Promise(resolve=>server.close(resolve));}
