import test from 'node:test';
import assert from 'node:assert/strict';
import { HistoryRequestGate, LocalData, localDataError, dataStatusWarning, chatUnavailableReason } from '../../src/lib/local-data.ts';
import type { DesktopBridge } from '../../src/lib/desktop-source.ts';
import { emptyDraft } from '../../src/lib/annotation-draft.ts';
const grant = '3ee1a7ee-82db-4554-8d84-751dd97b8880';
const binding = { bindingId: '6ba10989-2294-44a8-a2e8-9356ae9a74d0', documentId: 'feac8bd8-4f2f-4ea8-a97a-595a8ee71d36', revisionId: '5643163c-5b76-4b0d-80f6-860d15677ad2' };

test('desktop history carries granted bindings, preserves precise fit/offset positions, and has no path or SQL input', async () => {
  const calls: { command: string; args?: Parameters<DesktopBridge>[1] }[] = [];
  const bridge: DesktopBridge = async <T>(command: string, args?: Parameters<DesktopBridge>[1]) => {
    calls.push({ command, args });
    return (command === 'm4_bind_document' ? binding : command === 'm4_messages' ? [] : grant) as T;
  };
  const data = new LocalData(bridge); assert.deepEqual(await data.bind(grant, 3), binding);
  await data.messages(binding.bindingId);
  await data.add(binding.bindingId, '合成记录', 2);
  await data.edit(binding.bindingId, grant, '修改后');
  await data.generate(binding.bindingId, grant);
  await data.hide(binding.bindingId, grant);
  const position = { page: 2, zoom: 'page-width' as const, top: .37, left: -.2 };
  await data.position(binding.bindingId, position);
  assert.deepEqual(calls[0], { command: 'm4_bind_document', args: { grantId: grant, pageCount: 3 } });
  assert.deepEqual(calls.at(-1), { command: 'm4_position', args: { bindingId: binding.bindingId, position } });
  assert.ok(calls.every(call => !Object.keys(call.args ?? {}).some(key => ['path', 'hash', 'sql', 'documentId'].includes(key))));
});

test('late history responses cannot replace a newly selected document or a more recent query', async () => {
  const gate = new HistoryRequestGate(); gate.select(binding.bindingId);
  let finish!: (value: string) => void;
  const old = gate.run(binding.bindingId, () => new Promise<string>(resolve => { finish = resolve; }));
  gate.select(grant); finish('old document'); assert.equal((await old).current, false);
  gate.select(grant);
  const earlier = gate.run(grant, () => new Promise<string>(resolve => { finish = resolve; }));
  assert.deepEqual(await gate.run(grant, async () => 'latest'), { current: true, value: 'latest' });
  finish('outdated'); assert.equal((await earlier).current, false);
  const disposed = gate.run(grant, () => new Promise<string>(resolve => { finish = resolve; }));
  gate.select(null); finish('after close'); assert.equal((await disposed).current, false);
});

test('invalid local identifiers and positions fail before desktop invocation; invalid restored state is rejected', async () => {
  let called = 0;
  const bridge: DesktopBridge = async <T>() => { called++; return { ...binding, position: { page: 0, zoom: 1, top: 0, left: 0 } } as T; };
  const data = new LocalData(bridge);
  await assert.rejects(data.bind('../private.pdf', 2));
  await assert.rejects(data.add(binding.bindingId, 'text', 0));
  await assert.rejects(data.position(binding.bindingId, { page: 1, zoom: 200, top: 0, left: 0 }));
  assert.equal(called, 0);
  await assert.rejects(data.bind(grant, 2)); assert.equal(called, 1);
});

test('recovery entries are bounded validated annotation JSON, scoped only by the current desktop binding', async () => {
  const calls: { command: string; args?: Parameters<DesktopBridge>[1] }[] = [];
  const draft = emptyDraft();
  const bridge: DesktopBridge = async <T>(command: string, args?: Parameters<DesktopBridge>[1]) => {
    calls.push({ command, args });
    return (command === 'm4_list_drafts' ? [{ id: grant, json: JSON.stringify(draft), updated: 1 }] : undefined) as T;
  };
  const data = new LocalData(bridge);
  await data.putDraft(binding.bindingId, grant, draft);
  assert.deepEqual(await data.drafts(binding.bindingId), [{ id: grant, draft, updated: 1 }]);
  await data.dropDraft(binding.bindingId, grant);
  assert.deepEqual(calls[0], { command: 'm4_put_draft', args: { bindingId: binding.bindingId, id: grant, json: JSON.stringify(draft) } });
  assert.deepEqual(calls[2], { command: 'm4_drop_draft', args: { bindingId: binding.bindingId, id: grant } });
  await assert.rejects(data.putDraft(binding.bindingId, grant, { ...draft, entries: [{ annotationType: 3, id: 'private-existing-object' }] }));
  assert.equal(calls.length, 3);
});

test('local history rejects malformed records and private errors never become UI messages', async () => {
  const bad: DesktopBridge = async <T>() => [{ id: grant, role: 'assistant', content: 'x', createdMs: 1, status: 'complete', deleted: false, truncated: false, page: -1, metadata: {} }] as T;
  await assert.rejects(new LocalData(bad).messages(binding.bindingId));
  assert.ok(!localDataError({ code: 'unknown', message: 'C:/private/user/database' }).includes('private'));
  assert.ok(localDataError({ code: 'unauthorized' }).includes('授权'));
});

test('UTF-8 record budget is enforced before invoking the backend, including Chinese text', async () => {
  let called = 0;
  const bridge: DesktopBridge = async <T>() => { called++; return grant as T; };
  const data = new LocalData(bridge);
  await assert.rejects(data.add(binding.bindingId, '中'.repeat(6000), 1));
  await assert.rejects(data.edit(binding.bindingId, grant, 'α'.repeat(9000)));
  assert.equal(called, 0);
  await data.add(binding.bindingId, '中'.repeat(5000), 1); assert.equal(called, 1);
});

test('settings and backup operations remain typed and reject unsafe quotas or arbitrary backup paths', async () => {
  const calls: { command: string; args?: Parameters<DesktopBridge>[1] }[] = [];
  const settings = { schemaVersion: 1 as const, thumbnailOpen: true, cacheBytes: 64 * 1024 * 1024 };
  const bridge: DesktopBridge = async <T>(command: string, args?: Parameters<DesktopBridge>[1]) => {
    calls.push({ command, args });
    return (command === 'm4_list_backups' ? [{ id: grant, createdMs: 1, bytes: 4096 }] : command === 'm4_restore_backup' ? null : settings) as T;
  };
  const data = new LocalData(bridge);
  assert.deepEqual(await data.settings(), settings);
  assert.deepEqual(await data.updateSettings(true, settings.cacheBytes), settings);
  assert.equal((await data.backups())[0].id, grant);
  assert.equal(await data.restoreBackup(grant), null);
  const count = calls.length;
  await assert.rejects(data.updateSettings(false, 513 * 1024 * 1024));
  await assert.rejects(data.restoreBackup('D:/private/user.sqlite'));
  assert.equal(calls.length, count);
  assert.deepEqual(calls.at(-1), { command: 'm4_restore_backup', args: { backupId: grant } });
});

test('a usable store still exposes rollback and interrupted-material warnings independently of document binding', () => {
  const status = { available: true, path: 'D:/owned/data', areas: [], acl: 'private', generation: 2, error: '启动校验未完成，已恢复上一数据目录；材料保留。' };
  assert.equal(dataStatusWarning(status), status.error);
  assert.equal(status.available, true);
  assert.equal(dataStatusWarning({ ...status, generation: 3, error: '新目录保留未完成材料，请检查。' }), '新目录保留未完成材料，请检查。');
  assert.equal(dataStatusWarning({ ...status, error: '' }), '');
  assert.equal(dataStatusWarning(null), '');
  assert.equal(dataStatusWarning({ ...status, available: false }), status.error);
});
test('startup failures and per-PDF binding failures remain visible without enabling unbound chat',()=>{
  const status={available:false,path:'C:/M7Lab/app-data',areas:[],acl:'',generation:0,error:'创建定位目录失败；Windows 5；locator-create'};
  assert.ok(chatUnavailableReason(status,'',true).includes('Windows 5'));
  assert.ok(chatUnavailableReason(status,'',true).includes(status.path));
  const ready={...status,available:true,error:undefined};
  const first=localDataError({code:'document_identity_unavailable',message:'do not trust path/SQL'});
  assert.ok(chatUnavailableReason(ready,first,true).includes('可靠文件身份'));
  assert.ok(!chatUnavailableReason(ready,'',true).includes('可靠文件身份，聊天'));
  assert.ok(!first.includes('SQL'));
  assert.ok(chatUnavailableReason(null,'',true).includes('初始化'));
  assert.ok(chatUnavailableReason(ready,'',false).includes('先打开'));
});

test('document history deletion carries only the live binding and distinguishes native cancellation from confirmation', async () => {
  const calls: { command: string; args?: Parameters<DesktopBridge>[1] }[] = [];
  let confirmed = false;
  const bridge: DesktopBridge = async <T>(command: string, args?: Parameters<DesktopBridge>[1]) => { calls.push({command,args}); return confirmed as T; };
  const data = new LocalData(bridge);
  assert.equal(await data.deleteDocumentHistory(binding.bindingId), false);
  confirmed = true; assert.equal(await data.deleteDocumentHistory(binding.bindingId), true);
  assert.deepEqual(calls, Array.from({length:2}, () => ({ command: 'm4_delete_document_history', args: { bindingId: binding.bindingId } })));
  await assert.rejects(data.deleteDocumentHistory('../other-document'));
  assert.equal(calls.length, 2);
  const invalid: DesktopBridge = async <T>() => undefined as T;
  await assert.rejects(new LocalData(invalid).deleteDocumentHistory(binding.bindingId));
});
