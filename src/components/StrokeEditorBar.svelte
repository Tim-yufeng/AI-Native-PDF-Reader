<script lang="ts">
 import type {PdfReader,StrokeSelection} from '../lib/pdf-reader';import {nearEditor} from '../lib/annotation-toolbar';import Icon from './Icon.svelte';
 let {reader,container,selected}:{reader:PdfReader;container:HTMLElement;selected:StrokeSelection}=$props();let error=$state('');
 function action(run:()=>void){try{run();error='';}catch(e){error=e instanceof Error?e.message:'操作未完成';}}
</script>
<div class="stroke-edit-bar" use:nearEditor={{container,id:selected.id}} role="toolbar" tabindex="-1" aria-label="选中笔画或高亮工具" onpointerdown={e=>e.stopPropagation()}>
 <input aria-label="笔画或高亮颜色" type="color" title="颜色" value={selected.color} oninput={e=>action(()=>reader.strokeStyle(selected.kind,e.currentTarget.value))}/>
 {#if selected.canThicken}<label>粗细<input aria-label="笔画或高亮粗细" type="number" min="1" max="20" step="1" value={selected.thickness} oninput={e=>{const n=Number(e.currentTarget.value);if(n>=1&&n<=20)action(()=>reader.strokeStyle(selected.kind,undefined,n));}}/></label>{/if}
 <button title="删除选中批注（Delete）" aria-label="删除笔画或高亮" onpointerdown={e=>e.preventDefault()} onclick={()=>reader.edit('delete')}><Icon name="close"/></button>{#if error}<small role="alert">{error}</small>{/if}
</div>
<style>.stroke-edit-bar{position:fixed;z-index:24;display:flex;align-items:center;gap:5px;background:white;border:1px solid #cbd6df;border-radius:6px;box-shadow:0 3px 12px #24374b24;padding:4px 6px;color:#42576a;font:11px 'Segoe UI',sans-serif;}label{display:flex;align-items:center;gap:4px;}input{height:25px;border:1px solid #d5dce3;border-radius:3px;padding:2px;}input[type=color]{width:26px;background:white;}input[type=number]{width:43px;}button{border:0;background:transparent;color:#456078;width:26px;height:26px;padding:4px;border-radius:4px;}button :global(svg){width:16px;height:16px;}button:hover{background:#e8eff5;}button:active{background:#d9e5ee;}small{color:#a04b42;}</style>
