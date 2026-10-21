// Bounded editor records, never PDF bytes, paths, executable content or passwords.
export interface AnnotationDraft { schema: 1; pdfjs: '6.4.299'; entries: Record<string, unknown>[]; }
export const emptyDraft = (): AnnotationDraft => ({schema:1,pdfjs:'6.4.299',entries:[]});
export function validateDraft(value: unknown): AnnotationDraft {
  const text = JSON.stringify(value,(_key,node)=>ArrayBuffer.isView(node)?Array.from(node as unknown as ArrayLike<number>):node);
  if (!text || text.length > 256 * 1024) throw Error('批注草稿超过保护限制，请保留当前窗口。');
  const draft = JSON.parse(text) as AnnotationDraft;
  if (draft.schema !== 1 || draft.pdfjs !== '6.4.299' || !Array.isArray(draft.entries) || draft.entries.length > 500) throw Error('批注草稿版本或数量无效。');
  const finite = (v: unknown) => typeof v === 'number' && Number.isFinite(v) && Math.abs(v) <= 1e7;
  for (const entry of draft.entries) {
    if (!entry || typeof entry !== 'object' || ![3,9,15].includes(Number(entry.annotationType)) ||
      !Number.isInteger(entry.pageIndex) || Number(entry.pageIndex) < 0 || Number(entry.pageIndex) >= 2000 ||
      !Array.isArray(entry.rect) || entry.rect.length !== 4 || !entry.rect.every(finite) ||
      ![0,90,180,270].includes(Number(entry.rotation)) ||
      entry.id != null || entry.annotationElementId != null || entry.deleted) throw Error('已有批注修改尚未支持草稿恢复；请保留当前窗口。');
    if (entry.annotationType === 3 && (typeof entry.value !== 'string' || entry.value.length > 20000 || !finite(entry.fontSize) || Number(entry.fontSize) <= 0 || !Array.isArray(entry.color) || entry.color.length !== 3 || !entry.color.every(v=>finite(v)&&Number(v)>=0&&Number(v)<=255))) throw Error('文本批注草稿无效。');
    const coordinates = (v: unknown, multiple: number, minimum=multiple): v is number[] => Array.isArray(v) && v.length>=minimum && v.length%multiple===0 && v.every(finite);
    if(entry.annotationType!==3){
      if(!Array.isArray(entry.color)||entry.color.length!==3||!entry.color.every(v=>finite(v)&&Number(v)>=0&&Number(v)<=255)||!finite(entry.thickness)||Number(entry.thickness)<=0||!finite(entry.opacity)||Number(entry.opacity)<0||Number(entry.opacity)>1)throw Error('笔画或高亮样式无效。');
      if(entry.annotationType===15){
        const paths=entry.paths as {lines?:unknown;points?:unknown}|undefined;
        if(!paths||!Array.isArray(paths.lines)||!Array.isArray(paths.points)||!paths.lines.length||paths.lines.length!==paths.points.length||!paths.points.every(p=>coordinates(p,2))||!paths.lines.every(line=>Array.isArray(line)&&line.length>=6&&line.length%6===0&&line.every((v,i)=>finite(v)||(v===null&&i%6<4))))throw Error('笔画草稿坐标无效。');
      }else if(entry.quadPoints){
        if(!coordinates(entry.quadPoints,8))throw Error('高亮草稿范围无效。');
      }else{
        const outlines=entry.outlines as {points?:unknown}|undefined;
        if(!outlines||!Array.isArray(outlines.points)||!outlines.points.length||!outlines.points.every(p=>coordinates(p,2,4)))throw Error('高亮草稿坐标无效。');
      }
    }
    const inspect = (node: unknown, depth=0) => {
      if (depth > 12) throw Error('批注草稿结构过深。');
      if (typeof node === 'number' && !finite(node)) throw Error('批注坐标无效。');
      if (node && typeof node === 'object') for (const [key,child] of Object.entries(node)) {
        if (['__proto__','prototype','constructor'].includes(key)) throw Error('批注草稿字段无效。'); inspect(child,depth+1);
      }
    }; inspect(entry);
  }
  return draft;
}
