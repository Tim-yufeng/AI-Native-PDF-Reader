export const defaultComposerHeight=220;
export function composerHeight(value:number,panelHeight:number):number{
 const maximum=Math.max(100,Math.min(480,panelHeight-150,panelHeight*.6));
 return Math.round(Math.max(Math.min(140,maximum),Math.min(maximum,Number.isFinite(value)?value:defaultComposerHeight)));
}
export function sessionRoot(records:{id:string;parent?:string|null;editedFrom?:string|null}[],id:string):string|null{
 const byId=new Map(records.map(r=>[r.id,r]));let current=byId.get(id);const seen=new Set<string>();
 while(current&&(current.parent||current.editedFrom)){if(seen.has(current.id))return null;seen.add(current.id);const parent=byId.get((current.parent??current.editedFrom)!);if(!parent)return null;current=parent;}
 return current?.id??null;
}
