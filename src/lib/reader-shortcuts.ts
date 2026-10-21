export function pdfShortcut(event:{key:string;ctrlKey:boolean;altKey:boolean;shiftKey:boolean;metaKey:boolean;isComposing:boolean;keyCode?:number},selection:boolean,input:boolean,modal=false):'search'|'delete'|'protect'|null{
 const key=event.key.toLowerCase();
 if(modal)return ['delete','backspace','escape'].includes(key)||event.ctrlKey&&['z','y','a'].includes(key)?'protect':null;
 if(event.isComposing||event.keyCode===229)return ['delete','backspace'].includes(key)&&selection?'protect':null;
 if(event.ctrlKey&&!event.altKey&&!event.shiftKey&&!event.metaKey&&key==='f')return 'search';
 if(!['delete','backspace'].includes(key)||!selection)return null;
 if(input)return 'protect';
 return !event.ctrlKey&&!event.altKey&&!event.shiftKey&&!event.metaKey?'delete':null;
}
