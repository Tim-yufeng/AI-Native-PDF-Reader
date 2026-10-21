<script lang="ts">
 import {onMount} from 'svelte';
 import ModelSettings from './ModelSettings.svelte';import PreferenceSettings from './PreferenceSettings.svelte';
 import {preferences,defaultContext,contextError,type ContextSettings} from '../lib/preferences';
 let {initial='context',close,changed,directory,directoryEnabled=false,directoryBusy=false,hasDocument=false}:{initial?:'models'|'styles'|'keys'|'context';close:()=>void;changed:()=>void;directory:()=>void;directoryEnabled?:boolean;directoryBusy?:boolean;hasDocument?:boolean}=$props();
 let dialog:HTMLDialogElement,tab=$state<'models'|'styles'|'keys'|'context'>('context'),context=$state<ContextSettings>(defaultContext()),error=$state(''),status=$state(''),busy=$state(false);
 onMount(()=>{tab=initial;dialog.showModal();void preferences.get().then(p=>context={...defaultContext(),...p.context}).catch(()=>error='本地设置不可用，请使用隔离数据目录。');});
 async function save(){const problem=contextError(context);if(problem){error=problem;return;}busy=true;error='';try{await preferences.saveContext({...context});status='上下文设置已保存，下次准备来源时生效。';window.dispatchEvent(new Event('reader-preferences-changed'));changed();}catch{error='未能保存，原设置保留。';}finally{busy=false;}}
</script>
<dialog bind:this={dialog} class="reader-settings" oncancel={close} aria-label="阅读器设置">
 <header><strong>设置</strong><button class="close" aria-label="关闭设置" title="关闭设置" onclick={close}>×</button></header>
 <nav aria-label="设置分类">{#each [['models','模型'],['styles','风格'],['context','上下文'],['keys','快捷键']] as [id,label]}<button class:active={tab===id} aria-pressed={tab===id} onclick={()=>tab=id as typeof tab}>{label}</button>{/each}</nav>
 <div class="settings-content">
 {#if tab==='models'}<ModelSettings {close} {changed}/>
 {:else if tab==='styles'||tab==='keys'}<PreferenceSettings mode={tab} {close} {changed}/>
 {:else}<section class="context-settings"><h3>上下文补充</h3><p>当前 PDF 默认支持整份本地检索，仅发送与本轮问题相关的来源，不上传整份文件。手选页和区域优先，旧截图不会自动累积。</p>
 <fieldset disabled={busy||directoryBusy}><label>自动补充前面页数<input aria-label="前面邻近页数" type="number" min="0" max="5" step="1" bind:value={context.beforePages}/></label><label>自动补充后面页数<input aria-label="后面邻近页数" type="number" min="0" max="5" step="1" bind:value={context.afterPages}/></label><label>原位解释字数上限<input aria-label="原位解释字数上限" type="number" min="0" max="200" step="1" bind:value={context.inlineAnswerLimit}/></label></fieldset><small>默认前后均为0；围绕本轮手选页或区域补充，范围0–5。超出预算会提示，不静默丢页。原位解释字数上限默认20，范围0–200，0 表示不限；它是软上限，确实需要完整条件或公式时仍可适度超出，超出后框内会提示。</small>
 <label class="check"><input type="checkbox" aria-label="自动带入当前页" disabled={busy||directoryBusy} bind:checked={context.automaticCurrentPage}/>未选页面或区域时，自动带入当前页</label><p>默认开启。关闭后仍可手选页面、框选区域或使用全文检索补充；检索也可能找到当前页。仅在你发送问题时使用。</p>
 <label class="check"><input type="checkbox" aria-label="原位解释检索完整 PDF" disabled={busy||directoryBusy} bind:checked={context.inlineFullDocument}/>原位解释检索完整 PDF</label><p>默认开启。发送前逐页建立当前 PDF 的本地文字索引，补充所有检索命中的相关页；超过本轮容量会明确提示，不静默省略。没有可提取文字的页会单独提示，可选页或框选补充图像。此选项不授权读取其他 PDF，也不表示每轮上传整份文件。</p>
 <button disabled={busy||directoryBusy} onclick={()=>void save()}>{busy?'保存中…':'保存上下文设置'}</button>
 <hr/><h3>同目录 PDF</h3><p>默认无授权；仅主动允许后访问当前文件所在目录中直接包含的 PDF。撤销后不再使用该范围的缓存。</p><button disabled={!hasDocument||directoryBusy&&!directoryEnabled} onclick={directory}>{directoryEnabled?'撤销同目录授权':'允许同目录 PDF'}</button>
 {#if status}<p role="status">{status}</p>{/if}{#if error}<p class="error" role="alert">{error}</p>{/if}</section>{/if}
 </div>
</dialog>
<style>
 .check input{width:auto;}
 .reader-settings{width:min(540px,calc(100vw - 28px));height:min(650px,calc(100vh - 40px));padding:0;border:1px solid #cbd6df;border-radius:8px;color:#33485b;box-shadow:0 12px 50px #24374b30;}.reader-settings::backdrop{background:#20314235;}.reader-settings[open]{display:flex;flex-direction:column;}header{height:46px;display:flex;align-items:center;justify-content:space-between;padding:0 16px;border-bottom:1px solid #e0e6ec;}button{padding:5px 9px;border:1px solid #cdd7df;background:#fff;border-radius:4px;color:#425f78;font:inherit;font-size:12px;}button:hover:not(:disabled){background:#edf2f7;}button:disabled{opacity:.5;}.close{border:0;font-size:20px;padding:0 7px;}nav{display:flex;gap:5px;padding:10px 14px;border-bottom:1px solid #e8edf2;}nav button{border:0;}nav .active{background:#e6eef6;color:#254d71;}.settings-content{overflow:auto;min-height:0;flex:1;}.context-settings{padding:4px 16px 16px;font-size:12px;}h3{font-size:13px;margin:15px 0 8px;}p,small{line-height:1.7;color:#6b7f90;}small{display:block;margin-bottom:12px;}fieldset{display:flex;flex-wrap:wrap;gap:20px;padding:0;border:0;margin:15px 0;}label{display:flex;align-items:center;gap:7px;}input{width:53px;padding:5px;border:1px solid #ccd6df;border-radius:3px;}hr{margin:20px 0;border:0;border-top:1px solid #e5eaf0;}.error{color:#a4483f;}
</style>



