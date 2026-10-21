import type { Region } from './ai-context';
export class InlineSession {
 phase:'idle'|'preparing'|'ready'|'confirming'|'confirmed'|'cancelled'='idle';text='';generation=0;
 begin(){this.phase='preparing';this.text='';return ++this.generation;}
 complete(generation:number,text:string){if(this.generation!==generation||this.phase!=='preparing')return false;this.text=text;this.phase='ready';return true;}
 cancel(){if(this.phase==='confirming')return false;++this.generation;this.text='';this.phase='cancelled';return true;}
 confirmation(){if(this.phase!=='ready'||!this.text.trim())throw Error('临时回答尚未就绪。');this.phase='confirming';return this.text;}
 confirmed(){if(this.phase!=='confirming')throw Error('确认状态无效。');this.phase='confirmed';this.text='';}
 failed(){if(this.phase==='confirming')this.phase='ready';else if(this.phase==='preparing')this.phase='idle';}
}
export function answerBox(view:number[],source?:Region|null):Region['rect'] {
 const [x,y,right,top]=view;if(view.length!==4||!view.every(Number.isFinite)||right<=x||top<=y)throw Error('页面范围无效。');
 const marginX=Math.min(6,(right-x)/4),marginY=Math.min(6,(top-y)/4),width=Math.min(130,right-x-2*marginX),height=Math.min(65,top-y-2*marginY);
 const left=Math.max(x+marginX,Math.min(right-width-marginX,source?.rect[0]??x+24));
 const bottom=Math.max(y+marginY,Math.min(top-height-marginY,source?source.rect[1]-height-10:top-height-24));
 return [left,bottom,left+width,bottom+height];
}
export function textCapacity(rect:Region['rect'],fontSize=12):number {return Math.max(40,Math.min(280,Math.floor((rect[2]-rect[0]-4)/fontSize)*Math.floor((rect[3]-rect[1]-4)/(fontSize*1.35))));}
export function boundedPanel(left:number,top:number,width:number,height:number,bounds:{left:number;top:number;right:number;bottom:number}){return {left:Math.max(bounds.left,Math.min(bounds.right-width,left)),top:Math.max(bounds.top,Math.min(bounds.bottom-height,top))};}
export function dragBox(rect:Region['rect'],v:{convertToViewportPoint(x:number,y:number):number[];convertToPdfPoint(x:number,y:number):number[]},dx:number,dy:number,resize=false):Region['rect']{
 const r=[...v.convertToViewportPoint(rect[0],rect[1]),...v.convertToViewportPoint(rect[2],rect[3])],x=Math.min(r[0],r[2]),y=Math.min(r[1],r[3]),right=Math.max(r[0],r[2]),bottom=Math.max(r[1],r[3]);
 const a=v.convertToPdfPoint(x+(resize?0:dx),y+(resize?0:dy)),b=v.convertToPdfPoint(resize?Math.max(x+40,right+dx):right+dx,resize?Math.max(y+40,bottom+dy):bottom+dy);
 return [Math.min(a[0],b[0]),Math.min(a[1],b[1]),Math.max(a[0],b[0]),Math.max(a[1],b[1])];
}
