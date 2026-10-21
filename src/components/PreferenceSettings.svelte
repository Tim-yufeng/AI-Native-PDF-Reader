<script lang="ts">
 import { onMount } from 'svelte';import { preferences,latestStyles,defaultInputs,inputError,actions,type Styles,type Style,type InputSettings,type Action } from '../lib/preferences';import Icon from './Icon.svelte';
 let {mode,close,changed}:{mode:'styles'|'keys';close:()=>void;changed:()=>void}=$props();
 let styles=$state<Styles>({schemaVersion:1,selected:null,revisions:[]}),inputs=$state<InputSettings>(defaultInputs()),error=$state(''),status=$state(''),busy=$state(false),query=$state('');
 let id=$state<string|null>(null),name=$state(''),aliases=$state(''),guidelines=$state(''),avoid=$state(''),enabled=$state(true);
 const matches=$derived(latestStyles(styles,query));
 onMount(()=>{void preferences.get().then(p=>{styles=p.styles;inputs=p.inputs;}).catch(()=>error='本地设置不可用；请使用隔离数据目录。');});
 function edit(s?:Style){id=s?.id??null;name=s?.name??'';aliases=s?.aliases.join('、')??'';guidelines=s?.guidelines??'';avoid=s?.avoid??'';enabled=s?.enabled??true;status='';}
 async function save(){busy=true;error='';status='';try{if(mode==='styles'){const before=styles.revisions.length;const saved=await preferences.saveStyle({id,name:name.trim(),aliases:aliases.split(/[,，、\n]/).map(s=>s.trim()).filter(Boolean),guidelines,avoid,enabled});const created=saved.revisions.length>before;styles=saved;edit(saved.revisions.filter(s=>s.name===name.trim()).at(-1));status=created?'保存成功！已建立新版本，旧版本仍可在下方查看或删除。':'保存成功！内容与当前版本相同，未新建版本。';}else{const problem=inputError(inputs);if(problem)throw Error(problem);await preferences.saveInputs(inputs);status='保存成功！';}window.dispatchEvent(new Event('reader-preferences-changed'));changed();}catch(e){error=e instanceof Error?e.message:'设置未能保存，原记录保留。';}finally{busy=false;}}
 async function removeVersion(s:Style){if(busy||!confirm('删除 '+s.name+' 的 v'+s.version+'？'+String.fromCharCode(10)+'当前选择和历史回答不会改动；该版本的内容将不再保留。'))return;busy=true;error='';status='';try{const before=styles.revisions.length;styles=await preferences.deleteStyleVersion(s.id,s.version);status=before>styles.revisions.length?'已删除该版本。':'该版本未删除。';if(!styles.revisions.some(r=>r.id===id))edit(styles.revisions.at(-1));window.dispatchEvent(new Event('reader-preferences-changed'));changed();}catch(e){error=typeof e==='object'&&e&&'message'in e?String(e.message):'该版本未能删除，记录保留。';}finally{busy=false;}}
 async function importStyles(){busy=true;error='';status='';try{const imported=await preferences.importStyles();if(imported){styles=imported;status='已导入私有风格。默认选择保持不变，旧版本和历史保留。';window.dispatchEvent(new Event('reader-preferences-changed'));changed();}}catch(e){error=typeof e==='object'&&e&&'message'in e?String(e.message):'导入未完成，已有风格与历史保留。';}finally{busy=false;}}
</script>
<section class="preferences" class:keys={mode==='keys'}><header><strong>{mode==='styles'?'表达风格':'应用快捷键'}</strong><button onclick={close}>关闭设置</button></header>
{#if mode==='styles'}
 <input class="filter" aria-label="搜索表达风格及别名" placeholder="搜索名称或别名" bind:value={query}/>
 <div class="style-actions"><button disabled={busy} onclick={()=>edit()}>＋ 新建</button><button aria-label="导入私有风格" disabled={busy} onclick={()=>void importStyles()}>导入私有风格…</button></div>
 <div class="style-list">{#each matches as s}<button class:selected={id===s.id} onclick={()=>edit(s)} title={`${s.name}${s.aliases.length>1?' · '+s.aliases.slice(1).join(' · '):''}`}><span class="style-name">{s.name}</span><small>{styles.recent?.includes(s.id)?'最近 · ':''}v{s.version}{s.enabled?'':' · 停用'}</small></button>{/each}{#if !matches.length}<p>暂无匹配风格。可新建或导入仅含表达规则的 JSON。</p>{/if}</div>
 <fieldset disabled={busy}><label>名称<input bind:value={name} maxlength="60"/></label><label>别名（用顿号分隔）<input bind:value={aliases} maxlength="480"/></label><label>表达规范<textarea bind:value={guidelines} maxlength="2000" placeholder="只描述表达方式；准确性、数学条件和简洁性始终优先。"></textarea></label><label>避免表达<textarea bind:value={avoid} maxlength="1000"></textarea></label><label class="check"><input type="checkbox" bind:checked={enabled}/>启用此风格</label></fieldset>
 {#if id}{@const saved=styles.revisions.filter(s=>s.id===id)}{@const newest=saved.at(-1)?.version}
 <details class="versions"><summary>版本记录（{saved.length}）</summary>
  <div class="version-list">{#each saved as s}<span class="version-item" class:current={s.version===newest}><button class="version-pick" title={s.enabled?s.name:s.name+'（停用）'} onclick={()=>edit(s)}>v{s.version}</button><button class="version-remove" aria-label={'删除 v'+s.version+' 版本'} title="删除这个版本" disabled={busy||saved.length<2} onclick={()=>void removeVersion(s)}><Icon name="close"/></button></span>{/each}</div>
  <p>选择旧版后保存会建立新版本，不覆盖历史。删除某个版本不会改动已选中的风格或历史回答。</p>
 </details>{/if}
 <p>默认无风格。仅导入你选择的表达规则，不执行 SKILL；准确性、数学条件及简洁性优先。私有风格只保存在本机数据中，不随安装包分发。</p>
{:else}<p>仅应用获得焦点时生效；文字输入和中文组合输入中不触发。支持 Ctrl+Shift+1–9，留空可禁用，常见系统/PDF操作保留。</p><fieldset disabled={busy}>{#each Object.keys(actions) as key}<label>{actions[key as Action]}<input aria-label={`${actions[key as Action]}快捷键`} bind:value={inputs.bindings[key as Action]}/></label>{/each}</fieldset><button disabled={busy} onclick={()=>inputs=defaultInputs()}>恢复默认映射</button>{/if}
 <button class="save" disabled={busy||(mode==='styles'&&!name.trim())} onclick={()=>void save()}>{busy?'保存中…':'保存'}</button>{#if status}<p role="status">{status}</p>{/if}{#if error}<p class="error" role="alert">{error}</p>{/if}</section>
<style>
 .preferences{padding:12px;overflow:auto;font-size:12px;}
 header{display:flex;justify-content:space-between;align-items:center;}
 label{display:flex;flex-direction:column;gap:5px;margin:10px 0;}
 fieldset{border:0;padding:0;margin:0;}
 .check{flex-direction:row;}
 .keys label{display:grid;grid-template-columns:130px minmax(0,1fr);align-items:center;gap:12px;margin:12px 0;}
 @media(max-width:430px){.keys label{grid-template-columns:112px minmax(0,1fr);gap:6px;}}
 input,textarea{width:100%;padding:6px;border:1px solid #cfd8e1;border-radius:4px;font:inherit;}
 .check input{width:auto;}
 textarea{min-height:70px;resize:vertical;}
 button{padding:4px 7px;background:#fff;border:1px solid #d3dce4;border-radius:4px;color:#48627a;font-size:11px;}
 .filter{margin:10px 0;}
 .style-actions{display:flex;gap:5px;margin-bottom:7px;}
 /* The list is deliberately short (about two thirds of its original height) so
    the editor below stays reachable, and every name is allowed its full width:
    a name may shrink only when it is genuinely wider than the row. */
 .style-list{display:grid;gap:3px;width:100%;max-height:120px;overflow-y:auto;overflow-x:hidden;}
 .style-list button{display:flex;justify-content:space-between;align-items:center;gap:8px;text-align:left;width:100%;min-width:0;padding:4px 7px;line-height:1.5;}
 .style-list button.selected{background:#edf2f7;border-color:#8da5bc;}
 .style-name{flex:1 1 auto;min-width:max-content;overflow-wrap:anywhere;}
 .style-list small{flex:0 1 auto;color:#7b8b99;white-space:nowrap;}
 p{color:#778592;line-height:1.7;font-size:11px;}
 .error{color:#a03d39;}
 .save{margin-top:10px;}
 .versions summary{font-size:11px;color:#5d7086;cursor:pointer;}
 .version-list{display:flex;flex-wrap:wrap;gap:4px;margin:6px 0;}
 .version-item{display:inline-flex;align-items:center;border:1px solid #d3dce4;border-radius:4px;background:#fff;}
 .version-item.current{border-color:#8da5bc;background:#eef4f9;}
 .version-pick{border:0;background:transparent;padding:3px 4px 3px 7px;}
 .version-remove{border:0;background:transparent;padding:2px 5px 2px 2px;color:#8b9bab;line-height:0;}
 .version-remove:hover:not(:disabled){color:#a03d39;}
 .version-item:has(.version-remove:disabled) .version-remove{visibility:hidden;}
 p[role="status"]{color:#2f6b45;}
 button:hover:not(:disabled){background:#edf2f7;}
 button:disabled{opacity:.5;}
</style>
