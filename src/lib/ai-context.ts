import { AnnotationMode, getDocument } from 'pdfjs-dist';
import type { PDFPageProxy } from 'pdfjs-dist';
export interface PageReader { getPage(page: number): Promise<PDFPageProxy>; }
export interface Region { page: number; rect: [number, number, number, number]; }
export interface ContextSource { id: string; grantId: string; name: string; page: number; text: string; rect?: Region['rect'] | null; image: boolean; reasons?: string[]; documentId?: string | null; fileKey?: string | null; hash?: string | null; }
export const isAttachedImage = (source: ContextSource): boolean => (source.reasons ?? []).includes('attached');
export interface ContextPlan { id: string; sources: ContextSource[]; textBytes: number; expiresSeconds: number; historyMessages: number; presetVersion?:string|null; context?:{schemaVersion:1;beforePages:number;afterPages:number}; coverage?:{totalPages:number;indexedPages:number;imageOnlyPages:number;completeRequired:boolean}; trimmed?:number; }
export async function secondaryPdf(bytes: Uint8Array, signal: AbortSignal) {
  signal.throwIfAborted();
  const task = getDocument({ data: bytes, cMapUrl: '/pdfjs/cmaps/', cMapPacked: true, standardFontDataUrl: '/pdfjs/standard_fonts/', wasmUrl: '/pdfjs/wasm/', useSystemFonts: false });
  const stop = () => { void task.destroy(); }; signal.addEventListener('abort', stop, { once: true });
  try { const pdf = await task.promise; signal.throwIfAborted(); return { pdf, close: async () => { signal.removeEventListener('abort', stop); await task.destroy(); } }; }
  catch (e) { signal.removeEventListener('abort', stop); await task.destroy(); throw e; }
}
export async function extractPages(reader: PageReader, total: number, signal: AbortSignal, put: (pages: { page: number; text: string }[]) => Promise<void>, progress: (page: number) => void) {
  let batch: { page: number; text: string }[] = [], bytes = 0;
  for (let n = 1; n <= total; n++) {
    signal.throwIfAborted(); const page = await reader.getPage(n);
    const content = await page.getTextContent(); signal.throwIfAborted();
    const text = content.items.map(item => 'str' in item ? item.str + (item.hasEOL ? '\n' : ' ') : '').join('');
    const length = new TextEncoder().encode(text).byteLength; bytes += length;
    if (length > 32768 || bytes > 16 * 1024 * 1024) throw Error('本地索引超过本阶段预算，未静默删页；请使用较小测试 PDF。');
    batch.push({ page: n, text }); progress(n);
    if (batch.length === 16 || n === total) { await put(batch); batch = []; await new Promise(resolve => setTimeout(resolve, 0)); }
  }
}
export function normalizeRect(a: number[], b: number[]): Region['rect'] { return [Math.min(a[0], b[0]), Math.min(a[1], b[1]), Math.max(a[0], b[0]), Math.max(a[1], b[1])]; }
export async function pageImage(reader: PageReader, source: ContextSource, signal: AbortSignal): Promise<string> {
  signal.throwIfAborted(); const page = await reader.getPage(source.page);
  const original = page.getViewport({ scale: 1 });
  const scale = Math.min(1.5, 1600 / original.width, 1600 / original.height);
  const viewport = page.getViewport({ scale });
  const bounds = source.rect ? [...viewport.convertToViewportPoint(source.rect[0], source.rect[1]), ...viewport.convertToViewportPoint(source.rect[2], source.rect[3])] : [0, 0, viewport.width, viewport.height];
  const box = normalizeRect(bounds.slice(0, 2), bounds.slice(2));
  const left = Math.max(0, Math.floor(box[0])), top = Math.max(0, Math.floor(box[1]));
  const width = Math.ceil(Math.min(viewport.width, box[2])) - left, height = Math.ceil(Math.min(viewport.height, box[3])) - top;
  if (width < 1 || height < 1 || width * height > 2560000) throw Error('区域为空或图像超过预算。');
  const canvas = document.createElement('canvas'); canvas.width = width; canvas.height = height;
  const task = page.render({ canvas, viewport, transform: [1, 0, 0, 1, -left, -top], annotationMode: AnnotationMode.ENABLE });
  const cancel = () => task.cancel(); signal.addEventListener('abort', cancel, { once: true });
  try { await task.promise; signal.throwIfAborted(); return canvas.toDataURL('image/png'); }
  finally { signal.removeEventListener('abort', cancel); canvas.width = 0; canvas.height = 0; }
}
// Only explicit rectangular selection consumes pointer events. Ordinary reading
// and PDF.js annotations retain their own handlers outside this mode.
export function selectRegion(container: HTMLElement, reader: PageReader, completed: (region: Region) => void, cancelled: () => void): () => void {
  const lifetime = new AbortController(); let drawing = false, start: [number, number] = [0, 0], number = 0,pointerId=-1,active=true;
  let bounds: DOMRect, overlay: HTMLDivElement | null = null;
  const finish = () => { lifetime.abort(); overlay?.remove(); container.style.cursor = ''; };
  container.style.cursor = 'crosshair';
  container.addEventListener('pointerdown', event => {
    if (event.button !== 0 || drawing||event.pointerType==='touch'&&!event.isPrimary) return;
    const target = (event.target as Element).closest<HTMLElement>('.page');
    const canvas = target?.querySelector<HTMLElement>('.canvasWrapper'); if (!target || !canvas) return;
    event.preventDefault(); event.stopImmediatePropagation(); drawing = true; number = Number(target.dataset.pageNumber); bounds = canvas.getBoundingClientRect();
    pointerId=event.pointerId;start = [Math.max(bounds.left,Math.min(bounds.right,event.clientX)),Math.max(bounds.top,Math.min(bounds.bottom,event.clientY))]; overlay = document.createElement('div');
    Object.assign(overlay.style, { position: 'fixed', pointerEvents: 'none', border: '1px solid #557c9e', background: '#557c9e22', zIndex: '20' }); document.body.append(overlay);
    try{container.setPointerCapture(event.pointerId);}catch{/* Synthetic headless pointer events have no active native pointer. */}
  }, { capture: true, signal: lifetime.signal });
  container.addEventListener('pointermove', event => {
    if (!drawing || !overlay||event.pointerId!==pointerId) return; event.preventDefault(); event.stopImmediatePropagation();
    const x = Math.min(bounds.right, Math.max(bounds.left, event.clientX)), y = Math.min(bounds.bottom, Math.max(bounds.top, event.clientY));
    Object.assign(overlay.style, { left: `${Math.min(start[0], x)}px`, top: `${Math.min(start[1], y)}px`, width: `${Math.abs(start[0] - x)}px`, height: `${Math.abs(start[1] - y)}px` });
  }, { capture: true, signal: lifetime.signal });
  container.addEventListener('pointerup', event => {
    if (!drawing||event.pointerId!==pointerId) return; event.preventDefault(); event.stopImmediatePropagation();
    const end: [number, number] = [Math.min(bounds.right, Math.max(bounds.left, event.clientX)), Math.min(bounds.bottom, Math.max(bounds.top, event.clientY))];
    finish();try{if(container.hasPointerCapture(pointerId))container.releasePointerCapture(pointerId);}catch{} if (Math.abs(start[0] - end[0]) < 8 || Math.abs(start[1] - end[1]) < 8) {active=false;cancelled();return; }
    void reader.getPage(number).then(page => {
      if(!active)return;active=false;
      const v = page.getViewport({ scale: 1 });
      const convert = (p: number[]) => v.convertToPdfPoint((p[0] - bounds.left) * v.width / bounds.width, (p[1] - bounds.top) * v.height / bounds.height);
      completed({ page: number, rect: normalizeRect(convert(start), convert(end)) });
    }).catch(()=>{if(active){active=false;cancelled();}});
  }, { capture: true, signal: lifetime.signal });
  const cancel=()=>{if(!active)return;active=false;finish();cancelled();};
  container.addEventListener('pointercancel',cancel, { capture: true, signal: lifetime.signal });
  window.addEventListener('keydown', event => { if (event.key === 'Escape'&&!event.isComposing)cancel(); }, { signal: lifetime.signal });
  for(const event of ['wheel','scroll','resize'])window.addEventListener(event,cancel,{capture:true,passive:true,signal:lifetime.signal});
  return ()=>{active=false;finish();};
}
