<script lang="ts">
  import { onMount,tick } from 'svelte';
  import { browserFileSource, SourceError } from './lib/document-source';
  import { isDesktop, pickDesktopPdfs, desktopError, writerError, desktopBatch, DesktopSourceError, type DesktopDescriptor, type WriterStage } from './lib/desktop-source';
  import { invoke } from '@tauri-apps/api/core';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { getCurrentWebview } from '@tauri-apps/api/webview';
  import { carriesFiles, pdfDropHint, type PdfDropHint } from './lib/pdf-drop';
  import { DraftStore, recoveryBranch, type RecoveryDraft } from './lib/draft-store';
  import { createPdfReader, emptyViewState } from './lib/pdf-reader';
  import type { PdfReader } from './lib/pdf-reader';
  import { pageNumber } from './lib/navigation';
  import Thumbnails from './components/Thumbnails.svelte';
  import FolderTree from './components/FolderTree.svelte';
  import {folderWorkspace,type FolderWorkspace,type FolderEntry} from './lib/folder-workspace';
  import Bookmarks from './components/Bookmarks.svelte';
  import DocumentTabs from './components/DocumentTabs.svelte';
  import Icon from './components/Icon.svelte';
 import {toolPalette,defaultToolColors,parseToolColors,serializeToolColors,validToolColor,TOOL_COLOR_KEY} from './lib/annotation-colors.ts';
  import LocalHistory from './components/LocalHistory.svelte';
  import AiChat from './components/AiChat.svelte';
  import PaneDivider from './components/PaneDivider.svelte';
  import { defaultLayout, validLayout, resizedLayout } from './lib/reader-layout';
  import { preferences,defaultInputs,inputAction } from './lib/preferences';
  import InlineExplanation from './components/InlineExplanation.svelte';
  import TextEditorBar from './components/TextEditorBar.svelte';
  import StrokeEditorBar from './components/StrokeEditorBar.svelte';
  import RegionActions from './components/RegionActions.svelte';
  import {pdfShortcut} from './lib/reader-shortcuts';
  import {launchPoll} from './lib/launch-poll';
  import { answerBox } from './lib/inline-session';
  import { selectRegion, type Region } from './lib/ai-context';
  import { LocalData, localDataError, dataStatusWarning, chatUnavailableReason, type DataStatus, type DocumentBinding, type LocalSettings } from './lib/local-data';
  import type { DocumentTab } from './lib/document-tabs';
  import { DocumentTabs as TabStore } from './lib/document-tabs';
  import { PositionStore, contentHash } from './lib/position-store';
  import { zoomPercent, readingKey } from './lib/reading-controls';
  const workspace = new TabStore();
  const localData = new LocalData();
  let dataStatus = $state<DataStatus | null>(null), localBinding = $state<DocumentBinding | null>(null);
  let historyOpen = $state(false), dataNotice = $state('');
  let aiOpen = $state(false), region = $state<Region | null>(null),pendingRegion=$state<Region|null>(null);
  onMount(()=>{const fit=()=>{const width=document.documentElement.clientWidth;if(width>900){layout=resizedLayout(layout,'right',layout.right,width,sidebar||outlineOpen);layout=resizedLayout(layout,'left',layout.left,width,aiOpen);}};window.addEventListener('resize',fit);return()=>window.removeEventListener('resize',fit);});
  let selectingRegion=$state(false), layout=$state(defaultLayout()), layoutReady=false;
  let inputs=$state(defaultInputs());
  let inlineState=$state<{session:string;source:Region|null;box:Region;binding:string;token:number;quick:boolean}|null>(null),placingAnswer=$state(false),inlineConfirming=$state(false);let inlineIntent=0;
  function closeInline(force=false){if(force||!inlineConfirming){inlineIntent++;inlineState=null;}}
  // One call covers both spec paths: a region in hand means "ask about it",
  // no region means the one-click quick explanation with the default prompt.
  async function beginInline(explicit?:Region){if(inlineConfirming)return;if(!reader||!localBinding||!canZoom)return;const chosen=explicit??pendingRegion??region;chooseAnnotation('read');const intent=++inlineIntent,current=reader,b=localBinding,snapshot=view.token,source=chosen?{page:chosen.page,rect:[...chosen.rect] as Region['rect']}:null;const number=source?.page??view.page;try{await current.annotation('read');const p=await current.getPage(number);if(!inlineConfirming&&intent===inlineIntent&&localBinding?.bindingId===b.bindingId&&snapshot===view.token){inlineState={session:crypto.randomUUID(),source,box:{page:number,rect:answerBox(p.view,source)},binding:b.bindingId,token:snapshot,quick:!explicit};region=null;pendingRegion=null;}}catch{openError='原位解释尚未就绪，请等待页面加载。';}}
  function placeAnswer(){if(inlineConfirming)return;if(!reader||!inlineState)return;regionCleanup?.();placingAnswer=true;selectingRegion=true;const binding=inlineState.binding,snapshot=view.token;regionCleanup=selectRegion(container,reader,value=>{regionCleanup=null;placingAnswer=false;selectingRegion=false;if(inlineState?.binding===binding&&view.token===snapshot)inlineState.box=value;},()=>{regionCleanup=null;placingAnswer=false;selectingRegion=false;});}
  onMount(()=>{const load=()=>void preferences.get().then(p=>inputs=p.inputs).catch(()=>{});load();window.addEventListener('reader-preferences-changed',load);return()=>window.removeEventListener('reader-preferences-changed',load);});
  function resizePane(side:'left'|'right',n:number){layout=resizedLayout(layout,side,n,document.documentElement.clientWidth,side==='left'?aiOpen:sidebar||outlineOpen);}
  function consumedSelection(id:string){for(const [tab,b] of bindings)if(b.bindingId===id)aiPages[tab]=[];if(localBinding?.bindingId===id)region=null;}
  $effect(()=>{const value={...layout,aiOpen};if(layoutReady){try{localStorage.setItem('reader-layout-v1',JSON.stringify(value));}catch{/* session-only layout */}}});
  let regionCleanup: (() => void) | null = null;
  const aiPages = $state<Record<number, number[]>>({});
  function beginAiRegion() {
    if (!reader || !canZoom || !activeId||inlineConfirming) return;
    regionCleanup?.();chooseAnnotation('read');closeInline();pendingRegion=null;selectingRegion=true;
    const id = activeId, snapshot = view.token;
    regionCleanup = selectRegion(container, reader, value => {
      regionCleanup = null; selectingRegion=false; if (activeId === id && view.token === snapshot)pendingRegion=value;
    }, () => { regionCleanup = null; selectingRegion=false; });
  }
  function addRegionToChat(){if(!pendingRegion)return;region={page:pendingRegion.page,rect:[...pendingRegion.rect]};pendingRegion=null;aiOpen=true;historyOpen=false;}
  onMount(()=>{const capture=(event:KeyboardEvent)=>{if(!canZoom)return;const target=event.target instanceof Element?event.target:document.activeElement;const action=pdfShortcut(event,!!view.hasSelection,!!target?.closest('input,textarea,select,[contenteditable="true"]'),!!document.querySelector('dialog[open]'));if(!action)return;event.stopImmediatePropagation();if(action==='protect')return;event.preventDefault();if(action==='search'){closeMenu(false);searchOpen=true;void tick().then(()=>searchInput?.focus());}else reader?.edit('delete');};window.addEventListener('keydown',capture,true);return()=>window.removeEventListener('keydown',capture,true);});
  function toggleAiPage(page: number) {
    if (!activeId) return;
    const current = aiPages[activeId] ?? [];
    if (!current.includes(page) && current.length >= 8) { openError = '一次最多显式选择 8 页，未丢弃已有选择。'; return; }
    aiPages[activeId] = current.includes(page) ? current.filter(p => p !== page) : [...current, page].sort((a, b) => a - b);
    aiOpen = true; historyOpen = false;
  }
  let localSettings = $state<LocalSettings | null>(null), settingsTouched = false;
  let settingsWrite: Promise<LocalSettings | null> = Promise.resolve(null);
  function updateLocalSettings(cacheBytes?: number): Promise<LocalSettings> {
    const task = settingsWrite.catch(() => null).then(async () => {
      if (!dataStatus?.available) throw Error('本地设置不可用。');
      const current = localSettings ?? await localData.settings();
      const settings = await localData.updateSettings(sidebar, cacheBytes ?? current.cacheBytes);
      localSettings = settings; return settings;
    });
    settingsWrite = task;
    void task.catch(cause => { if (mounted) dataNotice = localDataError(cause); });
    return task;
  }
  function setSidebar(open: boolean) {
    sidebar = open; settingsTouched = true;
    if (isDesktop() && dataStatus?.available) void updateLocalSettings().catch(() => {});
  }
  const bindings = new Map<number, DocumentBinding>();
  const bindingTasks = new Map<number, Promise<DocumentBinding>>();
  const bindingFailures = $state<Record<number,string>>({});
  const needsReopen = new Set<number>();
  const draftReadLoads = new Map<number, number>();
  const draftWrites = new Map<number, Promise<void>>();
  const positionWrites = new Map<number, Promise<void>>();
  let dataEpoch = 0;
  let readerLoad = 0, positionInteracted = false;
  function localReadingInteraction() { positionInteracted = true; }
  async function acquireBinding(tab: DocumentTab, pageCount: number): Promise<DocumentBinding> {
    if (needsReopen.has(tab.id)) throw Error('备份恢复后需要重新打开文档。');
    const existing = bindings.get(tab.id); if (existing) return existing;
    const pending = bindingTasks.get(tab.id); if (pending) return pending;
    if (!tab.source.grantId) throw Error('无桌面文件授权。');
    const generation = dataEpoch;
    const task = localData.bind(tab.source.grantId, pageCount).then(async binding => {
      if (!mounted || generation !== dataEpoch || !workspace.tabs.some(open => open.id === tab.id)) { await localData.release(binding.bindingId).catch(() => {}); throw Error('标签或数据授权已变化。'); }
      bindings.set(tab.id, binding);
      delete bindingFailures[tab.id];
      if (workspace.active?.id === tab.id) { localBinding = binding; dataNotice = binding.identityNotice || ''; }
      return binding;
    }).finally(() => { if (bindingTasks.get(tab.id) === task) bindingTasks.delete(tab.id); });
    bindingTasks.set(tab.id, task); return task;
  }
  async function attachLocalData(tab: DocumentTab, pageCount: number) {
    const existing = bindings.get(tab.id);
    if (existing) { if (workspace.active?.id === tab.id) { localBinding = existing; void loadDesktopDrafts(tab, existing); } return; }
    if (!tab.source.grantId || !dataStatus?.available || bindingTasks.has(tab.id) || bindingFailures[tab.id]) return;
    const load = readerLoad;
    const generation = dataEpoch;
    const hadSessionPosition = !!tab.position;
    try {
      const binding = await acquireBinding(tab, pageCount);
      if (!mounted || workspace.active?.id !== tab.id || load !== readerLoad) return;
      localBinding = binding; dataNotice = binding.identityNotice || '';
      // The reader already owns exact page/fit/offset restore semantics. At most
      // one controlled reopen applies persisted position; any user interaction
      // cancels that late restore instead of jumping under the reader's cursor.
      if (binding.position && !hadSessionPosition && !positionInteracted && !tab.dirty) { tab.position = binding.position; loadActive(); return; }
      await loadDesktopDrafts(tab, binding);
    } catch (cause) {
      if (generation !== dataEpoch) return;
      bindingFailures[tab.id]=localDataError(cause);
      if (mounted && workspace.active?.id === tab.id) dataNotice = localDataError(cause);
    }
  }
  async function loadDesktopDrafts(tab: DocumentTab, binding: DocumentBinding) {
    if (!tab.hash || tab.dirty || tab.draft?.entries.length) return;
    const load = readerLoad;
    if (draftReadLoads.get(tab.id) === load) return;
    draftReadLoads.set(tab.id, load);
    try {
      const stored = await localData.drafts(binding.bindingId);
      if (!mounted || workspace.active?.id !== tab.id || load !== readerLoad || tab.dirty) return;
      const records: RecoveryDraft[] = stored.map(record => ({ version: 1, id: record.id, hash: tab.hash!, updated: record.updated, draft: record.draft }));
      const legacy = recovery?.list(tab.hash) ?? [];
      // Old localStorage records remain in their original store. Merely opening
      // a PDF never imports/deletes them; a restore creates a new branch.
      recoverable = [...records, ...legacy.filter(record => !records.some(current => current.id === record.id))].sort((a, b) => b.updated - a.updated);
    } catch (cause) { if (mounted && workspace.active?.id === tab.id && load === readerLoad) recoveryNotice = localDataError(cause); }
  }
  function queueDesktopDraft(tab: DocumentTab): Promise<void> {
    if (!dataStatus?.available || needsReopen.has(tab.id)) return Promise.resolve();
    if (!tab.source.grantId || !tab.recoveryId || !tab.draft) return Promise.resolve();
    const id = tab.recoveryId, draft = tab.draft, pageCount = view.pageCount, generation = dataEpoch;
    const previous = draftWrites.get(tab.id) ?? Promise.resolve();
    const task = previous.catch(() => {}).then(async () => {
      if (generation !== dataEpoch) throw Error('数据目录版本已变化。');
      const binding = bindings.get(tab.id) ?? await acquireBinding(tab, pageCount);
      if (draft.entries.length) await localData.putDraft(binding.bindingId, id, draft);
      else await localData.dropDraft(binding.bindingId, id);
    });
    draftWrites.set(tab.id, task);
    void task.catch(cause => { if (mounted && workspace.active?.id === tab.id) recoveryNotice = `${localDataError(cause)} 旧恢复区的副本仍保留。`; });
    return task;
  }
  async function checkpointDraft() {
    const okay = preserveDraft(), tab = workspace.active;
    if (!okay || !tab?.source.grantId) return okay;
    try { await queueDesktopDraft(tab); return true; }
    catch { recoveryNotice = '桌面恢复稿尚未确认保留；旧恢复区副本仍在，请重试或保持窗口。'; return false; }
  }
  async function releaseLocalBinding(id: number) {
    const binding = bindings.get(id); bindings.delete(id); delete bindingFailures[id]; needsReopen.delete(id); draftReadLoads.delete(id); draftWrites.delete(id); positionWrites.delete(id);
    if (binding) await localData.release(binding.bindingId).catch(() => {});
  }
  function migratedData(status: DataStatus) { dataStatus = status; dataNotice = '数据目录已切换，旧目录与原 PDF 保留。'; }
  async function prepareDataChange() {
    if (workspace.active?.dirty && needsReopen.has(workspace.active.id)) throw Error('请重开文档并恢复当前草稿，再迁移或备份。');
    if (!await checkpointDraft()) throw Error('草稿未确认保留。');
    remember();
    await Promise.all([...draftWrites.values(), ...positionWrites.values(), settingsWrite]);
  }
  function restoredData(status: DataStatus) {
    dataEpoch++; readerLoad++; dataStatus = status; localBinding = null; bindings.clear(); bindingTasks.clear(); draftReadLoads.clear();
    for (const tab of workspace.tabs) { bindingFailures[tab.id]='数据恢复后授权已撤销，请关闭此 PDF 后重新打开。'; needsReopen.add(tab.id); }
    draftWrites.clear(); positionWrites.clear();
    dataNotice = '备份已恢复，原目录保留；请关闭并重新打开 PDF 后继续使用历史。';
    void localData.settings().then(settings => { if (mounted) { localSettings = settings; sidebar = settings.thumbnailOpen; } }).catch(() => {});
  }
  let tabs = $state(workspace.tabs), activeId = $state<number | null>(null);
  const chatUnavailable = $derived(chatUnavailableReason(dataStatus,activeId?bindingFailures[activeId]||'':'',!!workspace.active));
  const selectedAiPages = $derived(activeId ? aiPages[activeId] ?? [] : []);
  let positions: PositionStore | null = null;
  let recovery: DraftStore | null = null;
  let recoverable = $state<RecoveryDraft[]>([]), recoveryNotice = $state('');
  let draftTimer: ReturnType<typeof setTimeout> | undefined;
  let closeDialog: HTMLDialogElement, closeTarget: number | 'window' | null = null;
  let closePreserved=$state(true),closeSaving=$state(false);
  let annotationMode = $state<'read'|'text'|'highlight'|'ink'|'erase'>('read');
 // Colour choices live in lib/annotation-colors.ts so they can be tested.
 let toolColor=$state<Record<'highlight'|'ink',string>>(defaultToolColors());
 let paletteFor=$state<'highlight'|'ink'|null>(null);
 function readToolColors(){try{toolColor=parseToolColors(localStorage.getItem(TOOL_COLOR_KEY));}catch{}}
 readToolColors();
 /** Applies the remembered colour so the next new annotation uses it. */
 function applyToolColor(kind:'highlight'|'ink'){try{reader?.strokeStyle(kind,toolColor[kind]);}catch{}}
 function pickToolColor(kind:'highlight'|'ink',color:string){if(!validToolColor(kind,color))return;toolColor={...toolColor,[kind]:color};paletteFor=null;try{localStorage.setItem(TOOL_COLOR_KEY,serializeToolColors(toolColor));}catch{}try{reader?.strokeStyle(kind,color);}catch{}}
 function togglePalette(kind:'highlight'|'ink'){closeMenu();paletteFor=paletteFor===kind?null:kind;}
  const writerBlocked = '桌面端可保存原件或另存副本；遇到外部修改或不支持的文件时会拒绝原件保存。浏览器中请使用桌面版保存。';
  function preserveDraft(commit=true) {
    const tab=workspace.active;
    if (!reader || !tab || view.status!=='ready')return true;
    try {
      const draft=reader.draft(commit); tab.draft=draft; tab.dirty=view.dirty||draft.entries.length>0; refreshTabs();
      if(tab.dirty&&!tab.hash){recoveryNotice='无法核对文档内容，草稿尚不能安全保存；请保留窗口。';return false;}
      if(tab.dirty&&tab.hash){tab.recoveryId??=crypto.randomUUID(); if(!recovery?.save(tab.recoveryId,tab.hash,draft)) {recoveryNotice=recovery?.warning||'恢复区不可用，请保留窗口。';return false;}}
      else if(tab.recoveryId&&!recovery?.discard(tab.recoveryId)) {recoveryNotice=recovery?.warning||'恢复区不可用，请保留窗口。';return false;}
      recoveryNotice=tab.source.grantId&&tab.dirty ? needsReopen.has(tab.id) ? '备份恢复后旧授权已撤销；当前草稿仅保留在旧恢复区，重新打开前不会写入已恢复的数据目录。' : !dataStatus?.available ? '仅在旧恢复区保留；桌面数据目录尚不可用，迁移不会包含这份旧副本。' : '' : '';
      if (tab.recoveryId && tab.source.grantId) void queueDesktopDraft(tab).catch(() => {});
      return true;
    } catch(error) { recoveryNotice=error instanceof Error?error.message:'草稿未能保留，请保持窗口。'; return false; }
  }
  function restoreDraft(record:RecoveryDraft) {
    const tab=workspace.active; if(!tab||tab.hash!==record.hash||tab.dirty)return;
    const branch=recoveryBranch(record,tab.hash);tab.draft=branch.draft;tab.recoveryId=branch.id;tab.dirty=true;recoverable=[];loadActive();
  }
  function chooseAnnotation(mode:typeof annotationMode) {if(inlineConfirming)return;inlineIntent++;regionCleanup?.();regionCleanup=null;selectingRegion=false;placingAnswer=false;pendingRegion=null;if(mode!=='read')inlineState=null;annotationMode=mode;void reader?.annotation(mode).catch(()=>{});container?.setAttribute('data-input-mode',mode);if(mode==='highlight'||mode==='ink')void reader?.annotation(mode).then(()=>applyToolColor(mode)).catch(()=>{});paletteFor=null;closeMenu();}
  function savePdf() {
    if(workspace.active?.source.saveOriginal){void saveOriginalCopy();return;}
    if (closeDialog?.open) { void checkpointDraft().then(okay => { closePreserved = okay; openError = okay ? writerBlocked : `${recoveryNotice} 原 PDF 未修改。`; }); }
    else openError=preserveDraft()?writerBlocked:`${recoveryNotice} 原 PDF 未修改。`;
  }
  let copying=$state(false),savingOriginal=$state(false),savedNotice=$state('');
  let noticeTimer:ReturnType<typeof setTimeout>|undefined;
  function saved(message:string){openError='';savedNotice=message;clearTimeout(noticeTimer);noticeTimer=setTimeout(()=>savedNotice='',5000);}
  type PendingRecovery={eventId:string;state:'completeOld'|'completeNew'|'incompletePreserved'|'externalChangePreserved'};
  let pendingRecovery=$state<PendingRecovery[]>([]);
  async function findSaveRecovery(tab:DocumentTab){
    if(!tab.source.grantId)return;const grant=tab.source.grantId;
    try{const result=await invoke<PendingRecovery[]>('pending_pdf_recovery',{grantId:grant});if(workspace.active===tab&&tab.source.grantId===grant)pendingRecovery=result;}
    catch(cause){if(workspace.active===tab&&!openError)openError=writerError(cause,'original');}
  }
  async function recoverSaved(item:PendingRecovery,newer:boolean){
    const tab=workspace.active;if(copying||!tab?.source.grantId)return;copying=true;
    try{const d=await invoke<DesktopDescriptor|null>('export_pdf_recovery',{grantId:tab.source.grantId,eventId:item.eventId,newer});
      if(d){await pendingCopy?.release?.();pendingCopy=desktopBatch({documents:[d],rejected:[]}).sources[0];pendingCopyOwner=tab.id;saved('恢复副本已保存；原件和恢复材料保留。');}}
    catch(cause){openError=writerError(cause,'original');}finally{copying=false;}
  }
  let writerChecking=$state(false),copyAbort:AbortController|null=null;
  let pendingCopy=$state.raw<import('./lib/document-source').PdfSource|null>(null);
  let pendingCopyOwner:number|null=null,copyOpenDialog:HTMLDialogElement;
  let copyOpenConfirm=$state.raw<{tab:number;source:import('./lib/document-source').PdfSource;name:string}|null>(null);
  async function exportTestCopy() {
    const tab=workspace.active;if(copying||!reader||!tab?.source.testCopy)return;
    copying=true;closeMenu(false);openError='';
    try {
      const bytes=await reader.testWriterBytes();const copy=await tab.source.testCopy(bytes);
      if(!mounted){await copy.release?.();return;}
      await pendingCopy?.release?.();if(!mounted){await copy.release?.();return;}pendingCopy=copy;
      openError='验证副本已生成，包含导出时的批注；当前标签和后续修改继续保留。输入 PDF 未修改。';
    }catch(error){openError=error instanceof Error?error.message:'验证副本未完成，原 PDF 未修改。';}
    finally{copying=false;}
  }
  async function checkWriterIsolation(){
    if(writerChecking||!isDesktop())return;writerChecking=true;closeMenu(false);openError='';
    try{openError=await invoke<string>('check_unicode_writer');}
    catch(cause){openError=writerError(cause,'check');}
    finally{writerChecking=false;}
  }
  async function exportCandidateCopy(){
    const tab=workspace.active,current=reader;if(copying||!current||!tab?.source.exportCopy)return;
    const documentToken=view.token;
    const stillCurrent=()=>workspace.active===tab&&reader===current&&view.token===documentToken;
    copying=true;copyAbort=new AbortController();const abort=copyAbort;let stage:WriterStage='check';closeMenu(false);openError='';
    const cancelCheck=()=>void invoke('cancel_unicode_export',{taskId:''}).catch(()=>{});
    abort.signal.addEventListener('abort',cancelCheck,{once:true});
    try{
      const draftRetained=await checkpointDraft();
      abort.signal.throwIfAborted();
      if(!stillCurrent())throw {code:'writer_document_changed'};
      // The release asset resolver verifies the byte writer before conversion.
      await invoke<string>('check_unicode_writer');abort.signal.throwIfAborted();
      if(!stillCurrent())throw {code:'writer_document_changed'};
      stage='serialize';const bytes=await current.testWriterBytes();abort.signal.throwIfAborted();
      if(!stillCurrent())throw {code:'writer_document_changed'};
      stage='finish';
      const copy=await tab.source.exportCopy(bytes,abort.signal);
      if(!copy){if(mounted)openError='已取消导出；原 PDF 和恢复稿保留。';return;}
      if(!mounted||abort.signal.aborted){await copy.release?.();return;}
      await pendingCopy?.release?.();pendingCopy=copy;pendingCopyOwner=tab.id;
      saved('批注副本已保存。');if(!draftRetained)recoveryNotice='当前编辑尚不能转为恢复稿；已保存的 PDF 副本可独立打开。';
    }catch(cause){if(mounted)openError=abort.signal.aborted?'导出已取消；原 PDF 未修改。':writerError(cause,stage);}
    finally{abort.signal.removeEventListener('abort',cancelCheck);copying=false;if(copyAbort===abort)copyAbort=null;}
  }
  async function saveOriginalCopy():Promise<boolean>{
    const tab=workspace.active,current=reader;if(copying||!current||!tab?.source.saveOriginal)return false;
    if(inlineState){openError='请先确认或取消当前原位解释，再保存 PDF。解释内容已保留。';return false;}
    if(!view.dirty&&!tab.dirty){saved('没有需要保存的修改。');return true;}
    const oldSource=tab.source;copying=true;savingOriginal=true;closeMenu(false);openError='';savedNotice='正在保存…';clearTimeout(draftTimer);draftTimer=undefined;
    try{
      // Existing-annotation edits may not fit the JSON recovery subset. They
      // remain in the editor; the complete PDF is backed up before any write.
      await checkpointDraft();remember();
      await Promise.all([draftWrites.get(tab.id)?.catch(()=>{}),positionWrites.get(tab.id)?.catch(()=>{}),bindingTasks.get(tab.id)?.catch(()=>{})]);
      await invoke('check_unicode_writer');
      const candidate=await current.testWriterBytes();
      if(workspace.active!==tab||tab.source!==oldSource)throw {code:'writer_document_changed'};
      const result=await oldSource.saveOriginal!(candidate,new AbortController().signal,bindings.get(tab.id)?.bindingId);
      if(!mounted){await result.source?.release?.();return false;}
      if(!result.source){needsReopen.add(tab.id);bindings.delete(tab.id);localBinding=null;openError='PDF 已写入，但重新取得文件授权失败。请关闭后重新打开文件核对；备份和恢复稿保留，不要重复保存。';return false;}
      bindings.delete(tab.id);draftReadLoads.delete(tab.id);draftWrites.delete(tab.id);positionWrites.delete(tab.id);delete bindingFailures[tab.id];
      if(result.binding){bindings.set(tab.id,result.binding);localBinding=result.binding;}
      else{localBinding=null;if(result.historyPending)bindingFailures[tab.id]='PDF 已保存，历史关联尚未完成；旧历史与草稿保留，请重开检查。';}
      tab.source=result.source;tab.hash=result.hash;tab.draft=undefined;tab.dirty=false;tab.recoveryId=undefined;tab.recoveryChecked=false;recoveryNotice='';recoverable=[];
      await oldSource.release?.();await loadActive();
      if(result.historyPending)openError='PDF 已保存，但历史关联尚未完成。请重新打开 PDF 核对，旧历史与草稿保留。';
      else saved(result.cleanupPending?'已保存；备份整理未完成，材料已保留。':'已保存。');
      return !result.historyPending;
    }catch(cause){if(mounted){
      openError=writerError(cause,'original');
      if(cause instanceof DesktopSourceError&&cause.recoverySource){
        tab.source=cause.recoverySource;bindings.delete(tab.id);localBinding=null;
        bindingFailures[tab.id]='保存未完成，原编辑仍保留；请先检查恢复材料。';
        // Rebind history only if the retained file still has the exact old
        // bytes. A partial append never becomes a new document identity here.
        try{const bytes=await tab.source.read(new AbortController().signal);if(await contentHash(bytes,new AbortController().signal)===tab.hash&&dataStatus?.available){delete bindingFailures[tab.id];await acquireBinding(tab,view.pageCount);}}catch{/* Keep the editor and explicit recovery authority. */}
      }
      void findSaveRecovery(tab);
    }}
    finally{copying=false;savingOriginal=false;refreshTabs();}
    return false;
  }
  async function saveClosingDocuments(){
    if(closeSaving||savingOriginal||closeTarget===null)return;
    const target=closeTarget,previous=workspace.activeId;
    closeSaving=true;openError='';
    try{
      const pending=workspace.tabs.filter(tab=>tab.dirty&&(target==='window'||tab.id===target));
      for(const tab of pending){
        if(workspace.active!==tab){
          if(!await checkpointDraft())return;
          remember();workspace.activate(tab.id);await loadActive();
        }
        // Opening resolves before PDF.js has restored all page/editor geometry.
        const deadline=performance.now()+15000;
        while(mounted&&workspace.active===tab&&(!canZoom||!view.saveReady||view.status==='loading')){
          if(view.status==='error'||performance.now()>deadline)throw Error('close-reader-not-ready');
          await new Promise<void>(resolve=>requestAnimationFrame(()=>resolve()));
        }
        if(!mounted||workspace.active!==tab||!canZoom)return;
        if(!tab.source.saveOriginal){openError=writerBlocked;return;}
        if(!await saveOriginalCopy()||tab.dirty||needsReopen.has(tab.id))return;
        const reloaded=performance.now()+15000;
        while(mounted&&workspace.active===tab&&!view.saveReady){
          if(view.status==='error'||performance.now()>reloaded)throw Error('close-reader-not-ready');
          await new Promise<void>(resolve=>requestAnimationFrame(()=>resolve()));
        }
      }
      await finishClose(false,target);
      if(target!=='window'&&previous!==target&&previous!==null&&workspace.tabs.some(tab=>tab.id===previous))await activate(previous);
    }catch{openError='待关闭文档尚未准备好，未继续关闭；请保留窗口后重试。';}
    finally{closeSaving=false;}
  }
  function openTestCopy(){
    if(!pendingCopy)return;
    if(!preserveDraft()){
      const tab=workspace.active;
      if(tab&&tab.id===pendingCopyOwner){copyOpenConfirm={tab:tab.id,source:pendingCopy,name:tab.source.name};copyOpenDialog.showModal();}
      return;
    }
    const copy=pendingCopy;pendingCopy=null;pendingCopyOwner=null;remember();workspace.add([copy]);loadActive();
  }
  function confirmOpenSavedCopy(){
    const choice=copyOpenConfirm;
    if(!choice||pendingCopy!==choice.source||workspace.activeId!==choice.tab||copying)return;
    copyOpenDialog.close();copyOpenConfirm=null;
    // Explicitly closing this in-memory edit is allowed only by this choice.
    // Keep every existing recovery record and the PDF-bound history untouched.
    // Commit the explicitly confirmed tab switch synchronously: an awaited
    // cleanup must never leave an editable old tab that is closed afterwards.
    remember();void releaseLocalBinding(choice.tab);
    workspace.close(choice.tab);pendingCopy=null;pendingCopyOwner=null;
    workspace.add([choice.source]);loadActive();
    saved('已打开批注副本。');
  }
  async function requestClose(id:number|'window') {
    if(savingOriginal||closeSaving)return;
    if(id==='window'){closingWindow=true;openEpoch++;}
    closePreserved=await checkpointDraft();
    const dirty=id==='window'?workspace.tabs.some(tab=>tab.dirty):workspace.tabs.find(tab=>tab.id===id)?.dirty;
    if(dirty||!closePreserved){closeTarget=id;if(!closeDialog.open)closeDialog.showModal();} else void finishClose(false,id);
  }
  async function finishClose(discard:boolean,target:number|'window'|null=closeTarget) {
    if(target===null)return;
    if(!discard)for(const tab of workspace.tabs.filter(tab=>tab.dirty&&(target==='window'||tab.id===target))){if(!tab.hash||!tab.recoveryId||!recovery?.list(tab.hash).some(record=>record.id===tab.recoveryId)){recoveryNotice='此草稿尚未安全保留，请保持窗口。';closePreserved=false;return;}}
    const closing = workspace.tabs.filter(tab => target === 'window' || tab.id === target);
    if (!discard) try { await Promise.all(closing.map(tab => draftWrites.get(tab.id))); } catch { recoveryNotice = '桌面恢复稿未确认保留，请重试并保持窗口。'; closePreserved = false; return; }
    if(discard)for(const tab of closing)if(tab.recoveryId){
      const binding = bindings.get(tab.id);
      if (binding) try { await (draftWrites.get(tab.id) ?? Promise.resolve()).catch(() => {}); await localData.dropDraft(binding.bindingId, tab.recoveryId); } catch { recoveryNotice = '未能放弃桌面恢复稿，已有内容保留。'; return; }
      if(!recovery?.discard(tab.recoveryId)){recoveryNotice=recovery?.warning||'草稿保留失败。';return;}
    }
    closeDialog.close();closeTarget=null;
    remember();
    await Promise.all(closing.map(tab => positionWrites.get(tab.id)?.catch(() => {})));
    for (const tab of closing) await releaseLocalBinding(tab.id);
    if(target==='window'){await getCurrentWindow().destroy();}
    else {if(workspace.close(target))loadActive();else refreshTabs();}
  }
  let opening = $state(false), openError = $state('');
  let folder=$state<FolderWorkspace|null>(null),folderView=$state<'files'|'pages'>('files');
  const folderTabs=new Map<string,number>();
  async function openFolder(){
    if(opening||savingOriginal||!isDesktop())return;opening=true;closeMenu(false);openError='';
    try{const chosen=await folderWorkspace.pick();if(!chosen)return;
      if(!mounted){await folderWorkspace.close(chosen.id);return;}
      const previous=folder;
      try{if(previous)await folderWorkspace.close(previous.id);}catch(error){await folderWorkspace.close(chosen.id).catch(()=>{});throw error;}
      if(!mounted){await folderWorkspace.close(chosen.id);return;}
      folder=chosen;folderTabs.clear();folderView='files';setSidebar(true);
    }catch{if(mounted)openError='文件夹未能打开。请选择普通本地课程文件夹，链接文件夹不受支持。';}
    finally{if(mounted)opening=false;}
  }
  async function closeFolder(){const previous=folder;if(!previous||opening)return;opening=true;
    try{await folderWorkspace.close(previous.id);folder=null;folderTabs.clear();folderView='pages';}
    catch{openError='工作区未能关闭，请重试。';}finally{opening=false;}}
  async function openFolderPdf(entry:FolderEntry){
    const current=folder;if(!current||opening||savingOriginal)return;
    const existing=folderTabs.get(entry.id);if(existing&&workspace.tabs.some(t=>t.id===existing)){await activate(existing);return;}
    opening=true;openError='';
    try{const descriptor=await folderWorkspace.open(current.id,entry.id);const source=desktopBatch({documents:[descriptor],rejected:[]}).sources[0];
      if(await addSources([source],()=>mounted&&folder?.id===current.id)&&workspace.active)folderTabs.set(entry.id,workspace.active.id);
    }catch{if(mounted)openError='PDF 未能打开，可能已删除、被占用或不在工作区范围。';}finally{if(mounted)opening=false;}
  }
  let mounted = false,closingWindow=false,openEpoch=0;
  async function openFile() {
    closeMenu(false); openError = '';
    if (!isDesktop()) { fileInput.click(); return; }
    if (opening||savingOriginal) return;
    opening = true;
    try {
      const result = await pickDesktopPdfs();
      if (!mounted) { for (const source of result.sources) void source.release?.(); return; }
      if (result.sources.length) await addSources(result.sources);
      openError = result.warning;
    } catch (error) { if (mounted) openError = desktopError(error); }
    finally { if (mounted) opening = false; }
  }
  let positionNotice = $state('只记最近 50 份文件的位置；刷新后需重新选择文件，不保存 PDF。');
  let positionTimer: ReturnType<typeof setTimeout> | undefined;
  function refreshTabs() { tabs = [...workspace.tabs]; activeId = workspace.activeId; }
  function remember() {
    clearTimeout(positionTimer); positionTimer = undefined;
    const position = reader?.position() ?? null;
    workspace.remember(position);
    const tab = workspace.active;
    const binding = tab && bindings.get(tab.id);
    if (position && tab?.source.grantId && binding) {
      const previous = positionWrites.get(tab.id) ?? Promise.resolve();
      const task = previous.catch(() => {}).then(() => localData.position(binding.bindingId, position));
      positionWrites.set(tab.id, task);
      void task.catch(cause => { if (mounted && workspace.active?.id === tab.id) positionNotice = localDataError(cause); });
    } else if (position && tab?.hash && positions && !tab.source.grantId) {
      positions.save(tab.hash, position);
      if (positions.warning) positionNotice = positions.warning;
    }
  }
  function loadActive() {
    regionCleanup?.(); regionCleanup = null; selectingRegion=false; region = null;pendingRegion=null;
    inlineIntent++;inlineState=null;placingAnswer=false;
    refreshTabs(); const tab = workspace.active;
    readerLoad++; positionInteracted = false;
    localBinding = tab ? bindings.get(tab.id) ?? null : null;
    dataNotice = tab && needsReopen.has(tab.id) ? '备份已恢复；请关闭并重新打开此 PDF 后继续使用历史。' : localBinding?.identityNotice || '';
    pendingRecovery=[];if(tab)void findSaveRecovery(tab);
    recoverable=[];annotationMode='read';passwordValue='';
    if (!tab) { void reader?.close(); return; }
    const opened=reader?.open({ name: tab.source.name, size: tab.source.size, async read(signal) {
      const bytes = await tab.source.read(signal);
      if (!tab.hash) {
        try { tab.hash = await contentHash(bytes, signal); }
        catch { signal.throwIfAborted(); positionNotice = '本次会话保留位置；此浏览器无法识别内容以恢复跨会话位置。'; }
      }
      return bytes;
    } }, () => tab.position ?? (tab.source.grantId ? bindings.get(tab.id)?.position : positions?.get(tab.hash)), tab.draft);
    const binding = bindings.get(tab.id); if (binding) void loadDesktopDrafts(tab, binding);
    return opened;
  }
  async function activate(id: number) { if(savingOriginal)return;if(!await checkpointDraft())return; remember(); if (workspace.activate(id)) loadActive(); }
  function closeTab(id: number) {void requestClose(id);}
  async function addSources(sources: import('./lib/document-source').PdfSource[],active=()=>true,releaseRejected=true) {const epoch=openEpoch;let adopted=false;try{if(!savingOriginal&&mounted&&!closingWindow&&active()&&await checkpointDraft()&&mounted&&!closingWindow&&reader&&active()&&epoch===openEpoch){remember();workspace.add(sources);adopted=true;loadActive();}return adopted;}finally{if(!adopted&&releaseRejected)for(const source of sources)await Promise.resolve(source.release?.()).catch(()=>{});}}
  async function openAiSource(descriptor: DesktopDescriptor, page: number) {
    const source = desktopBatch({ documents: [descriptor], rejected: [] }).sources[0];
    if (!await checkpointDraft()) { await source.release?.(); return; }
    remember(); workspace.add([source]);
    if (workspace.active) workspace.active.position = { page, zoom: 'page-width', top: 0, left: 0 };
    loadActive();
  }

  let view = $state(emptyViewState());
  let container = $state<HTMLDivElement>(null!);
  let pages: HTMLDivElement;
  let fileInput: HTMLInputElement;
  let reader = $state.raw<PdfReader | null>(null);
  let sidebar = $state(false), pageInput = $state('1'), pageError = $state(''), pageEditing = $state(false), zoomEditing = $state(false);
  let outlineOpen = $state(false), passwordValue = $state('');
  let previousPage = 0;
  let searchOpen = $state(false), query = $state('');
  let searchInput = $state<HTMLInputElement>();
  let zoomInput = $state('100'), zoomError = $state(''), previousScale = 0;
  const zoomDisplay = $derived(String(Number((view.scale * 100).toFixed(2))));
  $effect(() => { if (previousScale !== view.scale) { previousScale = view.scale; if (!zoomEditing) zoomInput = zoomDisplay; zoomError = ''; } });
  function applyZoom() { const value = zoomPercent(zoomInput); if (value === null) zoomError = '请输入 25–400 的缩放百分比'; else { zoomError = ''; reader?.zoom(value); closeMenu(); } }
  // Presentation state only; the reader and document state stay in their existing modules.
  let menu = $state<'zoom' | 'more' | null>(null);
  let popover = $state<HTMLDivElement>();
  let menuTrigger: HTMLButtonElement | undefined;
  let searchButton=$state<HTMLButtonElement>();
  let thumbnailButton: HTMLButtonElement;
  let helpDialog: HTMLDialogElement;
  function closeMenu(restoreFocus = true) {
    menu = null; zoomEditing = false; zoomInput = zoomDisplay;
    if (restoreFocus) menuTrigger?.focus();
  }
  function toggleMenu(kind: 'zoom' | 'more', event: MouseEvent) {
    const open = menu !== kind;
    closeMenu(false);
    menuTrigger = event.currentTarget as HTMLButtonElement;
    menu = open ? kind : null;
  }
  $effect(() => { if (menu && popover) popover.querySelector<HTMLButtonElement>('button')?.focus(); });
  function closeSearch() { searchOpen = false; reader?.clearSearch(); searchButton?.focus(); }
  function toggleSearch() { closeMenu(false); if (searchOpen) closeSearch(); else searchOpen = true; }
  function chooseZoom(value: import('./lib/pdf-reader').Zoom) {
    reader?.zoom(value);
    // Fit-page is an explicit request to see the whole page, not its old scroll offset.
    if (value === 'page-fit') reader?.goToPage(view.page);
    closeMenu();
  }
  function showHelp() { closeMenu(false); helpDialog.showModal(); }
  function outsideMenu(event: PointerEvent) {
    if ((event.target as Element).closest('.toolbar, .reading-layout')) localReadingInteraction();
    if (menu && !(event.target as Element).closest('[data-ui-menu]')) closeMenu(false);
  }
  function dismiss(event: KeyboardEvent) {
    if(document.querySelector('dialog[open]'))return;
    if (canZoom) localReadingInteraction();
    const target=event.target instanceof Element?event.target:document.activeElement;const mapped=inputAction(event,inputs,!!target?.closest('input,textarea,select,[contenteditable="true"]'));
    if(mapped&&canZoom&&!helpDialog?.open&&!closeDialog?.open){event.preventDefault();if(mapped==='region')beginAiRegion();else if(mapped==='quick'){aiOpen=true;if(region)window.dispatchEvent(new Event('reader-quick-question'));else beginAiRegion();}else if(mapped==='inline')beginInline();else if(mapped==='ai')aiOpen=!aiOpen;else chooseAnnotation(mapped);return;}
    if (event.ctrlKey&&!event.altKey&&!event.shiftKey&&event.key.toLowerCase()==='s'&&!event.isComposing&&canZoom) {event.preventDefault();savePdf();return;}
    if (isDesktop() && event.ctrlKey && !event.altKey && !event.shiftKey && event.key.toLowerCase() === 'o' && !event.isComposing) {
      event.preventDefault(); void openFile(); return;
    }
    if (event.key !== 'Escape' || event.isComposing || helpDialog?.open || closeDialog?.open) return;
    inlineIntent++;
    if(selectingRegion){event.preventDefault();regionCleanup?.();regionCleanup=null;selectingRegion=false;placingAnswer=false;}
    else if(pendingRegion){event.preventDefault();pendingRegion=null;}else if(inlineState){event.preventDefault();closeInline();}
    else if(annotationMode!=='read'){event.preventDefault();chooseAnnotation('read');}
    else if (menu) { event.preventDefault(); closeMenu(); }
    else if (searchOpen) { event.preventDefault(); closeSearch(); }
    else if (sidebar && window.matchMedia('(max-width: 720px)').matches) { event.preventDefault(); closeSidebar(); }
  }
  function closeSidebar() { setSidebar(false); thumbnailButton?.focus(); }
  let dropHint=$state<PdfDropHint>(null),dragDepth=0;
  function dragFiles(event:DragEvent){
    if(!carriesFiles(event.dataTransfer))return;
    event.preventDefault();
    if(event.type==='dragenter')dragDepth++;
    if(!activeId)dropHint=pdfDropHint(Array.from(event.dataTransfer?.files??[]).map(file=>file.name))??'files';
  }
  function leaveFiles(event:DragEvent){if(!carriesFiles(event.dataTransfer))return;if(--dragDepth<=0){dragDepth=0;dropHint=null;}}
  function dropFiles(event: DragEvent) {
    dragDepth=0;dropHint=null;
    if(!carriesFiles(event.dataTransfer))return;
    event.preventDefault();
    if (!isDesktop() && event.dataTransfer?.files.length) addSources(Array.from(event.dataTransfer.files).map(browserFileSource));
  }
  function jumpThumbnail(number: number) {
    reader?.goToPage(number);
    if (window.matchMedia('(max-width: 720px)').matches) setSidebar(false);
    container.focus({ preventScroll: true });
  }
  function keyboard(event: KeyboardEvent) {
    if(document.querySelector('dialog[open]'))return;
    const target = event.target as HTMLElement;
    if(target.closest('.annotationEditorLayer')||view.hasSelection&&annotationMode!=='read')return;
    const action = readingKey(event, !!target.closest('input,textarea,select,button,a,[contenteditable="true"]'));
    if (!action || !canZoom) return;
    event.preventDefault();
    if (action === 'next') reader?.goToPage(Math.min(view.pageCount, view.page + 1));
    if (action === 'previous') reader?.goToPage(Math.max(1, view.page - 1));
    if (action === 'first') reader?.goToPage(1);
    if (action === 'last') reader?.goToPage(view.pageCount);
    if (action === 'in') reader?.zoomBy(1.2);
    if (action === 'out') reader?.zoomBy(1 / 1.2);
    if (action === 'search') searchOpen = true;
  }
  $effect(() => { if (searchOpen && searchInput) searchInput.focus(); });
  let previousToken = 0;
  $effect(() => { if (view.token !== previousToken) { previousToken = view.token; query = ''; menu = null; zoomEditing = false; zoomInput = zoomDisplay; pageError = ''; zoomError = ''; } });
  function find(previous = false) { reader?.search(query, previous); }
  $effect(() => { if (view.page !== previousPage) { previousPage = view.page; if (!pageEditing) pageInput = String(view.page || 1); pageError = ''; } });
  function jump() {
    const number = pageNumber(pageInput, view.pageCount);
    if (number === null) { pageError = `请输入 1–${view.pageCount} 的整数页码`; return; }
    pageError = ''; reader?.goToPage(number);
  }
  const canZoom = $derived(view.status === 'ready' && view.viewReady);
  const notice = $derived(openError || zoomError || pageError || (view.status === 'ready' ? view.error : '') || savedNotice || dataStatusWarning(dataStatus) || (view.status === 'ready' ? view.textNotice : ''));

  onMount(() => {
    try{layout=validLayout(JSON.parse(localStorage.getItem('reader-layout-v1')??'null'));aiOpen=layout.aiOpen;}catch{layout=defaultLayout();}layoutReady=true;
    mounted = true;
    let stopDrop: (() => void) | undefined,stopDrag:(()=>void)|undefined,stopClose:(()=>void)|undefined;
    if(isDesktop())void getCurrentWebview().onDragDropEvent(event=>{const payload=event.payload;if(payload.type==='leave'||payload.type==='drop'){dropHint=null;dragDepth=0;}else if(payload.type==='enter'&&!activeId){dropHint=pdfDropHint(payload.paths);}}).then(stop=>{if(mounted)stopDrag=stop;else stop();}).catch(()=>{/* The original native drop/open handler remains available. */});
    if(isDesktop())void getCurrentWindow().onCloseRequested(event=>{event.preventDefault();void requestClose('window');}).then(stop=>{if(mounted)stopClose=stop;else stop();});
    if (isDesktop()) void getCurrentWindow().listen<{ documents: DesktopDescriptor[]; rejected: unknown[] }>('opened-pdfs', event => {
      try { const result = desktopBatch(event.payload); if (mounted) { if (result.sources.length) void addSources(result.sources); openError = result.warning; }
        else for (const source of result.sources) void source.release?.();
      } catch { if (mounted) openError = '拖入文件未能打开，请使用文件选择器重试。'; }
    }).then(stop => { if (mounted) stopDrop = stop; else stop(); }).catch(() => { if (mounted) openError = '拖入功能暂时不可用，请使用文件选择器。'; });
    let storage: Storage | null = null;
    try { storage = localStorage; } catch { /* Restricted browser profile: session-only fallback. */ }
    positions = new PositionStore(storage);
    recovery=new DraftStore(storage);
    if (positions.warning) positionNotice = positions.warning;
    if (isDesktop()) void invoke<{ notice: string }>('get_settings').then(settings => {
      if (mounted && settings.notice) positionNotice += ` ${settings.notice}`;
    }).catch(() => { if (mounted) positionNotice += ' 桌面设置暂时不可用，已有内容未修改。'; });
    if (isDesktop()) void localData.status().then(status => {
      if (!mounted) return;
      dataStatus = status;
      positionNotice = status.available ? '桌面按已授权文件身份保存阅读位置与本地历史；副本和内容变化会建立独立历史。旧浏览器位置与草稿保留。' : '桌面本地数据暂时不可用；本次会话位置仍保留，原文件未修改。';
      if (status.available && workspace.active && canZoom) void attachLocalData(workspace.active, view.pageCount);
      if (status.available && workspace.active?.dirty) void checkpointDraft();
      if (status.available) void localData.settings().then(settings => {
        if (!mounted) return;
        localSettings = settings;
        if (!settingsTouched) sidebar = settings.thumbnailOpen;
        else void updateLocalSettings().catch(() => {});
      }).catch(cause => { if (mounted) dataNotice = localDataError(cause); });
    }).catch(cause => { if (mounted) { dataNotice = localDataError(cause); dataStatus = { available: false, path: '', areas: [], acl: '', generation: 0, error: dataNotice }; } });
    reader = createPdfReader(container, pages, next => {
      view = next;
      if(next.annotationMode){annotationMode=next.annotationMode;container.dataset.inputMode=annotationMode;}
      const tab=workspace.active;
      if(next.status==='ready'&&next.viewReady&&tab&&!savingOriginal){
        // The ready reader includes pending restore/drawing state. A cached
        // draft is an older snapshot, not evidence that a just-undone/deleted
        // stroke still exists. Keep persisted recovery records untouched here.
        tab.dirty=next.dirty;if(!next.dirty)tab.draft=undefined;refreshTabs();
        if(!tab.recoveryChecked&&tab.hash){tab.recoveryChecked=true;if(!tab.draft?.entries.length)recoverable=recovery?.list(tab.hash)??[];if(recovery?.warning)recoveryNotice=recovery.warning;}
        if(next.dirty&&!draftTimer)draftTimer=setTimeout(()=>{draftTimer=undefined;preserveDraft(false);},700);
        if (tab.source.grantId) void attachLocalData(tab, next.pageCount);
      }
      if (next.status === 'ready' && next.viewReady && !positionTimer) positionTimer = setTimeout(remember, 600);
    });
    window.addEventListener('pagehide', remember);
    const leaving=(event:BeforeUnloadEvent)=>{preserveDraft();if(workspace.tabs.some(tab=>tab.dirty)){event.preventDefault();event.returnValue='';}};
    window.addEventListener('beforeunload',leaving);
    const stopLaunch=isDesktop()?launchPoll<{documents:DesktopDescriptor[];rejected:unknown[]}>({take:()=>invoke('m7_open_pending'),accept:async(batch,active)=>{const result=desktopBatch(batch);const adopted=!result.sources.length||await addSources(result.sources,active,false);if(mounted&&result.warning)openError=result.warning;return adopted;},release:async batch=>{for(const d of batch.documents)await invoke('release_pdf',{grantId:d.grantId}).catch(()=>{});},error:()=>{if(mounted)openError='启动文件请求未能处理；请使用打开按钮重试。';}}):undefined;
    return () => { mounted = false;if(folder)void folderWorkspace.close(folder.id).catch(()=>{});copyAbort?.abort();openEpoch++;stopLaunch?.(); readerLoad++; stopDrop?.();stopDrag?.();stopClose?.();clearTimeout(draftTimer);clearTimeout(noticeTimer); remember(); for (const tab of workspace.tabs) void releaseLocalBinding(tab.id); window.removeEventListener('pagehide', remember);window.removeEventListener('beforeunload',leaving);void pendingCopy?.release?.();pendingCopy=null; workspace.clear(); void reader?.dispose(); reader = null; };
  });
  async function pickFile(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    const files = Array.from(input.files ?? []);
    input.value = '';
    if (files.length) await addSources(files.map(browserFileSource));
  }
  async function sample() {
    await addSources([{ name: '阅读示例.pdf', size: 2475,
      async read(signal) {
        const response = await fetch('/__samples/geometry.pdf', { signal });
        if (!response.ok) throw new SourceError('示例暂时无法打开，请选择本地 PDF。');
        return new Uint8Array(await response.arrayBuffer());
      },
    }]);
  }
</script>

<svelte:window onpointerdown={outsideMenu} onkeydown={dismiss} />

<div class="shell" inert={savingOriginal} aria-busy={savingOriginal}>
  <input class="file-input" type="file" multiple accept=".pdf,application/pdf" bind:this={fileInput} onchange={pickFile} />
  <header class="document-bar">
    <div class="app-brand" title="PDF Reader"><Icon name="file" /><span>PDF Reader</span></div>
    <div class="tab-strip">{#if tabs.length}<DocumentTabs {tabs} {activeId} {activate} close={closeTab} />{/if}</div>
    <button class="icon-button" type="button" aria-label="打开 PDF" title="打开本地 PDF（可选择多份）" disabled={opening} onclick={() => void openFile()}><Icon name="folder" /></button>
    {#if isDesktop()}<button class="folder-open" type="button" disabled={opening} onclick={()=>void openFolder()}>打开文件夹</button>{/if}
  </header>

  <div class="toolbar" role="group" aria-label="阅读操作">
    <div class="toolbar-start">
      <button class="icon-button" type="button" aria-label="缩略图" title="展开或收起缩略图" aria-expanded={sidebar} aria-controls="thumbnails" bind:this={thumbnailButton} disabled={!canZoom&&!folder} onclick={() => { closeMenu(false); setSidebar(!sidebar); }}><Icon name="thumbs" /></button>
      <span class="separator"></span>
      <button class="icon-button" class:active={annotationMode==='read'&&!selectingRegion} aria-label="阅读与选择" title="阅读与选择（Esc）" aria-pressed={annotationMode==='read'&&!selectingRegion} disabled={!canZoom} onclick={()=>chooseAnnotation('read')}><Icon name="select"/></button>
      <button class="icon-button" class:active={selectingRegion} aria-label="区域框选" title="区域框选：加入对话或原位解释" aria-pressed={selectingRegion} disabled={!canZoom||inlineConfirming} onclick={beginAiRegion}><Icon name="region"/></button>
      <button class="icon-button" class:active={annotationMode==='text'} aria-label="文本框" title="文本框" aria-pressed={annotationMode==='text'} disabled={!canZoom||!view.editingAllowed} onclick={()=>chooseAnnotation('text')}><Icon name="text"/></button>
      <span class="tool-group"><button class="icon-button" class:active={annotationMode==='highlight'} aria-label="高亮" title="高亮" aria-pressed={annotationMode==='highlight'} disabled={!canZoom||!view.editingAllowed} onclick={()=>chooseAnnotation('highlight')}><Icon name="highlight"/></button><button class="tool-caret" aria-label="高亮颜色" title="高亮颜色" aria-haspopup="true" aria-expanded={paletteFor==='highlight'} disabled={!canZoom||!view.editingAllowed} onclick={(event)=>{event.stopPropagation();togglePalette('highlight');}}><span class="caret-dot" style={`background:${toolColor.highlight}`}></span><span class="caret-arrow" aria-hidden="true">▾</span></button>{#if paletteFor==='highlight'}<div class="tool-palette" role="group" aria-label="高亮颜色选择">{#each toolPalette.highlight as option}<button class="swatch" class:chosen={toolColor.highlight.toLowerCase()===option.color.toLowerCase()} aria-label={`高亮 ${option.name}`} title={option.name} style={`background:${option.color}`} onclick={(event)=>{event.stopPropagation();pickToolColor('highlight',option.color);}}></button>{/each}</div>{/if}</span>
      <span class="tool-group"><button class="icon-button" class:active={annotationMode==='ink'} aria-label="自由笔画" title="自由笔画" aria-pressed={annotationMode==='ink'} disabled={!canZoom||!view.editingAllowed} onclick={()=>chooseAnnotation('ink')}><Icon name="pen"/></button><button class="tool-caret" aria-label="自由笔画颜色" title="自由笔画颜色" aria-haspopup="true" aria-expanded={paletteFor==='ink'} disabled={!canZoom||!view.editingAllowed} onclick={(event)=>{event.stopPropagation();togglePalette('ink');}}><span class="caret-dot" style={`background:${toolColor.ink}`}></span><span class="caret-arrow" aria-hidden="true">▾</span></button>{#if paletteFor==='ink'}<div class="tool-palette" role="group" aria-label="自由笔画颜色选择">{#each toolPalette.ink as option}<button class="swatch" class:chosen={toolColor.ink.toLowerCase()===option.color.toLowerCase()} aria-label={`自由笔画 ${option.name}`} title={option.name} style={`background:${option.color}`} onclick={(event)=>{event.stopPropagation();pickToolColor('ink',option.color);}}></button>{/each}</div>{/if}</span>
      <button class="icon-button" class:active={annotationMode==='erase'} aria-label="橡皮擦" title="橡皮擦（整笔或整项删除）" aria-pressed={annotationMode==='erase'} disabled={!canZoom||!view.editingAllowed} onclick={()=>chooseAnnotation('erase')}><Icon name="eraser"/></button>
      <button class="icon-button" aria-label="撤销" title="撤销批注（Ctrl+Z）" disabled={!canZoom||!view.canUndo} onclick={()=>reader?.edit('undo')}><Icon name="undo"/></button>
      <button class="icon-button" aria-label="重做" title="重做批注（Ctrl+Y）" disabled={!canZoom||!view.canRedo} onclick={()=>reader?.edit('redo')}><Icon name="redo"/></button>
    </div>
    <div class="navigation-controls">
      <div class="page-controls">
        <button class="icon-button" type="button" aria-label="上一页" title="上一页" disabled={!canZoom || view.page <= 1} onclick={() => reader?.goToPage(view.page - 1)}><Icon name="previous" /></button>
        <form class="page-navigation" onsubmit={(event) => { event.preventDefault(); jump(); }}>
          <input aria-label="页码" title="输入页码后按 Enter 跳转" aria-invalid={!!pageError} aria-describedby={pageError ? 'reader-notice' : undefined} inputmode="numeric" bind:value={pageInput} onfocus={() => pageEditing = true} onblur={() => pageEditing = false} disabled={!canZoom} />
          <span class="page-total">/ {view.pageCount || '–'}</span>
        </form>
        <button class="icon-button" type="button" aria-label="下一页" title="下一页" disabled={!canZoom || view.page >= view.pageCount} onclick={() => reader?.goToPage(view.page + 1)}><Icon name="next" /></button>
      </div>
      <span class="separator"></span>
      <div class="zoom-controls" data-ui-menu>
        <button class="icon-button zoom-step" type="button" aria-label="缩小" title="缩小" disabled={!canZoom} onclick={() => reader?.zoomBy(1 / 1.2)}><Icon name="minus" /></button>
        <button class="zoom-trigger" type="button" aria-label="缩放与适合方式" title="缩放、适合宽度或整页" aria-expanded={menu === 'zoom'} aria-controls="zoom-options" disabled={!canZoom} onclick={(event) => toggleMenu('zoom', event)}><span>{zoomDisplay}%</span><Icon name="down" /></button>
        <button class="icon-button zoom-step" type="button" aria-label="放大" title="放大" disabled={!canZoom} onclick={() => reader?.zoomBy(1.2)}><Icon name="plus" /></button>
        {#if menu === 'zoom'}
          <div class="popover zoom-popover" id="zoom-options" role="group" aria-label="缩放选项" bind:this={popover}>
            <button class="menu-item" type="button" onclick={() => chooseZoom('page-width')}><Icon name="width" /><span>适合宽度</span></button>
            <button class="menu-item" type="button" onclick={() => chooseZoom('page-fit')}><Icon name="fit" /><span>整页</span></button>
            <button class="menu-item" type="button" onclick={() => chooseZoom(1)}><Icon name="file" /><span>100%</span></button>
            <div class="menu-separator"></div>
            <form class="custom-zoom" onsubmit={(event) => { event.preventDefault(); applyZoom(); }}>
              <label for="zoom-percent">自定义</label><input id="zoom-percent" aria-label="缩放百分比" inputmode="decimal" aria-invalid={!!zoomError} aria-describedby={zoomError ? 'reader-notice' : undefined} bind:value={zoomInput} onfocus={() => zoomEditing = true} onblur={() => zoomEditing = false} /><span>%</span><button type="submit">应用</button>
            </form>
          </div>
        {/if}
      </div>
    </div>
    <div class="toolbar-end" data-ui-menu>
      <button class="icon-button desktop-utility" type="button" aria-label="保存当前 PDF" title="保存当前 PDF（Ctrl+S）" disabled={!canZoom||copying||!workspace.active?.source.saveOriginal} onclick={savePdf}><Icon name="save"/></button>
      <button class="icon-button desktop-utility" type="button" aria-label="搜索当前 PDF" title="搜索当前 PDF（Ctrl+F）" disabled={!canZoom} onclick={toggleSearch}><Icon name="search"/></button>
      <button class="icon-button" type="button" aria-label="AI 对话" title="AI 对话与区域问答" aria-expanded={aiOpen} onclick={() => { closeMenu(false); aiOpen = !aiOpen; historyOpen = false; }}><span style="font-size:12px;font-weight:600">AI</span></button>
      <button class="icon-button" type="button" aria-label="阅读器设置" title="设置：模型、风格、上下文与快捷键" onclick={()=>{closeMenu(false);window.dispatchEvent(new Event('reader-open-settings'));}}><Icon name="settings"/></button>
      <button class="icon-button" type="button" aria-label="更多操作" title="更多操作与阅读帮助" aria-expanded={menu === 'more'} aria-controls="more-options" onclick={(event) => toggleMenu('more', event)}><Icon name="more" /></button>
      {#if menu === 'more'}
        <div class="popover more-popover" id="more-options" role="group" aria-label="更多操作" bind:this={popover}>
          <button class="menu-item" type="button" disabled={opening} onclick={() => void openFile()}><Icon name="folder" /><span>打开 PDF</span></button>
          <button class="menu-item" aria-label="保存 PDF" disabled={!canZoom||copying||!workspace.active?.source.saveOriginal} onclick={savePdf}><Icon name="save"/><span>保存</span><small>Ctrl+S</small></button>
          <button class="menu-item" type="button" aria-label="搜索" title="搜索当前 PDF（Ctrl+F）" bind:this={searchButton} disabled={!canZoom} onclick={toggleSearch}><Icon name="search"/><span>搜索 PDF</span><small>Ctrl+F</small></button>
          <button class="menu-item" aria-label="书签" disabled={!canZoom} onclick={()=>{closeMenu(false);outlineOpen=!outlineOpen;setSidebar(false);}}><Icon name="file"/><span>文档书签</span></button>
          <button class="menu-item" disabled={!canZoom||!view.hasSelection} onclick={()=>{reader?.edit('delete');closeMenu(false);}}><Icon name="close"/><span>删除选中批注</span><small>Delete</small></button>
          <button class="menu-item" disabled={!localBinding||!canZoom} onclick={()=>void beginInline()}><Icon name="text"/><span>原位解释所选内容</span></button>
          <div class="menu-separator"></div>
          <button class="menu-item" type="button" onclick={showHelp}><Icon name="help" /><span>阅读操作与位置恢复</span></button>
          <button class="menu-item" type="button" onclick={() => { closeMenu(false); historyOpen = true; }}><Icon name="file" /><span>本地数据与备份</span></button>
          {#if isDesktop()}<button class="menu-item" aria-label="另存为批注副本" disabled={!canZoom||copying||writerChecking} onclick={()=>void exportCandidateCopy()}>另存为批注副本…</button>{/if}
          {#if copying}<button class="menu-item" onclick={()=>copyAbort?.abort()}>取消另存为</button>{/if}
          {#if import.meta.env.DEV}
            <details class="development-tools">
              <summary>{import.meta.env.DEV?'开发工具':'候选验证'}</summary>
              {#if import.meta.env.DEV}<button class="menu-item" type="button" onclick={() => { closeMenu(false); void sample(); }}>打开自制示例</button>{/if}
              {#if import.meta.env.DEV&&import.meta.env.VITE_M3_COPY==='1'&&isDesktop()}<button class="menu-item" disabled={!canZoom||copying} onclick={()=>void exportTestCopy()}>M3：导出验证副本</button>{/if}
              {#if import.meta.env.VITE_WRITER_CANDIDATE==='1'&&isDesktop()}
                <button class="menu-item" disabled={writerChecking||copying} onclick={()=>void checkWriterIsolation()}>检查候选 Writer 隔离</button>
              {/if}
            </details>
          {/if}
        </div>
      {/if}
    </div>
  </div>

  {#if pendingRecovery.length}<div class="recovery-bar" role="alert"><span>发现未完成保存。原件未自动回滚，恢复材料保留。</span>{#each pendingRecovery as item}<button disabled={copying} onclick={()=>void recoverSaved(item,true)}>恢复新版副本</button><button disabled={copying} onclick={()=>void recoverSaved(item,false)}>恢复保存前副本</button>{/each}<button onclick={()=>pendingRecovery=[]}>保留，稍后处理</button></div>{/if}
  {#if recoverable.length&&canZoom}<div class="recovery-bar" role="status"><span>此文档有未保存批注草稿；恢复不会修改 PDF。</span>{#each recoverable as record}<button onclick={()=>restoreDraft(record)}>恢复 {new Date(record.updated).toLocaleString()}</button>{/each}<button onclick={()=>recoverable=[]}>稍后</button></div>{/if}
  {#if recoveryNotice&&canZoom}<div class="draft-status" role="status">{recoveryNotice}</div>{/if}
  {#if canZoom&&view.overflowPages?.length}<div class="draft-status" role="status">文字框已增高至页面边缘，仍有文字放不下。请移动文字框、调宽、减小字号或减少文字；不会截断保存。{#each view.overflowPages as page}<button onclick={()=>{reader?.goToPage(page);void reader?.annotation('text').then(()=>reader?.focusOverflow(page));}}>定位第 {page} 页问题框</button>{/each}</div>{/if}
  {#if canZoom&&(workspace.active?.source.size??0)>16*1024*1024}<div class="draft-status" role="status">此 PDF 超过当前 16 MB 写入上限，可继续阅读；本文件的批注暂不能保存，另存为也受此限制。请先使用较小副本再批注。</div>{/if}

  {#if searchOpen}
    <form id="pdf-search" class="search-bar" aria-label="文档内搜索" onsubmit={(event) => { event.preventDefault(); find(); }}>
      <Icon name="search" /><input aria-label="搜索文字" placeholder="在当前 PDF 中搜索" bind:this={searchInput} bind:value={query} disabled={!canZoom} onkeydown={(event) => { if (event.key === 'Enter' && event.shiftKey) { event.preventDefault(); find(true); } }} />
      <span class="search-status" role="status">{view.search.pending ? '正在搜索…' : view.search.missing ? '未找到结果' : view.search.total ? `${view.search.current} / ${view.search.total}${view.search.wrapped ? ' · 已循环' : ''}` : '按 Enter 查找'}</span>
      <button class="icon-button" type="button" aria-label="上一个搜索结果" title="上一个结果（Shift+Enter）" disabled={!canZoom || !query} onclick={() => find(true)}><Icon name="previous" /></button>
      <button class="icon-button" type="submit" aria-label="下一个搜索结果" title="下一个结果（Enter）" disabled={!canZoom || !query}><Icon name="next" /></button>
      <button class="icon-button" type="button" aria-label="关闭搜索" title="关闭搜索（Esc）" onclick={closeSearch}><Icon name="close" /></button>
    </form>
  {/if}

  <!-- svelte-ignore a11y_no_noninteractive_element_interactions (User files are accepted only via explicit drag/drop.) -->
  <main class="reading-layout" style={`--thumbnail-width:${layout.left}px;--ai-width:${layout.right}px`} ondragenter={dragFiles} ondragover={dragFiles} ondragleave={leaveFiles} ondrop={dropFiles}>
    {#if sidebar && (folder || canZoom && reader)}
      <button class="sidebar-backdrop" type="button" aria-label="关闭缩略图抽屉" onclick={closeSidebar}></button>
      <div class="left-pane" class:folder-mode={!!folder}>
       {#if folder}<nav class="folder-views" aria-label="左侧视图"><button aria-pressed={folderView==='files'} onclick={()=>folderView='files'}>文件</button><button aria-pressed={folderView==='pages'} disabled={!canZoom} onclick={()=>folderView='pages'}>页面</button><button class="pane-close" aria-label="收起侧栏" onclick={closeSidebar}>×</button></nav>{/if}
       {#if folder&&folderView==='files'}{#key folder.id}<FolderTree {folder} open={entry=>void openFolderPdf(entry)} close={()=>void closeFolder()} busy={opening||savingOriginal}/>{/key}
       {:else if canZoom&&reader}<Thumbnails {reader} token={view.token} total={view.pageCount} page={view.page} jump={jumpThumbnail} close={closeSidebar} selected={selectedAiPages} toggleSelected={toggleAiPage}/>{/if}
      </div>
      <PaneDivider label="调整缩略图宽度" value={layout.left} change={n=>resizePane('left',n)} reset={()=>layout=defaultLayout()} />
    {:else if outlineOpen && canZoom && reader}
      <Bookmarks items={view.outline} jump={(destination) => reader?.bookmark(destination)} close={() => outlineOpen = false} />
    {/if}
    <div class="reading-area">
      {#if dropHint&&!activeId}<div class="pdf-drop-hint" class:invalid={dropHint==='invalid'} role="status">{dropHint==='invalid'?'仅支持 PDF 文件，请拖入 PDF。':'释放鼠标，打开 PDF'}</div>{/if}
      <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions (The focusable reading region offers documented navigation keys without replacing native scrolling.) -->
      <div class="viewer-container" bind:this={container} role="region" tabindex="0" aria-label="PDF 连续阅读区域" onkeydown={event => { localReadingInteraction(); keyboard(event); }} onpointerdown={localReadingInteraction} onwheel={localReadingInteraction}>
        <div class="pdfViewer" bind:this={pages}></div>
      </div>
      {#if view.status !== 'ready' || !view.viewReady}
        <div class="empty-state" role="status">
          {#if view.passwordRequired}
            <h1>打开加密 PDF</h1><p>{view.passwordRequired === 2 ? '密码不正确，请重试。' : '请输入文档密码；仅在本次打开时使用。'}</p>
            <form class="password-form" onsubmit={(event) => { event.preventDefault(); reader?.password(passwordValue); passwordValue = ''; }}>
              <input type="password" aria-label="PDF 密码" autocomplete="off" bind:value={passwordValue} /><button type="submit">打开</button>
              <button type="button" onclick={() => { passwordValue = ''; if (activeId) closeTab(activeId); }}>取消</button>
            </form>
          {:else if view.status === 'loading' || view.status === 'ready'}<div class="loading-mark" aria-hidden="true"></div><p>正在打开 PDF…</p>
          {:else if view.status === 'error'}<Icon name="file" /><h1>未能打开文件</h1><p>{view.error}</p><button type="button" disabled={opening} onclick={() => void openFile()}>选择其他 PDF</button>
          {:else}<div class="empty-icon"><Icon name="file" /></div><h1>从一份 PDF 开始</h1><p>打开本地文件，专注阅读。</p><button class="open-primary" type="button" disabled={opening} onclick={() => void openFile()}><Icon name="folder" /><span>打开 PDF</span></button><small>阅读在本机完成；AI 仅在确认发送时使用所选内容。</small>{/if}
        </div>
      {/if}
    </div>
    {#if historyOpen}<LocalHistory testHistory={import.meta.env.DEV} data={localData} binding={localBinding} status={dataStatus} settings={localSettings} changeQuota={updateLocalSettings} {prepareDataChange} page={view.page} notice={dataNotice} jumpPage={(page, id) => { if (id === localBinding?.bindingId && canZoom && page >= 1 && page <= view.pageCount) { localReadingInteraction(); reader?.goToPage(page); } }} close={() => historyOpen = false} migrated={migratedData} restored={restoredData} />{/if}
    {#if view.selectedText&&reader&&!inlineState}<TextEditorBar {reader} {container} selected={view.selectedText}/>{/if}
    {#if view.selectedStroke&&reader&&!inlineState}<StrokeEditorBar {reader} {container} selected={view.selectedStroke}/>{/if}
    {#if pendingRegion&&reader}<RegionActions {reader} {container} box={pendingRegion} chat={addRegionToChat} explain={()=>void beginInline(pendingRegion!)} cancel={()=>pendingRegion=null} canExplain={!!localBinding} explainDisabledReason={chatUnavailable}/>{/if}
    {#if inlineState&&reader&&localBinding&&inlineState.binding===localBinding.bindingId}{#key inlineState.session}<InlineExplanation {reader} binding={localBinding} {container} token={view.token} source={inlineState.source} box={inlineState.box} quick={inlineState.quick} totalPages={view.pageCount} changed={box=>{if(inlineState)inlineState.box=box;}} place={placeAnswer} placing={placingAnswer} busyChanged={value=>inlineConfirming=value} close={()=>closeInline(true)}/>{/key}{/if}
    {#if aiOpen}<PaneDivider label="调整 AI 面板宽度" value={layout.right} direction={-1} change={n=>resizePane('right',n)} reset={()=>layout=defaultLayout()} />{/if}
    <AiChat visible={aiOpen} {reader} binding={localBinding} unavailableReason={chatUnavailable} token={view.token} total={view.pageCount} page={view.page} data={localData} selected={selectedAiPages} documentName={tabs.find(t=>t.id===activeId)?.source.name??'当前 PDF'} removePage={n=>{if(activeId)aiPages[activeId]=(aiPages[activeId]??[]).filter(p=>p!==n);}} {region} selecting={selectingRegion} beginRegion={beginAiRegion} beginInline={()=>void beginInline()} clearRegion={() => region = null} clearSelected={()=>{if(activeId)aiPages[activeId]=[];}} consumed={consumedSelection} close={() => { aiOpen = false; regionCleanup?.(); regionCleanup = null; selectingRegion=false; }} jump={(page, id) => { if (id === localBinding?.bindingId && canZoom) reader?.goToPage(page); }} openSource={openAiSource} />
  </main>
  {#if notice||pendingCopy}
    <div class="reader-notice" class:error={!!openError || !!view.error || !!pageError || !!zoomError} id="reader-notice" role="status" aria-live="polite">{notice}{#if pendingCopy}<button class="saved-copy-action" onclick={openTestCopy}>打开批注副本</button>{/if}</div>
  {/if}
</div>

<div class="save-progress" hidden={!savingOriginal} role="status" aria-live="polite">正在保存 PDF，请稍候…</div>

<dialog class="help-dialog" bind:this={copyOpenDialog} aria-labelledby="copy-open-title" oncancel={()=>copyOpenConfirm=null}>
  <h2 id="copy-open-title">副本已保存，打开副本继续编辑</h2>
  <p>当前“{copyOpenConfirm?.name}”的已有批注修改暂时不能转为恢复稿。继续将关闭当前标签并打开“{copyOpenConfirm?.source.name}”。导出后新增的修改不在该副本内，请先再次另存。</p>
  <p>原 PDF、聊天历史及已有恢复稿不会删除。</p>
  <div class="dialog-actions"><button type="button" onclick={()=>{copyOpenDialog.close();copyOpenConfirm=null;}}>取消切换</button><button type="button" onclick={()=>void confirmOpenSavedCopy()}>打开已保存副本并关闭当前标签</button></div>
</dialog>
<dialog class="help-dialog" bind:this={helpDialog} aria-labelledby="reading-help-title" onclose={() => menuTrigger?.focus()}>
  <div class="help-heading"><h2 id="reading-help-title">阅读操作与位置恢复</h2><button class="icon-button" type="button" aria-label="关闭阅读帮助" onclick={() => helpDialog.close()}><Icon name="close" /></button></div>
  <p>先点阅读区域，再使用以下快捷键：</p>
  <dl class="shortcut-list"><dt>← / →</dt><dd>上一页 / 下一页</dd><dt>Home / End</dt><dd>首页 / 末页</dd><dt>＋ / −</dt><dd>放大 / 缩小</dd><dt>/</dt><dd>搜索当前 PDF</dd><dt>Enter / Shift+Enter</dt><dd>搜索下一结果 / 上一结果</dd></dl>
  <p>滚轮与触控板保持自然滚动；浏览器通用快捷键保持原有作用。</p>
  <h3>阅读位置</h3><p>{positionNotice}</p>
  <h3>保存与恢复</h3><p>桌面版使用 Ctrl+S 保存。成功后，每份文档最近两次保存前的完整版本保存在应用数据目录的 pdf-backups 中，位置随数据目录设置迁移，不作为缓存清理。确认归档成功后才移除原文件旁的本次临时材料。归档失败、未完成保存及既有旧备份均保留。若保存中断，重开 PDF 后可恢复为新副本；不会自动覆盖原件。当前增量保存支持 16 MB 以内、通过结构核验的普通无保护文件，特殊文件请另存。</p>
</dialog>

<dialog class="help-dialog" bind:this={closeDialog} inert={savingOriginal} aria-labelledby="close-title" oncancel={(event)=>{if(savingOriginal||closeSaving){event.preventDefault();return;}closeTarget=null;closingWindow=false;}}>
  <h2 id="close-title">批注尚未保存到 PDF</h2><p>{writerBlocked}</p>
  {#if !closePreserved}<p>{recoveryNotice}</p>{/if}
  {#if openError}<p role="alert">{openError}</p>{/if}
  <div class="close-actions"><button disabled={closeSaving} onclick={()=>void saveClosingDocuments()}>{closeSaving?'正在保存待关闭文档…':'保存'}</button><button disabled={!closePreserved||closeSaving} onclick={()=>void finishClose(false)}>保留草稿并关闭</button><button disabled={closeSaving} onclick={()=>void finishClose(true)}>放弃</button><button disabled={closeSaving} onclick={()=>{closeDialog.close();closeTarget=null;closingWindow=false;}}>取消</button></div>
</dialog>

<style>
 .folder-open{background:transparent;border:1px solid #ccd7e0;border-radius:4px;color:#4f6c83;font:inherit;font-size:11px;padding:5px 8px;margin-right:8px;white-space:nowrap;}
 .left-pane{display:flex;flex-direction:column;width:var(--thumbnail-width,210px);min-width:0;flex-shrink:0;height:100%;background:#f6f8fa;}
 .left-pane :global(.thumbnail-sidebar){width:100%;flex:1;min-height:0;position:relative;box-shadow:none;}
 .folder-views{display:flex;gap:3px;padding:5px;border-bottom:1px solid #dce5ed;}
 .folder-views button{border:0;background:transparent;color:#5c7488;padding:5px 9px;font:inherit;font-size:12px;}
 .folder-views button[aria-pressed="true"]{background:#e2ecf5;color:#244c6a;border-radius:3px;}
 .folder-views .pane-close{margin-left:auto;}
 @media(max-width:900px){.left-pane{position:absolute;z-index:10;inset:0 auto 0 0;box-shadow:6px 0 25px #24374b1a;}}
</style>
