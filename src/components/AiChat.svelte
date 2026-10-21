<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import type { PdfReader } from '../lib/pdf-reader';
  import type { DocumentBinding, HistoryMessage, LocalData,ChatView } from '../lib/local-data';
  import { aiClient, aiError, availableModels, type AiDetails, type AiEvent, type AiConfig, type AiSlot } from '../lib/ai-client';
  import { readSlot, writeSlot } from '../lib/ai-slot';
  import { tokenUsage } from '../lib/ai-usage';
  import { extractPages, pageImage, isAttachedImage, secondaryPdf, type ContextPlan, type ContextSource, type Region } from '../lib/ai-context';
  import { clipboardImages, droppedImages, carriesImage, addImage, toAttachedImage, type AttachedImage } from '../lib/ai-attachment';
  import { desktopBatch, type DesktopDescriptor } from '../lib/desktop-source';
  import AiAnswer from './AiAnswer.svelte';
  import ReaderSettings from './ReaderSettings.svelte';
  import ChatSessions from './ChatSessions.svelte';
  import ContextAttachments from './ContextAttachments.svelte';
  import ContextChoices from './ContextChoices.svelte';
  import ComposerDivider from './ComposerDivider.svelte';
  import Icon from './Icon.svelte';
  import {composerHeight,defaultComposerHeight} from '../lib/chat-layout';
  import { enterSends, sourceReason } from '../lib/chat-input';
  import { preferences,latestStyles,defaultContext,contextPages,type Styles } from '../lib/preferences';
  let { visible, reader, binding, unavailableReason='请先打开桌面 PDF。', token, total, page, data, selected, region, beginRegion, beginInline, clearRegion, clearSelected,removePage,documentName='当前 PDF', consumed, selecting=false, close, jump, openSource }: {
    visible: boolean; reader: PdfReader | null; binding: DocumentBinding | null; unavailableReason?:string; token: number; total: number; page: number; data: LocalData;
    selected: number[]; region: Region | null; beginRegion: () => void; beginInline:()=>void; clearRegion: () => void; clearSelected:()=>void;removePage:(page:number)=>void;documentName?:string; consumed:(bindingId:string)=>void; selecting?:boolean; close: () => void; jump: (page: number, bindingId: string) => void; openSource: (source: DesktopDescriptor, page: number) => Promise<void>;
  } = $props();
  interface Round { query:string; page:number; selected:number[]; pinned:number[]; region:Region|null; currentPdf:boolean; parent:string|null; edit:string|null; images:AttachedImage[]; }
  interface Chat { epoch:number; records: HistoryMessage[]; prompt: string; status: string; error: string; request: string | null; plan: ContextPlan | null; parent: string | null; edit: string | null; currentPdf: boolean; details: Record<string, AiDetails>; busy: boolean; members: DesktopDescriptor[]; directory: boolean; pinned:number[]; excluded:string[]; round:Round|null; historyReady:boolean; session:string|null; sent:ContextPlan|null; images:AttachedImage[]; }
  const chats = $state<Record<string, Chat>>({});
  const empty = (): Chat => ({ epoch:0, records: [], prompt: '', status: '', error: '', request: null, plan: null, parent: null, edit: null, currentPdf: true, details: {}, busy: false, members: [], directory: false, pinned:[],excluded:[],round:null,historyReady:false,session:null,sent:null,images:[] });
  const chat = $derived(binding ? chats[binding.bindingId] : null);
  // Starts neutral: the real provider string is read from the stored config, so
  // a real-provider build must not advertise Mock before that read completes.
  let settings = $state<'models'|'styles'|'keys'|'context'|null>(null), model = $state('模型配置读取中…'), controller: AbortController | null = null;
  // Per-request model choice. It sits beside the send button and is remembered
  // per surface; the record of each answer names the model that actually ran.
  let sendSlot = $state<AiSlot>(readSlot('chat')), models = $state(availableModels(null));
  let attachNotice = $state('');
  const chosen = $derived(models.find(m => m.slot === sendSlot) ?? models[0]);
  function pickSlot(value: AiSlot) { sendSlot = value; writeSlot('chat', value); }
  let panel:HTMLElement,height=$state(defaultComposerHeight),panelHeight=$state(650),layoutReady=false,sourceOpen=$state(false);
  let conversationMenu=$state<HTMLDetailsElement>(),styleMenu=$state<HTMLDetailsElement>();
  const views=$state<Record<string,ChatView>>({});let manageSessions=$state(false);
  const currentView=$derived(binding?views[binding.bindingId]:null);
  const visibleRecords=$derived(chat?.records.filter(r=>!r.deleted)??[]);
  const shownPlan=$derived(chat?.plan??(chat?.request?chat.sent:null));
  const sessions=$derived(currentView?.sessions??[]);
  function resizeComposer(n:number){height=composerHeight(n,panelHeight);if(layoutReady)try{localStorage.setItem('reader-composer-height-v1',String(height));}catch{/* Session-only layout is safe. */}}
  let styles=$state<Styles>({schemaVersion:1,selected:null,revisions:[]}),styleQuery=$state('');let styleComposing=false,styleCompositionEnded=-Infinity;  const styleMatches=$derived(latestStyles(styles,styleQuery)),currentStyle=$derived(styleMatches.find(s=>s.id===styles.selected)??latestStyles(styles).find(s=>s.id===styles.selected));
  async function loadPreferences(){try{const p=await preferences.get();styles=p.styles;for(const c of Object.values(chats))c.plan=null;}catch{/* Data directory error is shown by the reader. */}}
  async function selectStyle(id:string|null){if(styleMenu)styleMenu.open=false;styleQuery='';try{styles=await preferences.select(id);for(const c of Object.values(chats))c.plan=null;}catch(e){if(chat)chat.error=aiError(e);}}
  function styleSearchKey(event:KeyboardEvent){if(event.isComposing||styleComposing||event.keyCode===229)return;if(event.key==='Escape'){if(styleMenu)styleMenu.open=false;styleQuery='';event.preventDefault();}else if(enterSends(event,styleComposing,performance.now()-styleCompositionEnded<100)){const choices=styleMatches.filter(s=>s.enabled);if(choices.length===1){event.preventDefault();void selectStyle(choices[0].id);}}}
  onMount(()=>{void loadPreferences();try{const raw=localStorage.getItem('reader-composer-height-v1');if(raw)height=composerHeight(Number(raw),panelHeight);}catch{/* Session-only layout. */}layoutReady=true;
    const size=new ResizeObserver(()=>{if(panel.clientHeight){panelHeight=panel.clientHeight;resizeComposer(height);}});size.observe(panel);
    const reload=()=>void loadPreferences(),open=()=>settings='context';const quick=()=>{if(chat&&!chat.request&&!chat.busy){chat.prompt='请简短解释所选区域的公式和含义。';void send();}};
    const dismissMenus=(event:PointerEvent)=>{const target=event.target as Element;if(!target.closest('.conversation-menu')&&conversationMenu)conversationMenu.open=false;if(!target.closest('.style-picker')&&styleMenu)styleMenu.open=false;};
    window.addEventListener('reader-preferences-changed',reload);window.addEventListener('reader-quick-question',quick);window.addEventListener('reader-open-settings',open);window.addEventListener('pointerdown',dismissMenus);
    return()=>{size.disconnect();window.removeEventListener('reader-preferences-changed',reload);window.removeEventListener('reader-quick-question',quick);window.removeEventListener('reader-open-settings',open);window.removeEventListener('pointerdown',dismissMenus);};});
  let indexStatus = $state(''), indexKey = '', alive = true;
  const indexed = new Set<string>();
  const documentBindings=new Map<string,string>();
  let composing=false, compositionEnded=-Infinity;
  const viewEpoch=new Map<string,number>();
  const nextView=(id:string)=>{const value=(viewEpoch.get(id)??0)+1;viewEpoch.set(id,value);return value;};
  function invalidateDocument(id:string,clear=false){const doc=documentBindings.get(id);for(const [key,documentId] of documentBindings){if(documentId!==doc)continue;const c=chats[key];if(!c)continue;c.epoch++;nextView(key);c.request=null;c.busy=false;c.plan=null;c.sent=null;c.round=null;c.edit=null;c.details={};c.historyReady=false;c.images=[];if(views[key])views[key]={...views[key],path:[],versions:[]};c.records=[];c.parent=null;c.session=null;if(clear){c.members=[];c.directory=false;c.pinned=[];for(const entry of indexed)if(entry.startsWith(key+':'))indexed.delete(entry);}if(key!==id)void refresh(key).catch(()=>{});}}
  const pendingDetails=new Set<string>();
  async function restoreDetails(id:string,message:string,state:Chat){const epoch=state.epoch,key=`${id}:${epoch}:${message}`;if(pendingDetails.has(key))return;pendingDetails.add(key);try{const result=await aiClient.details(id,message);if(alive&&chats[id]===state&&state.epoch===epoch)state.details[message]=result;}catch{/* A missing usage report must not prevent reading history. */}finally{pendingDetails.delete(key);}}
  function acceptView(id:string,value:ChatView){views[id]=value;const state=chats[id];if(!state)return;state.records=value.path;state.session=value.sessionId;state.parent=value.path.filter(r=>!r.deleted&&r.role==='assistant'&&r.status==='complete').at(-1)?.id??null;state.historyReady=true;for(const record of value.path)if(!record.deleted&&record.role==='assistant'&&record.status!=='streaming'&&state.details[record.id]?.usage===undefined)void restoreDetails(id,record.id,state);}
  async function refresh(id: string) {const expected=nextView(id);for(let attempt=0;attempt<4;attempt++){try{const result = await data.chatView(id);if(alive&&chats[id]&&expected===viewEpoch.get(id))acceptView(id,result);return;}catch(e){if(!(e&&typeof e==='object'&&'code'in e&&e.code==='busy')||attempt===3)throw e;await new Promise(resolve=>setTimeout(resolve,40));}} }
  async function updateModel() { try { const info = await aiClient.config(); models = availableModels(info.config); model = info.config.main.provider === 'mock' ? 'Mock · 本地测试' : `${info.config.main.provider} · ${info.config.main.model}`; if (!models.some(m => m.slot === sendSlot)) pickSlot('main'); } catch { model = '模型配置不可用'; } }
  $effect(() => {
    const id = binding?.bindingId; void token;
    controller?.abort(); controller = null; indexKey = ''; indexStatus = '';manageSessions=false;
    if (!id) return;
    if(binding)documentBindings.set(id,binding.documentId);
    chats[id] ??= empty(); void refresh(id).catch(e => { if (chats[id]) chats[id].error = aiError(e); }); void updateModel();
  });
  $effect(() => { if (chat) { void chat.prompt; void chat.currentPdf; void selected.join(','); void region; void chat.pinned.join(','); void chat.parent; void chat.edit; chat.plan = null; chat.excluded=[]; } });
  onDestroy(() => { alive = false; controller?.abort(); for (const c of Object.values(chats)) if (c.request) void aiClient.cancel(c.request).catch(() => {}); });
  async function preview(override?:Round) {
    if (!binding || !reader || !chat || !chat.prompt.trim() || chat.request || chat.busy) return;
    const id = binding.bindingId, current = reader, snapshot = token, state = chat;
    const round=override??{query:state.prompt.trim(),page,selected:[...selected],pinned:[...state.pinned],region:region?{page:region.page,rect:[...region.rect] as Region['rect']}:null,currentPdf:state.currentPdf,parent:state.parent,edit:state.edit,images:[...state.images]};
    controller?.abort(); const abort = controller = new AbortController(); state.busy = true; state.error = ''; state.plan = null;
    try {
      const key = `${id}:${snapshot}`;
      if (round.currentPdf && !indexed.has(key)) { indexKey = key; await extractPages(current, total, abort.signal, batch => aiClient.index(id, batch), n => { indexStatus = `本地索引 ${n}/${total}`; }); indexed.add(key); }
      else if (!round.currentPdf && !indexed.has(key)) {
        // Explicit page/region questions can proceed independently of a large
        // full-document index. No page or text is silently dropped on overflow.
        const currentContext=(await preferences.get()).context??defaultContext();abort.signal.throwIfAborted();
        const numbers = [...new Set([...contextPages(round.page,round.selected,round.region?.page,total,currentContext),...round.pinned])], batch: { page: number; text: string }[] = [];
        for (const n of numbers) {
          abort.signal.throwIfAborted(); let text = '';
          if (!(round.region && round.region.page === n && !round.selected.includes(n) && !round.pinned.includes(n))) { const p = await current.getPage(n), content = await p.getTextContent(); text = content.items.map(item => 'str' in item ? item.str + (item.hasEOL ? '\n' : ' ') : '').join(''); }
          if (new TextEncoder().encode(text).byteLength > 32768) throw Error('此页文字超出预算；请框选区域作为图像发送，未静默截断文字。');
          batch.push({ page: n, text });
        }
        await aiClient.index(id, batch);
      }
      abort.signal.throwIfAborted();
      const plan = await aiClient.prepare(id, round.query, round.page, round.selected, round.currentPdf, round.region,round.pinned,round.parent,round.edit,round.images.map(item=>({name:item.name,png:item.png})));
      const images: { sourceId: string; png: string }[] = [];
      for (const source of plan.sources.filter(source => source.image)) { abort.signal.throwIfAborted();
        // An attached screenshot already carries its own pixels; the backend
        // stored them while preparing this plan and nothing is rendered here.
        if (isAttachedImage(source)) continue;
        if (source.documentId === binding!.documentId) images.push({ sourceId: source.id, png: await pageImage(current, source, abort.signal) });
        else { const member = state.members.find(m => m.grantId === source.grantId); if (!member) throw Error('其他文档授权已撤销。'); const secondary = desktopBatch({ documents: [member], rejected: [] }).sources[0]; const opened = await secondaryPdf(await secondary.read(abort.signal), abort.signal);
          try { images.push({ sourceId: source.id, png: await pageImage({ getPage: n => opened.pdf.getPage(n) } as PdfReader, source, abort.signal) }); } finally { await opened.close(); } }
      }
      await aiClient.images(id, plan.id, images); abort.signal.throwIfAborted();
      if (binding?.bindingId !== id || token !== snapshot) throw Error('文档已切换，未发送上下文。');
      state.plan = plan; state.round=round; state.excluded=[]; state.status = '来源已准备，可移除不需要的项目。'; indexStatus = '';
    } catch (e) { if (!abort.signal.aborted) state.error = aiError(e); }
    finally { state.busy = false; if (indexKey) indexStatus = ''; }
  }
  async function send(retry=false) {
    if (!binding || !chat || chat.request || chat.busy || !chat.prompt.trim()) return;
    const id = binding.bindingId, state = chat,epoch=chat.epoch;
    const now={query:state.prompt.trim(),page,selected:[...selected],pinned:[...state.pinned],region,currentPdf:state.currentPdf,parent:state.parent,edit:state.edit,images:[...state.images]};
    if(retry&&state.round){state.prompt=state.round.query;await preview(state.round);}else if(!state.plan||JSON.stringify(state.round)!==JSON.stringify(now))await preview();
    if(binding?.bindingId!==id||state.epoch!==epoch||!state.plan||!state.round)return;
    const round=state.round, plan=state.plan, abort=controller??new AbortController();controller=abort;state.busy = true; state.error = '';
    let ended=false;
    const receive = (event: AiEvent) => {
      if (!alive || ended || event.bindingId !== id || !chats[id]||state.epoch!==epoch) return;
      if (state.request && state.request !== event.requestId) return;
      state.request = event.requestId;
      if (event.kind === 'started') { state.status = '正在生成 · '+(state.sent?.sources.length??0)+' 项来源';
        consumed(id);
        const cut=round.edit?state.records.findIndex(r=>r.id===round.edit):round.parent?state.records.findIndex(r=>r.id===round.parent)+1:0;
        state.records=state.records.slice(0,Math.max(0,cut));
        const common = { status: 'complete', createdMs: Date.now(), page:round.page, deleted: false, truncated: false, metadata: { provider: model.split(' · ')[0], model: model.split(' · ').slice(1).join(' · ') } };
        if (event.userId) {state.records.push({ ...common, id: event.userId, role: 'user', content: round.query, parent: round.parent, editedFrom: round.edit });if(state.session===null)state.session=event.userId;}
        if (event.answerId) {state.records.push({ ...common, id: event.answerId, role: 'assistant', content: '', parent: event.userId, status: 'streaming' });state.details[event.answerId]={sources:state.sent?.sources??[]};}
        void refresh(id).catch(()=>{});
        if(abort.signal.aborted)void aiClient.cancel(event.requestId).catch(()=>{});
      }
      else if (event.kind === 'delta') {
        const record = state.records.find(r => r.id === event.answerId);
        if (record) record.content += event.text ?? ''; else void refresh(id).catch(() => {});
      } else {
        ended=true;
        if(event.answerId&&event.usage)state.details[event.answerId]={...state.details[event.answerId],usage:event.usage};
        state.request = null; state.busy = false; state.status = event.kind === 'complete' ? '生成完成' : event.kind === 'cancelled' ? '已取消，保留已提交内容' : '生成未完成';
        if (event.code && event.kind !== 'cancelled') state.error = aiError({ code: event.code });
        if (event.kind === 'complete') { state.parent = event.answerId ?? null; state.edit = null; state.prompt = ''; }
        void refresh(id).then(async () => { if (event.answerId&&state.epoch===epoch){const value=await aiClient.details(id,event.answerId);if(state.epoch===epoch)state.details[event.answerId]=value;} }).catch(() => {});
      }
    };
    try { const filtered=await aiClient.refine(id,plan.id,plan.sources.filter(s=>!state.excluded.includes(s.id)).map(s=>s.id));abort.signal.throwIfAborted();if(state.epoch!==epoch)return;state.sent=filtered;state.plan=null;state.status='正在发送…'; const request = await aiClient.start(id, filtered.id, round.parent, round.edit, receive, sendSlot); if(state.epoch!==epoch)return;if (!state.request && state.busy) state.request = request;if(abort.signal.aborted&&state.request)void aiClient.cancel(state.request).catch(()=>{}); }
    catch (e) { if(state.epoch!==epoch)return;if(abort.signal.aborted)state.status='已停止准备';else state.error = aiError(e); state.request = null; state.busy = false;state.plan=null; }
  }
  function stop(){if(!chat)return;if(chat.request)void aiClient.cancel(chat.request).catch(e=>{if(chat)chat.error=aiError(e);});else if(chat.busy){controller?.abort();chat.plan=null;chat.status='已停止准备，未发送';}}
  function keydown(event:KeyboardEvent){if(enterSends(event,composing,performance.now()-compositionEnded<100)){event.preventDefault();void send();}}
  let promptBox=$state<HTMLTextAreaElement>();
  function branch(record: HistoryMessage) { if (!chat || chat.request || chat.busy || record.truncated) return;chat.prompt = record.content; chat.edit = record.id; chat.parent = record.parent ?? null; chat.plan = null; chat.status = '修改问题后发送将创建新版本，旧记录保留。';queueMicrotask(()=>{promptBox?.focus();promptBox?.setSelectionRange(promptBox.value.length,promptBox.value.length);}); }
  function resetRound(state:Chat){state.edit=null;state.plan=null;state.round=null;state.sent=null;state.prompt='';state.pinned=[];state.excluded=[];state.images=[];sourceOpen=false;clearSelected();clearRegion();}
  // A pasted screenshot becomes a removable attachment in the composer. The
  // event is claimed only when it really carries image files, and the default is
  // prevented before any await, so an image can never fall through to the page,
  // to a PDF editor or to the browser's own document handling.
  function pasteImages(event: ClipboardEvent) {
    if (!carriesImage(event.clipboardData)) return;
    event.preventDefault();
    event.stopPropagation();
    const files = clipboardImages(event.clipboardData);
    if (!files.length) return;
    void attachImages(files);
  }
  function dropImages(event: DragEvent) {
    if (!carriesImage(event.dataTransfer)) return;
    event.preventDefault();
    event.stopPropagation();
    const files = droppedImages(event.dataTransfer);
    if (files.length) void attachImages(files);
  }
  function dragOverImages(event: DragEvent) {
    if (!carriesImage(event.dataTransfer)) return;
    // Claim the drag so the window-level PDF drop handler never sees it.
    event.preventDefault();
    event.stopPropagation();
  }
  async function attachImages(files: (File | Blob)[]) {
    const state = chat;
    // Without a bound document the composer is unavailable anyway; report it in
    // the composer's own status line instead of dropping the paste silently.
    if (!state) { attachNotice = files.length ? '请先打开 PDF，再把图片加入本轮。' : ''; return; }
    attachNotice = '';
    let next = state.images;
    try {
      for (const [index, file] of files.entries()) {
        const named = (file as File).name || `image-${state.images.length + index + 1}.png`;
        next = addImage(next, await toAttachedImage(file, named));
      }
      state.images = next; state.plan = null; state.status = `已附加 ${state.images.length} 张图片；发送前可移除。`; state.error = '';
    } catch (e) { state.error = e instanceof Error ? e.message : '图片未能加入本轮。'; }
  }
  async function selectPath(messageId:string|null){if(!binding||!chat||chat.busy||chat.request)return;const id=binding.bindingId,state=chat;state.busy=true;state.error='';try{const value=await data.chatSelect(id,messageId);if(alive&&chats[id]){invalidateDocument(id);acceptView(id,value);resetRound(state);manageSessions=false;state.status=messageId?'版本及后续路径已切换。':'新的独立对话；旧历史保留。';}}catch(e){state.error=aiError(e);}finally{state.busy=false;}}
  function newConversation(){if(conversationMenu)conversationMenu.open=false;void selectPath(null);}
  function regenerate(record:HistoryMessage){const user=chat?.records.find(r=>r.id===record.parent&&r.role==='user');if(user)branch(user);}
  function cancelEdit(){if(!chat)return;chat.edit=null;chat.prompt='';chat.plan=null;chat.round=null;chat.parent=currentView?.path.filter(r=>r.role==='assistant'&&r.status==='complete').at(-1)?.id??null;chat.status='已取消编辑，当前版本保留。';}
  async function sessionAction(action:'archive'|'restore'|'delete'|'clear',sessionId?:string){if(!binding||!chat||chat.busy||chat.request)return;const id=binding.bindingId,state=chat;state.busy=true;state.error='';try{if(action==='clear'){if(!await data.deleteDocumentHistory(id))return;}else if(action==='delete'){if(!await data.chatDelete(id,sessionId!))return;}else await data.chatArchive(id,sessionId!,action==='archive');invalidateDocument(id,action==='clear');state.busy=true;await refresh(id);resetRound(state);state.status=action==='archive'?'会话已归档，数据保留。':action==='restore'?'会话已恢复。':'聊天记录已清除；PDF、批注和旧备份不变。';}catch(e){state.error=aiError(e);}finally{state.busy=false;}}
  function contextOptions(){sourceOpen=true;if(chat&&!chat.plan&&!chat.busy&&!chat.request&&chat.prompt.trim())void preview();}
  async function details(record: HistoryMessage) { if (!binding || !chat) return; const id = binding.bindingId, state = chat,epoch=chat.epoch; try {const result=await aiClient.details(id,record.id);if(state.epoch===epoch)state.details[record.id]=result;} catch (e) {if(state.epoch===epoch)state.error = aiError(e); } }
  async function directory() {
    if (!binding || !chat || chat.busy&&!chat.directory) return; const id = binding.bindingId, state = chat;
    if(state.directory){controller?.abort();state.plan=null;try{await aiClient.directory(id,false);state.members=[];state.directory=false;state.status='同目录授权已撤销；原缓存不再用于请求。';}catch(e){state.error=aiError(e);state.status='未能确认撤销，请重试；授权状态未被隐藏。';}return;}
    controller?.abort(); const abort = controller = new AbortController(); state.busy = true; state.plan = null; state.error = '';
    try {
      const enabling = !state.directory; state.members = await aiClient.directory(id, enabling); state.directory = enabling && state.members.length > 0;
      for (const member of state.members) {
        abort.signal.throwIfAborted(); const source = desktopBatch({ documents: [member], rejected: [] }).sources[0]; const opened = await secondaryPdf(await source.read(abort.signal), abort.signal);
        try { await extractPages({ getPage: n => opened.pdf.getPage(n) } as PdfReader, opened.pdf.numPages, abort.signal, batch => aiClient.indexMember(id, member.grantId, opened.pdf.numPages, batch).then(() => {}), n => { state.status = `同目录本地索引：${member.name} ${n}/${opened.pdf.numPages}`; }); }
        finally { await opened.close(); }
      }
      state.status = state.directory ? `已授权并索引 ${state.members.length} 份直接包含的 PDF。` : '同目录未授权；缓存不再用于请求。';
    } catch (e) { state.error = aiError(e); await aiClient.directory(id, false).catch(() => {}); state.members = []; state.directory = false; }
    finally { state.busy = false; }
  }
  async function sourceJump(record: HistoryMessage, source: ContextSource) {
    if (!binding || !chat) return;
    if (source.documentId === binding.documentId) { jump(source.page, binding.bindingId); return; }
    try { const [descriptor, n] = await aiClient.source(binding.bindingId, record.id, source.id); await openSource(descriptor, n); } catch (e) { chat.error = aiError(e); }
  }
</script>
<aside class="ai-panel" bind:this={panel} hidden={!visible} aria-label="AI 对话">
  <div class="panel-head">
    <button class="model-picker" title="选择模型" aria-label="模型设置" onclick={()=>settings='models'}>{model}<Icon name="down"/></button>
    <details class="style-picker" bind:this={styleMenu}><summary title="选择表达风格">{currentStyle?.name??'无风格'}</summary><div class="style-menu"><input aria-label="搜索风格" placeholder="人物、称呼或学校" bind:value={styleQuery} onkeydown={styleSearchKey} oncompositionstart={()=>styleComposing=true} oncompositionend={()=>{styleComposing=false;styleCompositionEnded=performance.now();}}/><button class:chosen={!styles.selected} onclick={()=>void selectStyle(null)}>无风格 / Default</button>{#each styleMatches as s}<button class:chosen={styles.selected===s.id} disabled={!s.enabled} onclick={()=>void selectStyle(s.id)} title={s.aliases.join(' · ')}><span>{s.name}</span><small>{styles.recent?.includes(s.id)?'最近':''}{s.enabled?'':'停用'}</small></button>{/each}{#if !styleMatches.length}<small class="style-empty">没有匹配的风格</small>{/if}<button class="style-manage" onclick={()=>{settings='styles';if(styleMenu)styleMenu.open=false;}}>管理 / 导入风格…</button></div></details>
    <button class="mini" aria-label="收起 AI 对话" title="收起 AI 对话" onclick={close}><Icon name="close"/></button>
  </div>
  {#if !binding || !chat}<div class="chat-records"><p class="empty" role="status">{unavailableReason}</p></div><div class="composer" style:height={defaultComposerHeight+'px'}><div class="prompt-box"><textarea aria-label="向 PDF 提问" placeholder="本地数据或 PDF 身份未就绪，暂时无法发送" disabled></textarea><div class="composer-actions"><span></span><button class="send-button" aria-label="发送问题" title={unavailableReason} disabled><Icon name="send"/></button></div></div></div>
  {:else}
    <div class="conversation-line"><span>{chat.session===null?'新对话':'当前对话'}</span><details class="conversation-menu" bind:this={conversationMenu}><summary aria-label="对话菜单" title="对话与历史"><Icon name="more"/></summary><div><button aria-label="开始新对话" disabled={chat.busy||!!chat.request} onclick={newConversation}>＋ 开始新对话</button><button disabled={chat.busy||!!chat.request} onclick={()=>{manageSessions=true;conversationMenu!.open=false;}}>管理此 PDF 会话…</button>{#each sessions.filter(s=>!s.archived).slice(-20).reverse() as root}<button disabled={chat.busy||!!chat.request} class:chosen={chat.session===root.id} onclick={()=>void selectPath(root.leafId)} title={root.title}>{root.title.slice(0,38)}</button>{/each}</div></details></div>
    <div class="chat-records" aria-label="对话记录">
      {#each visibleRecords as record (record.id)}
        <article class:user={record.role==='user'}>
          <div class="record-meta"><span>{record.role==='user'?'你':record.metadata.provider==='mock'?'Mock 测试':record.metadata.model??'历史回复'}{#if record.metadata.preset_version} · 风格 v{record.metadata.preset_version.split('@').at(-1)}{/if}</span><small>{({complete:'',streaming:'生成中',cancelled:'已停止',failed:'未完成'} as Record<string,string>)[record.status]??''}</small></div>
          {#if record.role==='user'}<p class="prompt-text">{record.content}</p>{:else}<AiAnswer text={record.content}/>{/if}
          {#if record.role==='assistant'&&record.status!=='streaming'}<small class="token-usage">{tokenUsage(chat.details[record.id]?.usage)}</small>{/if}
          <div class="record-actions">
            {#each currentView?.versions.filter(v=>v.messageId===record.id&&v.total>1)??[] as v}<span class="version-nav" aria-label="消息版本"><button class="mini" title="上一版本及对应后续" aria-label="上一消息版本" disabled={!v.previousId||chat.busy||!!chat.request} onclick={()=>void selectPath(v.previousId)}>←</button><small>{v.index} / {v.total}</small><button class="mini" title="下一版本及对应后续" aria-label="下一消息版本" disabled={!v.nextId||chat.busy||!!chat.request} onclick={()=>void selectPath(v.nextId)}>→</button></span>{/each}
            {#if record.role==='user'}<button class="mini" title="编辑问题，保留原分支" aria-label="编辑问题" disabled={!!chat.request||chat.busy||record.truncated} onclick={()=>branch(record)}><Icon name="pen"/></button>{:else}<button class="mini" title="重新生成，保留原回答" aria-label="重新生成回答" disabled={!!chat.request||chat.busy} onclick={()=>regenerate(record)}><Icon name="refresh"/></button><button class="source-chip" title="查看本次实际来源及用量" onclick={()=>void details(record)}>{chat.details[record.id]?.sources?.length??'查看'} 来源</button>{/if}
            {#if record.page}<button class="page-chip" title="跳到提问页" onclick={()=>jump(record.page!,binding!.bindingId)}>{record.page} 页 ↗</button>{/if}
          </div>
          {#if chat.details[record.id]}<details class="sources"><summary>{chat.details[record.id].sources?.length??0} 项实际来源 · 用量</summary><div class="source-list">{#each chat.details[record.id].sources??[] as source}<button title={source.name+' · '+sourceReason(source.reasons)} onclick={()=>void sourceJump(record,source)}>[{source.id}] {source.page}页{source.rect?'区域':''}</button>{/each}</div>{#if chat.details[record.id].usage}<small>{#if chat.details[record.id].usage?.mock}本地 Mock，无 API 费用{:else}Token：输入 {chat.details[record.id].usage?.promptTokens??'—'} · 输出 {chat.details[record.id].usage?.completionTokens??'—'}{/if}</small>{/if}</details>{/if}
        </article>
      {/each}
      {#if !visibleRecords.length}<p class="empty">{chat.session===null?'开始新的独立对话，旧历史已保留。':'选择区域或页面，直接输入问题。'}</p>{/if}
    </div>
    <ComposerDivider value={height} change={resizeComposer} reset={()=>resizeComposer(defaultComposerHeight)}/>
    <div class="composer" data-compact={height<180} style:height={height+'px'}>
      {#if chat.edit}<small class="branch-note">正在编辑旧问题 · 原记录保留</small>{/if}
      <div class="prompt-box">
        <ContextAttachments {reader} {token} name={documentName} pages={selected} pinned={chat.pinned} {region} images={chat.images} changeImages={next=>{chat!.images=next;chat!.plan=null;}} disabled={chat.busy||!!chat.request} removePage={n=>{removePage(n);chat!.pinned=chat!.pinned.filter(p=>p!==n);}} removePinned={n=>chat!.pinned=chat!.pinned.filter(p=>p!==n)} removeRegion={clearRegion}/>
        <textarea bind:this={promptBox} aria-label="向 PDF 提问" placeholder="向 PDF 提问… Enter 发送，Shift+Enter 换行；可直接粘贴截图" bind:value={chat.prompt} maxlength="8000" disabled={chat.busy||!!chat.request} onkeydown={keydown} onpaste={pasteImages} ondragover={dragOverImages} ondrop={dropImages} oncompositionstart={()=>composing=true} oncompositionend={()=>{composing=false;compositionEnded=performance.now();}}></textarea>
        <div class="composer-actions"><button class="context-button" aria-label="上下文选项" title="本轮检索与可选来源检查" disabled={chat.busy||!!chat.request} onclick={contextOptions}><Icon name="more"/></button><span></span>{#if chat.edit&&!chat.busy&&!chat.request}<button class="cancel-edit" aria-label="取消问题编辑" title="保留原消息及分支，不发送" onclick={cancelEdit}>取消</button>{/if}
        <label class="model-slot" title={models.map(m=>`${m.label}：${m.detail}${m.vision?'（支持图像）':'（未启用图像）'}`).join('\n')}><span class="visually-hidden">本轮模型</span><select aria-label="本轮模型" disabled={chat.busy||!!chat.request||models.length<2} value={sendSlot} onchange={(event)=>pickSlot(event.currentTarget.value as AiSlot)}>{#each models as choice}<option value={choice.slot}>{choice.label}</option>{/each}</select></label>
        {#if chat.request||chat.busy}<button class="send-button" aria-label="停止生成" title="停止准备或生成" onclick={stop}><Icon name="stop"/></button>{:else}<button class="send-button" aria-label="发送问题" title="发送（Enter）" disabled={!chat.prompt.trim()} onclick={()=>void send()}><Icon name="send"/></button>{/if}</div>
        {#if chosen&&!chosen.vision&&(selected.length>0||region||chat.pinned.length>0)}<small class="slot-warning" role="status">本轮包含图像，但所选的 {chosen.label} 未启用图像能力；发送会被拒绝，请改用支持图像的模型或移除图像。</small>{/if}
      </div>
      <div class="feedback">{#if attachNotice}<small class="error" role="alert">{attachNotice}</small>{/if}{#if indexStatus||chat.status}<small role="status">{indexStatus||chat.status}</small>{/if}{#if chat.error}<small class="error" role="alert">{chat.error}</small>{#if chat.round}<button disabled={chat.busy||!!chat.request} onclick={()=>void send(true)}>重试</button>{/if}{/if}</div>
    </div>
  {/if}
</aside>
{#if sourceOpen&&chat}<ContextChoices plan={shownPlan} currentPdf={chat.currentPdf} busy={chat.busy||!!chat.request} excluded={chat.excluded} changeCurrent={value=>{chat!.currentPdf=value;}} toggle={id=>chat!.excluded=chat!.excluded.includes(id)?chat!.excluded.filter(n=>n!==id):[...chat!.excluded,id]} close={()=>sourceOpen=false}/>{/if}
{#if settings}<ReaderSettings initial={settings} close={()=>settings=null} changed={()=>{void updateModel();void loadPreferences();}} directory={()=>void directory()} directoryEnabled={chat?.directory??false} directoryBusy={chat?.busy??false} hasDocument={!!binding&&!!chat}/>{/if}
{#if manageSessions&&currentView}<ChatSessions sessions={currentView.sessions} busy={chat?.busy??false} error={chat?.error} status={chat?.status} close={()=>manageSessions=false} select={id=>void selectPath(id)} action={(action,id)=>void sessionAction(action,id)}/>{/if}
<style>
 .ai-panel{width:var(--ai-width,350px);min-width:280px;flex-shrink:0;display:flex;flex-direction:column;background:#fff;border-left:1px solid #dce1e7;font-size:12px;min-height:0;}.ai-panel[hidden]{display:none;}
 button{border:1px solid #d5dce3;border-radius:4px;padding:3px 6px;color:#41566a;background:#fff;font-size:11px;}button:disabled{opacity:.45;}button:hover:not(:disabled){background:#edf2f7;}button:active:not(:disabled){background:#dfe8f0;}.chosen{background:#e6eef6;color:#254d71;border-color:#7898b6;}button:focus-visible,textarea:focus-visible,summary:focus-visible{outline:2px solid #6689a8;outline-offset:1px;}
 .panel-head{display:flex;align-items:center;gap:8px;min-height:42px;padding:0 9px;border-bottom:1px solid #e5e9ee;}.model-picker{display:flex;align-items:center;gap:3px;flex:1;min-width:0;border:0;overflow:hidden;white-space:nowrap;text-overflow:ellipsis;font-size:11px;}.model-picker :global(svg){width:12px;height:12px;}.mini{width:24px;height:24px;display:inline-flex;align-items:center;justify-content:center;border:0;padding:4px;flex-shrink:0;}.mini :global(svg){width:15px;height:15px;}
 .style-picker,.conversation-menu{position:relative;}.style-picker summary{max-width:96px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;}.style-menu,.conversation-menu>div{position:absolute;right:0;top:calc(100% + 5px);z-index:30;background:white;border:1px solid #ccd6df;box-shadow:0 4px 12px #20344922;border-radius:5px;padding:6px;display:flex;flex-direction:column;gap:3px;width:min(240px,calc(100vw - 30px));max-height:200px;overflow-y:auto;overflow-x:hidden;}.style-menu input{width:100%;padding:5px;border:1px solid #ccd6df;border-radius:3px;flex:none;}
 .style-menu button,.conversation-menu button{text-align:left;min-width:0;padding:4px 6px;line-height:1.5;}
 /* A name is shown in full. It may only shrink when it is genuinely wider than
    the row, and it wraps rather than being clipped to a character or two. The
    recent marker gives way instead. */
 .style-menu button span,.style-menu button{white-space:normal;overflow-wrap:anywhere;}
 .style-menu button{display:flex;align-items:baseline;justify-content:space-between;gap:6px;}
 .style-menu button span{flex:1 1 auto;min-width:0;overflow:visible;text-overflow:clip;}.style-menu button small,.style-empty{color:#7c8a98;font-size:10px;}.style-menu button.chosen{background:#edf2f7;color:#345879;}.style-manage{border-top:1px solid #e0e6ec;padding-top:6px!important;}
 .conversation-line{display:flex;align-items:center;justify-content:space-between;padding:5px 11px;color:#788896;font-size:10px;}.conversation-menu summary{display:flex;align-items:center;padding:2px;}.conversation-menu :global(svg){width:16px;height:16px;}
 .chat-records{flex:1;overflow:auto;min-height:80px;padding:7px 10px;}article{margin:0 0 12px;padding:8px;border:1px solid #e8edf2;border-radius:5px;}article.user{background:#f5f8fb;}.record-meta{display:flex;justify-content:space-between;gap:5px;color:#7a8c9b;font-size:10px;}.record-meta small{font-size:10px;}.prompt-text{white-space:pre-wrap;line-height:1.6;font-size:13px;margin:7px 0;}.version-nav{display:inline-flex;align-items:center;gap:1px;font-size:10px;color:#71869a;}.version-nav small{font-size:10px;white-space:nowrap;}.record-actions{display:flex;align-items:center;gap:4px;margin-top:4px;}.source-chip,.page-chip{border:0;background:transparent;font-size:10px;color:#778ea1;padding:3px 5px;}.sources{margin-top:4px;font-size:10px;color:#708393;}.source-list{display:flex;flex-wrap:wrap;gap:3px;margin:5px 0;}.sources small{display:block;overflow-wrap:anywhere;}
 .token-usage{display:block;margin-top:6px;color:#708393;font-size:10px;line-height:1.5;overflow-wrap:anywhere;}
 .composer{flex:0 0 auto;min-height:100px;padding:7px 10px;display:flex;flex-direction:column;gap:5px;overflow:hidden;}summary{cursor:pointer;color:#708799;font-size:11px;}summary::marker{font-size:9px;}.branch-note{font-size:10px;color:#80673e;flex:none;}
 .prompt-box{position:relative;display:flex;flex-direction:column;min-height:65px;flex:1;border:1px solid #ccd6df;border-radius:7px;background:#fff;overflow:hidden;}.prompt-box:focus-within{border-color:#7898b6;box-shadow:0 0 0 1px #b7cada;}
 /* The textarea takes the room that is left, so the box never shows an empty
    band under the cursor. The actions row keeps its own height and the chip row
    only grows while there are attachments. */
 .prompt-box textarea{width:100%;flex:1 1 0;min-height:28px;padding:7px 9px;resize:none;border:0;font-family:inherit;font-size:12px;line-height:1.6;outline:none;background:transparent;}.composer-actions{display:flex;align-items:center;gap:5px;flex:none;height:34px;padding:1px 6px 5px;}.composer-actions>span{flex:1;}.composer-actions .send-button{width:28px;height:28px;padding:5px;display:flex;align-items:center;justify-content:center;border:0;border-radius:5px;background:#416987;color:white;}.send-button :global(svg){width:17px;height:17px;}.send-button:disabled{background:#bdcad5;}.context-button{border:0;display:flex;align-items:center;justify-content:center;width:26px;height:26px;padding:4px;}.context-button :global(svg){width:16px;height:16px;}.cancel-edit{border:0;font-size:11px;}.feedback{flex:0 1 auto;max-height:38px;overflow:auto;font-size:10px;color:#7d8d9c;display:flex;gap:4px;flex-wrap:wrap;}.feedback small{font-size:10px;line-height:1.5;}.feedback .error{color:#a4483f;}.empty{padding:12px;color:#7a8e9f;line-height:1.6;font-size:12px;}
 .composer[data-compact="true"]{padding:5px 10px;gap:3px;}.composer[data-compact="true"] .branch-note{display:none;}.composer[data-compact="true"] .feedback{max-height:14px;min-height:0;}
 .model-slot{display:inline-flex;align-items:center;flex:none;}
 .model-slot select{max-width:86px;height:24px;padding:2px 4px;border:1px solid #d5dce3;border-radius:4px;background:#fff;color:#41566a;font:11px "Segoe UI","Microsoft YaHei",sans-serif;}
 .model-slot select:disabled{opacity:.6;}
 .slot-warning{flex:none;color:#97551c;font-size:10px;line-height:1.5;}
 .visually-hidden{position:absolute;width:1px;height:1px;overflow:hidden;clip-path:inset(50%);white-space:nowrap;}
 @media(max-width:900px){.ai-panel{position:absolute;right:0;top:0;bottom:0;z-index:15;box-shadow:-4px 0 14px #20344922;width:min(var(--ai-width,350px),90vw);}}
</style>
