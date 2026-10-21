export interface Layout { left: number; right: number; aiOpen: boolean; }
export const defaultLayout = (): Layout => ({ left:164,right:350,aiOpen:false });
export function validLayout(value:unknown):Layout {
 const v=value as Partial<Layout>|null;
 return v&&Number.isFinite(v.left)&&Number.isFinite(v.right)&&typeof v.aiOpen==='boolean'?{left:Math.max(154,Math.min(280,v.left!)),right:Math.max(280,Math.min(560,v.right!)),aiOpen:v.aiOpen}:defaultLayout();
}
export function resizedLayout(layout:Layout,side:'left'|'right',value:number,width:number,otherOpen:boolean):Layout {
 const other=otherOpen?(side==='left'?layout.right:layout.left):0;
 const min=side==='left'?154:280,max=Math.min(side==='left'?280:560,Math.max(min,width-other-370));
 return {...layout,[side]:Math.max(min,Math.min(max,Number.isFinite(value)?value:layout[side]))};
}
