/** Drain native-only launch requests after the reader is ready, one at a time. */
export function launchPoll<T>(options:{take:()=>Promise<T>;accept:(batch:T,active:()=>boolean)=>Promise<boolean>;release:(batch:T)=>Promise<void>;error:()=>void;interval?:number}){
 let stopped=false,busy=false;const timer=setInterval(()=>void run(),options.interval??500);
 async function run(){if(stopped||busy)return;busy=true;let batch:T|undefined,owned=false;try{batch=await options.take();owned=true;if(!stopped&&await options.accept(batch,()=>!stopped))owned=false;}catch{if(!stopped)options.error();}finally{if(owned)await options.release(batch as T).catch(()=>{});busy=false;}}
 void run();return()=>{stopped=true;clearInterval(timer);};
}
