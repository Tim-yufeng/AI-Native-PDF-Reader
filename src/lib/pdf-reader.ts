import { AnnotationEditorType, AnnotationEditorParamsType, AnnotationMode, GlobalWorkerOptions, PermissionFlag, getDocument } from 'pdfjs-dist';
import type { AnnotationEditorUIManager } from 'pdfjs-dist/types/src/display/editor/tools';
import { emptyDraft, validateDraft, type AnnotationDraft } from './annotation-draft';
import type { PDFDocumentProxy, PDFPageProxy } from 'pdfjs-dist';
import { EventBus, PDFLinkService, PDFViewer, PDFFindController, FindState } from 'pdfjs-dist/web/pdf_viewer.mjs';
import workerUrl from 'pdfjs-dist/build/pdf.worker.min.mjs?url';
import { ReaderSession, emptyReaderState } from './reader-session';
import type { ReaderState } from './reader-session';
import type { PdfSource } from './document-source';
import { bookmarks, type Bookmark } from './outline';
import { attachPdfWheelZoom } from './wheel-zoom';
import { freeTextEntry, type NewText } from './free-text-entry';
import { WriterRefusal } from './writer-refusal.ts';
import { createFreeTextControl, pickReadableTextColor, type TextSelection, type TextRect } from './freetext-control';
import { trackDrawingDirty } from './drawing-dirty';
import { prepareFreeTextFont } from './freetext-layout';

GlobalWorkerOptions.workerSrc = workerUrl;
import type { ReadingPosition, ReadingZoom } from './reading-position';
export type Zoom = ReadingZoom;
export interface SearchState { query: string; current: number; total: number; pending: boolean; missing: boolean; wrapped: boolean; }
export const emptySearch = (): SearchState => ({ query: '', current: 0, total: 0, pending: false, missing: false, wrapped: false });
export interface StrokeSelection {id:string;kind:'highlight'|'ink';color:string;thickness:number;canThicken:boolean;}
export interface ViewState extends ReaderState { page: number; scale: number; viewReady: boolean; saveReady?:boolean; overflowPages?:number[]; token: number; search: SearchState; textNotice: string; outline: Bookmark[]; passwordRequired: number; dirty: boolean; editingAllowed: boolean; canUndo: boolean; canRedo: boolean; annotationMode?:'read'|'text'|'highlight'|'ink'|'erase'; selectedText?:TextSelection|null; selectedStroke?:StrokeSelection|null; hasSelection?:boolean; }
export const emptyViewState = (): ViewState => ({ ...emptyReaderState(), page: 0, scale: 1, viewReady: false, token: 0, search: emptySearch(), textNotice: '', outline: [], passwordRequired: 0, dirty:false,editingAllowed:false,canUndo:false,canRedo:false });
export interface PdfReader {
  open(source: PdfSource, position?: ReadingPosition | (() => ReadingPosition | undefined), draft?: AnnotationDraft): Promise<void>;
  position(): ReadingPosition | null;
  close(): Promise<void>;
  zoom(value: Zoom): void;
  zoomBy(factor: number): void;
  goToPage(page: number): void;
  search(query: string, previous?: boolean): void;
  clearSearch(): void;
  password(value: string): void;
  bookmark(destination: string | unknown[]): void;
  annotation(mode: 'read' | 'text' | 'highlight' | 'ink' | 'erase'): Promise<void>;
  edit(action: 'undo' | 'redo' | 'delete'): void;
  textSize(value: number): void;
  textColor(value:string):void;
  textEdit():void;
  strokeStyle(kind:'highlight'|'ink',color?:string,thickness?:number):void;
  resizeText(rect:TextRect):void;
  focusOverflow(page:number):void;
  addText(value:NewText):Promise<void>;
  draft(commit?: boolean): AnnotationDraft;
  testWriterBytes(): Promise<Uint8Array>;
  // Read-only page access for later contextual extraction; no writer exposed.
  getPage(pageNumber: number): Promise<PDFPageProxy>;
  dispose(): Promise<void>;
}

export function createPdfReader(container: HTMLDivElement, pages: HTMLDivElement,
  changed: (state: ViewState) => void): PdfReader {
  const events = new EventBus();
  const links = new PDFLinkService({ eventBus: events });
  links.externalLinkEnabled = false;
  const lifetime = new AbortController();
  const finder = new PDFFindController({ eventBus: events, linkService: links });
  // The locked runtime supports abortSignal; its published typings omit it.
  const options: ConstructorParameters<typeof PDFViewer>[0] & { abortSignal: AbortSignal } = {
    container, viewer: pages, eventBus: events, linkService: links, findController: finder,
    annotationEditorMode: AnnotationEditorType.NONE,
    // The standalone editor needs a named palette even without Mozilla's toolbar.
    annotationEditorHighlightColors: 'yellow=#FFFF00',
    annotationMode: AnnotationMode.ENABLE,
    enablePermissions: true,
    // No scripting manager is created: viewer.enableScripting is false.
    enableAutoLinking: false,
    imageResourcesPath: '/pdfjs/images/',
    maxCanvasPixels: 16 * 1024 * 1024,
    abortSignal: lifetime.signal,
  };
  const viewer = new PDFViewer(options);
  links.setViewer(viewer);
  let document: PDFDocumentProxy | null = null;
  let manager: AnnotationEditorUIManager | null = null;
  let restoreEditorFocus:(()=>void)|null=null;
  let drawingDirty: ReturnType<typeof trackDrawingDirty> | null = null;
  let modeWait:{mode:number;promise:Promise<void>;cancel:()=>void}|null=null;
  let erasing=false,erasePending=false;
  const textControl=createFreeTextControl({container,manager:()=>manager,changed:()=>{dirty=true;notify();}});
  let draftRestore = emptyDraft();
  let pendingRestore = new Map<number, Record<string,unknown>[]>();
  const restoringPages = new Set<number>();
  let dirty = false, editingAllowed = false, canUndo = false, canRedo = false;
  let snapshot = emptyReaderState();
  let viewReady = false;
  let geometryReady = false;
  let disposed = false;
  let token = 0;
  let restore: ReadingPosition | (() => ReadingPosition | undefined) | undefined;
  let restoring = false;
  let restoreIntent = 0;
  let wheelZoom: ReturnType<typeof attachPdfWheelZoom> | null = null;
  const cancelRestore = () => { restoreIntent++; restoring = false; };
  for (const event of ['wheel', 'pointerdown', 'keydown']) container.addEventListener(event, cancelRestore, { signal: lifetime.signal, passive: true });
  let search = emptySearch();
  let outline: Bookmark[] = [];
  let passwordRequired = 0;
  let updatePassword: ((value: string) => void) | null = null;
  const textNotices = new Map<number, string>();
  function strokeSelection():StrokeSelection|null {const e=manager?.firstSelectedEditor;if(!e||!['ink','highlight'].includes(e.editorType))return null;const kind=e.editorType as StrokeSelection['kind'];const colorType=kind==='ink'?AnnotationEditorParamsType.INK_COLOR:AnnotationEditorParamsType.HIGHLIGHT_COLOR,thicknessType=kind==='ink'?AnnotationEditorParamsType.INK_THICKNESS:AnnotationEditorParamsType.HIGHLIGHT_THICKNESS;const properties=e.propertiesToUpdate as unknown[][];const color=String(properties.find(([id])=>id===colorType)?.[1]??(kind==='ink'?'#000000':'#ffff00'));e.div?.classList.add('reader-stroke-controls');return{id:e.id,kind,color,thickness:Number(properties.find(([id])=>id===thicknessType)?.[1]??1),canThicken:kind==='ink'||properties.some(([id,value])=>id===AnnotationEditorParamsType.HIGHLIGHT_FREE&&value===true)};}
  const notify = () => {
    if (!disposed) changed({ ...snapshot, page: viewReady ? viewer.currentPageNumber : 0,overflowPages:textControl.overflowPages(),
      scale: viewReady ? viewer.currentScale : 1, viewReady: viewReady && geometryReady, saveReady:viewReady&&geometryReady&&editingAllowed&&!pendingRestore.size&&!restoringPages.size, token, search: { ...search }, textNotice: textNotices.get(viewer.currentPageNumber) ?? '', outline, passwordRequired, dirty:dirty||pendingRestore.size>0||restoringPages.size>0, editingAllowed, canUndo, canRedo,selectedText:textControl.selection(),selectedStroke:strokeSelection(),hasSelection:!!manager?.firstSelectedEditor,annotationMode:erasing?'erase':manager?.getMode()===AnnotationEditorType.FREETEXT?'text':manager?.getMode()===AnnotationEditorType.HIGHLIGHT?'highlight':manager?.getMode()===AnnotationEditorType.INK?'ink':'read' });
  };
  const session = new ReaderSession<PDFDocumentProxy>(bytes => {
    const task = getDocument({
    data: bytes,
    cMapUrl: '/pdfjs/cmaps/', cMapPacked: true,
    standardFontDataUrl: '/pdfjs/standard_fonts/', wasmUrl: '/pdfjs/wasm/',
    useSystemFonts: true,
    });
    task.onPassword = (update: (value: string) => void, reason: number) => { updatePassword = update; passwordRequired = reason; notify(); };
    return task;
  }, state => { snapshot = state; notify(); }, next => {
    wheelZoom?.reset();modeWait?.cancel();textControl.reset();
    erasing=false;pages.classList.remove('reader-erasing');
    outline = []; passwordRequired = 0; updatePassword = null;
    drawingDirty?.dispose(); drawingDirty = null;restoreEditorFocus?.();restoreEditorFocus=null;
    manager = null; dirty = false; editingAllowed = false; canUndo = false; canRedo = false; pendingRestore.clear();restoringPages.clear();
    if (next) for (const entry of draftRestore.entries) { const page = Number(entry.pageIndex); pendingRestore.set(page,[...(pendingRestore.get(page)??[]),entry]); }
    viewReady = false; geometryReady = false; restoring = false; restoreIntent++; search = emptySearch(); textNotices.clear(); document = next; token++;
    viewer.setDocument(next); links.setDocument(next);
    if (next) void next.getOutline().then(items => { if (!disposed && document === next) { outline = bookmarks(items); notify(); } }).catch(() => {});
    if (next) void next.getPermissions().then(permissions => { if (!disposed && document === next) {
      editingAllowed = !permissions || permissions.has(PermissionFlag.MODIFY_CONTENTS) && permissions.has(PermissionFlag.MODIFY_ANNOTATIONS); notify();
      if(editingAllowed)for(const index of pendingRestore.keys())void restoreEditors({pageNumber:index+1});
    } }).catch(() => {});
    if (next) void (viewer.pagesPromise as Promise<void> | null)?.catch(() => {
      if (disposed || document !== next) return;
      viewReady = false;
      snapshot = { ...snapshot, status: 'error', error: '这份 PDF 的页面无法显示，请关闭后重新选择文件。' };
      notify();
    });
  });
  const active = () => !disposed && document !== null && viewer.pdfDocument === document;
  const changeMode=async(mode:number,editId?:string|null,mustEnterInEditMode?:boolean):Promise<void>=>{
    if(!active()||!manager||!editingAllowed)return Promise.reject(Error('批注模式尚未就绪。'));
    const expected=document,expectedToken=token,ui=manager;
    if(mode!==AnnotationEditorType.NONE)await prepareFreeTextFont();
    if(!active()||document!==expected||token!==expectedToken||manager!==ui)throw Error('文档已切换，批注模式请求取消。');
    const enter=()=>{if(active()&&document===expected&&token===expectedToken&&manager===ui&&mode===AnnotationEditorType.FREETEXT&&editId&&mustEnterInEditMode)textControl.editById(editId);};
    if(modeWait?.mode===mode)return modeWait.promise.then(enter);
    if(modeWait){const previous=modeWait;return previous.promise.catch(()=>{}).then(()=>{if(!active()||document!==expected||token!==expectedToken||manager!==ui)throw Error('文档已切换，批注模式请求取消。');return changeMode(mode,editId,mustEnterInEditMode);});}
    // Locked PDF.js returns a number here; its declaration mistakenly repeats
    // the setter's object type. Await the completed event, not manager.getMode.
    if((viewer.annotationEditorMode as unknown as number)===mode){enter();return Promise.resolve();}
    let finish:()=>void=()=>{},cancel:()=>void=()=>{};
    const promise=new Promise<void>((resolve,reject)=>{
      const cleanup=()=>{clearTimeout(timer);events.off('annotationeditormodechanged',changed);};
      const changed=(event:{source:unknown;mode:number})=>{if(event.source===viewer&&event.mode===mode){cleanup();resolve();}};
      const timer=setTimeout(()=>{cleanup();reject(Error('批注模式切换未完成，请稍后重试。'));},3000);
      finish=cleanup;cancel=()=>{cleanup();reject(Error('批注模式切换已取消。'));};events.on('annotationeditormodechanged',changed);
    });
    const pending={mode,promise,cancel};modeWait=pending;
    void promise.then(()=>{if(modeWait===pending)modeWait=null;},()=>{if(modeWait===pending)modeWait=null;});
    try{viewer.annotationEditorMode={mode,editId,mustEnterInEditMode};}catch(e){finish();cancel();}
    return promise.then(enter);
  };
  const captureDraft = (commit = true): AnnotationDraft => {
    if(restoringPages.size)throw Error('草稿正在恢复，请稍候再切换或关闭。');
    if (commit) textControl.commit();
    else if(manager?.getActive()?.shouldGetKeyboardEvents()) throw Error('完成当前文字输入后将保留恢复草稿。');
    const entries: Record<string,unknown>[] = [...pendingRestore.values()].flat();
    if (document) for (const [, value] of document.annotationStorage) {
      const serialized = typeof value.serialize === 'function' ? value.serialize(false) : value;
      if (serialized) entries.push(serialized);
    }
    return validateDraft({...emptyDraft(),entries});
  };
  const editorReady = (event: { uiManager: AnnotationEditorUIManager }) => {
    drawingDirty?.dispose();restoreEditorFocus?.(); manager = event.uiManager;
    const ui=manager,add=ui.addEditor,editors=new WeakSet<object>(),nodes=new WeakSet<HTMLElement>();
    // PDF.js focuses FreeText in onceAdded/focusin/enterInEditMode, before our
    // fixed box is adopted. A rotated/partly visible empty box can then scroll
    // the browser to another page. Set the focus option at this source, before
    // its first render returns; never rewind scrolling or wrap undo/redo.
    const addEditor:typeof add=function(editor){
      if(editor.editorType==='freetext'&&!editors.has(editor)){
        editors.add(editor);const render=editor.render;
        editor.render=function(){
          const div=render.call(this);
          for(const node of [div,this.contentDiv])if(node&&!nodes.has(node)){
            nodes.add(node);const focus=node.focus;
            node.focus=options=>focus.call(node,{...options,preventScroll:true});
          }
          return div;
        };
      }
      add.call(ui,editor);
    };
    ui.addEditor=addEditor;
    restoreEditorFocus=()=>{if(ui.addEditor===addEditor)ui.addEditor=add;};
    drawingDirty = trackDrawingDirty(manager, () => editorStates({source: event.uiManager, details: {}})); notify();
  };
  const editorStates = (event: { source: AnnotationEditorUIManager; details: { hasSomethingToUndo?: boolean; hasSomethingToRedo?: boolean; isEmpty?: boolean } }) => {
    if (event.source !== manager) return;
    const selected=manager?.firstSelectedEditor;
    if(selected?.editorType==='ink'&&!erasing)manager?.unselectAll();
    if(selected?.editorType==='freetext'&&!selected.isEmpty()&&selected.width&&selected.height)textControl.attach(selected);
    if(event.details.hasSomethingToUndo!==undefined)canUndo=event.details.hasSomethingToUndo;
    if(event.details.hasSomethingToRedo!==undefined)canRedo=event.details.hasSomethingToRedo;
    try { dirty = captureDraft(false).entries.length > 0 || !!drawingDirty?.hasChanges(); } catch { dirty = true; }
    notify();
  };
  const restoreEditors = async (event: { pageNumber: number }) => {
    const index = event.pageNumber - 1, layer = manager?.getLayer(index), entries = pendingRestore.get(index);
    if (!layer || !entries || !editingAllowed || restoringPages.has(index)) return;
    const expected = token; restoringPages.add(index);
    try {
      await prepareFreeTextFont();
      if(!active()||expected!==token)return;
      while(entries.length) {
        const entry=entries[0];
        // Writer records omit the clipboard flag. FreeText.render only populates
        // deserialized text for existing PDF objects or clipboard records.
        const editor = await layer.deserialize(entry.annotationType===3?{...entry,isCopy:true}:entry);
        if (!active() || expected !== token) return;
        if (!editor) throw Error('Unsupported draft');
        layer.addOrRebuild(editor);
        // FreeText's serialized PDF rectangle excludes its editor padding;
        // clipboard rendering also translates the object. Anchor the rendered
        // editor back to the saved PDF rectangle using page-space dimensions.
        if(entry.annotationType===3)textControl.attach(editor,entry.rect as number[]);
        if(editor.isEmpty())throw Error('Restored editor is empty');
        entries.shift();
      }
      pendingRestore.delete(index);
      // addOrRebuild already installed the normal editor. Re-rendering this
      // layer here races a user's mode/selection change during draft recovery.
      if (!active() || expected !== token) return;
      if(manager?.getMode()===AnnotationEditorType.NONE)layer.disable();
      dirty = true; notify();
    } catch { if (active() && expected === token) { snapshot={...snapshot,error:'批注草稿未能完整恢复，原文件未修改；请保留草稿。'}; dirty=true; notify(); } }
    finally { if(expected===token){restoringPages.delete(index);notify();} }
  };
  events.on('annotationeditoruimanager',editorReady); events.on('editingstateschanged',editorStates);
  events.on('annotationeditormodechanged',notify);
  events.on('annotationeditorparamschanged',notify);
  const switchEditorMode=(event:{mode:number;editId?:string|null;isFromKeyboard?:boolean;mustEnterInEditMode?:boolean})=>{
    if(erasing)return;
    if(active()&&editingAllowed&&[AnnotationEditorType.NONE,AnnotationEditorType.FREETEXT,AnnotationEditorType.HIGHLIGHT,AnnotationEditorType.INK].includes(event.mode))void changeMode(event.mode,event.editId,event.mustEnterInEditMode).catch(()=>{});
  };
  events.on('switchannotationeditormode',switchEditorMode);
  events.on('annotationeditorlayerrendered',restoreEditors);
  events.on('annotationlayerrendered',(event:{pageNumber:number;source:unknown})=>{
    if(!active())return;
    const page=viewer.getPageView(event.pageNumber-1);
    if(event.source!==page)return;
    // Our AP contains visual soft breaks. PDF.js normally reconstructs editable
    // text from those drawing lines. For our standard FreeText objects, keep the
    // original /Contents (including explicit blank lines) as the editable value.
    for(const annotation of page.annotationLayer?.annotationLayer?.getEditableAnnotations()??[]) {
      const data=annotation.data;
      if(data.subtype==='FreeText'&&data.defaultAppearanceData?.fontName==='AUPW'&&typeof data.contentsObj?.str==='string')
        annotation.textContent=data.contentsObj.str.split('\n');
    }
  });
  events.on('editorsrendered',(event:{pageNumber:number})=>{
    if(!active())return;
    for(const editor of manager?.getEditors(event.pageNumber-1)??[]){
      if(editor.editorType==='freetext')textControl.attach(editor);
      else if(editor.editorType==='ink'&&editor.div)editor.div.tabIndex=-1;
    }
  });
  // Keep ordinary editable annotation objects enabled, and intercept only the
  // eraser gesture. Native deletion owns undo, redo and existing-PDF tombstones.
  // A blank-page hit never creates a text editor or touches page content.
  container.addEventListener('pointerdown',event=>{
    if(!erasing||erasePending||!active()||!editingAllowed||!manager||event.button!==0)return;
    const target=event.target instanceof Element?event.target:null;
    if(!target?.closest('.page'))return;
    event.preventDefault();event.stopImmediatePropagation();
    const node=target.closest<HTMLElement>('.freeTextEditor,.inkEditor,.highlightEditor');
    const editor=node&&pages.contains(node)?manager.getEditor(node.id):null;
    if(!editor||!['freetext','ink','highlight'].includes(editor.editorType))return;
    const ui=manager,snapshot=token;erasePending=true;
    ui.endCurrentEditing();ui.setSelected(editor);
    // PDF.js builds a selected stroke's toolbar asynchronously. Deleting it
    // within that same stack destroys a toolbar that its continuation still
    // uses. Finish that UI turn, then delete only the original selected object.
    requestAnimationFrame(()=>{
      erasePending=false;
      if(!erasing||!active()||token!==snapshot||manager!==ui||ui.firstSelectedEditor!==editor)return;
      ui.delete();editorStates({source:ui,details:{}});
    });
  },{capture:true,signal:lifetime.signal});
  for(const type of ['pointerup','click','dblclick'])container.addEventListener(type,event=>{
    if(erasing&&event.target instanceof Element&&event.target.closest('.page')){event.preventDefault();event.stopImmediatePropagation();}
  },{capture:true,signal:lifetime.signal});
  container.addEventListener('input',event=> { if ((event.target as Element).closest('.annotationEditorLayer')) { dirty=true; notify(); } },{signal:lifetime.signal});
  container.addEventListener('pointerup',()=>queueMicrotask(()=>{if(active()&&manager)editorStates({source:manager,details:{}});}),{signal:lifetime.signal});
  let measuredWidth=container.clientWidth,measuredHeight=container.clientHeight;
  const resize = new ResizeObserver(() => {
    if (!active() || !viewReady) return;
    const value = viewer.currentScaleValue;
    const width=container.clientWidth,height=container.clientHeight;
    const changedWidth=width!==measuredWidth,changedHeight=height!==measuredHeight;
    // A newly shown dirty-status bar changes height. Redundant fit refresh removes
    // a just-created empty PDF.js editor before the user can type.
    if(manager?.getActive()?.shouldGetKeyboardEvents())return;
    measuredWidth=width;measuredHeight=height;
    if (value === 'page-width'&&changedWidth || value === 'page-fit'&&(changedWidth||changedHeight)) viewer.currentScaleValue = value;
  });
  resize.observe(container);
  const pagesInit = async () => {
    if (!active()) return;
    const position = typeof restore === 'function' ? restore() : restore; restore = undefined;
    const expected = document, expectedToken = token, intent = restoreIntent;
    // Start at the saved zoom. Drawing at page-width first leaves PDF.js's
    // canvas rounding on earlier pages until they redraw at the target zoom;
    // restoring against those temporary dimensions drifts on each reload.
    viewer.currentScaleValue = String(position?.zoom ?? 'page-width');
    viewReady = true; restoring = !!position; notify();
    // Eager page geometry initialization is bounded by our existing 2000-page guard.
    await viewer.pagesPromise;
    if (!active() || document !== expected || token !== expectedToken) return;
    geometryReady = true; notify();
    await new Promise<void>(resolve => requestAnimationFrame(() => resolve()));
    if (!active() || document !== expected || token !== expectedToken) return;
    if (!position || intent !== restoreIntent) { restoring = false; notify(); return; }
    const page = Math.min(position.page, viewer.pagesCount);
    viewer.currentPageNumber = page;
    viewer.currentScaleValue = String(position.zoom);
    viewer.scrollPageIntoView({ pageNumber: page });
    const div = viewer.getPageView(page - 1).div;
    container.scrollTop = div.offsetTop + position.top * div.clientHeight;
    container.scrollLeft = Math.max(0, div.offsetLeft + position.left * div.clientWidth);
    viewer.update(); restoring = false; notify();
  };
  const positionChanged = () => { if (active()) notify(); };
  const scaleChanged=()=>{if(active()){textControl.cancelGesture();const expected=token;notify();requestAnimationFrame(()=>{if(active()&&token===expected){textControl.refresh();notify();}});}};
  const rendered = (event: { pageNumber: number; source: unknown; error?: Error }) => {
    if (!active() || event.source !== viewer.getPageView(event.pageNumber - 1) || !event.error || event.error.name === 'RenderingCancelledException') return;
    snapshot = { ...snapshot, error: '此页未能完整显示，请关闭后重新打开，或换用其他 PDF。' };
    notify();
  };
  const initialize = () => { const expected = document, expectedToken = token; void pagesInit().catch(() => { if (active() && document === expected && token === expectedToken) { restoring = false; geometryReady = false; snapshot = { ...snapshot, status: 'error', error: '页面准备失败，请关闭后重新选择文件。' }; notify(); } }); };
  events.on('pagesinit', initialize);
  events.on('pagechanging', positionChanged);
  events.on('scalechanging', scaleChanged);
  events.on('updateviewarea', positionChanged);
  events.on('pagerendered', rendered);
  const searchUpdate = (event: { rawQuery?: string; state?: number; matchesCount?: { current: number; total: number } }) => {
    if (!active() || finder._pdfDocument !== document || !search.query || event.rawQuery != null && event.rawQuery !== search.query) return;
    if (event.matchesCount) { search.current = event.matchesCount.current; search.total = event.matchesCount.total; }
    if (event.state !== undefined) { search.pending = event.state === FindState.PENDING; search.missing = event.state === FindState.NOT_FOUND; search.wrapped = event.state === FindState.WRAPPED; }
    notify();
  };
  const textRendered = (event: { pageNumber: number; source: unknown; error?: Error }) => {
    if (!active() || event.source !== viewer.getPageView(event.pageNumber - 1)) return;
    const layer = viewer.getPageView(event.pageNumber - 1).textLayer;
    textNotices.set(event.pageNumber, event.error ? '此页文字无法提取，搜索与复制可能不完整。'
      : layer?.div.textContent?.trim() ? '' : '此页没有可提取文字；可阅读图像，本阶段不提供 OCR。');
    notify();
  };
  events.on('updatefindcontrolstate', searchUpdate); events.on('updatefindmatchescount', searchUpdate);
  events.on('textlayerrendered', textRendered);
  const zoom = (value: Zoom) => {
    wheelZoom?.reset();
    if (!active() || !viewReady) return;
    cancelRestore();
    if (typeof value === 'number' && !Number.isFinite(value)) return;
    viewer.currentScaleValue = typeof value === 'number'
      ? String(Math.min(4, Math.max(0.25, value))) : value;
  };
  wheelZoom = attachPdfWheelZoom(container, {
    signal: lifetime.signal,
    ready: () => active() && viewReady && geometryReady && snapshot.status === 'ready',
    scale: () => viewer.currentScale,
    apply: (scaleFactor, origin) => {
      cancelRestore();
      // Public PDF.js API updates canvas/text/editors together and compensates
      // scroll offsets around the pointer. Delay heavy redraw until input rests.
      viewer.updateScale({ scaleFactor, origin, drawingDelay: 100 });
    },
  });
  return {
    open(source, position, draft) { restore = position; draftRestore = draft ? validateDraft(draft) : emptyDraft(); return session.open(source); },
    close: () => { restore = undefined; return session.close(); }, zoom,
    position() {
      if (!active() || !viewReady || !geometryReady || snapshot.status !== 'ready' || restoring) return null;
      const page = viewer.currentPageNumber, div = viewer.getPageView(page - 1).div;
      const value = viewer.currentScaleValue;
      return { page, zoom: value === 'page-width' || value === 'page-fit' ? value : viewer.currentScale,
        top: Math.max(-2, Math.min(2, (container.scrollTop - div.offsetTop) / div.clientHeight)),
        left: Math.max(-2, Math.min(2, (container.scrollLeft - div.offsetLeft) / div.clientWidth)) };
    },
    zoomBy: factor => { if (Number.isFinite(factor) && factor > 0) zoom(viewer.currentScale * factor); },
    goToPage(pageNumber) {
      wheelZoom?.reset();
      if (active() && viewReady && Number.isInteger(pageNumber) && pageNumber >= 1 && pageNumber <= viewer.pagesCount) {
        cancelRestore(); viewer.currentPageNumber = pageNumber; viewer.scrollPageIntoView({ pageNumber }); viewer.update();
      }
    },
    search(query, previous = false) {
      if (!active() || !viewReady) return;
      cancelRestore();
      const again = query === search.query;
      search = { ...emptySearch(), query, pending: !!query }; notify();
      events.dispatch('find', { source: viewer, type: again ? 'again' : '', query,
        caseSensitive: false, entireWord: false, highlightAll: true, findPrevious: previous, matchDiacritics: false });
    },
    clearSearch() { events.dispatch('findbarclose', { source: viewer }); search = emptySearch(); notify(); },
    password(value) { const update = updatePassword; updatePassword = null; passwordRequired = 0; notify(); update?.(value); },
    bookmark(destination) { if (active() && viewReady) { cancelRestore(); void links.goToDestination(destination as string | any[]).catch(() => { snapshot = { ...snapshot, error: '此书签目标无法定位。' }; notify(); }); } },
    async annotation(mode) { if (!active() || !viewReady || !manager || !editingAllowed) return; textControl.commit();erasing=mode==='erase';pages.classList.toggle('reader-erasing',erasing);await changeMode(({read:AnnotationEditorType.NONE,text:AnnotationEditorType.FREETEXT,highlight:AnnotationEditorType.HIGHLIGHT,ink:AnnotationEditorType.INK,erase:AnnotationEditorType.FREETEXT})[mode]);notify(); },
    edit(action) {
      // Keep PDF.js's in-progress multi-stroke undo/redo commands alive. Ending
      // the session first discards DRAW_STEP redo and can race toolbar creation.
      if(action==='delete'||!drawingDirty?.hasSession())manager?.endCurrentEditing();
      events.dispatch('editingaction',{name:action});
    },
    textSize(value) { if(textControl.selection())textControl.textSize(value);else if (Number.isFinite(value)&&value>=8&&value<=72) manager?.updateParams(AnnotationEditorParamsType.FREETEXT_SIZE,value); },
    textColor(value){textControl.textColor(value);},
    textEdit(){textControl.edit();notify();},
    strokeStyle(kind,color,thickness){if(!manager||!editingAllowed)return;const selection=strokeSelection();if(selection&&selection.kind!==kind)return;const colorType=kind==='ink'?AnnotationEditorParamsType.INK_COLOR:AnnotationEditorParamsType.HIGHLIGHT_COLOR,thicknessType=kind==='ink'?AnnotationEditorParamsType.INK_THICKNESS:AnnotationEditorParamsType.HIGHLIGHT_THICKNESS;if(color!==undefined){if(!/^#[0-9a-f]{6}$/i.test(color))throw Error('颜色无效。');manager.updateParams(colorType,color);}if(thickness!==undefined){if(!Number.isFinite(thickness)||thickness<1||thickness>20)throw Error('粗细应为1–20。');if(selection&&!selection.canThicken)throw Error('文字高亮范围由所选文字决定。');manager.updateParams(thicknessType,thickness);}notify();},
    resizeText(rect){textControl.resize(rect);notify();},
    draft: captureDraft,
    async addText(value) {
      if(!active()||!document||!manager||!editingAllowed)throw Error('此PDF尚未就绪或不允许批注；临时回答保留。');
      const expected=document,expectedToken=token,ui=manager;
      const page=await expected.getPage(value.page);
      const canvas=pages.querySelector<HTMLCanvasElement>(`.page[data-page-number="${value.page}"] .canvasWrapper canvas`);
      const color=value.color??(canvas?pickReadableTextColor(canvas,page.getViewport({scale:1}),value.rect):'#000000');
      const entry=freeTextEntry({...value,color},page.view,page.rotate);
      if(!active()||document!==expected||token!==expectedToken)throw Error('文档已切换，未确认批注。');
      erasing=false;pages.classList.remove('reader-erasing');await changeMode(AnnotationEditorType.FREETEXT);
      if(!active()||document!==expected||token!==expectedToken||ui!==manager)throw Error('文档已切换，未确认批注。');
      const layer=ui.getLayer(value.page-1);if(!layer)throw Error('请先显示回答所在页面并等待批注层就绪。');
      ui.endCurrentEditing();const editor=await layer.deserialize({...entry,isCopy:true});
      if(!active()||document!==expected||token!==expectedToken||ui!==manager)throw Error('文档已切换，未确认批注。');
      if(!editor)throw Error('文字批注未能建立，临时回答保留。');
      layer.addOrRebuild(editor);
      textControl.attach(editor,value.rect);
      layer.addUndoableEditor(editor);ui.setSelected(editor);dirty=true;notify();
    },
    focusOverflow(page:number){textControl.focusOverflow(page);notify();},
    async testWriterBytes() {
      // Renderer half of the writer stage: refusals carry an approved code and
      // the serialize step, so a save can never fail with an unclassified
      // message. The size budget is enforced once, by writer_pdf_budget (16 MB)
      // in the desktop source; the historical 1 MB experiment cap is gone.
      if(!active()||!document||!editingAllowed||restoringPages.size||pendingRestore.size)throw new WriterRefusal('writer_renderer_busy','serialize');
      // Commit native text, then synchronously restore the already-visible PDF
      // box. Native commit remeasures rounded screen pixels on rotated pages;
      // waiting for the geometry observer would let saveDocument race it.
      textControl.commit();const expected=document,expectedToken=token;
      const output=await expected.saveDocument();
      if(!active()||document!==expected||token!==expectedToken)throw new WriterRefusal('writer_document_changed','serialize');
      return output;
    },
    async getPage(pageNumber) {
      if (!document || !Number.isInteger(pageNumber) || pageNumber < 1 || pageNumber > document.numPages) {
        throw new Error('Requested page is not available in the active document');
      }
      return document.getPage(pageNumber);
    },
    async dispose() {
      disposed = true;
      restoreEditorFocus?.();restoreEditorFocus=null;
      drawingDirty?.dispose(); drawingDirty = null;
      modeWait?.cancel();
      textControl.dispose();
      wheelZoom?.reset();
      resize.disconnect();
      events.off('pagesinit', initialize); events.off('pagechanging', positionChanged);
      events.off('scalechanging', scaleChanged); events.off('updateviewarea', positionChanged); events.off('pagerendered', rendered);
      events.off('updatefindcontrolstate', searchUpdate); events.off('updatefindmatchescount', searchUpdate); events.off('textlayerrendered', textRendered);
      events.off('annotationeditoruimanager',editorReady); events.off('editingstateschanged',editorStates); events.off('annotationeditorlayerrendered',restoreEditors);
      events.off('annotationeditormodechanged',notify);
      events.off('annotationeditorparamschanged',notify);
      events.off('switchannotationeditormode',switchEditorMode);
      try { await session.dispose(); } finally { lifetime.abort(); }
    },
  };
}
