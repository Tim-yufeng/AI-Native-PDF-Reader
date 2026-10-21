import { invoke } from '@tauri-apps/api/core';
export interface Style {id:string;version:number;name:string;aliases:string[];guidelines:string;avoid:string;enabled:boolean;}
export interface Styles {schemaVersion:number;selected:string|null;revisions:Style[];recent?:string[];}
export const actions={region:'区域提问',quick:'快速提问',inline:'原位解释',read:'阅读模式',text:'文本批注',highlight:'高亮',ink:'笔画',ai:'展开/折叠 AI'};
export type Action=keyof typeof actions;
export interface InputSettings {schemaVersion:number;bindings:Record<Action,string>;}
export interface ContextSettings {schemaVersion:1;beforePages:number;afterPages:number;automaticCurrentPage?:boolean;inlineFullDocument?:boolean;/** Soft character budget for one in-place explanation. 0 disables it. */inlineAnswerLimit:number;}
/** The default the user asked for: an in-place answer of about twenty characters. */
export const DEFAULT_INLINE_ANSWER_LIMIT=20;
export const defaultContext=():ContextSettings=>({schemaVersion:1,beforePages:0,afterPages:0,automaticCurrentPage:true,inlineFullDocument:true,inlineAnswerLimit:DEFAULT_INLINE_ANSWER_LIMIT});
export function contextError(value:ContextSettings):string{
 if(value.schemaVersion!==1||![value.beforePages,value.afterPages].every(n=>Number.isInteger(n)&&n>=0&&n<=5))return '邻近页范围应为前后各 0–5 页。';
 if(!Number.isInteger(value.inlineAnswerLimit)||value.inlineAnswerLimit<0||value.inlineAnswerLimit>200)return '原位解释字数上限应为 0–200（0 表示不限）。';
 if(value.automaticCurrentPage!==undefined&&typeof value.automaticCurrentPage!=='boolean')return '自动带入当前页设置无效。';
 if(value.inlineFullDocument!==undefined&&typeof value.inlineFullDocument!=='boolean')return '完整文档检索设置无效。';
 return '';
}
export function contextPages(page:number,selected:number[],regionPage:number|undefined,total:number,settings:ContextSettings):number[]{
 const fallback=regionPage!==undefined?[regionPage]:settings.automaticCurrentPage!==false?[page]:[];
 const anchors=selected.length?[...new Set(selected)]:fallback,pages=[...new Set([...selected,...(regionPage!==undefined?[regionPage]:selected.length?[]:fallback)])];
 for(const n of anchors)for(let p=Math.max(1,n-settings.beforePages);p<=Math.min(total,n+settings.afterPages);p++)if(!pages.includes(p))pages.push(p);
 return pages;
}
export function defaultInputs():InputSettings{return {schemaVersion:1,bindings:Object.fromEntries(Object.keys(actions).map((action,i)=>[action,`Ctrl+Shift+${i+1}`])) as InputSettings['bindings']};}
export function inputError(value:InputSettings):string {
 const keys=new Set<string>();if(value.schemaVersion!==1||Object.keys(value.bindings).length!==8)return '快捷键版本或操作无效。';
 for(const action of Object.keys(actions) as Action[]){const key=value.bindings[action];if(key===undefined)return '缺少快捷操作。';if(key){if(!/^Ctrl\+Shift\+[1-9]$/.test(key))return '本阶段支持 Ctrl+Shift+1–9 或留空；保留常见系统、浏览器及PDF快捷键。';if(keys.has(key))return '快捷键重复，请为每项选择不同组合。';keys.add(key);}}return '';
}
export function inputAction(event:KeyboardEvent|{key:string;code?:string;ctrlKey:boolean;shiftKey:boolean;altKey:boolean;metaKey:boolean;isComposing:boolean;repeat?:boolean;keyCode?:number},settings:InputSettings,editable:boolean):Action|null {
 if(editable||event.isComposing||event.keyCode===229||event.repeat||!event.ctrlKey||!event.shiftKey||event.altKey||event.metaKey)return null;
 const digit=event.code?.match(/^Digit([1-9])$/)?.[1]??event.key;return (Object.entries(settings.bindings).find(([,key])=>key===`Ctrl+Shift+${digit}`)?.[0] as Action)??null;
}
const normalizeStyleSearch=(value:string)=>value.normalize('NFKC').trim().toLocaleLowerCase();
export function latestStyles(styles:Styles,query=''):Style[]{const latest=new Map<string,Style>();for(const s of styles.revisions){if((latest.get(s.id)?.version??0)<s.version)latest.set(s.id,s);}const q=normalizeStyleSearch(query),recent=styles.recent??[];return [...latest.values()].filter(s=>!q||[s.name,...s.aliases].some(v=>normalizeStyleSearch(v).includes(q))).sort((a,b)=>{const ai=recent.indexOf(a.id),bi=recent.indexOf(b.id);return (ai<0?Infinity:ai)-(bi<0?Infinity:bi)||Number(b.enabled)-Number(a.enabled);});}
export const preferences={get:()=>invoke<{styles:Styles;inputs:InputSettings;context:ContextSettings}>('m6_preferences'),saveStyle:(input:Omit<Style,'version'|'id'> &{id:string|null})=>invoke<Styles>('m6_edit_style',{input}),select:(id:string|null)=>invoke<Styles>('m6_select_style',{id}),deleteStyleVersion:(id:string,version:number)=>invoke<Styles>('m6_delete_style_version',{id,version}),importStyles:()=>invoke<Styles|null>('m6_import_styles'),saveInputs:(inputs:InputSettings)=>invoke<void>('m6_set_inputs',{inputs}),saveContext:(context:ContextSettings)=>invoke<void>('m6_set_context',{context})};
