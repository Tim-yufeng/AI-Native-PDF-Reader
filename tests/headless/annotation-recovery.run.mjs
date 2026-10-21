// An isolated headless Edge process. Never connects to an existing browser/profile.
import { spawn,execFile } from 'node:child_process';import { mkdir,writeFile,readFile,readdir,lstat,realpath } from 'node:fs/promises';import path from 'node:path';import { fileURLToPath } from 'node:url';import { build } from 'vite';import { createServer } from 'node:http';import {randomUUID} from 'node:crypto';
const test='annotation-recovery',dpr=process.argv[2]??'1';if(test!=='annotation-recovery'||!['1','2'].includes(dpr))throw Error('Unknown isolated test');
if(test==='annotation-recovery'){if(test==='writer-menu')process.env.VITE_WRITER_CANDIDATE='1';else delete process.env.VITE_WRITER_CANDIDATE;}
const root=path.resolve(fileURLToPath(new URL('../../',import.meta.url))),work=path.join(root,test==='data-init'?'tests/work/m7-runtime':test==='m6'?'tests/work/m6':'tests/work/m6-ux');await mkdir(work,{recursive:true});
const profile=path.join(root,'.cache/headless-annotation',randomUUID());await mkdir(profile,{recursive:true});
process.env.TEMP=path.join(root,'.cache/tmp');process.env.TMP=process.env.TEMP;
await build({configFile:false,root,publicDir:false,cacheDir:path.join(root,'.cache/annotation-vite'),logLevel:'error',build:{outDir:path.join(root,'.cache/annotation-build'),emptyOutDir:false,target:'es2022',rollupOptions:{input:path.join(root,'tests/headless/annotation-recovery.html')}}});
const allowed=new Map();async function inventory(base,prefix,dir=base){for(const entry of await readdir(dir,{withFileTypes:true})){const p=path.join(dir,entry.name);const stat=await lstat(p);if(stat.isSymbolicLink()||(await realpath(p)).toLowerCase()!==p.toLowerCase())continue;if(stat.isDirectory())await inventory(base,prefix,p);else if(stat.isFile())allowed.set(prefix+path.relative(base,p).replaceAll('\\','/'),p);}}
await inventory(path.join(root,'.cache/annotation-build'),'/');await inventory(path.join(root,'public/pdfjs'),'/pdfjs/');
// The real Writer requests only these pinned project resources, read-only.
if(test==='annotation-recovery'){await inventory(path.join(root,'public/mupdf'),'/mupdf/');await inventory(path.join(root,'public/noto'),'/noto/');}
const nonce=randomUUID();let finished;const resultPromise=new Promise(resolve=>finished=resolve);
const served={html:0,scripts:0,wasm:0,font:0,bootstrap:0};
// Report failures before an HTML module's imports/top-level setup reach its own
// try/catch. This fixed script runs only in the disposable headless test page.
const earlyDiagnostics=`const send=(kind,error)=>fetch('/__test-result/${nonce}',{method:'POST',body:JSON.stringify({status:'FAIL',stage:'headless-bootstrap',kind,error:String(error).slice(0,4000)})});window.addEventListener('error',e=>send('error',e.error?.stack??e.message));window.addEventListener('unhandledrejection',e=>send('unhandledrejection',e.reason?.stack??e.reason));`;
const server=createServer(async(request,response)=>{if(request.method==='POST'&&request.url===`/__stage/${nonce}`){let stage='';for await(const chunk of request)stage+=chunk;console.log('stage: '+stage.slice(0,150));response.end('ok');return;}if(request.method==='POST'&&request.url===`/__test-result/${nonce}`){let raw='';for await(const chunk of request){raw+=chunk;if(raw.length>16384){response.writeHead(413).end();return;}}try{const result=JSON.parse(raw);if(!['PASS','FAIL'].includes(result.status))throw Error('invalid result');response.end('ok');finished(result);}catch{response.writeHead(400).end();}return;}
 if(test==='annotation-recovery'&&request.method==='GET'&&request.url==='/__headless-diagnostics.js'){served.bootstrap++;response.setHeader('Content-Type','text/javascript');response.end(earlyDiagnostics);return;}
 const file=allowed.get((request.url??'').split('?')[0]);if(request.method!=='GET'||!file){response.writeHead(404).end();return;}const ext=path.extname(file);if(ext==='.html')served.html++;if(['.js','.mjs'].includes(ext))served.scripts++;if(ext==='.wasm')served.wasm++;if(ext==='.ttf')served.font++;response.setHeader('Content-Security-Policy',"default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline'; img-src 'self' blob: data:; connect-src 'self'; worker-src 'self' blob:; font-src 'self' blob: data:; object-src 'none'; base-uri 'none'");response.setHeader('Content-Type',({'.html':'text/html','.js':'text/javascript','.mjs':'text/javascript','.css':'text/css','.wasm':'application/wasm'}[ext])??'application/octet-stream');try{let bytes=await readFile(file);if(test==='annotation-recovery'&&ext==='.html')bytes=bytes.toString().replace(/<!doctype html>/i,'<!doctype html><script src="/__headless-diagnostics.js"></script>');response.end(bytes);}catch{response.writeHead(404).end();}});
await new Promise((resolve,reject)=>{server.once('error',reject);server.listen(0,'127.0.0.1',resolve);});const port=server.address().port;
const env={...process.env,npm_config_cache:path.join(root,'.cache/npm'),TEMP:path.join(root,'.cache/tmp'),TMP:path.join(root,'.cache/tmp')};
const stop=child=>new Promise(resolve=>{if(child.exitCode!==null||!child.pid)return resolve();execFile('taskkill',['/PID',String(child.pid),'/T','/F'],{windowsHide:true},()=>resolve());});
let browser;
try{
 browser=spawn('C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',['--headless=new',`--user-data-dir=${profile}`,`--force-device-scale-factor=${dpr}`,'--window-size=1400,1200','--no-first-run','--no-default-browser-check','--disable-sync','--disable-extensions','--disable-background-networking',`http://127.0.0.1:${port}/tests/headless/${test}.html?nonce=${nonce}`],{env,windowsHide:true,stdio:['ignore','pipe','pipe']});
 browser.once('exit',code=>finished({status:'FAIL',error:'isolated browser exited before test report',browserExit:code,served}));
 browser.once('error',error=>finished({status:'FAIL',error:String(error),served}));
 let output='',errors='';browser.stdout.on('data',b=>{if(output.length<1e6)output+=b;});browser.stderr.on('data',b=>{if(errors.length<1e5)errors+=b;});
 let timeout;const result=await Promise.race([resultPromise,new Promise(resolve=>timeout=setTimeout(()=>resolve({status:'FAIL',error:'headless result timed out',served,browserExit:browser.exitCode}),180000))]);clearTimeout(timeout);
 await writeFile(path.join(work,`headless-${test}-dpr${dpr}.json`),JSON.stringify(result));console.log(JSON.stringify(result));
 if(result.status!=='PASS'){await writeFile(path.join(work,'headless-startup.txt'),errors);process.exitCode=1;}
}finally{if(browser)await stop(browser);await new Promise(resolve=>server.close(resolve));}
