<script lang="ts">
 import {onMount,onDestroy} from 'svelte';
 import {folderWorkspace,type FolderWorkspace,type FolderEntry,type FolderListing} from '../lib/folder-workspace';
 let {folder,open,close,busy=false}:{folder:FolderWorkspace;open:(node:FolderEntry)=>void;close:()=>void;busy?:boolean}=$props();
 let children=$state<Record<string,FolderListing>>({}),expanded=$state<Record<string,boolean>>({}),loading=$state<Record<string,boolean>>({}),error=$state('');
 let alive=true;
 onDestroy(()=>{alive=false;});
 async function load(id:string){if(loading[id])return;loading[id]=true;error='';try{const value=await folderWorkspace.list(folder.id,id);if(alive)children[id]=value;}catch{if(alive){error='文件夹未能完整读取，可能已改变、包含链接或超过容量。可重试，或重新打开文件夹。';expanded[id]=false;}}finally{if(alive)loading[id]=false;}}
 async function toggle(entry:FolderEntry){if(!entry.directory){open(entry);return;}expanded[entry.id]=!expanded[entry.id];if(expanded[entry.id]&&!children[entry.id])await load(entry.id);}
 onMount(()=>{void load(folder.id);});
</script>
{#snippet rows(items:FolderEntry[],depth:number)}
 {#each items as entry(entry.id)}
  <button class="entry" style:padding-left={`${10+depth*14}px`} disabled={busy||loading[entry.id]} aria-expanded={entry.directory?!!expanded[entry.id]:undefined} title={entry.name} onclick={()=>void toggle(entry)}><span aria-hidden="true">{entry.directory?(expanded[entry.id]?'▾':'▸'):'▤'}</span><span>{entry.name}</span></button>
  {#if entry.directory&&expanded[entry.id]}{#if loading[entry.id]}<small>正在读取…</small>{:else if children[entry.id]}{@render rows(children[entry.id].entries,depth+1)}{#if children[entry.id].skippedLinks}<small>跳过 {children[entry.id].skippedLinks} 个链接</small>{/if}{#if !children[entry.id].entries.length}<small>没有 PDF 或子文件夹</small>{/if}{/if}{/if}
 {/each}
{/snippet}
<div class="folder-tree" aria-label="工作区文件">
 <header><strong title={folder.name}>{folder.name}</strong><button aria-label="刷新文件树" disabled={busy||loading[folder.id]} onclick={()=>{children={};expanded={};void load(folder.id);}}>↻</button><button aria-label="关闭工作区" disabled={busy} onclick={close}>×</button></header>
 <small>仅浏览文件；点击 PDF 才打开。AI 跨文档读取须另行授权。</small>
 <div class="tree-scroll">{#if loading[folder.id]}<p role="status">正在读取文件夹…</p>{:else if children[folder.id]}{@render rows(children[folder.id].entries,0)}{#if children[folder.id].skippedLinks}<small>跳过 {children[folder.id].skippedLinks} 个链接</small>{/if}{#if !children[folder.id].entries.length}<p>没有 PDF 或子文件夹</p>{/if}{/if}{#if error}<p class="error" role="alert">{error}</p><button onclick={()=>void load(folder.id)}>重试</button>{/if}</div>
</div>
<style>
 .folder-tree{display:flex;flex-direction:column;min-height:0;height:100%;font-size:12px;color:#435c70;background:#f6f8fa;}header{display:flex;gap:3px;padding:8px;align-items:center;border-bottom:1px solid #e1e7ec;}strong{flex:1;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;}button{font:inherit;color:inherit;border:0;background:transparent;cursor:pointer;padding:5px;}button:hover:not(:disabled){background:#e5edf4;}button:disabled{opacity:.5;}button:focus-visible{outline:2px solid #7599b6;outline-offset:-2px;}small{display:block;font-size:10px;line-height:1.5;padding:6px 10px;color:#6c8191;}.tree-scroll{overflow:auto;min-height:0;flex:1;}.entry{display:flex;align-items:center;text-align:left;width:100%;gap:6px;white-space:nowrap;}.entry span:last-child{overflow:hidden;text-overflow:ellipsis;}p{padding:8px;line-height:1.6;}.error{color:#a4493e;}
</style>
