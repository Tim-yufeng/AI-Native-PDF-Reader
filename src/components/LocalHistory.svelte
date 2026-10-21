<script lang="ts">
  import { onDestroy } from 'svelte';
  import Icon from './Icon.svelte';
  import { HistoryRequestGate, localDataError, dataStatusWarning, type LocalData, type DataStatus, type DocumentBinding, type HistoryMessage, type LocalSettings, type LocalBackup } from '../lib/local-data';
  let { data, binding, status, settings, changeQuota, prepareDataChange, page, notice = '', close, migrated, restored, jumpPage,testHistory=false }: {
    data: LocalData; binding: DocumentBinding | null; status: DataStatus | null; page: number;
    settings: LocalSettings | null; changeQuota: (bytes: number) => Promise<LocalSettings>;
    prepareDataChange: () => Promise<void>; restored: (status: DataStatus) => void;
    jumpPage: (page: number, bindingId: string) => void;
    notice?: string; close: () => void; migrated: (status: DataStatus) => void;testHistory?:boolean;
  } = $props();
  let records = $state<HistoryMessage[]>([]), content = $state(''), error = $state(''), info = $state('');
  let busy = $state(false), loading = $state(false), showHidden = $state(false);
  let parent = $state<string | null>(null), editing = $state<string | null>(null), confirmHide = $state<string | null>(null);
  let cacheMiB = $state(64), quotaTouched = false;
  let backups = $state<LocalBackup[]>([]), backupId = $state('');
  $effect(() => { if (settings && !quotaTouched) cacheMiB = settings.cacheBytes / (1024 * 1024); });
  let operation = 0, refreshSerial = 0;
  const gate = new HistoryRequestGate();
  onDestroy(() => { gate.select(null); operation++; refreshSerial++; });
  let selectedBinding: string | null = null;
  $effect(() => {
    const id = binding?.bindingId ?? null;
    if (id === selectedBinding) return;
    selectedBinding = id; gate.select(id); operation++; busy = false;
    records = []; content = ''; editing = null; parent = null; error = ''; info = ''; confirmHide = null;
    loading = !!id;
    if (id&&testHistory) void refresh(id);else loading=false;
  });
  async function refresh(id = binding?.bindingId) {
    if (!id) return;
    const serial = ++refreshSerial;
    loading = true;
    try { const result = await gate.run(id, () => data.messages(id)); if (result.current && serial === refreshSerial) { records = result.value; loading = false; error = ''; } }
    catch (cause) { if (serial === refreshSerial && selectedBinding === id) { error = localDataError(cause); loading = false; } }
  }
  async function action(run: (id: string) => Promise<unknown>, success: string, clearInput = false) {
    const id = binding?.bindingId; if (!id || busy) return;
    const token = ++operation; busy = true; error = ''; info = '';
    try {
      await run(id);
      if (token !== operation || selectedBinding !== id) return;
      if (clearInput) { content = ''; editing = null; parent = null; }
      confirmHide = null; info = success; await refresh(id);
    } catch (cause) { if (token === operation && selectedBinding === id) error = localDataError(cause); }
    finally { if (token === operation) busy = false; }
  }
  async function submit() {
    const text = content.trim(); if (!text) return;
    if (new TextEncoder().encode(text).byteLength > 16 * 1024) { error = '文字超过本阶段记录上限（16 KB），请缩短后提交。'; return; }
    const editId = editing, parentId = parent, anchor = page;
    await action(id => editId ? data.edit(id, editId, text) : data.add(id, text, anchor, parentId ?? undefined), editId ? '已建立新分支，原提问仍保留。' : '本地测试记录已提交。', true);
  }
  async function maintenance(kind: 'backup' | 'migrate' | 'cache') {
    if (busy) return;
    const token = ++operation; busy = true; error = ''; info = '';
    try {
      if (kind === 'backup') { await prepareDataChange(); const result = await data.backup(); if (token === operation) { info = `备份已完成：${result}`; await listBackups(); } }
      if (kind === 'cache') { await data.clearCache(); if (token === operation) info = '可重建缓存已清理，历史、草稿和备份均保留。'; }
      if (kind === 'migrate') {
        await prepareDataChange();
        const result = await data.migrate();
        if (result) { migrated(result); if (token === operation) info = '数据目录已切换；旧目录保留。'; }
        else if (token === operation) info = '已取消，数据目录未切换。';
      }
    } catch (cause) { if (token === operation) error = localDataError(cause); }
    finally { if (token === operation) busy = false; }
  }
  async function listBackups() {
    try { const listed = await data.backups(); backups = listed; if (!listed.some(backup => backup.id === backupId)) backupId = listed[0]?.id ?? ''; }
    catch (cause) { error = localDataError(cause); }
  }
  async function restoreSelectedBackup() {
    if (busy || !backupId) return;
    const token = ++operation; busy = true; error = ''; info = '';
    try {
      await prepareDataChange(); const result = await data.restoreBackup(backupId);
      if (result) { restored(result); if (token === operation) info = '备份已恢复到新目录，原目录保留；请重新打开 PDF 以恢复对应历史。'; }
      else if (token === operation) info = '已取消恢复，现有数据保留。';
    } catch (cause) { if (token === operation) error = localDataError(cause); }
    finally { if (token === operation) busy = false; }
  }
  async function updateQuota() {
    if (busy || !Number.isInteger(cacheMiB) || cacheMiB < 0 || cacheMiB > 512) { error = '请输入 0–512 MB 的缓存配额。'; return; }
    const token = ++operation; busy = true; error = ''; info = '';
    try { await changeQuota(cacheMiB * 1024 * 1024); if (token === operation) { quotaTouched = false; info = '缓存配额已保存；历史、草稿与重要备份不计入自动清理。'; } }
    catch (cause) { if (token === operation) error = localDataError(cause); }
    finally { if (token === operation) busy = false; }
  }
  async function deleteDocumentHistory() {
    const id = binding?.bindingId; if (!id || busy) return;
    const token = ++operation; busy = true; error = ''; info = '';
    try {
      const deleted = await data.deleteDocumentHistory(id);
      if (token !== operation || selectedBinding !== id) return;
      if (!deleted) { info = '已取消删除，原历史保持不变。'; return; }
      gate.select(id); records = []; content = ''; editing = null; parent = null; confirmHide = null;
      info = '当前 PDF 的全部聊天记录与分支已从活动数据库删除；旧备份和旧目录保留，其他文档、草稿与 PDF 未修改。';
      await refresh(id);
    } catch (cause) { if (token === operation && selectedBinding === id) error = localDataError(cause); }
    finally { if (token === operation) busy = false; }
  }
  const visible = $derived(records.filter(record => showHidden || !record.deleted));
  const statusWarning = $derived(dataStatusWarning(status));
  const short = (id: string) => id.slice(0, 8);
</script>

<aside class="local-history" aria-label="本地历史与数据" id="local-history">
  <div class="local-history-heading"><h2>{testHistory?'本地测试历史':'本地数据与备份'}</h2><button class="icon-button" aria-label="关闭本地历史" title="关闭本地数据" onclick={close}><Icon name="close" /></button></div>
  <p class="local-history-intro">{testHistory?'开发工具：合成测试记录。':'聊天会话请在 AI 对话中查看；这里管理本地数据、备份和可重建缓存。'}</p>
  {#if statusWarning}<p class="local-history-notice" role="status">{statusWarning}</p>{/if}
  {#if notice}<p class="local-history-notice" role="status">{notice}</p>{/if}
  {#if !status?.available}
    <p class="local-history-notice" role="status">{status?.error || '本地历史仅在桌面应用中可用；浏览器阅读功能不受影响。'}</p>
  {:else if !binding}
    <p class="local-history-notice" role="status">{loading ? '正在恢复文档身份…' : '打开本地 PDF 后，可查看对应历史。'}</p>
  {:else if testHistory}
    <p class="local-history-intro">显示最近 100 条本地测试记录。</p>
    <div class="history-controls"><span>{visible.length} 条可见</span><label><input type="checkbox" bind:checked={showHidden} />显示已隐藏</label><button disabled={busy || loading} onclick={() => void refresh()}>刷新</button></div>
    <ol class="history-records" aria-label="已提交的本地测试记录" aria-busy={loading}>
      {#each visible as record (record.id)}
        <li class:hidden-record={record.deleted} class:selected-record={parent === record.id}>
          <div class="history-record-meta"><strong>{record.role === 'user' ? '测试提问' : '合成测试回复'}</strong><span>{new Date(record.createdMs).toLocaleString()}</span></div>
          <p class="history-record-content">{record.deleted ? '原记录仍保留，此处不展示已隐藏内容。' : record.content}</p>
          {#if record.truncated}<p class="local-history-intro">内容较长，预览已截断；此处不能编辑残缺内容。</p>{/if}
          <div class="history-record-details">{#if record.page}<button title="跳回来源页" onclick={() => { if (binding) jumpPage(record.page!, binding.bindingId); }}>第 {record.page} 页</button> · {/if}#{short(record.id)}{record.parent ? ` · 上级 #${short(record.parent)}` : ''}{record.editedFrom ? ` · 修改自 #${short(record.editedFrom)}` : ''}{record.deleted ? ' · 已隐藏' : ''}{record.status !== 'complete' ? ` · ${record.status === 'failed' ? '已中断' : record.status === 'cancelled' ? '已取消' : '未完成'}` : ''}</div>
          {#if !record.deleted}
            <div class="history-record-actions">
              <button disabled={busy} onclick={() => { parent = record.id; editing = null; content = ''; }}>继续此分支</button>
              {#if record.role === 'user' && record.status === 'complete'}
                <button disabled={busy || record.truncated} onclick={() => { editing = record.id; parent = null; content = record.content; }}>编辑为新分支</button>
                <button disabled={busy} onclick={() => void action(id => data.generate(id, record.id), '合成回复已作为新分支保存。')}>生成测试回复</button>
              {/if}
              <button disabled={busy} onclick={() => confirmHide = record.id}>隐藏</button>
            </div>
            {#if confirmHide === record.id}<div class="history-hide-confirm"><span>仅从常规列表隐藏，底层历史保留。</span><button disabled={busy} onclick={() => void action(id => data.hide(id, record.id), '已隐藏此记录，原记录仍保留。')}>确认隐藏</button><button disabled={busy} onclick={() => confirmHide = null}>取消</button></div>{/if}
          {/if}
        </li>
      {:else}<li class="history-empty">{loading ? '正在读取历史…' : '此文档尚无本地测试记录。'}</li>{/each}
    </ol>
    <form class="history-entry" onsubmit={event => { event.preventDefault(); void submit(); }}>
      <label for="local-test-prompt">{editing ? `编辑 #${short(editing)}（保留原提问）` : parent ? `继续 #${short(parent)} 分支` : `新增本地测试提问 · 第 ${page || 1} 页`}</label>
      <textarea id="local-test-prompt" aria-label="本地测试提问" maxlength="20000" rows="3" bind:value={content} disabled={busy}></textarea>
      <span class="local-history-intro">合成记录上限 16 KB，全部内容只保存在本机。</span>
      <div><button type="submit" disabled={busy || !content.trim() || page < 1}>{editing ? '保存为新分支' : '提交测试记录'}</button>{#if editing || parent}<button type="button" disabled={busy} onclick={() => { editing = null; parent = null; content = ''; }}>取消分支选择</button>{/if}</div>
    </form>
  {/if}
  {#if error || info}<p class="local-history-notice" class:error={!!error} role="status" aria-live="polite">{error || info}</p>{/if}
  <details class="history-maintenance" open={!testHistory}><summary>本地数据管理</summary>
    {#if status?.available}
      <p class="data-path">{status.path}</p><p>目录保护：{status.acl}</p><p>数据库、设置、恢复稿与可重建缓存分开管理。</p>
      {#if settings}<form class="cache-quota" onsubmit={event => { event.preventDefault(); void updateQuota(); }}><label for="cache-quota">可重建缓存上限</label><input id="cache-quota" aria-label="缓存上限 MB" type="number" min="0" max="512" step="1" bind:value={cacheMiB} oninput={() => quotaTouched = true} /><span>MB</span><button disabled={busy} type="submit">应用</button></form>{/if}
      <div><button disabled={busy} onclick={() => void maintenance('backup')}>一致备份</button><button disabled={busy} onclick={() => void maintenance('migrate')}>选择新数据目录</button><button disabled={busy} onclick={() => void maintenance('cache')}>清理可重建缓存</button></div>
      <div class="history-backups"><button disabled={busy} onclick={() => void listBackups()}>查看备份</button>{#if backups.length}<select aria-label="选择历史备份" bind:value={backupId} disabled={busy}>{#each backups as backup}<option value={backup.id}>{new Date(backup.createdMs).toLocaleString()} · {(backup.bytes / 1024).toFixed(0)} KB</option>{/each}</select><button disabled={busy || !backupId} onclick={() => void restoreSelectedBackup()}>恢复所选备份</button>{/if}</div>
      <p>切换前会要求确认；复制并验证完成后才切换，旧数据不自动删除。</p>
      <p>旧浏览器恢复稿仍留在原恢复区，未自动迁移；主动恢复后产生的新桌面草稿会随数据目录迁移。</p>
      {#if binding}<button disabled={busy} onclick={() => void deleteDocumentHistory()}>删除当前文档的全部聊天历史</button><p>删除前需要确认；只删除活动数据库中的记录。旧备份保留，不执行安全擦除。</p>{/if}
    {:else}<p>需要在桌面应用中使用。浏览器不会模拟迁移或备份。</p>{/if}
  </details>
</aside>
