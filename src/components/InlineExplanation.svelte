<script lang="ts">
  import { onMount, onDestroy, tick } from 'svelte';
  import type { PdfReader } from '../lib/pdf-reader';
  import { pageImage, type Region } from '../lib/ai-context';
  import { indexInlineContext } from '../lib/inline-context';
  import { LocalData, type DocumentBinding } from '../lib/local-data';
  import { aiClient, aiError, availableModels, type AiEvent, type AiSlot } from '../lib/ai-client';
  import { readSlot, writeSlot } from '../lib/ai-slot';
  import { preferences, defaultContext } from '../lib/preferences';
  import { InlineSession, dragBox, boundedPanel } from '../lib/inline-session';
  import { placeInline } from '../lib/inline-position';
  import Icon from './Icon.svelte';
  import {enterSends} from '../lib/chat-input';
  // defaultQuick is the one-click explanation path; it needs no manual prompt and
  // no separately drawn output box. The custom-prompt path stays identical.
  const defaultQuick='请用一到两句解释所选内容的含义与关键条件，公式用 LaTeX。';
  let { reader, binding, container, token, source, box, changed, place, close, busyChanged, placing=false, quick=false, totalPages=1 }:{
    reader:PdfReader;binding:DocumentBinding;container:HTMLElement;token:number;source:Region|null;box:Region;
    changed:(box:Region)=>void;place:()=>void;close:()=>void;busyChanged:(busy:boolean)=>void;placing?:boolean;quick?:boolean;totalPages?:number;
  }=$props();
  // quick is a per-mount mode: the parent keys this component per session, so
  // only the initial choice applies. binding/token are likewise fixed for this
  // mount because App.svelte re-keys the component whenever they change.
  const quickMode=quick;
  let prompt=$state(quickMode?defaultQuick:''),text=$state(''),phase=$state('idle'),error=$state(''),details=$state(!quickMode),status=$state('');
  let promptInput=$state<HTMLTextAreaElement>();let composing=false,compositionEnded=-Infinity;
  const fontSize=12,session=new InlineSession();let alive=true,abort:AbortController|null=null,drag:AbortController|null=null;
  let answer:HTMLDivElement;let output=$state<HTMLDivElement>();
  let request=$state<string|null>(null),answerId=$state<string|null>(null);
  // Same per-request choice as the side chat, remembered separately so one
  // surface cannot silently change what the other sends.
  let sendSlot=$state<AiSlot>(readSlot('inline')),models=$state(availableModels(null));
  // The soft budget for this answer, from the reader settings (0 = unlimited).
  let answerLimit=$state(0),overBudget=$state(false),coverageNotice=$state('');
  let fullDocument=true;
  const chosen=$derived(models.find(m=>m.slot===sendSlot)??models[0]);
  function pickSlot(value:AiSlot){sendSlot=value;writeSlot('inline',value);}
  const localData=new LocalData();
  const indexKey=`${binding.bindingId}:${token}`,indexed=new Set<string>();
  const hasOverflow=()=>output&&(output.scrollHeight>output.clientHeight+1.5||output.scrollWidth>output.clientWidth+1.5);
  // Non-empty tail of the streamed text, never truncated: a shortened answer
  // could silently drop a necessary mathematical condition. Overflow is shown
  // and the user may enlarge the box or confirm as is.
  // The inline turn is committed to the local conversation so a failed or
  // cancelled generation stays inspectable. Cancelling removes only this one
  // turn, so an unconfirmed temporary explanation is never kept as a draft.
  async function tipOf(id:string){try{const view=await localData.chatView(id);return view.path.filter((r:{id:string;deleted:boolean;role:string;status:string})=>!r.deleted&&r.role==='assistant'&&r.status==='complete').at(-1)?.id??null;}catch{return null;}}
  async function ensureIndex(id:string,snapshot:number,signal:AbortSignal){
    const context=(await preferences.get()).context??defaultContext();answerLimit=context.inlineAnswerLimit??0;
    fullDocument=context.inlineFullDocument!==false;
    const key=`${indexKey}:${JSON.stringify(context)}:${box.page}:${source?.page??''}`;
    if(indexed.has(key))return;
    status='正在建立本地索引…';
    await indexInlineContext(reader,totalPages,box.page,source?.page,context,signal,
      batch=>aiClient.index(id,batch),n=>{status=fullDocument?`本地索引 ${n}/${totalPages} 页`:`本地索引第 ${n} 页`;});
    if(alive&&binding.bindingId===id&&token===snapshot)indexed.add(key);
    status='';
  }
  async function generate(){
    if(!prompt.trim()||phase==='preparing'||phase==='confirming')return;
    abort?.abort();const controller=abort=new AbortController(),generation=session.begin(),id=binding.bindingId,snapshot=token;
    const previousAnswer=answerId;
    phase=session.phase;error='';text='';details=false;request=null;answerId=null;overBudget=false;busyChanged(true);
    const current=()=>alive&&!controller.signal.aborted&&session.generation===generation&&binding.bindingId===id&&token===snapshot;
    let ended=false,streaming=false;
    try{
      if(previousAnswer)await localData.hide(id,previousAnswer);
      await ensureIndex(id,snapshot,controller.signal);controller.signal.throwIfAborted();
      const parent=await tipOf(id);controller.signal.throwIfAborted();
      const plan=await aiClient.prepare(id,prompt.trim(),source?.page??box.page,[],fullDocument,source,[],parent,null,[],answerLimit>0?answerLimit:null,fullDocument);
      const coverage=plan.coverage;
      if(fullDocument&&(!coverage||coverage.indexedPages!==totalPages||coverage.totalPages!==totalPages||!coverage.completeRequired))throw {code:'index_incomplete'};
      const used=[...new Set(plan.sources.map(s=>`${s.name} 第 ${s.page} 页`))].join('、')||'无';
      coverageNotice=fullDocument?`全文文字检索 ${coverage?.indexedPages??totalPages}/${totalPages} 页；本轮来源：${used}。${coverage?.imageOnlyPages?`${coverage.imageOnlyPages} 页无可检索文字，需选页或框选补充图像。`:''}`:`未启用当前 PDF 全文检索；本轮来源：${used}。`;
      controller.signal.throwIfAborted();
      // Every image source the plan carries must be supplied, exactly as the
      // side chat does. A page with no extractable text is an image source
      // without a rectangle, and the backend refuses a request that leaves any
      // of them without pixels instead of silently dropping the page image.
      const images:{sourceId:string;png:string}[]=[];
      for(const item of plan.sources.filter(s=>s.image)){controller.signal.throwIfAborted();
        images.push({sourceId:item.id,png:await pageImage(reader,item,controller.signal)});
      }
      await aiClient.images(id,plan.id,images);controller.signal.throwIfAborted();
      if(!current())return;
      // The stream is appended into the temporary box while PDF.js stays fully
      // interactive; only the final terminal event moves the session to ready.
      const receive=(event:AiEvent)=>{
        if(event.bindingId!==id)return;
        // A cancelled/unmounted request can report its IDs after start resolves.
        // Dispose that turn instead of resurrecting it or leaving it running.
        if(!current()){
          if(event.kind==='started')void aiClient.cancel(event.requestId).catch(()=>{});
          if(event.answerId)void localData.hide(id,event.answerId).catch(()=>{});
          return;
        }
        if(ended)return;
        if(request&&request!==event.requestId)return;
        request=event.requestId;
        if(event.answerId)answerId=event.answerId;
        if(event.kind==='delta'){text+=event.text??'';return;}
        if(event.kind==='started')return;
        ended=true;request=null;busyChanged(false);
        if(event.kind==='complete'){
          if(text.trim()&&session.complete(generation,text)){phase=session.phase;void tick().then(()=>{if(!alive)return;overBudget=answerLimit>0&&[...text.trim()].length>answerLimit;if(hasOverflow())error='回答超出框体，请拖动边角扩大；文字不会被缩小或截断。';});}
          else{session.failed();phase=session.phase;details=true;error='生成结果为空，可重新生成或改问。';}
        }else{session.failed();phase=session.phase;details=true;error=event.kind==='cancelled'?'已取消生成。':aiError({code:event.code??'protocol'});}
      };
      streaming=true;
      const started=await aiClient.start(id,plan.id,parent,null,receive,sendSlot);
      if(!current())void aiClient.cancel(started).catch(()=>{});
      else if(!ended&&!request)request=started;
    }catch(e){
      streaming=false;
      if(current()){session.failed();phase=session.phase;details=true;error=aiError(e);}
    }finally{if(!streaming&&current())busyChanged(false);}
  }
  // Cancelling also removes this unconfirmed turn from the local conversation:
  // an unconfirmed temporary explanation must not survive as a permanent record.
  function cancel(){
    if(!session.cancel())return;
    abort?.abort();const id=binding.bindingId,turn=request,drop=answerId;text='';busyChanged(false);close();
    if(turn)void aiClient.cancel(turn).catch(()=>{});
    // hide() is applied after the cancelled append transactions, so a late
    // stream sequence cannot resurrect a turn the user just discarded.
    if(drop)void localData.hide(id,drop).catch(()=>{});
  }
  async function confirm(){
    if(phase!=='ready')return;
    try{
      // No overflow veto: a long but correct explanation is committed as real
      // text in the box the user chose, and the box may be enlarged afterwards.
      const value=session.confirmation();phase=session.phase;busyChanged(true);
      await reader.addText({page:box.page,rect:box.rect,text:value,fontSize});
      session.confirmed();text='';phase=session.phase;await reader.annotation('text');close();
    }catch(e){session.failed();phase=session.phase;error=e instanceof Error?e.message:'确认未完成，临时回答保留。';}
    finally{busyChanged(false);}
  }
  async function move(event:PointerEvent,resize=false){
    if(event.button!==0||phase==='confirming'||(event.target as HTMLElement).closest('button,input,textarea'))return;
    if(event.pointerType==='touch'&&!event.isPrimary)return;
    event.preventDefault();drag?.abort();const lifetime=drag=new AbortController(),original={page:box.page,rect:[...box.rect] as Region['rect']},snapshot=token;
    const end=()=>lifetime.abort();window.addEventListener('pointerup',e=>{if(e.pointerId===event.pointerId)end();},{signal:lifetime.signal});window.addEventListener('pointercancel',e=>{if(e.pointerId===event.pointerId)end();},{signal:lifetime.signal});
    for(const name of ['scroll','wheel','resize'])window.addEventListener(name,end,{capture:true,passive:true,signal:lifetime.signal});
    try{
      const page=await reader.getPage(original.page);if(!alive||lifetime.signal.aborted||token!==snapshot||box.page!==original.page){end();return;}
      const v=page.getViewport({scale:1}),canvas=container.querySelector<HTMLElement>(`.page[data-page-number="${original.page}"] .canvasWrapper`);
      if(!canvas){end();return;}const bounds=canvas.getBoundingClientRect(),start=[event.clientX,event.clientY],view=page.view;
      if(!bounds.width||!bounds.height){end();return;}
      window.addEventListener('pointermove',e=>{if(e.pointerId!==event.pointerId)return;if(!alive||token!==snapshot){end();return;}
        const r=dragBox(original.rect,v,(e.clientX-start[0])*v.width/bounds.width,(e.clientY-start[1])*v.height/bounds.height,resize);
        if(r[0]>=view[0]&&r[1]>=view[1]&&r[2]<=view[2]&&r[3]<=view[3])changed({page:original.page,rect:r});
      },{signal:lifetime.signal});
    }catch{end();error='页面尚未就绪，请稍后重试。';}
  }
  function movePrompt(event:PointerEvent){
    if(event.button!==0||phase==='confirming'||(event.target as HTMLElement).closest('button'))return;
    if(event.pointerType==='touch'&&!event.isPrimary)return;
    const panel=(event.currentTarget as HTMLElement).closest<HTMLElement>('.inline-detail');if(!panel)return;
    event.preventDefault();drag?.abort();const lifetime=drag=new AbortController(),bounds=panel.getBoundingClientRect(),origin=[event.clientX,event.clientY],offset=[Number(answer.dataset.promptOffsetX)||0,Number(answer.dataset.promptOffsetY)||0];
    const end=()=>lifetime.abort();window.addEventListener('pointerup',e=>{if(e.pointerId===event.pointerId)end();},{signal:lifetime.signal});window.addEventListener('pointercancel',end,{signal:lifetime.signal});
    for(const name of ['scroll','wheel','resize'])window.addEventListener(name,end,{capture:true,passive:true,signal:lifetime.signal});
    window.addEventListener('pointermove',e=>{if(e.pointerId!==event.pointerId||!alive)return;const clip=container.getBoundingClientRect(),next=boundedPanel(bounds.left+e.clientX-origin[0],bounds.top+e.clientY-origin[1],bounds.width,bounds.height,clip);answer.dataset.promptOffsetX=String(offset[0]+next.left-bounds.left);answer.dataset.promptOffsetY=String(offset[1]+next.top-bounds.top);answer.dispatchEvent(new Event('reader-overlay-layout',{bubbles:true}));},{signal:lifetime.signal});
  }
  function keydown(event:KeyboardEvent){if(enterSends(event,composing,performance.now()-compositionEnded<100)){event.preventDefault();event.stopPropagation();void generate();}}
  function regenerate(){void generate();}
  $effect(()=>{if(details&&promptInput)promptInput.focus({preventScroll:true});});
  onMount(()=>{
    void aiClient.config().then(info=>{models=availableModels(info.config);if(!models.some(m=>m.slot===sendSlot))pickSlot('main');}).catch(()=>{});
    if(quick){void tick().then(()=>{if(alive)void generate();});return;}
    void tick().then(()=>promptInput?.focus({preventScroll:true}));
  });
  onDestroy(()=>{const discard=session.phase!=='confirmed';alive=false;session.cancel();abort?.abort();drag?.abort();if(request)void aiClient.cancel(request).catch(()=>{});if(discard&&answerId)void localData.hide(binding.bindingId,answerId).catch(()=>{});});
</script>
<div class="inline-answer" class:prompt-mode={details} bind:this={answer} data-tools-below="false" hidden={placing} use:placeInline={{reader,container,box:details&&source?source:box,fontSize}} role="dialog" tabindex="-1" aria-modal="false" aria-label="临时原位解释" onkeydown={e=>{if(e.key==='Escape'&&!e.isComposing){e.preventDefault();e.stopPropagation();cancel();}}}>
  {#if !details}
  <div class="inline-tools" role="toolbar" aria-label="临时解释操作">
    <span class="drag-title" role="presentation" title="拖动回答框" onpointerdown={e=>void move(e)}>⋮⋮</span>
    <button title="编辑或更换问题" aria-label="编辑原位问题" disabled={phase==='preparing'||phase==='confirming'} onclick={()=>details=true}><Icon name="pen"/></button>
    <button title="用当前问题重新生成" aria-label="重新生成原位解释" disabled={phase==='preparing'||phase==='confirming'} onclick={regenerate}><Icon name="refresh"/></button>
    <button title="确认成普通未保存批注" aria-label="确认批注" disabled={phase!=='ready'} onclick={()=>void confirm()}>✓</button>
    <button title="取消，不写入 PDF 也不保留本轮" aria-label="取消原位解释" disabled={phase==='confirming'} onclick={cancel}><Icon name="close"/></button>
  </div>
  <div class="inline-content" bind:this={output} aria-label="回答输出区域">{text|| (phase==='preparing'?(status||'生成中…'):'打开问题窗口输入问题。')}</div>
  <div class="inline-resize" role="presentation" title="拖动调整回答框" onpointerdown={e=>void move(e,true)}></div>
  {#if coverageNotice}<details class="coverage-note"><summary>本轮来源</summary>{coverageNotice}</details>{/if}
  {#if overBudget}<p class="answer-budget" role="status">回答 {[...text.trim()].length} 字，超出建议上限 {answerLimit} 字；可点 ↻ 重新生成更短的版本，或调高设置里的上限。</p>{/if}{#if error}<p class="answer-error" role="alert">{error}</p>{/if}
  {:else}<section class="inline-detail" aria-label="原位问题">
    <header onpointerdown={movePrompt} role="presentation"><span>原位提问</span><button title="取消，不保存" aria-label="取消原位解释" onclick={cancel}><Icon name="close"/></button></header>
    <div class="inline-prompt-box"><textarea bind:this={promptInput} aria-label="原位解释 Prompt" bind:value={prompt} maxlength="2000" placeholder="询问所选内容…" onkeydown={keydown} oncompositionstart={()=>composing=true} oncompositionend={()=>{composing=false;compositionEnded=performance.now();}}></textarea><select class="inline-slot" aria-label="本轮模型" title={models.map(m=>`${m.label}：${m.detail}${m.vision?'（支持图像）':'（未启用图像）'}`).join('\n')} disabled={phase==='preparing'||phase==='confirming'||models.length<2} value={sendSlot} onchange={(event)=>pickSlot(event.currentTarget.value as AiSlot)}>{#each models as choice}<option value={choice.slot}>{choice.label}</option>{/each}</select><button class="inline-send" title="发送（Enter）" aria-label="发送原位问题" disabled={!prompt.trim()} onclick={()=>void generate()}><Icon name="send"/></button></div>
    {#if chosen&&!chosen.vision&&source}<p class="error" role="status">所选 {chosen.label} 未启用图像能力，而本轮要发送所选区域图像；发送会被拒绝。</p>{/if}
    {#if error}<p class="error" role="alert">{error}</p>{/if}
  </section>{/if}
</div>
<style>
  .coverage-note{position:absolute;top:100%;right:0;max-width:280px;background:#fffdf1;padding:3px;color:#607385;font-size:10px;z-index:2;}
  .inline-answer{position:fixed;z-index:25;border:1px solid #819cb2;background:rgba(255,253,241,.96);color:#202a34;box-shadow:0 2px 7px #21324626;}
  .inline-answer.prompt-mode{border-color:transparent;background:transparent;box-shadow:none;pointer-events:none;}
  .inline-answer[hidden]{display:none;}
  .inline-content{box-sizing:border-box;width:100%;height:100%;padding:calc(2px * var(--inline-scale,1));font:var(--inline-font-size,12px)/1.35 Arial,sans-serif;white-space:pre-wrap;overflow-wrap:anywhere;overflow:auto;}
  .inline-tools{position:absolute;bottom:calc(100% + 4px);left:var(--inline-tools-left,0px);display:flex;align-items:center;gap:1px;background:#fff;border:1px solid #cbd6df;border-radius:4px;padding:2px;box-shadow:0 2px 8px #24374b24;color:#465d70;font-size:11px;white-space:nowrap;}
  .inline-answer:global([data-tools-below="true"]) .inline-tools{top:calc(100% + 4px);bottom:auto;}
  .drag-title{padding:0 3px;cursor:move;touch-action:none;}
  button{display:inline-flex;align-items:center;justify-content:center;border:0;border-radius:3px;background:transparent;color:#456078;min-width:22px;height:22px;padding:3px;font:inherit;font-size:13px;}
  button :global(svg){width:15px;height:15px;}
  button:hover:not(:disabled){background:#e8eff5;}button:active:not(:disabled){background:#d9e5ee;}button:disabled{opacity:.4;}button:focus-visible{outline:2px solid #6689a8;}
  .inline-resize{position:absolute;right:-4px;bottom:-4px;width:9px;height:9px;border:1px solid #527999;background:#fff;cursor:nwse-resize;touch-action:none;}
  .inline-detail{position:fixed;top:var(--inline-prompt-top,24px);left:var(--inline-prompt-left,24px);display:flex;flex-direction:column;width:190px;height:110px;min-width:150px;min-height:96px;max-width:min(320px,calc(100vw - 24px));max-height:min(240px,calc(100vh - 24px));resize:both;overflow:auto;background:rgba(255,253,241,.96);color:#29333d;border:1px solid #819cb2;border-radius:5px;padding:5px;box-shadow:0 3px 12px #24374b35;font:11px/1.4 "Segoe UI","Microsoft YaHei",sans-serif;pointer-events:auto;}
  header{display:flex;justify-content:space-between;align-items:center;flex:none;cursor:move;touch-action:none;}.inline-prompt-box{position:relative;flex:1;min-height:48px;}textarea{width:100%;height:100%;padding:4px 84px 4px 4px;border:1px solid #cbd6df;border-radius:3px;resize:none;background:#ffffffa0;font:12px/1.4 "Segoe UI","Microsoft YaHei",sans-serif;}.inline-slot{position:absolute;right:29px;bottom:4px;max-width:86px;height:22px;padding:1px 2px;border:1px solid #cbd6df;border-radius:3px;background:#fff;color:#456078;font:10px "Segoe UI","Microsoft YaHei",sans-serif;}.inline-slot:disabled{opacity:.6;}.inline-prompt-box .inline-send{position:absolute;right:3px;bottom:3px;background:#416987;color:white;width:23px;height:23px;}.inline-send:disabled{background:#bdcad5;}.error{flex:none;max-height:36px;overflow:auto;margin:3px 0;color:#a13e38;font-size:10px;}.answer-budget{position:absolute;top:100%;background:#fffdf1;color:#7a6a3c;max-width:250px;font-size:10px;margin:3px 0;padding:4px;}.answer-error{position:absolute;top:100%;background:#fffdf1;color:#a13e38;max-width:250px;font-size:10px;margin:3px 0;padding:4px;}
</style>
