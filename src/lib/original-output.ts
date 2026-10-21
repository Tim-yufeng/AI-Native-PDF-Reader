import { SourceError } from './document-source.ts';

type SavedAnnotation = {id:string;type:string;rect:number[];text:string|undefined};
/** PDF.js uses doubles; MuPDF parses/writes PDF reals as float32. Its decimal
 * parsing can differ by one float32 ULP from JS round-to-nearest. Accept that
 * representation bound only, with a hard 0.0001 PDF-point cap.
 * IDs, type, order and text remain exact. No writer bytes or permissions change. */
export function sameSavedAnnotations(before:SavedAnnotation[],after:SavedAnnotation[]):boolean {
  return before.length===after.length&&before.every((a,i)=>{
    const b=after[i];
    return a.id===b.id&&a.type===b.type&&a.text===b.text&&a.rect.length===4&&b.rect.length===4&&a.rect.every((v,j)=>{
      const w=b.rect[j],magnitude=Math.max(Math.abs(v),Math.abs(w));
      const ulp=Math.max(2**-149,2**(Math.floor(Math.log2(magnitude))-23));
      return Number.isFinite(v)&&Number.isFinite(w)&&(v===w||(Math.abs(v-w)<=0.0001&&Math.abs(Math.fround(v)-Math.fround(w))<=ulp));
    });
  });
}

/** Independent reader admission for original save. The frozen Writer is unchanged. */
export async function validateOriginalOutput(candidate: Uint8Array, output: Uint8Array, signal: AbortSignal) {
  const {getDocument,GlobalWorkerOptions}=await import('pdfjs-dist');
  // The existing reader configures its bundled worker before a document opens.
  if(!GlobalWorkerOptions.workerSrc)throw new SourceError('PDF 读取器尚未就绪，请重新打开文件后保存。');
  const options={isEvalSupported:false,enableScripting:false,useSystemFonts:false,useWasm:false,stopAtErrors:true,
    standardFontDataUrl:'/pdfjs/standard_fonts/'};
  const before=getDocument({...options,data:candidate.slice()}),after=getDocument({...options,data:output.slice()});
  const cancel=()=>{void before.destroy();void after.destroy();};signal.addEventListener('abort',cancel,{once:true});
  try {
    const [a,b]=await Promise.all([before.promise,after.promise]);signal.throwIfAborted();
    if(a.numPages!==b.numPages||a.numPages>2000||a.isPureXfa||await a.getPermissions()!==null||await a.getFieldObjects()!==null)
      throw {code:'writer_original_unsupported'};
    for(let page=1;page<=a.numPages;page++) {
      signal.throwIfAborted();const [x,y]=await Promise.all([a.getPage(page),b.getPage(page)]);
      if(x.rotate!==y.rotate||x.userUnit!==y.userUnit||JSON.stringify(x.view)!==JSON.stringify(y.view))throw {code:'writer_original_geometry_changed'};
      const [xa,ya,xt,yt]=await Promise.all([x.getAnnotations(),y.getAnnotations(),x.getTextContent(),y.getTextContent()]);
      const annotations=(items:typeof xa)=>items.map(v=>({id:v.id,type:v.subtype,rect:v.rect,text:v.contentsObj?.str}));
      const text=(v:typeof xt)=>v.items.map(t=>'str'in t?t.str:'').join('\n');
      if(!sameSavedAnnotations(annotations(xa),annotations(ya)))throw {code:'writer_original_annotations_changed'};
      if(text(xt)!==text(yt))throw {code:'writer_original_text_changed'};
      // Resolves page resources and annotation appearances without rasterising.
      await y.getOperatorList();x.cleanup();y.cleanup();
    }
  } catch(error) {
    signal.throwIfAborted();if(error instanceof SourceError)throw error;
    const code=error&&typeof error==='object'?Object.getOwnPropertyDescriptor(error,'code')?.value:undefined;
    if(['writer_original_unsupported','writer_original_geometry_changed','writer_original_annotations_changed','writer_original_text_changed'].includes(code))throw {code};
    throw {code:'writer_original_reader_failed'};
  } finally {signal.removeEventListener('abort',cancel);await Promise.allSettled([before.destroy(),after.destroy()]);}
}
