<script lang="ts">
  import type { PdfReader } from '../lib/pdf-reader';
  import type { TextSelection } from '../lib/freetext-control';
  import Icon from './Icon.svelte';
  import {nearEditor} from '../lib/annotation-toolbar';
  let {reader,selected,container}:{reader:PdfReader;selected:TextSelection;container:HTMLElement}=$props();
  let error=$state('');
  function action(run:()=>void){try{run();error='';}catch(e){error=e instanceof Error?e.message:'操作未完成。';}}
</script>
<div class="text-edit-bar" use:nearEditor={{container,id:selected.id}} role="toolbar" tabindex="-1" aria-label="选中文字批注工具" onpointerdown={e=>e.stopPropagation()}>
  <button title="编辑文字（也可双击文字框）" aria-label="编辑批注文字" onpointerdown={e=>e.preventDefault()} onclick={()=>action(()=>reader.textEdit())}><Icon name="pen"/></button>
  <label title="选中文字的字号">字号<input aria-label="文字批注字号" type="number" min="8" max="72" value={selected.fontSize} oninput={e=>{const n=Number(e.currentTarget.value);if(n>=8&&n<=72)action(()=>reader.textSize(n));}}/></label>
  <input class="text-color" title="选中文字的颜色" aria-label="文字批注颜色" type="color" value={selected.color} oninput={e=>action(()=>reader.textColor(e.currentTarget.value))}/>
  <span class="divider"></span>
  <button title="撤销" aria-label="撤销批注修改" onpointerdown={e=>e.preventDefault()} onclick={()=>action(()=>reader.edit('undo'))}><Icon name="undo"/></button>
  <button title="重做" aria-label="重做批注修改" onpointerdown={e=>e.preventDefault()} onclick={()=>action(()=>reader.edit('redo'))}><Icon name="redo"/></button>
  <button title="删除选中批注" aria-label="删除文字批注" onpointerdown={e=>e.preventDefault()} onclick={()=>action(()=>reader.edit('delete'))}><Icon name="close"/></button>
  {#if !selected.canResize}<small>已有PDF调框待支持</small>{/if}
  {#if selected.overflow}<span class="warning" role="status">页面剩余空间不足，请移动、调宽、减小字号或减少文字</span>{/if}
  {#if error}<span class="warning" role="alert">{error}</span>{/if}
</div>
<style>
  .text-edit-bar{position:fixed;z-index:24;display:flex;align-items:center;flex-wrap:wrap;gap:4px;width:max-content;max-width:calc(100% - 28px);background:#fff;color:#42576a;border:1px solid #cbd6df;border-radius:6px;padding:4px 6px;box-shadow:0 3px 12px #24374b24;font:11px "Segoe UI","Microsoft YaHei",sans-serif;}
  button{display:inline-flex;align-items:center;justify-content:center;width:26px;height:26px;border:0;background:transparent;padding:4px;border-radius:4px;color:#43617b;}
  button :global(svg){width:16px;height:16px;}button:hover{background:#e8eff5;}button:active{background:#d9e5ee;}
  label{display:flex;align-items:center;gap:4px;}input[type=number]{width:47px;height:25px;padding:3px;border:1px solid #d5dce3;border-radius:3px;font:inherit;}.text-color{width:26px;height:25px;padding:2px;border:1px solid #d5dce3;background:#fff;border-radius:3px;}.divider{height:17px;width:1px;background:#dce3e9;}small{font-size:10px;color:#718190;}.warning{color:#97551c;font-size:10px;}
</style>
