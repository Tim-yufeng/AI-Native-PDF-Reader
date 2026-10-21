import { validateDraft,emptyDraft } from './annotation-draft.ts';
export interface NewText {page:number;rect:[number,number,number,number];text:string;fontSize:number;color?:string;}
export function freeTextEntry(value:NewText,view:number[],rotation:number):Record<string,unknown> {
 if(value.rect[0]<view[0]||value.rect[1]<view[1]||value.rect[2]>view[2]||value.rect[3]>view[3]||value.rect[0]>=value.rect[2]||value.rect[1]>=value.rect[3]||!value.text.trim()||value.fontSize<12||value.fontSize>72)throw Error('回答位置、文字或字号无效，请调整回答框。');
 const color=value.color??'#000000';if(!/^#[0-9a-f]{6}$/i.test(color))throw Error('文字颜色无效。');
 return validateDraft({...emptyDraft(),entries:[{annotationType:3,pageIndex:value.page-1,rect:value.rect,rotation,value:value.text,fontSize:value.fontSize,color:[1,3,5].map(i=>parseInt(color.slice(i,i+2),16))}]}).entries[0];
}
