<script lang="ts">
 import {onDestroy} from 'svelte';import {pageImage,type PageReader,type Region} from '../lib/ai-context';import type {AttachedImage} from '../lib/ai-attachment';import Icon from './Icon.svelte';
 let {reader,token,name,pages,pinned,region,disabled=false,removePage,removePinned,removeRegion,images=[],changeImages}:{reader:PageReader|null;token:number;name:string;pages:number[];pinned:number[];region:Region|null;disabled?:boolean;removePage:(page:number)=>void;removePinned:(page:number)=>void;removeRegion:()=>void;images?:AttachedImage[];changeImages?:(next:AttachedImage[])=>void}=$props();
 let dialog=$state<HTMLDialogElement>(),image=$state(''),error=$state(''),loading=$state(false),label=$state(''),previewing=$state(false);let abort:AbortController|null=null,intent=0,alive=true;
 function close(){++intent;abort?.abort();dialog?.close();image='';loading=false;error='';previewing=false;}
 $effect(()=>{void token;void reader;void name;void region;void pages.join(',');close();});onDestroy(()=>{alive=false;close();});
 async function preview(page:number,rect?:Region['rect']){if(!reader||disabled)return;close();const version=++intent,current=reader;abort=new AbortController();const signal=abort.signal;loading=true;label=`${name} · 第${page}页${rect?'区域':''}`;dialog?.showModal();try{const png=await pageImage(current,{id:'preview',grantId:'',name,page,text:'',image:true,rect},signal);if(alive&&!signal.aborted&&version===intent)image=png;}catch(e){if(!signal.aborted&&version===intent)error=e instanceof Error?e.message:'预览暂不可用';}finally{if(version===intent)loading=false;}}
 // An attached image is already a bounded PNG data URL, so it previews directly
 // and never touches the PDF renderer.
 function previewAttached(item:AttachedImage){close();previewing=true;label=item.name;image=item.png;dialog?.showModal();}
</script>
<div class="attachment-chips" aria-label="本轮上下文附件">
 {#if region}<span class="attachment-chip" title={`${name} · 第${region.page}页区域，点击预览`}><button class="chip-name" disabled={disabled} aria-label={`预览区域 ${region.page}页`} onclick={()=>void preview(region!.page,region!.rect)}><Icon name="image"/><span>区域 · {region.page}页</span></button><button class="chip-remove" title="移除此区域" aria-label="移除区域附件" disabled={disabled} onclick={removeRegion}><Icon name="close"/></button></span>{/if}
 {#each pages as page}<span class="attachment-chip" title={`${name} · 第${page}页整页，点击预览`}><button class="chip-name" disabled={disabled} aria-label={`预览整页 ${page}页`} onclick={()=>void preview(page)}><Icon name="file"/><span>{name} · {page}页</span></button><button class="chip-remove" title={`移除第${page}页`} aria-label={`移除整页附件 ${page}页`} disabled={disabled} onclick={()=>removePage(page)}><Icon name="close"/></button></span>{/each}
 {#each pinned.filter(n=>!pages.includes(n)) as page}<span class="attachment-chip pinned" title={`${name} · 固定第${page}页`}><button class="chip-name" disabled={disabled} onclick={()=>void preview(page)}><Icon name="file"/><span>固定 · {page}页</span></button><button class="chip-remove" title="取消固定页面" aria-label={`移除固定附件 ${page}页`} disabled={disabled} onclick={()=>removePinned(page)}><Icon name="close"/></button></span>{/each}
 {#each images as item (item.id)}<span class="attachment-chip attached" title={`${item.name} · ${item.width}×${item.height}，点击预览`}><button class="chip-name" disabled={disabled} aria-label={`预览附加图片 ${item.name}`} onclick={()=>previewAttached(item)}><Icon name="image"/><span>{item.name}</span></button><button class="chip-remove" title="移除这张图片" aria-label={`移除附加图片 ${item.name}`} disabled={disabled} onclick={()=>changeImages?.(images.filter(other=>other.id!==item.id))}><Icon name="close"/></button></span>{/each}
 {#if error}<span class="attach-error" role="alert">{error}</span>{/if}
</div>
<dialog bind:this={dialog} class="attachment-preview" aria-label="附件预览" oncancel={close}><header><span>{label}</span><button title="关闭预览" aria-label="关闭附件预览" onclick={close}><Icon name="close"/></button></header>{#if loading}<p role="status">正在生成本机预览…</p>{:else if error}<p role="alert">{error}</p>{:else if image}<img class:raw={previewing} src={image} alt={label}/>{/if}</dialog>
<style>
 .attachment-chips{display:flex;gap:5px;flex-wrap:wrap;padding:6px 7px 0;overflow:auto;max-height:45%;min-height:0;flex:0 1 auto;}
 .attachment-chips:empty{display:none;}
 .attachment-chip{display:inline-flex;align-items:center;max-width:100%;background:#e9e9f6;border:1px solid #d5d5ea;border-radius:5px;height:26px;flex-shrink:0;color:#4a4a63;}
 .attached{background:#e6e6f4;border-color:#cfcfe6;}
 .pinned{background:#f5f1e7;border-color:#e7dcc7;}
 .chip-name{display:flex;align-items:center;gap:5px;max-width:min(220px,45vw);min-width:0;}
 .chip-name span{overflow:hidden;text-overflow:ellipsis;white-space:nowrap;}
 .chip-name :global(svg){width:14px;height:14px;flex:none;opacity:.75;}
 .chip-remove{flex:none;}
 .chip-remove :global(svg){width:12px;height:12px;opacity:.7;}
 button{border:0;background:transparent;color:inherit;padding:3px 6px;font:12px 'Segoe UI','Microsoft YaHei',sans-serif;cursor:pointer;border-radius:3px;}
 button:hover:not(:disabled){background:#dcdcef;}
 button:disabled{opacity:.45;cursor:default;}
 .attach-error{flex:1 1 100%;color:#a13e38;font-size:10px;}
 .attachment-preview{max-width:min(760px,90vw);max-height:85vh;border:1px solid #cbd6df;border-radius:6px;padding:10px;color:#41566a;background:#fff;}
 .attachment-preview::backdrop{background:#20314235;}
 header{display:flex;gap:12px;justify-content:space-between;align-items:center;font-size:12px;margin-bottom:8px;}
 img{display:block;max-width:100%;max-height:70vh;object-fit:contain;}
 img.raw{image-rendering:auto;border:1px solid #e2e8ee;border-radius:3px;}
 p{font-size:12px;}
</style>
