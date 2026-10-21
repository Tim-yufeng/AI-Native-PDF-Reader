import { AnnotationEditorParamsType, AnnotationEditorType, type PageViewport } from 'pdfjs-dist';
import type { AnnotationEditorUIManager } from 'pdfjs-dist/types/src/display/editor/tools';
import { freeTextStyle, normalizeFreeTextSpaces, measureFreeTextBox, freeTextContents } from './freetext-layout';

export type TextRect = [number, number, number, number];
export interface TextSelection { id:string; page:number; rect:TextRect; text:string; fontSize:number; color:string; overflow:boolean; canResize:boolean; }

// This adapter is deliberately limited to the public editor geometry and command
// surface of the locked PDF.js version. The narrow serialization fallback below
// preserves a resized existing box that PDF.js otherwise treats as unchanged.
interface TextEditor {
  id:string; pageIndex:number; rotation:number; pageDimensions:number[]; annotationElementId:string|null;
  pageTranslation:number[]; width:number; height:number; x:number; y:number; parentDimensions:number[];
  div:HTMLDivElement|null; contentDiv:HTMLElement; editorType:string;
  parent:{div:HTMLDivElement; viewport:PageViewport; getEditableAnnotation(id:string):{data:{rect:number[]}}|undefined}|null;
  isSelected:boolean; propertiesToUpdate:unknown[][];
  getPDFRect():number[]; setDims():void; fixAndSetPosition():void;
  getRect(tx:number,ty:number):number[]; getBaseTranslation():number[];
  screenToPageTranslation(x:number,y:number):number[]; pageTranslationToScreen(x:number,y:number):number[];
  translateInPage(x:number,y:number):void; translationDone():void;
  isInEditMode():boolean; enterInEditMode():void;
  updateParams(type:number,value:unknown):void; addToAnnotationStorage():void;
  serialize(isForCopying?:boolean):Record<string,unknown>|null;
  renderAnnotationElement(annotation:unknown):HTMLElement|null;
}
interface BoxState { editor:TextEditor; rect:TextRect; minimumHeight:number; autoFit:boolean; layoutKey:string; handles:HTMLDivElement; lifetime:AbortController; observer:MutationObserver; nativeSerialize?:TextEditor['serialize']; nativeRender?:TextEditor['renderAnnotationElement']; }
const handles = ['TL','TM','TR','MR','BR','BM','BL','ML'] as const;
const equalRect = (a:number[],b:number[],epsilon=0.03) => a.every((v,i)=>Math.abs(v-b[i])<=epsilon);
const copyRect = (r:number[]):TextRect => [r[0],r[1],r[2],r[3]];
function textAxes(editor:TextEditor,rect:number[]) {
  const [pw,ph]=editor.pageDimensions,[left,bottom]=editor.pageTranslation,r=editor.rotation;
  return {width:r%180?rect[3]-rect[1]:rect[2]-rect[0],height:r%180?rect[2]-rect[0]:rect[3]-rect[1],
    maxHeight:r===90?left+pw-rect[0]:r===180?bottom+ph-rect[1]:r===270?rect[2]-left:rect[3]-bottom,
    maxWidth:r===90?bottom+ph-rect[1]:r===180?rect[2]-left:r===270?rect[3]-bottom:left+pw-rect[0]};
}
function withTextHeight(editor:TextEditor,rect:TextRect,height:number):TextRect {
  const next=copyRect(rect);
  if(editor.rotation===90)next[2]=next[0]+height;
  else if(editor.rotation===180)next[3]=next[1]+height;
  else if(editor.rotation===270)next[0]=next[2]-height;
  else next[1]=next[3]-height;
  return next;
}
function asTextEditor(value:unknown):TextEditor|null {
  const editor=value as TextEditor|null;
  return editor?.editorType==='freetext'&&editor.div&&typeof editor.getPDFRect==='function'?editor:null;
}
function validateRect(editor:TextEditor,rect:number[]):TextRect {
  if(rect.length!==4||!rect.every(Number.isFinite)||rect[2]<=rect[0]||rect[3]<=rect[1])throw Error('文字框坐标无效。');
  const [width,height]=editor.pageDimensions,[left,bottom]=editor.pageTranslation;
  if(rect[0]<left-0.01||rect[1]<bottom-0.01||rect[2]>left+width+0.01||rect[3]>bottom+height+0.01)throw Error('文字框必须留在所在 PDF 页面内。');
  return copyRect(rect);
}

/** Set one authoritative PDF-space rectangle, including rotated/CropBox pages. */
export function setTextBox(value:unknown,rect:number[]):TextRect {
  const editor=asTextEditor(value);if(!editor?.parent)throw Error('文字批注尚未就绪。');
  const target=validateRect(editor,rect),[width,height]=editor.pageDimensions;
  const turned=editor.rotation%180!==0,w=target[2]-target[0],h=target[3]-target[1];
  editor.width=(turned?h:w)/width;editor.height=(turned?w:h)/height;
  editor.setDims();
  // getPDFRect accounts for FreeText padding, page translation and rotation;
  // anchoring against its actual result avoids duplicating those assumptions.
  const actual=editor.getPDFRect();
  editor.x+=(target[0]-actual[0])/width;
  editor.y-=(target[1]-actual[1])/height;
  editor.fixAndSetPosition();
  calibrateDisplay(editor);
  return copyRect(editor.getPDFRect());
}

/** Keep normal PDF.js contentEditable, with wrapping inside a real FreeText box. */
export function installTextBox(value:unknown,rect?:number[]):TextRect|null {
  const editor=asTextEditor(value);if(!editor)return null;
  editor.div!.classList.add('reader-free-text');
  Object.assign(editor.contentDiv.style,freeTextStyle);
  normalizeFreeTextSpaces(editor.contentDiv);
  let target=rect??editor.getPDFRect();
  if(!rect){
    // A freshly typed long line is first measured by PDF.js as an auto-sized
    // editor, possibly beyond the page. Bound its box before adopting it; keep
    // every character and report overflow instead of throwing from a UI event.
    const [width,height]=editor.pageDimensions,[left,bottom]=editor.pageTranslation;
    if(target.every(Number.isFinite)&&target[2]>target[0]&&target[3]>target[1]){
      const x=Math.max(left,Math.min(left+width-1,target[0])),y=Math.max(bottom,Math.min(bottom+height-1,target[1]));
      target=[x,y,Math.max(x+1,Math.min(left+width,target[2])),Math.max(y+1,Math.min(bottom+height,target[3]))];
    }
    if(!editor.annotationElementId){
      const width=Math.min(220,textAxes(editor,target).maxWidth);
      if(editor.rotation===90)target[3]=target[1]+width;
      else if(editor.rotation===180)target[0]=target[2]-width;
      else if(editor.rotation===270)target[1]=target[3]-width;
      else target[2]=target[0]+width;
    }
  }
  return setTextBox(editor,target);
}
const measurements=new WeakMap<TextEditor,{key:string;box:ReturnType<typeof measureFreeTextBox>}>();
function measured(editor:TextEditor,width:number) {
  const text=freeTextContents(editor.contentDiv),size=Number(property(editor,AnnotationEditorParamsType.FREETEXT_SIZE,12)),key=JSON.stringify([text,width,size]);
  let cached=measurements.get(editor);
  if(cached?.key!==key){cached={key,box:measureFreeTextBox(text,width,size)};measurements.set(editor,cached);}
  return cached.box;
}
function overflow(editor:TextEditor):boolean {
  // Offscreen/temporarily hidden editors report zero scroll dimensions. Use
  // the same canonical layout as Writer, cached until text/width/font changes.
  const axes=textAxes(editor,editor.getPDFRect()),box=measured(editor,axes.width);
  return box.height>axes.height+.05||box.width>axes.width+.05;
}
function property(editor:TextEditor,type:number,fallback:unknown):unknown {
  return editor.propertiesToUpdate.find(([id])=>id===type)?.[1]??fallback;
}
function translateBox(editor:TextEditor,x:number,y:number) {
  // Keep PDF.js's native movement marker for existing-annotation saves. Its
  // translateInPage also scrolls the element into view, which is inappropriate
  // for our layout correction and undo/redo. Suppress only that synchronous
  // call on this editor; navigation and user scrolling keep their normal APIs.
  const div=editor.div!,scroll=div.scrollIntoView;
  div.scrollIntoView=()=>{};
  try{editor.translateInPage(x,y);}finally{div.scrollIntoView=scroll;}
}
function frame(editor:TextEditor) {
  // EditorLayerBuilder deliberately uses dontFlip:true for its own DOM axes.
  // PDF coordinates and the rendered canvas instead require the normal Y flip.
  // Clone through the public viewport API for both pointer and display mapping.
  const viewport=editor.parent!.viewport.clone({dontFlip:false});
  const canvas=editor.parent!.div.closest('.page')?.querySelector<HTMLElement>('.canvasWrapper');
  const bounds=(canvas??editor.parent!.div).getBoundingClientRect();
  if(!bounds.width||!bounds.height)throw Error('页面显示尺寸尚未就绪。');
  const toScreen=(x:number,y:number)=>{
    const p=viewport.convertToViewportPoint(x,y);
    return [bounds.left+p[0]*bounds.width/viewport.width,bounds.top+p[1]*bounds.height/viewport.height];
  };
  const toPdf=(x:number,y:number)=>viewport.convertToPdfPoint((x-bounds.left)*viewport.width/bounds.width,(y-bounds.top)*viewport.height/bounds.height);
  return {viewport,bounds,toScreen,toPdf};
}
function screenRect(f:ReturnType<typeof frame>,rect:TextRect):TextRect {
  const a=f.toScreen(rect[0],rect[1]),b=f.toScreen(rect[2],rect[3]);
  return [Math.min(a[0],b[0]),Math.min(a[1],b[1]),Math.max(a[0],b[0]),Math.max(a[1],b[1])];
}
function pdfRect(f:ReturnType<typeof frame>,rect:TextRect):TextRect {
  const a=f.toPdf(rect[0],rect[1]),b=f.toPdf(rect[2],rect[3]);
  return [Math.min(a[0],b[0]),Math.min(a[1],b[1]),Math.max(a[0],b[0]),Math.max(a[1],b[1])];
}

function calibrateDisplay(editor:TextEditor) {
  if(!editor.parent||!editor.div?.isConnected)return;
  // A connected editor layer can still be hidden while PDF.js updates scale
  // or enables editing. Its zero rectangle is not a usable local origin.
  const layer=editor.parent.div.getBoundingClientRect();
  if(!layer.width||!layer.height)return;
  let f:ReturnType<typeof frame>;try{f=frame(editor);}catch{return;}
  // The native normalized axes use theoretical unrotated dimensions. Rounded
  // CSS page sizes at higher DPR can instead scale X and Y differently. Keep
  // the PDF model intact and project its native box through the actual canvas.
  const rect=editor.getPDFRect(),expected=screenRect(f,copyRect(rect));
  const desiredLeft=expected[0],desiredTop=expected[1];
  const relativeRotation=(f.viewport.rotation-editor.rotation+360)%360;
  const turned=relativeRotation%180!==0;
  const width=turned?expected[3]-expected[1]:expected[2]-expected[0];
  const height=turned?expected[2]-expected[0]:expected[3]-expected[1];
  const style=editor.div.style;
  const put=(name:'width'|'height'|'left'|'top',value:number)=>{
    // CSSOM may serialize a six-digit number with fewer trailing decimals.
    // Compare numeric pixel values, so that normalization cannot loop forever.
    if(style[name].endsWith('px')&&Math.abs(parseFloat(style[name])-value)<=0.001)return;
    if(Number.isFinite(value))style[name]=`${value.toFixed(6)}px`;
  };
  put('width',width);put('height',height);
  // Layout in PDF points, then project it with the box. Zoom/DPR and selection
  // borders must never change where a word wraps. Native geometry is retained.
  const localWidth=editor.rotation%180?rect[3]-rect[1]:rect[2]-rect[0];
  const localHeight=editor.rotation%180?rect[2]-rect[0]:rect[3]-rect[1];
  const content=editor.contentDiv.style;
  const size=Number(property(editor,AnnotationEditorParamsType.FREETEXT_SIZE,12));
  const values={fontSize:`${size}px`,width:`${localWidth}px`,height:`${localHeight}px`,
    transform:`scale(${width/localWidth},${height/localHeight})`,transformOrigin:'0 0'};
  for(const [key,value] of Object.entries(values))if(content[key as keyof typeof values]!==value)content[key as keyof typeof values]=value;
  // Absolute origins avoid feeding resolved CSS left/top (and automatic scroll
  // compensation) back into another delta. The parent is a normal PDF.js layer
  // with one of four quarter-turn transforms; the editor rotates about 0,0.
  const desiredRight=desiredLeft+expected[2]-expected[0],desiredBottom=desiredTop+expected[3]-expected[1];
  const editorOrigin=relativeRotation===90?[desiredRight,desiredTop]:relativeRotation===180?[desiredRight,desiredBottom]:relativeRotation===270?[desiredLeft,desiredBottom]:[desiredLeft,desiredTop];
  const rotation=f.viewport.rotation;
  const layerOrigin=rotation===90?[layer.right,layer.top]:rotation===180?[layer.right,layer.bottom]:rotation===270?[layer.left,layer.bottom]:[layer.left,layer.top];
  const dx=editorOrigin[0]-layerOrigin[0],dy=editorOrigin[1]-layerOrigin[1];
  const local=rotation===90?[dy,-dx]:rotation===180?[-dx,-dy]:rotation===270?[-dy,dx]:[dx,dy];
  put('left',local[0]);put('top',local[1]);
}

/** Sample only the rendered PDF canvas beneath this box; no screenshot is kept. */
export function pickReadableTextColor(canvas:HTMLCanvasElement,viewport:PageViewport,rect:number[]):string {
  try {
    const context=canvas.getContext('2d',{willReadFrequently:true});if(!context)return '#000000';
    const a=viewport.convertToViewportPoint(rect[0],rect[1]),b=viewport.convertToViewportPoint(rect[2],rect[3]);
    const left=Math.max(0,Math.min(a[0],b[0])),top=Math.max(0,Math.min(a[1],b[1]));
    const right=Math.min(viewport.width,Math.max(a[0],b[0])),bottom=Math.min(viewport.height,Math.max(a[1],b[1]));
    if(right<=left||bottom<=top)return '#000000';
    let luminance=0,count=0;
    const linear=(channel:number)=>{const value=channel/255;return value<=0.04045?value/12.92:((value+0.055)/1.055)**2.4;};
    for(let row=0;row<7;row++)for(let col=0;col<7;col++){
      const x=Math.min(canvas.width-1,Math.floor((left+(right-left)*(col+0.5)/7)*canvas.width/viewport.width));
      const y=Math.min(canvas.height-1,Math.floor((top+(bottom-top)*(row+0.5)/7)*canvas.height/viewport.height));
      const pixel=context.getImageData(x,y,1,1).data,alpha=pixel[3]/255;
      luminance+=0.2126*linear(pixel[0]*alpha+255*(1-alpha))+0.7152*linear(pixel[1]*alpha+255*(1-alpha))+0.0722*linear(pixel[2]*alpha+255*(1-alpha));count++;
    }
    const mean=luminance/count;
    return 1.05/(mean+0.05)>(mean+0.05)/0.05?'#ffffff':'#000000';
  }catch{return '#000000';}
}

export function createFreeTextControl(options:{container:HTMLElement;manager:()=>AnnotationEditorUIManager|null;changed?:()=>void}) {
  const controlLifetime=new AbortController();
  const states=new Map<string,BoxState>();let gesture:AbortController|null=null,gestureCancel:(()=>void)|null=null,disposed=false,committing=false;
  let layoutFrame=0;
  const changed=()=>{if(!disposed)options.changed?.();};
  const scheduleLayout=()=>{
    if(disposed||!states.size||layoutFrame)return;
    layoutFrame=requestAnimationFrame(()=>{
      layoutFrame=0;if(disposed)return;
      for(const state of states.values())if(state.editor.parent&&state.editor.div?.isConnected)calibrateDisplay(state.editor);
    });
  };
  // One shared observer pair per reader, not one pair per annotation. Only
  // page/layer layout attributes trigger work; our own editor style writes do
  // not schedule another frame and do not create an observer feedback loop.
  const layoutResize=new ResizeObserver(scheduleLayout);
  const layoutMutation=new MutationObserver(records=>{
    if(records.some(({target})=>target instanceof HTMLElement&&target.matches('.annotationEditorLayer,.canvasWrapper,.page,.pdfViewer')))scheduleLayout();
  });
  const observeLayout=(editor:TextEditor)=>{
    layoutResize.observe(options.container);layoutResize.observe(editor.parent!.div);
    const canvas=editor.parent!.div.closest('.page')?.querySelector<HTMLElement>('.canvasWrapper');
    if(canvas)layoutResize.observe(canvas);
    layoutMutation.observe(options.container,{subtree:true,attributes:true,attributeFilter:['hidden','style']});
    scheduleLayout();
  };
  const stateFor=(value:unknown):BoxState|null=>{const editor=asTextEditor(value);return editor?states.get(editor.id)??null:null;};
  const selected=()=>stateFor(options.manager()?.firstSelectedEditor);
  const beginEditing=(state:BoxState)=>{
    const ui=options.manager();if(!ui||!state.editor.parent)return;
    ui.setSelected(state.editor as never);
    if(ui.getMode()===AnnotationEditorType.FREETEXT)state.editor.enterInEditMode();
    else {
      // The host forwards this standard request to PDFViewer. Edit entry must
      // wait for the viewer to finish enabling its normal annotation layers.
      const request={mode:AnnotationEditorType.FREETEXT,editId:state.editor.annotationElementId||state.editor.id,mustEnterInEditMode:true};
      ui.updateToolbar(request);
    }
  };
  const apply=(state:BoxState,rect:TextRect)=>{
    if(!state.editor.parent)return;
    // Native existing-PDF serialization uses a movement marker. Public native
    // translation creates that marker; directly writing x/y alone would lose a
    // move when saving an otherwise unchanged existing annotation.
    const current=state.editor.getPDFRect(),dx=rect[0]-current[0],dy=rect[1]-current[1];
    if((!gesture||gesture.signal.aborted)&&(Math.abs(dx)>0.01||Math.abs(dy)>0.01)){translateBox(state.editor,dx,-dy);state.editor.translationDone();}
    // Preserve the native movement baseline; dimension-only edits are handled
    // by the bounded native serialization fallback installed at attachment.
    else if(state.editor.annotationElementId)translateBox(state.editor,0,0);
    state.rect=setTextBox(state.editor,rect);state.editor.addToAnnotationStorage();
    state.editor.div?.classList.toggle('reader-text-overflow',overflow(state.editor));
  };
  const fit=(state:BoxState)=>{
    if(!state.autoFit||!state.editor.parent||!state.editor.div?.isConnected)return false;
    const editor=state.editor,axes=textAxes(editor,state.rect),size=Number(property(editor,AnnotationEditorParamsType.FREETEXT_SIZE,12));
    const text=freeTextContents(editor.contentDiv),key=JSON.stringify([text,axes.width,size,axes.maxHeight,state.minimumHeight]);
    if(key===state.layoutKey)return false;
    state.layoutKey=key;
    const required=measured(editor,axes.width).height;
    const height=Math.min(axes.maxHeight,Math.max(state.minimumHeight,Math.ceil(required*100)/100));
    const next=withTextHeight(editor,state.rect,height);
    const resized=!equalRect(state.rect,next);
    if(resized)apply(state,next);else calibrateDisplay(editor);
    editor.div?.classList.toggle('reader-text-overflow',overflow(editor));
    return resized;
  };
  const record=(state:BoxState,previous:TextRect,next:TextRect)=>{
    if(equalRect(previous,next))return;
    const beforeMinimum=state.minimumHeight,beforeAuto=state.autoFit,afterMinimum=Math.abs(textAxes(state.editor,next).height-textAxes(state.editor,previous).height)>.03?textAxes(state.editor,next).height:beforeMinimum;
    const update=(rect:TextRect,minimum:number,auto:boolean)=>{state.autoFit=auto;state.minimumHeight=minimum;state.layoutKey='';apply(state,rect);fit(state);changed();};
    options.manager()?.addCommands({cmd:()=>update(next,afterMinimum,true),undo:()=>update(previous,beforeMinimum,beforeAuto),mustExec:true});
  };
  function pointer(state:BoxState,event:PointerEvent,handle?:HTMLElement) {
    if(disposed||event.button!==0||!state.editor.parent||handle&&(handle as HTMLButtonElement).disabled||(!handle&&state.editor.isInEditMode()))return;
    if(event.pointerType==='touch'&&!event.isPrimary)return;
    const target=event.target as HTMLElement;
    if(!handle&&target.closest('button,input,textarea,.editToolbar,.reader-freetext-handles'))return;
    event.preventDefault();event.stopImmediatePropagation();gestureCancel?.();gesture?.abort();
    const lifetime=gesture=new AbortController(),before=copyRect(state.rect),origin=[event.clientX,event.clientY];
    const parent=state.editor.parent,ui=options.manager();
    const attached=()=>!disposed&&states.get(state.editor.id)===state&&state.editor.parent===parent&&options.manager()===ui&&!!state.editor.div?.isConnected&&options.container.contains(state.editor.div);
    const finish=()=>{lifetime.abort();if(gesture===lifetime){gesture=null;gestureCancel=null;}};
    ui?.setSelected(state.editor as never);
    if(!attached()){finish();return;}
    if(state.editor.annotationElementId)translateBox(state.editor,0,0);
    let f:ReturnType<typeof frame>;
    try{f=frame(state.editor);}catch{finish();return;}
    const screen=screenRect(f,before);
    // Handle names are in editor-local orientation. Classify their *screen*
    // positions instead of assuming TL remains top-left after page rotation.
    const hb=handle?.getBoundingClientRect();
    const hx=hb?((hb.left+hb.right)/2-screen[0])/(screen[2]-screen[0]):0.5;
    const hy=hb?((hb.top+hb.bottom)/2-screen[1])/(screen[3]-screen[1]):0.5;
    const horizontal=handle?(hx<0.25?-1:hx>0.75?1:0):0;
    const vertical=handle?(hy<0.25?-1:hy>0.75?1:0):0;
    let moved=false;
    const restore=()=>{
      if(lifetime.signal.aborted)return;
      try{if(attached()){apply(state,before);changed();}}
      finally{finish();}
    };
    gestureCancel=restore;
    window.addEventListener('pointermove',e=>{
      if(e.pointerId!==event.pointerId)return;if(!attached()){finish();return;}
      e.preventDefault();const dx=e.clientX-origin[0],dy=e.clientY-origin[1];
      if(Math.hypot(dx,dy)<2&&!moved)return;moved=true;
      const next=copyRect(screen);
      if(!handle){next[0]+=dx;next[2]+=dx;next[1]+=dy;next[3]+=dy;}
      else {if(horizontal<0)next[0]+=dx;else if(horizontal>0)next[2]+=dx;if(vertical<0)next[1]+=dy;else if(vertical>0)next[3]+=dy;}
      if(next[2]-next[0]<24||next[3]-next[1]<20)return;
      try{apply(state,validateRect(state.editor,pdfRect(f,next)));changed();}catch{/* Keep the last valid on-page box. */}
    },{signal:lifetime.signal});
    window.addEventListener('pointerup',e=>{
      if(e.pointerId!==event.pointerId)return;if(!attached()){finish();return;}e.preventDefault();const after=copyRect(state.rect);finish();
      state.editor.translationDone();
      if(moved)record(state,before,after);changed();
    },{signal:lifetime.signal});
    window.addEventListener('pointercancel',restore,{signal:lifetime.signal});
    window.addEventListener('pointerdown',restore,{capture:true,signal:lifetime.signal});
    window.addEventListener('wheel',restore,{capture:true,passive:true,signal:lifetime.signal});
    window.addEventListener('scroll',restore,{capture:true,passive:true,signal:lifetime.signal});
    window.addEventListener('resize',restore,{signal:lifetime.signal});
    window.addEventListener('keydown',e=>{if(e.key==='Escape'){e.preventDefault();e.stopPropagation();restore();}},{capture:true,signal:lifetime.signal});
  }
  function attach(value:unknown,rect?:number[]) {
    const editor=asTextEditor(value);if(!editor?.parent||disposed)return;
    const current=states.get(editor.id);
    if(current&&current.editor===editor){if(rect){current.minimumHeight=textAxes(editor,rect).height;current.layoutKey='';apply(current,copyRect(rect));fit(current);}return;}
    if(current){current.lifetime.abort();current.observer.disconnect();current.handles.remove();if(current.nativeSerialize)current.editor.serialize=current.nativeSerialize;states.delete(editor.id);}
    // Existing FreeText is positioned by PDF.js from its appearance's first
    // text baseline. Our fixed-size box must use the annotation's saved Rect,
    // otherwise each reopen accumulates the font descent as a vertical shift.
    const savedRect=editor.annotationElementId?editor.parent.getEditableAnnotation(editor.annotationElementId)?.data.rect:undefined;
    const target=installTextBox(editor,rect??savedRect);if(!target)return;
    const lifetime=new AbortController(),holder=document.createElement('div');holder.className='reader-freetext-handles';
    const observer=new MutationObserver(()=>{
      // Native font/commit commands may translate or remeasure their auto-sized
      // box. Preserve this fixed box, without touching any private editor method.
      const state=states.get(editor.id);if(!state||gesture&&!gesture.signal.aborted||!editor.parent)return;
      normalizeFreeTextSpaces(editor.contentDiv);
      const corrected=!equalRect(editor.getPDFRect(),state.rect);
      if(corrected)apply(state,state.rect);
      else calibrateDisplay(editor);
      const resized=fit(state);
      // Reprojecting an untouched saved box is display work. Only an actual
      // edit/auto-height change should notify the host's dirty callback.
      if(state.autoFit&&(corrected||resized))changed();
    });
    const state:BoxState={editor,rect:target,minimumHeight:textAxes(editor,target).height,autoFit:!editor.annotationElementId,layoutKey:'',handles:holder,lifetime,observer};states.set(editor.id,state);
    // PDF.js creates a separate HTML overlay for unsaved text in read mode.
    // Keep that preview on the same canonical layout as the editor and /AP.
    const render=state.nativeRender=editor.renderAnnotationElement;
    editor.renderAnnotationElement=function(annotation){
      const content=render.call(this,annotation);if(!content)return content;
      const r=this.getPDFRect(),turned=this.rotation%180!==0;
      Object.assign(content.style,freeTextStyle,{fontSize:`${property(this,AnnotationEditorParamsType.FREETEXT_SIZE,12)}px`,
        width:`${turned?r[3]-r[1]:r[2]-r[0]}px`,height:`${turned?r[2]-r[0]:r[3]-r[1]}px`,
        transform:'scale(var(--total-scale-factor))',transformOrigin:'0 0'});
      normalizeFreeTextSpaces(content);return content;
    };
    if(editor.annotationElementId&&savedRect){
      const original=state.nativeSerialize=editor.serialize,baseline=copyRect(savedRect);
      editor.serialize=function(isForCopying=false){
        const result=original.call(this,isForCopying);
        // Native deletion, text/font/color/move changes and copy all take their
        // unchanged path. Only an omitted, resized existing object needs help.
        if(result||isForCopying||equalRect(this.getPDFRect(),baseline))return result;
        const full=original.call(this,true);if(!full)return null;
        const {isCopy:_,...serialized}=full;
        return {...serialized,id:this.annotationElementId};
      };
    }
    for(const name of handles){
      const button=document.createElement('button');button.type='button';button.dataset.handle=name;
      button.setAttribute('aria-label',`调整文字框 ${name}`);button.title='拖动调整文字框；方向键移动';
      button.addEventListener('pointerdown',e=>pointer(state,e,button),{capture:true,signal:lifetime.signal});
      button.addEventListener('dblclick',e=>{e.preventDefault();e.stopPropagation();},{signal:lifetime.signal});
      holder.append(button);
    }
    editor.div!.append(holder);
    editor.div!.addEventListener('pointerdown',e=>pointer(state,e),{capture:true,signal:lifetime.signal});
    editor.div!.addEventListener('dblclick',e=>{
      if((e.target as HTMLElement).closest('.editToolbar,.reader-freetext-handles'))return;
      e.preventDefault();e.stopImmediatePropagation();beginEditing(state);
    },{capture:true,signal:lifetime.signal});
    // Enter/double click continue to use PDF.js native contentEditable. Arrow
    // movement outside text input uses the same checked rectangle/undo path.
    editor.div!.addEventListener('keydown',e=>{
      if(editor.isInEditMode()||e.isComposing||e.altKey||e.metaKey||!['ArrowLeft','ArrowRight','ArrowUp','ArrowDown'].includes(e.key))return;
      e.preventDefault();e.stopImmediatePropagation();const f=frame(editor),before=copyRect(state.rect),screen=screenRect(f,before),delta=e.shiftKey?10:1;
      const dx=e.key==='ArrowLeft'?-delta:e.key==='ArrowRight'?delta:0,dy=e.key==='ArrowUp'?-delta:e.key==='ArrowDown'?delta:0;
      screen[0]+=dx;screen[2]+=dx;screen[1]+=dy;screen[3]+=dy;
      try{record(state,before,validateRect(editor,pdfRect(f,screen)));}catch{/* Boundary reached. */}
    },{capture:true,signal:lifetime.signal});
    editor.contentDiv.addEventListener('input',()=>{state.autoFit=true;state.layoutKey='';fit(state);changed();},{signal:lifetime.signal});
    observer.observe(editor.div!,{attributes:true,attributeFilter:['style']});
    observer.observe(editor.contentDiv,{childList:true,subtree:true,characterData:true});
    editor.div!.classList.toggle('reader-text-overflow',overflow(editor));
    fit(state);
    observeLayout(editor);
  }
  // Native typed editors have no dimensions until their first commit. Adopt
  // their public DOM dimensions on first input, while the caret is still live.
  options.container.addEventListener('input',event=>{
    const node=(event.target as HTMLElement).closest<HTMLElement>('.freeTextEditor');if(!node)return;
    const editor=asTextEditor(options.manager()?.getEditor(node.id));if(!editor?.parent||states.has(editor.id))return;
    if(!editor.width||!editor.height){const r=node.getBoundingClientRect(),[pw,ph]=editor.parentDimensions;
      editor.width=(editor.rotation%180===editor.parent.viewport.rotation%180?r.width:r.height)/pw;
      editor.height=(editor.rotation%180===editor.parent.viewport.rotation%180?r.height:r.width)/ph;}
    attach(editor);const state=states.get(editor.id);if(state){state.autoFit=true;fit(state);changed();}
  },{capture:true,signal:controlLifetime.signal});
  function commit() {
    if(disposed||committing)return;committing=true;
    try{gestureCancel?.();options.manager()?.endCurrentEditing();for(const state of states.values())if(state.editor.parent)apply(state,state.rect);}
    finally{committing=false;}
  }
  function reset() {
    // A document switch drops the old gesture without invoking its restore
    // closure: it must never rewrite geometry or notify the new document.
    gesture?.abort();gesture=null;gestureCancel=null;
    cancelAnimationFrame(layoutFrame);layoutFrame=0;layoutResize.disconnect();layoutMutation.disconnect();
    for(const state of states.values()){state.lifetime.abort();state.observer.disconnect();state.handles.remove();if(state.nativeSerialize)state.editor.serialize=state.nativeSerialize;if(state.nativeRender)state.editor.renderAnnotationElement=state.nativeRender;}
    states.clear();
  }
  return {
    attach,commit,reset,
    refresh(){gestureCancel?.();for(const state of states.values())if(state.editor.parent)setTextBox(state.editor,state.rect);scheduleLayout();},
    cancelGesture(){gestureCancel?.();},
    selection():TextSelection|null {
      const state=selected();if(!state?.editor.parent)return null;
      return {id:state.editor.id,page:state.editor.pageIndex+1,rect:copyRect(state.rect),text:freeTextContents(state.editor.contentDiv),fontSize:Number(property(state.editor,AnnotationEditorParamsType.FREETEXT_SIZE,12)),color:String(property(state.editor,AnnotationEditorParamsType.FREETEXT_COLOR,'#000000')),overflow:overflow(state.editor),canResize:true};
    },
    overflowPages(){return [...new Set([...states.values()].filter(s=>s.autoFit&&s.editor.parent&&s.editor.div?.isConnected&&s.editor.contentDiv.innerText.trim()&&overflow(s.editor)).map(s=>s.editor.pageIndex+1))].sort((a,b)=>a-b);},
    focusOverflow(page:number){const state=[...states.values()].find(s=>s.autoFit&&s.editor.pageIndex+1===page&&s.editor.parent&&s.editor.div?.isConnected&&overflow(s.editor));if(state){options.manager()?.setSelected(state.editor as never);state.editor.div?.scrollIntoView({block:'nearest'});state.editor.div?.focus({preventScroll:true});}},
    edit(){const state=selected();if(state)beginEditing(state);},
    editById(id:string,mustEnter=true){
      const ui=options.manager();if(disposed||ui?.getMode()!==AnnotationEditorType.FREETEXT)return false;
      const state=[...states.values()].find(item=>item.editor.id===id||item.editor.annotationElementId===id);
      if(!state?.editor.parent||!state.editor.div?.isConnected||!options.container.contains(state.editor.div))return false;
      ui.setSelected(state.editor as never);if(mustEnter)state.editor.enterInEditMode();return true;
    },
    resize(rect:number[]){const state=selected();if(!state?.editor.parent)throw Error('请先选择文字批注。');commit();record(state,copyRect(state.rect),validateRect(state.editor,rect));},
    textSize(value:number){const state=selected();if(!state?.editor.parent)return;if(!Number.isFinite(value)||value<8||value>72)throw Error('文字字号应为 8–72。');state.autoFit=true;options.manager()?.setSelected(state.editor as never);state.editor.updateParams(AnnotationEditorParamsType.FREETEXT_SIZE,value);apply(state,state.rect);fit(state);changed();},
    textColor(value:string){const state=selected();if(!state?.editor.parent)return;if(!/^#[0-9a-f]{6}$/i.test(value))throw Error('文字颜色无效。');options.manager()?.setSelected(state.editor as never);state.editor.updateParams(AnnotationEditorParamsType.FREETEXT_COLOR,value);apply(state,state.rect);changed();},
    dispose(){disposed=true;controlLifetime.abort();reset();},
  };
}
