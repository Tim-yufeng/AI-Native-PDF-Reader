<script lang="ts">
  import { onDestroy } from 'svelte';
  let { label, value, change, reset, direction=1 }: {label:string;value:number;change:(n:number)=>void;reset:()=>void;direction?:number}=$props();
  let active=false, start=0, original=0, abort:AbortController|null=null;
  function down(event:PointerEvent){if(event.button!==0)return;event.preventDefault();abort?.abort();abort=new AbortController();active=true;start=event.clientX;original=value;
    const done=()=>{active=false;abort?.abort();document.body.style.cursor='';document.body.style.userSelect='';};
    document.body.style.cursor='col-resize';document.body.style.userSelect='none';
    window.addEventListener('pointermove',e=>change(original+(e.clientX-start)*direction),{signal:abort.signal});
    window.addEventListener('pointerup',done,{once:true,signal:abort.signal});window.addEventListener('pointercancel',done,{once:true,signal:abort.signal});
  }
  onDestroy(()=>{abort?.abort();if(active){document.body.style.cursor='';document.body.style.userSelect='';}});
</script>
<button class="divider" aria-label={label} title={`${label}；双击恢复默认`} onpointerdown={down} ondblclick={reset} onkeydown={e=>{if(e.key==='ArrowLeft'||e.key==='ArrowRight'){e.preventDefault();change(value+(e.key==='ArrowLeft'?-12:12)*direction);}else if(e.key==='Home'){e.preventDefault();reset();}}}></button>
<style>.divider{flex:0 0 5px;width:5px;padding:0;border:0;background:#f3f5f7;cursor:col-resize;touch-action:none;}.divider:hover,.divider:focus-visible{background:#7896af;outline:none;}@media(max-width:900px){.divider{display:none;}}</style>
