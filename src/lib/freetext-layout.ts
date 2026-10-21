import { WriterRefusal } from './writer-refusal.ts';

// Use the same static Regular outlines in the editor and embedded PDF font.
// CSS weight alone cannot change MuPDF's default variable-font instance (100).
export const freeTextStyle = {
  fontFamily: 'ReaderFreeText', fontWeight: '400', fontStyle: 'normal',
  fontVariationSettings: 'normal', fontKerning: 'none', fontVariantLigatures: 'none',
  // Preserve spaces, but wrap them inside the box instead of letting a trailing
  // space hang past its edge. Writer and editor use this exact same layout.
  lineHeight: '1.35', whiteSpace: 'break-spaces', overflowWrap: 'anywhere',
  wordBreak: 'normal', hyphens: 'none', textAlign: 'left', direction: 'ltr',
} as const;
export interface TextLine { text:string; x:number; baseline:number; }
export type FreeTextLayouter = (text:string,width:number,fontSize:number) => TextLine[];
let fontBytes:Promise<Uint8Array>|null=null;
let fontFace:Promise<void>|null=null;
let fontReady=false;

export function loadFreeTextFont():Promise<Uint8Array> {
  if(!fontBytes)fontBytes=fetch('/noto/NotoSansSC-Regular-v41.ttf').then(async response=>{
    if(!response.ok)throw new WriterRefusal('writer_font_fetch_failed','resources',undefined,response.status);
    return new Uint8Array(await response.arrayBuffer());
  }).catch(error=>{fontBytes=null;throw error;});
  return fontBytes;
}
export function prepareFreeTextFont():Promise<void> {
  if(!fontFace)fontFace=loadFreeTextFont().then(async bytes=>{
    const face=new FontFace('ReaderFreeText',bytes.slice().buffer,{weight:freeTextStyle.fontWeight,style:'normal'});
    await face.load();document.fonts.add(face);fontReady=true;
  }).catch(cause=>{fontFace=null;throw cause instanceof WriterRefusal?cause:new WriterRefusal('writer_font_load_failed','font',cause);});
  return fontFace;
}

// PDF.js uses NBSP on deserialization, but serializes it as a normal space.
// With pre-wrap we can keep real spaces in both editing and saved layout.
export function normalizeFreeTextSpaces(content:HTMLElement) {
  const selection=document.getSelection();
  const saved=selection?.anchorNode&&selection.focusNode&&content.contains(selection.anchorNode)&&content.contains(selection.focusNode)
    ? {anchor:selection.anchorNode,anchorOffset:selection.anchorOffset,focus:selection.focusNode,focusOffset:selection.focusOffset}:null;
  const walker=document.createTreeWalker(content,NodeFilter.SHOW_TEXT);
  let node:Node|null,changed=false;
  while((node=walker.nextNode()))if(node.nodeValue?.includes('\xa0')){node.nodeValue=node.nodeValue.replaceAll('\xa0',' ');changed=true;}
  // Replacing text data resets DOM selection offsets. The one-for-one space
  // normalization must not move the caret after native paste/undo or input.
  if(changed&&saved)selection!.setBaseAndExtent(saved.anchor,saved.anchorOffset,saved.focus,saved.focusOffset);
}

/** Same plain paragraphs PDF.js commits, including when its editor is hidden
 * for read preview. innerText loses block separators on a hidden element. */
export function freeTextContents(content:HTMLElement):string {
  const lines:string[]=[];let previousText=false,adjacentText=false;
  for(const node of content.childNodes){
    if(node.nodeType===Node.TEXT_NODE){const text=(node.textContent??'').replace(/[\r\n]/g,'');if(adjacentText)lines[lines.length-1]+=text;else lines.push(text);previousText=true;adjacentText=true;}
    else {adjacentText=false;if(!(previousText&&node.nodeName==='BR')){lines.push((node.textContent??'').replace(/[\r\n]/g,''));previousText=false;}}
  }
  return lines.join('\n').replaceAll('\xa0',' ');
}

/** Measure plain text using the same browser layout as the contentEditable.
 * Both run at canonical PDF-point sizes; only the editor's outer transform
 * follows zoom. No HTML from the document is inserted or executed. */
export function measureFreeTextBox(text:string,width:number,fontSize:number):{lines:TextLine[];height:number;width:number} {
  if(!fontReady)throw new WriterRefusal('writer_font_load_failed','font');
  const box=document.createElement('div');
  Object.assign(box.style,freeTextStyle,{position:'fixed',left:'0',top:'0',visibility:'hidden',
    pointerEvents:'none',width:`${width}px`,fontSize:`${fontSize}px`,padding:'0',border:'0',margin:'0'});
  const lines:TextLine[]=[];
  let contentWidth=0;
  document.body.append(box);
  try {
    for(const paragraph of text.split('\n')) {
      const div=document.createElement('div'),anchor=document.createElement('span');
      Object.assign(anchor.style,{display:'inline-block',width:'0',height:'0',padding:'0',margin:'0'});
      div.append(anchor);
      const node=document.createTextNode(paragraph);div.append(node);
      if(!paragraph)div.append(document.createElement('br'));
      box.append(div);
      const origin=box.getBoundingClientRect(),firstBaseline=anchor.getBoundingClientRect().top-origin.top;
      let firstTop:number|undefined,lastTop:number|undefined,line:TextLine|undefined;
      const range=document.createRange();
      for(let offset=0;offset<paragraph.length;) {
        const char=String.fromCodePoint(paragraph.codePointAt(offset)!);
        range.setStart(node,offset);offset+=char.length;range.setEnd(node,offset);
        const rect=range.getBoundingClientRect();firstTop??=rect.top;contentWidth=Math.max(contentWidth,rect.right-origin.left);
        if(lastTop===undefined||Math.abs(rect.top-lastTop)>0.5) {
          line={text:'',x:rect.left-origin.left,baseline:firstBaseline+rect.top-firstTop};
          lines.push(line);lastTop=rect.top;
        }
        line!.text+=char;
      }
    }
    return {lines,height:Math.max(box.getBoundingClientRect().height,...lines.map(line=>line.baseline+fontSize*.2+.1)),width:contentWidth};
  } finally {box.remove();}
}
export const measureFreeText:FreeTextLayouter=(text,width,fontSize)=>measureFreeTextBox(text,width,fontSize).lines;
