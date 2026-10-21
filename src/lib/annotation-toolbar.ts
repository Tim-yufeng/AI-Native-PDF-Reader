/** A small UI anchor over the native editor, never a replacement PDF layer. */
export function nearEditor(node:HTMLElement,options:{container:HTMLElement;id:string}){
  let current=options,frame=0,alive=true,editor:HTMLElement|null=null;
  const size=new ResizeObserver(schedule);
  const changes=new MutationObserver(records=>{if(records.some(r=>r.type==='childList'||r.target instanceof Element&&r.target.matches('.page,.annotationEditorLayer,.freeTextEditor,.inkEditor,.highlightEditor')))schedule();});
  changes.observe(options.container,{subtree:true,childList:true,attributes:true,attributeFilter:['style','hidden','class']});size.observe(options.container);size.observe(node);
  function measure(){if(!alive)return;const next=document.getElementById(current.id);if(!next||!current.container.contains(next)){node.style.visibility='hidden';return;}if(next!==editor){if(editor)size.unobserve(editor);editor=next;size.observe(editor);}const r=editor.getBoundingClientRect(),clip=current.container.getBoundingClientRect(),w=node.offsetWidth,h=node.offsetHeight;
    node.style.visibility=r.bottom<clip.top||r.top>clip.bottom||r.right<clip.left||r.left>clip.right?'hidden':'visible';
    node.style.left=`${Math.max(clip.left+8,Math.min(r.left,clip.right-w-8))}px`;
    node.style.top=`${Math.max(clip.top+5,Math.min(r.top-h-16>=clip.top?r.top-h-16:r.bottom+16,clip.bottom-h-5))}px`;
  }
  function schedule(){if(!alive||frame)return;frame=requestAnimationFrame(()=>{frame=0;measure();});}
  window.addEventListener('scroll',schedule,true);window.addEventListener('resize',schedule);schedule();
  return{update(next:typeof options){current=next;schedule();},destroy(){alive=false;cancelAnimationFrame(frame);size.disconnect();changes.disconnect();window.removeEventListener('scroll',schedule,true);window.removeEventListener('resize',schedule);}};
}
