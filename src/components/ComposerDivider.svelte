<script lang="ts">
 import {onDestroy} from 'svelte';
 let {value,change,reset}:{value:number;change:(n:number)=>void;reset:()=>void}=$props();
 let abort:AbortController|null=null,restore:(()=>void)|null=null;
 function down(e:PointerEvent){if(e.button!==0)return;e.preventDefault();restore?.();abort=new AbortController();const id=e.pointerId,start=e.clientY,initial=value,previousCursor=document.body.style.cursor,previousSelection=document.body.style.userSelect;
 document.body.style.cursor='row-resize';document.body.style.userSelect='none';
 const finish=()=>{abort?.abort();document.body.style.cursor=previousCursor;document.body.style.userSelect=previousSelection;restore=null;};restore=finish;
 window.addEventListener('pointermove',event=>{if(event.pointerId===id)change(initial+start-event.clientY);},{signal:abort.signal});
 window.addEventListener('pointerup',event=>{if(event.pointerId===id)finish();},{signal:abort.signal});window.addEventListener('pointercancel',finish,{signal:abort.signal});window.addEventListener('blur',finish,{signal:abort.signal});
 }
 onDestroy(()=>restore?.());
</script>
<button class="composer-divider" aria-label="调整提问区高度" title="拖动调整提问区；双击恢复默认" onpointerdown={down} ondblclick={reset} onkeydown={e=>{if(e.key==='ArrowUp'||e.key==='ArrowDown'){e.preventDefault();change(value+(e.key==='ArrowUp'?16:-16));}else if(e.key==='Home'){e.preventDefault();reset();}}}></button>
<style>.composer-divider{flex:0 0 6px;width:100%;height:6px;background:#f3f5f7;border:0;padding:0;cursor:row-resize;touch-action:none;}.composer-divider:hover,.composer-divider:focus-visible{background:#90a9be;outline:none;}</style>
