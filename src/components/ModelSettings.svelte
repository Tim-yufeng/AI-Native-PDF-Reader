<script lang="ts">
  import { aiClient, aiError, providerDefaults, type AiConfig, type ModelConfig } from '../lib/ai-client';
  let { close, changed }: { close: () => void; changed: () => void } = $props();
  let config = $state<AiConfig | null>(null), message = $state(''), error = $state(''), busy = $state(false);
  let slot = $state<'main' | 'auxiliary'>('main'), key = $state(''), present = $state(false), realEnabled = $state(false);
  let confirming = $state(false),savedConfig=$state('');
  async function load() { try { const info = await aiClient.config(); config = info.config;savedConfig=JSON.stringify(info.config); realEnabled=info.realProviderEnabled; present = slot === 'main' ? info.mainKeyPresent : info.auxiliaryKeyPresent; } catch (e) { error = aiError(e); } }
  $effect(() => { void load(); });
  const model = $derived(config ? slot === 'main' ? config.main : config.auxiliary : null);
  const endpoint = $derived(model ? model.endpoint : '');
  function provider(value: ModelConfig['provider']) { if (!config) return; if (slot === 'main') config.main = providerDefaults(value); else config.auxiliary = providerDefaults(value); present = false; confirming = false; }
  function say(value: string) { message = value; error = ''; }
  function fail(e: unknown) { error = aiError(e); message = ''; }
  async function save() { if (!config) return; busy = true; message = ''; error = ''; const secret = key, target = slot, snapshot = JSON.parse(JSON.stringify(config)) as AiConfig; key = ''; try { await aiClient.configure(snapshot); if (secret) await aiClient.key(target, secret, target === 'main' ? snapshot.main.endpoint : snapshot.auxiliary!.endpoint); await load(); changed(); say(secret ? '配置与安全凭据已保存；密钥只写入 Windows 凭据管理器。' : '配置已保存。'); } catch (e) { fail(e); } finally { busy = false; } }
  // The credential is deleted through the backend, which addresses it by slot
  // and re-validates the stored endpoint. The renderer never holds the secret.
  async function remove() { if (!config || !model) return; busy = true; message = ''; error = ''; const target = slot; try { await aiClient.deleteKey(target); confirming = false; await load(); changed(); say('已删除该 Endpoint 的安全凭据；需要时请重新输入。'); } catch (e) { fail(e); } finally { busy = false; } }
  async function test() {
    if(!config||!model)return;
    if(key||JSON.stringify(config)!==savedConfig){fail(Error('请先保存配置和新凭据，再测试连接。当前输入已保留。'));return;}
    busy=true;error='';message='';
    try{const result=await aiClient.test(slot);say(result.mock?'Mock 接口正常，不是真实 API 验收。':`已保存模型的目录连接正常；图像能力：${result.vision?'支持':'未确认'}。配置没有改变，真实生成效果仍需实际提问验证。`);}
    catch(e){fail(e);}finally{busy=false;}
  }
</script>
<section class="models" aria-label="模型与安全凭据">
  <div class="title"><strong>模型设置</strong><button disabled={busy} onclick={() => { key = ''; close(); }}>关闭设置</button></div>
  <fieldset disabled={busy}>
  <label>模型角色<select bind:value={slot} onchange={() => { key = ''; confirming = false; void load(); }}><option value="main">主模型</option><option value="auxiliary">辅助模型（可选）</option></select></label>
  {#if config && slot === 'auxiliary'}<label class="check"><input type="checkbox" checked={!!config.auxiliary} onchange={(e) => { config!.auxiliary = e.currentTarget.checked ? providerDefaults('mock') : null; confirming = false; }} />启用辅助配置</label>{/if}
  {#if model}
    <label>Provider<select value={model.provider} onchange={(e) => provider(e.currentTarget.value as ModelConfig['provider'])}><option value="mock">Mock · 本地测试</option><option value="deepseek">DeepSeek</option><option value="openrouter">OpenRouter</option><option value="compatible">自定义兼容接口</option></select></label>
    <label>模型 ID<input bind:value={model.model} maxlength="160" spellcheck="false" /></label>
    {#if model.provider !== 'mock'}
      <label>HTTPS Endpoint<input bind:value={model.endpoint} spellcheck="false" disabled={model.provider !== 'compatible'} /></label>
      {#if realEnabled}
        <label>API Key<input type="password" bind:value={key} autocomplete="new-password" placeholder={present ? '已安全保存；输入可更新' : '仅此次输入，不回显已存密钥'} /></label>
        <div class="credential"><span role="status">{present ? '此 Endpoint 已有安全凭据。' : '此 Endpoint 尚未保存凭据。'}</span>
          {#if present && endpoint}{#if confirming}<button onclick={() => void remove()}>确认删除</button><button onclick={() => confirming = false}>保留</button>{:else}<button onclick={() => confirming = true}>删除已存凭据…</button>{/if}{/if}
        </div>
      {:else}<p>当前安装包未启用真实 Provider：凭据与网络请求已禁用。请改用 Mock，或安装正式版本后再配置。</p>{/if}
      <label class="check"><input type="checkbox" bind:checked={model.vision} />支持图像（请根据模型能力设置）</label>
    {/if}
  {/if}
  </fieldset>
  <div class="actions"><button disabled={busy || !config} onclick={() => void save()}>保存配置</button><button disabled={busy || !model || (!realEnabled && model.provider !== 'mock')} onclick={() => void test()}>测试连接</button></div>
  <p>辅助配置不自动调用，失败不会阻断主模型。自定义接口只支持安全的公网 HTTPS Chat Completions；不要填写本机或内网地址。</p>
  {#if message}<p role="status">{message}</p>{/if}
  {#if error}<p class="failure" role="alert">{error}</p>{/if}
</section>
<style>
  .models { padding: 12px; font-size: 12px; }.title,.actions { display:flex; gap:8px; align-items:center; justify-content:space-between; }
  label { display:flex; flex-direction:column; gap:5px; margin:12px 0; }.check { flex-direction:row; align-items:center; }
  input,select { width:100%; padding:6px; min-width:0; border:1px solid #cfd8e1; border-radius:4px; }.check input { width:auto; }
  p { color:#697582; line-height:1.6; }button { padding:5px 9px; border:1px solid #cfd8e1; background:#fff; border-radius:4px; }
  .failure { color:#a4483f; }.credential { display:flex; align-items:center; gap:6px; flex-wrap:wrap; color:#697582; }
  fieldset { border:0; padding:0; margin:0; min-width:0; }
</style>
